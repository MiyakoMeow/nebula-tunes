//! 视觉循环：事件线程上的渲染
//!
//! - 在 `Resumed` 创建窗口与渲染器，发送启动信号
//! - 在 `RedrawRequested` 非阻塞接收最新帧并渲染
//! - 在 `about_to_wait` 请求重绘以维持刷新

use std::collections::HashMap;
use tokio::sync::mpsc;

use winit::{
    application::ApplicationHandler, dpi::LogicalSize, event::WindowEvent,
    event_loop::ActiveEventLoop, window::WindowId,
};

use crate::Instance;
use crate::loops::{ControlMsg, InputMsg};

use crate::key_to_lane;
use anyhow::Result;
use bms_rs::chart_process::prelude::*;
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;
use winit::event::ElementState;
use winit::keyboard::{KeyCode, PhysicalKey};

/// 视觉应用状态
pub struct App {
    renderer: Renderer,
    visual_rx: mpsc::Receiver<Vec<Instance>>,
    latest_instances: Vec<Instance>,
}

/// 视觉事件处理器
pub struct Handler {
    pub app: Option<App>,
    pub visual_rx: Option<mpsc::Receiver<Vec<Instance>>>,
    pub control_tx: mpsc::Sender<ControlMsg>,
    pub input_tx: mpsc::Sender<InputMsg>,
    key_map: HashMap<KeyCode, usize>,
}

impl Handler {
    pub fn new(
        visual_rx: mpsc::Receiver<Vec<Instance>>,
        control_tx: mpsc::Sender<ControlMsg>,
        input_tx: mpsc::Sender<InputMsg>,
        key_codes: Vec<KeyCode>,
    ) -> Self {
        let mut map = HashMap::new();
        for (i, code) in key_codes.into_iter().enumerate().take(8) {
            map.insert(code, i);
        }
        Self {
            app: None,
            visual_rx: Some(visual_rx),
            control_tx,
            input_tx,
            key_map: map,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Zeroable, Pod)]
struct ScreenUniform {
    size: [f32; 2],
}

/// 简易矩形渲染器，负责上传实例并绘制
pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    screen_buffer: wgpu::Buffer,
    quad_vb: wgpu::Buffer,
    idx_buf: wgpu::Buffer,
    instance_buf: wgpu::Buffer,
    index_count: u32,
    pub(crate) window: winit::window::Window,
    logical_size: [f32; 2],
}

impl Renderer {
    async fn new(window: winit::window::Window) -> Result<Self> {
        let instance = wgpu::Instance::default();
        let surface = unsafe {
            instance.create_surface_unsafe(
                wgpu::SurfaceTargetUnsafe::from_window(&window)
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?,
            )
        }?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .await
            .map_err(|e| anyhow::anyhow!("request_adapter failed: {:?}", e))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
                label: None,
            })
            .await?;
        let size = window.inner_size();
        let format = surface.get_capabilities(&adapter).formats[0];
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::FifoRelaxed,
            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
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
        let logical_size = [size.width as f32, size.height as f32];
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
            index_count: 6,
            window,
            logical_size,
        })
    }

    /// 调整画布大小
    fn resize(&mut self, width: u32, height: u32) {
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
    fn draw(&self, instances: &[Instance]) -> Result<()> {
        self.upload_screen_uniform();
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
        }
        self.queue.submit(Some(encoder.finish()));
        frame.present();
        Ok(())
    }
}

/// 视觉区域高度（像素）
pub const VISIBLE_HEIGHT: f32 = 600.0;

const LANE_COUNT: usize = 8;
const LANE_WIDTH: f32 = 60.0;
const LANE_GAP: f32 = 8.0;
const NOTE_HEIGHT: f32 = 12.0;

/// 计算总宽度（含轨道与间隔）
#[must_use]
pub fn total_width() -> f32 {
    LANE_COUNT as f32 * LANE_WIDTH + (LANE_COUNT as f32 - 1.0) * LANE_GAP
}

fn lane_x(idx: usize) -> f32 {
    let left = -total_width() / 2.0 + LANE_WIDTH / 2.0;
    left + idx as f32 * (LANE_WIDTH + LANE_GAP)
}

