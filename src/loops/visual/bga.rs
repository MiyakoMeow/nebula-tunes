//! BGA（背景动画）渲染模块
//!
//! - 负责 BGA 纹理的渲染和合成
//! - 根据屏幕尺寸居中缩放绘制单张图片
//! - 与主矩形渲染管线复用统一缓冲

use anyhow::Result;
use bytemuck::{Pod, Zeroable};

use crate::loops::BgaLayer;
use crate::media::ffmpeg::DecodedFrame;

/// 图片上传所需的 GPU 上下文
pub struct UploadCtx<'a> {
    /// 设备
    pub(crate) device: &'a wgpu::Device,
    /// 队列
    pub(crate) queue: &'a wgpu::Queue,
    /// 屏幕统一缓冲
    pub(crate) screen_buffer: &'a wgpu::Buffer,
}

/// RGBA8 sRGB 图片数据
pub struct RgbaImage<'a> {
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
        #[expect(clippy::cast_precision_loss)]
        let iw = img.width as f32;
        #[expect(clippy::cast_precision_loss)]
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
