//! 视觉循环：事件线程上的渲染
//!
//! - 在 `Resumed` 创建窗口与渲染器，发送启动信号
//! - 在 `RedrawRequested` 非阻塞接收最新帧并渲染
//! - 在 `about_to_wait` 请求重绘以维持刷新

mod bga;
mod note;
mod video;

pub use bga::{BgaDecodeCache, BgaDecodedImage, preload_bga_files};
pub use note::{base_instances, build_instances_for_processor_with_state};
pub use video::{DecodedFrame, VideoDecoder};

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, mpsc},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use anyhow::Result;
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::Instance;
use crate::loops::BgaLayer;

#[repr(C)]
#[derive(Clone, Copy, Zeroable, Pod)]
/// 屏幕统一参数
struct ScreenUniform {
    /// 屏幕尺寸（宽, 高）
    size: [f32; 2],
}

/// 简易矩形渲染器，负责上传实例并绘制
pub struct Renderer {
    /// 渲染表面
    surface: wgpu::Surface<'static>,
    /// 图形设备
    device: wgpu::Device,
    /// 命令队列
    queue: wgpu::Queue,
    /// 渲染表面配置
    config: wgpu::SurfaceConfiguration,
    /// 渲染管线
    pipeline: wgpu::RenderPipeline,
    /// 绑定组
    bind_group: wgpu::BindGroup,
    /// 屏幕统一缓冲
    screen_buffer: wgpu::Buffer,
    /// 四边形顶点缓冲
    quad_vb: wgpu::Buffer,
    /// 索引缓冲
    idx_buf: wgpu::Buffer,
    /// 实例缓冲
    instance_buf: wgpu::Buffer,
    /// BGA 渲染器
    bga: bga::BgaRenderer,
    /// BGA 解码请求发送端（发送图片路径）
    bga_decode_tx: Option<mpsc::Sender<(BgaLayer, PathBuf)>>,
    /// BGA 解码结果接收端
    bga_decoded_rx: mpsc::Receiver<(BgaLayer, Arc<BgaDecodedImage>)>,
    /// BGA 解码线程句柄
    bga_decode_thread: Option<JoinHandle<()>>,
    /// 三角形索引数量
    index_count: u32,
    /// 逻辑屏幕尺寸
    logical_size: [f32; 2],
    /// POOR 图层可见到的时间点（到期自动隐藏）
    poor_until: Option<Instant>,
}