/// 构建基础可视实例（车道与判定线）
#[must_use]
pub fn base_instances() -> Vec<Instance> {
    let mut instances: Vec<Instance> = Vec::with_capacity(1024);
    for i in 0..LANE_COUNT {
        instances.push(Instance {
            pos: [lane_x(i), 0.0],
            size: [LANE_WIDTH, VISIBLE_HEIGHT],
            color: [0.15, 0.15, 0.18, 1.0],
        });
    }
    instances.push(Instance {
        pos: [0.0, -VISIBLE_HEIGHT / 2.0 + 2.0],
        size: [total_width(), 4.0],
        color: [0.9, 0.9, 0.9, 1.0],
    });
    instances
}

pub fn build_instances_for_processor_with_state(
    p: &mut BmsProcessor,
    pressed: &[bool; 8],
    gauge: f32,
) -> Vec<Instance> {
    fn lane_color(idx: usize) -> [f32; 4] {
        const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
        const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
        const BLUE: [f32; 4] = [0.2, 0.6, 1.0, 1.0];
        match idx % 8 {
            0 => RED,
            1 => WHITE,
            2 => BLUE,
            3 => WHITE,
            4 => BLUE,
            5 => WHITE,
            6 => BLUE,
            _ => WHITE,
        }
    }
    let mut instances = base_instances();
    if p.started_at().is_some() {
        for (ev, ratio) in p.visible_events() {
            let ChartEvent::Note { side, key, .. } = ev.event() else {
                continue;
            };
            if *side != PlayerSide::Player1 {
                continue;
            }
            let Some(idx) = key_to_lane(*key) else {
                continue;
            };
            let x = lane_x(idx);
            let r = (ratio.as_f64() as f32).clamp(0.0, 1.0);
            let y = -VISIBLE_HEIGHT / 2.0 + r * VISIBLE_HEIGHT;
            instances.push(Instance {
                pos: [x, y],
                size: [LANE_WIDTH - 4.0, NOTE_HEIGHT],
                color: lane_color(idx),
            });
        }
    }
    for (i, pressed_flag) in pressed.iter().enumerate() {
        if *pressed_flag {
            instances.push(Instance {
                pos: [lane_x(i), -VISIBLE_HEIGHT / 2.0 + 24.0],
                size: [LANE_WIDTH - 8.0, 24.0],
                color: [1.0, 1.0, 1.0, 0.25],
            });
        }
    }
    let gw = total_width();
    let gy = VISIBLE_HEIGHT / 2.0 - 20.0;
    instances.push(Instance {
        pos: [0.0, gy],
        size: [gw, 8.0],
        color: [0.3, 0.3, 0.35, 1.0],
    });
    instances.push(Instance {
        pos: [-gw / 2.0 + (gw * gauge) / 2.0, gy],
        size: [gw * gauge, 8.0],
        color: [0.2, 0.8, 0.4, 1.0],
    });
    instances
}
impl ApplicationHandler for Handler {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attrs = winit::window::Window::default_attributes()
            .with_title("Nebula Tunes")
            .with_inner_size(LogicalSize::new(
                total_width() as f64 + 64.0,
                VISIBLE_HEIGHT as f64 + 64.0,
            ));
        let window = match event_loop.create_window(attrs) {
            Ok(w) => w,
            Err(_) => return,
        };
        let renderer = match pollster::block_on(Renderer::new(window)) {
            Ok(r) => r,
            Err(_) => return,
        };
        let rx = match self.visual_rx.take() {
            Some(r) => r,
            None => return,
        };
        self.app = Some(App {
            renderer,
            visual_rx: rx,
            latest_instances: base_instances(),
        });
        let _ = self.control_tx.try_send(ControlMsg::Start);
    }
    fn window_event(&mut self, _event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {}
            WindowEvent::Resized(size) => {
                if let Some(app) = self.app.as_mut() {
                    app.renderer.resize(size.width, size.height);
                    app.renderer.window.request_redraw();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let lane = match event.physical_key {
                    PhysicalKey::Code(code) => self.key_map.get(&code).copied(),
                    _ => None,
                };
                if let Some(idx) = lane {
                    match event.state {
                        ElementState::Pressed => {
                            let _ = self.input_tx.try_send(InputMsg::KeyDown(idx));
                        }
                        ElementState::Released => {
                            let _ = self.input_tx.try_send(InputMsg::KeyUp(idx));
                        }
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(app) = self.app.as_mut() {
                    loop {
                        match app.visual_rx.try_recv() {
                            Ok(instances) => {
                                app.latest_instances = instances;
                            }
                            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => break,
                        }
                    }
                    let _ = app.renderer.draw(&app.latest_instances);
                }
            }
            _ => {}
        }
    }
    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(app) = self.app.as_mut() {
            app.renderer.window.request_redraw();
        }
    }
}
