//! BGA（背景动画）渲染模块
//!
//! - 负责加载图片纹理并写入绑定组
//! - 根据屏幕尺寸居中缩放绘制单张图片
//! - 与主矩形渲染管线复用统一缓冲

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    thread,
    time::Duration,
};

use anyhow::Result;
use async_fs as fs;
use bytemuck::{Pod, Zeroable};
use futures_lite::future;
use image::{ImageBuffer, Luma};
use imageproc::region_labelling::{Connectivity, connected_components};

use crate::loops::BgaLayer;
use crate::loops::visual::video::DecodedFrame;

/// 已解码的 BGA 图片数据
pub struct BgaDecodedImage {
    /// RGBA8 像素缓冲
    pub rgba: Vec<u8>,
    /// 宽度
    pub width: u32,
    /// 高度
    pub height: u32,
}

/// 解码后的缓存变体
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum DecodeVariant {
    /// 原始 RGBA
    Raw,
    /// 去除背景后的 RGBA
    RemoveBackground,
}

/// 缓存键：路径 + 解码变体
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct CacheKey {
    /// 文件路径
    path: PathBuf,
    /// 解码变体
    variant: DecodeVariant,
}

/// BGA 图片解码缓存（跨线程共享）
pub struct BgaDecodeCache {
    /// (路径, 变体) 到已解码图片的映射
    inner: Mutex<HashMap<CacheKey, Arc<BgaDecodedImage>>>,
}

impl BgaDecodeCache {
    /// 创建空缓存
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// 查询指定变体的缓存条目
    #[must_use]
    fn get_variant(&self, variant: DecodeVariant, path: &Path) -> Option<Arc<BgaDecodedImage>> {
        let key = CacheKey {
            path: path.to_path_buf(),
            variant,
        };
        self.inner.lock().ok()?.get(&key).cloned()
    }

    /// 写入指定变体的缓存条目并返回共享引用
    fn insert_variant(
        &self,
        variant: DecodeVariant,
        path: PathBuf,
        rgba: Vec<u8>,
        width: u32,
        height: u32,
    ) -> Arc<BgaDecodedImage> {
        let decoded = Arc::new(BgaDecodedImage {
            rgba,
            width,
            height,
        });
        if let Ok(mut map) = self.inner.lock() {
            map.insert(CacheKey { path, variant }, decoded.clone());
        }
        decoded
    }
}

/// 将指定图层映射到预处理变体
const fn layer_to_variant(layer: BgaLayer) -> DecodeVariant {
    match layer {
        BgaLayer::Layer | BgaLayer::Layer2 => DecodeVariant::RemoveBackground,
        BgaLayer::Bga | BgaLayer::Poor => DecodeVariant::Raw,
    }
}

