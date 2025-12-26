use anyhow::Result;
use std::path::Path;

pub struct BgaRenderer {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    bind_group: Option<wgpu::BindGroup>,
    instance_buf: wgpu::Buffer,
    active: bool,
}

impl BgaRenderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
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
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
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
        let instance_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bga-instance-buf"),
            size: (std::mem::size_of::<crate::Instance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            pipeline,
            bind_group_layout,
            bind_group: None,
            instance_buf,
            active: false,
        }
    }

    pub fn update_image_from_path(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen_buffer: &wgpu::Buffer,
        path: &Path,
    ) -> Result<()> {
        let bytes = std::fs::read(path)?;
        let img = image::load_from_memory(&bytes)?;
        let rgba = img.to_rgba8();
        let (w, h) = (rgba.width(), rgba.height());
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("bga-texture"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
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
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * w),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bga-bg"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: screen_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        self.bind_group = Some(bg);
        let side = super::VISIBLE_HEIGHT;
        let iw = w as f32;
        let ih = h as f32;
        let scale = if iw >= ih { side / iw } else { side / ih };
        let draw_w = iw * scale;
        let draw_h = ih * scale;
        let gw = super::total_width();
        let center_x = gw / 2.0 + super::RIGHT_PANEL_GAP / 2.0;
        let inst = crate::Instance {
            pos: [center_x, 0.0],
            size: [draw_w, draw_h],
            color: [1.0, 1.0, 1.0, 1.0],
        };
        queue.write_buffer(&self.instance_buf, 0, bytemuck::bytes_of(&inst));
        self.active = true;
        Ok(())
    }

    pub fn draw(
        &self,
        rpass: &mut wgpu::RenderPass<'_>,
        quad_vb: &wgpu::Buffer,
        idx_buf: &wgpu::Buffer,
        index_count: u32,
    ) {
        if self.active
            && let Some(bg) = &self.bind_group
        {
            rpass.set_pipeline(&self.pipeline);
            rpass.set_bind_group(0, bg, &[]);
            rpass.set_vertex_buffer(0, quad_vb.slice(..));
            rpass.set_vertex_buffer(1, self.instance_buf.slice(..));
            rpass.set_index_buffer(idx_buf.slice(..), wgpu::IndexFormat::Uint16);
            rpass.draw_indexed(0..index_count, 0, 0..1);
        }
    }
}