impl Renderer {
    /// 创建渲染器并初始化管线、缓冲与资源
    pub fn new(
        surface: wgpu::Surface<'static>,
        device: wgpu::Device,
        queue: wgpu::Queue,
        config: wgpu::SurfaceConfiguration,
        bga_cache: Arc<BgaDecodeCache>,
    ) -> Result<Self> {
        let format = config.format;
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rect-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../rect.wgsl").into()),
        });
        let screen_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screen-uniform"),
            size: std::mem::size_of::<ScreenUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rect-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rect-bg"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: screen_buffer.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rect-pl"),
            bind_group_layouts: &[&bind_group_layout],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rect-pipeline"),
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
                        array_stride: std::mem::size_of::<Instance>() as u64,
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
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x4,
                                offset: 16,
                                shader_location: 3,
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
        let quad_vertices: [[f32; 2]; 4] = [[-0.5, -0.5], [0.5, -0.5], [0.5, 0.5], [-0.5, 0.5]];
        let quad_vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("quad-vb"),
            contents: bytemuck::cast_slice(&quad_vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices: [u16; 6] = [0, 1, 2, 0, 2, 3];
        let idx_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("quad-ib"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let instance_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("instance-buf"),
            size: (std::mem::size_of::<Instance>() * 1024) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bga = bga::BgaRenderer::new(&device, &queue, &screen_buffer, format);
        let logical_size = [config.width as f32, config.height as f32];

        let (bga_decode_tx, bga_decode_rx) = mpsc::channel::<(BgaLayer, PathBuf)>();
        let (bga_decoded_tx, bga_decoded_rx) = mpsc::channel::<(BgaLayer, Arc<BgaDecodedImage>)>();
        let bga_cache_for_decode = bga_cache;
        let bga_decode_thread = thread::spawn(move || {
            loop {
                let Ok((mut layer, mut path)) = bga_decode_rx.recv() else {
                    break;
                };
                let mut latest: HashMap<BgaLayer, PathBuf> = HashMap::new();
                latest.insert(layer, path);
                loop {
                    match bga_decode_rx.try_recv() {
                        Ok((new_layer, new_path)) => {
                            layer = new_layer;
                            path = new_path;
                            latest.insert(layer, path.clone());
                        }
                        Err(mpsc::TryRecvError::Empty) => break,
                        Err(mpsc::TryRecvError::Disconnected) => return,
                    }
                }
                for (latest_layer, latest_path) in latest {
                    if let Some(decoded) =
                        bga::decode_and_cache(&bga_cache_for_decode, latest_layer, latest_path)
                    {
                        let _ = bga_decoded_tx.send((latest_layer, decoded));
                    }
                }
            }
        });

        Ok(Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            bind_group,
            screen_buffer,
            quad_vb,
            idx_buf,
            instance_buf,
            bga,
            bga_decode_tx: Some(bga_decode_tx),
            bga_decoded_rx,
            bga_decode_thread: Some(bga_decode_thread),
            index_count: 6,
            logical_size,
            poor_until: None,
        })
    }

    /// 非阻塞消费已完成的 BGA 解码结果，并将图片上传到 GPU
    fn drain_bga_decoded(&mut self) {
        loop {
            match self.bga_decoded_rx.try_recv() {
                Ok((layer, decoded)) => {
                    let _ = self.update_bga_image_from_rgba(
                        layer,
                        decoded.rgba.as_slice(),
                        decoded.width,
                        decoded.height,
                    );
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
        }
    }

    /// 请求解码指定图层的 BGA 图片（将自动合并重复请求）
    pub fn request_bga_decode(&self, layer: BgaLayer, path: PathBuf) {
        if let Some(tx) = &self.bga_decode_tx {
            let _ = tx.send((layer, path));
        }
    }

    /// 触发显示 POOR 图层（短暂显示后自动隐藏）
    pub fn trigger_poor(&mut self) {
        const POOR_VISIBLE_FOR: Duration = Duration::from_millis(200);
        self.bga.set_layer_visible(BgaLayer::Poor, true);
        self.poor_until = Instant::now().checked_add(POOR_VISIBLE_FOR);
    }

    /// 调整画布大小
    fn resize_surface(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
            self.logical_size = [width as f32, height as f32];
        }
    }

    /// 上传屏幕尺寸到统一缓冲
    fn upload_screen_uniform(&self) {
        let uni = ScreenUniform {
            size: self.logical_size,
        };
        self.queue
            .write_buffer(&self.screen_buffer, 0, bytemuck::bytes_of(&uni));
    }

    /// 绘制一帧可视实例
    pub fn draw(&mut self, instances: &[Instance]) -> Result<()> {
        if let Some(until) = self.poor_until
            && Instant::now() >= until
        {
            self.bga.set_layer_visible(BgaLayer::Poor, false);
            self.poor_until = None;
        }
        self.drain_bga_decoded();
        self.upload_screen_uniform();
        self.bga.prepare(&self.queue);
        let frame = self.surface.get_current_texture()?;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.queue
            .write_buffer(&self.instance_buf, 0, bytemuck::cast_slice(instances));
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("encoder"),
            });
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("render-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            rpass.set_pipeline(&self.pipeline);
            rpass.set_bind_group(0, &self.bind_group, &[]);
            rpass.set_vertex_buffer(0, self.quad_vb.slice(..));
            rpass.set_vertex_buffer(1, self.instance_buf.slice(..));
            rpass.set_index_buffer(self.idx_buf.slice(..), wgpu::IndexFormat::Uint16);
            rpass.draw_indexed(0..self.index_count, 0, 0..instances.len() as u32);
            self.bga
                .draw(&mut rpass, &self.quad_vb, &self.idx_buf, self.index_count);
        }
        self.queue.submit(Some(encoder.finish()));
        frame.present();
        Ok(())
    }

    /// 更新指定图层的 BGA 图片（RGBA8，sRGB）
    pub fn update_bga_image_from_rgba(
        &mut self,
        layer: BgaLayer,
        rgba: &[u8],
        w: u32,
        h: u32,
    ) -> Result<()> {
        self.bga.update_layer_image(
            layer,
            bga::UploadCtx {
                device: &self.device,
                queue: &self.queue,
                screen_buffer: &self.screen_buffer,
            },
            bga::RgbaImage {
                rgba,
                width: w,
                height: h,
            },
        )
    }
    /// 处理窗口尺寸变化
    pub fn resize(&mut self, width: u32, height: u32) {
        self.resize_surface(width, height);
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        let _ = self.bga_decode_tx.take();
        if let Some(handle) = self.bga_decode_thread.take() {
            let _ = handle.join();
        }
    }
}

/// 视觉区域高度（像素）
pub const VISIBLE_HEIGHT: f32 = 600.0;

/// 右侧BGA区域与轨道之间的间隔（像素）
pub const RIGHT_PANEL_GAP: f32 = 16.0;

/// 轨道数量
const LANE_COUNT: usize = 8;
/// 单个轨道宽度（像素）
const LANE_WIDTH: f32 = 60.0;
/// 轨道间距（像素）
const LANE_GAP: f32 = 8.0;
/// 音符高度（像素）
const NOTE_HEIGHT: f32 = 12.0;

/// 计算总宽度（含轨道与间隔）
#[must_use]
pub fn total_width() -> f32 {
    LANE_COUNT as f32 * LANE_WIDTH + (LANE_COUNT as f32 - 1.0) * LANE_GAP
}

/// 计算指定轨道的 x 坐标
fn lane_x(idx: usize) -> f32 {
    let left = -total_width() / 2.0 + LANE_WIDTH / 2.0;
    let offset = (RIGHT_PANEL_GAP + VISIBLE_HEIGHT) / 2.0;
    left + idx as f32 * (LANE_WIDTH + LANE_GAP) - offset
}