/// 去除背景（黑色背景转透明）
fn remove_background(rgba_buf: &mut [u8], width: u32, height: u32) {
    let width_usize = width as usize;
    let mask = ImageBuffer::from_fn(width, height, |x, y| {
        let base = ((y as usize) * width_usize + (x as usize)) * 4;
        let is_black = rgba_buf
            .get(base..base + 4)
            .and_then(|px| <[u8; 4]>::try_from(px).ok())
            .is_some_and(|[r, g, b, a]| r == 0 && g == 0 && b == 0 && a != 0);
        Luma([u8::from(is_black)])
    });

    let labels = connected_components(&mask, Connectivity::Four, Luma([0u8]));
    let corners = [
        (0u32, 0u32),
        (width - 1, 0u32),
        (0u32, height - 1),
        (width - 1, height - 1),
    ];
    let mut targets = [0u32; 4];
    let mut targets_len = 0usize;
    for (x, y) in corners {
        let label = *labels.get_pixel(x, y).0.first().unwrap_or(&0);
        if label == 0 || targets.iter().take(targets_len).any(|v| *v == label) {
            continue;
        }
        let Some(slot) = targets.get_mut(targets_len) else {
            break;
        };
        *slot = label;
        targets_len += 1;
    }

    if targets_len != 0 {
        for (x, y, p) in labels.enumerate_pixels() {
            let label = *p.0.first().unwrap_or(&0);
            if label == 0 || !targets.iter().take(targets_len).any(|v| *v == label) {
                continue;
            }

            let base = ((y as usize) * width_usize + (x as usize)) * 4;
            if let Some(px) = rgba_buf.get_mut(base..base + 4) {
                px.copy_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
}

/// 从文件路径读取并解码图片为 RGBA8 缓冲
async fn decode_image_async(path: &Path) -> Option<(Vec<u8>, u32, u32)> {
    let bytes = fs::read(path).await.ok()?;
    let img = image::load_from_memory(&bytes).ok()?;
    let rgba = img.to_rgba8();
    let w = rgba.width();
    let h = rgba.height();
    Some((rgba.into_raw(), w, h))
}

/// 对 RGBA 缓冲按变体进行预处理
fn preprocess_rgba(mut rgba: Vec<u8>, width: u32, height: u32, variant: DecodeVariant) -> Vec<u8> {
    if variant == DecodeVariant::RemoveBackground && width != 0 && height != 0 {
        remove_background(&mut rgba, width, height);
    }
    rgba
}

/// 解码图片并写入缓存（缓存命中则直接返回）
pub(crate) fn decode_and_cache(
    cache: &BgaDecodeCache,
    layer: BgaLayer,
    path: PathBuf,
) -> Option<Arc<BgaDecodedImage>> {
    let want = layer_to_variant(layer);
    if let Some(decoded) = cache.get_variant(want, path.as_path()) {
        return Some(decoded);
    }

    if want == DecodeVariant::RemoveBackground
        && let Some(raw) = cache.get_variant(DecodeVariant::Raw, path.as_path())
    {
        let rgba = preprocess_rgba(raw.rgba.clone(), raw.width, raw.height, want);
        return Some(cache.insert_variant(want, path, rgba, raw.width, raw.height));
    }

    let (raw_rgba, w, h) = future::block_on(decode_image_async(path.as_path()))?;
    let raw = cache.insert_variant(DecodeVariant::Raw, path.clone(), raw_rgba.clone(), w, h);
    let processed = preprocess_rgba(raw_rgba, w, h, DecodeVariant::RemoveBackground);
    let processed = cache.insert_variant(DecodeVariant::RemoveBackground, path, processed, w, h);
    Some(match want {
        DecodeVariant::Raw => raw,
        DecodeVariant::RemoveBackground => processed,
    })
}

/// 确保指定路径的两种预处理变体都已进入缓存
fn ensure_preprocessed(cache: &BgaDecodeCache, path: PathBuf) {
    let raw_exists = cache
        .get_variant(DecodeVariant::Raw, path.as_path())
        .is_some();
    let processed_exists = cache
        .get_variant(DecodeVariant::RemoveBackground, path.as_path())
        .is_some();
    if raw_exists && processed_exists {
        return;
    }

    if !processed_exists && let Some(raw) = cache.get_variant(DecodeVariant::Raw, path.as_path()) {
        let rgba = preprocess_rgba(
            raw.rgba.clone(),
            raw.width,
            raw.height,
            DecodeVariant::RemoveBackground,
        );
        let _ = cache.insert_variant(
            DecodeVariant::RemoveBackground,
            path,
            rgba,
            raw.width,
            raw.height,
        );
        return;
    }

    let Some((raw_rgba, w, h)) = future::block_on(decode_image_async(path.as_path())) else {
        return;
    };
    if !raw_exists {
        let _ = cache.insert_variant(DecodeVariant::Raw, path.clone(), raw_rgba.clone(), w, h);
    }
    if !processed_exists {
        let rgba = preprocess_rgba(raw_rgba, w, h, DecodeVariant::RemoveBackground);
        let _ = cache.insert_variant(DecodeVariant::RemoveBackground, path, rgba, w, h);
    }
}

/// 预先解码所有 BGA 图片到缓存，并每秒输出一次进度
pub fn preload_bga_files(cache: Arc<BgaDecodeCache>, files: Vec<PathBuf>) {
    let paths: Vec<PathBuf> = files
        .into_iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let total = paths.len() as u32;
    if total == 0 {
        println!("BGA预加载进度：0/0");
        println!("BGA预加载完成");
        return;
    }

    let loaded = Arc::new(AtomicU32::new(0));
    let done = Arc::new(AtomicBool::new(false));

    let loaded_for_log = loaded.clone();
    let done_for_log = done.clone();
    let logger = thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_secs(1));
            let c = loaded_for_log.load(Ordering::Relaxed);
            println!("BGA预加载进度：{}/{}", c, total);
            if done_for_log.load(Ordering::Relaxed) {
                break;
            }
        }
    });

    let (work_tx, work_rx) = std::sync::mpsc::channel::<PathBuf>();
    let work_rx = Arc::new(std::sync::Mutex::new(work_rx));
    let workers = thread::available_parallelism()
        .map(std::num::NonZero::get)
        .unwrap_or(1)
        .clamp(1, 8);

    let mut handles = Vec::with_capacity(workers);
    for _ in 0..workers {
        let work_rx = work_rx.clone();
        let cache = cache.clone();
        let loaded = loaded.clone();
        handles.push(thread::spawn(move || {
            loop {
                let path = {
                    let Ok(work_rx) = work_rx.lock() else {
                        break;
                    };
                    match work_rx.recv() {
                        Ok(p) => p,
                        Err(_) => break,
                    }
                };
                ensure_preprocessed(cache.as_ref(), path);
                loaded.fetch_add(1, Ordering::Relaxed);
            }
        }));
    }

    for path in paths {
        let _ = work_tx.send(path);
    }
    drop(work_tx);

    for h in handles {
        let _ = h.join();
    }
    done.store(true, Ordering::Relaxed);
    let _ = logger.join();
    println!("BGA预加载完成");
}

/// 图片上传所需的 GPU 上下文
pub(crate) struct UploadCtx<'a> {
    /// 设备
    pub(crate) device: &'a wgpu::Device,
    /// 队列
    pub(crate) queue: &'a wgpu::Queue,
    /// 屏幕统一缓冲
    pub(crate) screen_buffer: &'a wgpu::Buffer,
}

/// RGBA8 sRGB 图片数据
pub(crate) struct RgbaImage<'a> {
    /// RGBA 像素数据
    pub(crate) rgba: &'a [u8],
    /// 宽度
    pub(crate) width: u32,
    /// 高度
    pub(crate) height: u32,
}

/// 单个图层的纹理与可见状态
struct LayerState {
    /// 当前纹理视图（未加载则为 None）
    texture_view: Option<wgpu::TextureView>,
    /// 图层绘制区域（中心 x, 中心 y, 宽, 高）
    rect: [f32; 4],
    /// 是否可见
    visible: bool,
}

/// 简易 BGA 渲染器：负责加载图片并绘制到屏幕
pub struct BgaRenderer {
    /// 渲染管线
    pipeline: wgpu::RenderPipeline,
    /// 绑定组布局
    bind_group_layout: wgpu::BindGroupLayout,
    /// 绑定组（屏幕参数 + 图层参数 + 纹理 + 采样器）
    bind_group: wgpu::BindGroup,
    /// 统一采样器
    sampler: wgpu::Sampler,
    /// 图层参数缓冲
    params_buffer: wgpu::Buffer,
    /// 单实例缓冲（覆盖整个 BGA 面板区域）
    instance_buf: wgpu::Buffer,
    /// 未加载纹理时使用的占位视图
    placeholder_view: wgpu::TextureView,
    /// 当前帧是否有任意图层可绘制
    has_any_enabled: bool,
    /// BGA 主图层状态
    bga: LayerState,
    /// LAYER 叠加图层状态
    layer: LayerState,
    /// LAYER2 叠加图层状态
    layer2: LayerState,
    /// POOR 图层状态
    poor: LayerState,
}

#[repr(C, align(16))]
#[derive(Clone, Copy, Zeroable, Pod)]
/// BGA 多图层合成参数
struct BgaParamsUniform {
    /// 每个图层的绘制区域（中心 x, 中心 y, 宽, 高）
    rects: [[f32; 4]; 4],
    /// 每个图层是否启用（0/1）
    flags: [u32; 4],
}

impl BgaRenderer {
    /// 创建 BGA 渲染器并初始化管线与缓冲
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen_buffer: &wgpu::Buffer,
        format: wgpu::TextureFormat,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("tex-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../../tex.wgsl").into()),
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("tex-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("tex-pl"),
            bind_group_layouts: &[&bind_group_layout],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("tex-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<[f32; 2]>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &[wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 0,
                            shader_location: 0,
                        }],
                    },
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<crate::Instance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &[
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x2,
                                offset: 0,
                                shader_location: 1,
                            },
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x2,
                                offset: 8,
                                shader_location: 2,
                            },
                        ],
                    },
                ],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let params_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bga-params"),
            size: std::mem::size_of::<BgaParamsUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let instance_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bga-instance"),
            size: std::mem::size_of::<crate::Instance>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
        let placeholder = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("bga-placeholder"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &placeholder,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &[0, 0, 0, 0],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        let placeholder_view = placeholder.create_view(&wgpu::TextureViewDescriptor::default());

        let new_state = || LayerState {
            texture_view: None,
            rect: [0.0; 4],
            visible: false,
        };
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bga-bg"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: screen_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: params_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&placeholder_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&placeholder_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&placeholder_view),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&placeholder_view),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        Self {
            pipeline,
            bind_group_layout,
            bind_group,
            sampler,
            params_buffer,
            instance_buf,
            placeholder_view,
            has_any_enabled: false,
            bga: new_state(),
            layer: new_state(),
            layer2: new_state(),
            poor: new_state(),
        }
    }

    /// 更新指定图层的图片（RGBA8，sRGB）
    pub fn update_layer_image(
        &mut self,
        layer: BgaLayer,
        ctx: UploadCtx<'_>,
        img: RgbaImage<'_>,
    ) -> Result<()> {
        let texture = ctx.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("bga-texture"),
            size: wgpu::Extent3d {
                width: img.width,
                height: img.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        ctx.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            img.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * img.width),
                rows_per_image: Some(img.height),
            },
            wgpu::Extent3d {
                width: img.width,
                height: img.height,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let side = super::VISIBLE_HEIGHT;
        let iw = img.width as f32;
        let ih = img.height as f32;
        let scale = if iw >= ih { side / iw } else { side / ih };
        let draw_w = iw * scale;
        let draw_h = ih * scale;
        let center_x = (super::RIGHT_PANEL_GAP + super::VISIBLE_HEIGHT) / 2.0;
        let state = self.state_mut(layer);
        state.texture_view = Some(view);
        state.rect = [center_x, 0.0, draw_w, draw_h];
        if layer != BgaLayer::Poor {
            state.visible = true;
        }
        self.rebuild_bind_group(ctx.device, ctx.screen_buffer);
        Ok(())
    }

    /// 更新指定图层的视频帧（RGBA8，sRGB）
    ///
    /// 从 `DecodedFrame` 更新纹理，用于视频 BGA
    pub fn update_video_frame(
        &mut self,
        layer: BgaLayer,
        ctx: UploadCtx<'_>,
        frame: &DecodedFrame,
    ) -> Result<()> {
        self.update_layer_image(
            layer,
            ctx,
            RgbaImage {
                rgba: &frame.rgba,
                width: frame.width,
                height: frame.height,
            },
        )
    }

    /// 设置指定图层是否可见
    pub const fn set_layer_visible(&mut self, layer: BgaLayer, visible: bool) {
        let state = self.state_mut(layer);
        state.visible = visible;
    }

    /// 刷新图层合成参数并上传到 GPU
    pub fn prepare(&mut self, queue: &wgpu::Queue) {
        let bga_enabled = self.bga.visible && self.bga.texture_view.is_some();
        let layer_enabled = self.layer.visible && self.layer.texture_view.is_some();
        let layer2_enabled = self.layer2.visible && self.layer2.texture_view.is_some();
        let poor_enabled = self.poor.visible && self.poor.texture_view.is_some();

        let rect_bga = if bga_enabled { self.bga.rect } else { [0.0; 4] };
        let rect_layer = if layer_enabled {
            self.layer.rect
        } else {
            [0.0; 4]
        };
        let rect_layer2 = if layer2_enabled {
            self.layer2.rect
        } else {
            [0.0; 4]
        };
        let rect_poor = if poor_enabled {
            self.poor.rect
        } else {
            [0.0; 4]
        };

        let rects = [rect_bga, rect_layer, rect_layer2, rect_poor];
        let flags = [
            if bga_enabled { 1 } else { 0 },
            if layer_enabled { 1 } else { 0 },
            if layer2_enabled { 1 } else { 0 },
            if poor_enabled { 1 } else { 0 },
        ];

        let mut max_w = 0.0f32;
        let mut max_h = 0.0f32;
        if bga_enabled {
            max_w = max_w.max(rect_bga[2]);
            max_h = max_h.max(rect_bga[3]);
        }
        if layer_enabled {
            max_w = max_w.max(rect_layer[2]);
            max_h = max_h.max(rect_layer[3]);
        }
        if layer2_enabled {
            max_w = max_w.max(rect_layer2[2]);
            max_h = max_h.max(rect_layer2[3]);
        }
        if poor_enabled {
            max_w = max_w.max(rect_poor[2]);
            max_h = max_h.max(rect_poor[3]);
        }

        self.has_any_enabled = bga_enabled || layer_enabled || layer2_enabled || poor_enabled;
        let params = BgaParamsUniform { rects, flags };
        queue.write_buffer(&self.params_buffer, 0, bytemuck::bytes_of(&params));

        let center_x = (super::RIGHT_PANEL_GAP + super::VISIBLE_HEIGHT) / 2.0;
        let inst = crate::Instance {
            pos: [center_x, 0.0],
            size: [max_w, max_h],
            color: [1.0, 1.0, 1.0, 1.0],
        };
        queue.write_buffer(&self.instance_buf, 0, bytemuck::bytes_of(&inst));
    }

    /// 在当前渲染通道中绘制已激活的 BGA
    pub fn draw(
        &self,
        rpass: &mut wgpu::RenderPass<'_>,
        quad_vb: &wgpu::Buffer,
        idx_buf: &wgpu::Buffer,
        index_count: u32,
    ) {
        if !self.has_any_enabled {
            return;
        }
        rpass.set_pipeline(&self.pipeline);
        rpass.set_bind_group(0, &self.bind_group, &[]);
        rpass.set_vertex_buffer(0, quad_vb.slice(..));
        rpass.set_vertex_buffer(1, self.instance_buf.slice(..));
        rpass.set_index_buffer(idx_buf.slice(..), wgpu::IndexFormat::Uint16);
        rpass.draw_indexed(0..index_count, 0, 0..1);
    }

    /// 获取可变的图层状态
    const fn state_mut(&mut self, layer: BgaLayer) -> &mut LayerState {
        match layer {
            BgaLayer::Bga => &mut self.bga,
            BgaLayer::Layer => &mut self.layer,
            BgaLayer::Layer2 => &mut self.layer2,
            BgaLayer::Poor => &mut self.poor,
        }
    }

    /// 重建绑定组以指向最新纹理
    fn rebuild_bind_group(&mut self, device: &wgpu::Device, screen_buffer: &wgpu::Buffer) {
        let view_bga = self
            .bga
            .texture_view
            .as_ref()
            .unwrap_or(&self.placeholder_view);
        let view_layer = self
            .layer
            .texture_view
            .as_ref()
            .unwrap_or(&self.placeholder_view);
        let view_layer2 = self
            .layer2
            .texture_view
            .as_ref()
            .unwrap_or(&self.placeholder_view);
        let view_poor = self
            .poor
            .texture_view
            .as_ref()
            .unwrap_or(&self.placeholder_view);

        self.bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bga-bg"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: screen_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.params_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(view_bga),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(view_layer),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(view_layer2),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(view_poor),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
    }
}
