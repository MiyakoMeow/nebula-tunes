//! 视觉循环：事件线程上的渲染
//!
//! - 在 `Resumed` 创建窗口与渲染器，发送启动信号
//! - 在 `RedrawRequested` 非阻塞接收最新帧并渲染
//! - 在 `about_to_wait` 请求重绘以维持刷新

/// BGA 渲染子模块
mod bga;
/// 音符与轨道实例构建
mod note;
pub use note::{base_instances, build_instances_for_processor_with_state};
use std::{collections::HashMap, path::Path};
use tokio::sync::mpsc;

use winit::{
    application::ApplicationHandler, dpi::LogicalSize, event::WindowEvent,
    event_loop::ActiveEventLoop, window::WindowId,
};

use crate::Instance;
use crate::loops::{ControlMsg, InputMsg, VisualMsg};

use anyhow::Result;
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;
use winit::event::ElementState;
use winit::keyboard::{KeyCode, PhysicalKey};

/// 视觉应用状态
pub struct App {
    /// 窗口实例
    window: winit::window::Window,
    /// 渲染器实例
    renderer: Option<Box<dyn RenderBackend>>,
    /// 视觉消息接收端
    visual_rx: mpsc::Receiver<VisualMsg>,
    /// 最新一帧的实例列表
    latest_instances: Vec<Instance>,
}

impl Drop for App {
    fn drop(&mut self) {
        let _ = self.renderer.take();
    }
}

/// 视觉事件处理器
pub struct Handler {
    /// 可选的视觉应用状态
    pub app: Option<App>,
    /// 视觉消息接收端
    pub visual_rx: Option<mpsc::Receiver<VisualMsg>>,
    /// 控制消息发送端
    pub control_tx: mpsc::Sender<ControlMsg>,
    /// 输入消息发送端
    pub input_tx: mpsc::Sender<InputMsg>,
    /// 键位到轨道索引映射
    key_map: HashMap<KeyCode, usize>,
}

impl Handler {
    /// 创建视觉事件处理器并建立键位映射
    pub fn new(
        visual_rx: mpsc::Receiver<VisualMsg>,
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
/// 屏幕统一参数
struct ScreenUniform {
    /// 屏幕尺寸（宽, 高）
    size: [f32; 2],
}

/// 渲染后端接口
pub trait RenderBackend {
    /// 处理窗口尺寸变化
    fn resize(&mut self, width: u32, height: u32);
    /// 绘制一帧
    fn draw(&self, instances: &[Instance]) -> Result<()>;
    /// 根据给定路径加载并更新 BGA 图片
    fn update_bga_image_from_path(&mut self, path: &Path) -> Result<()>;
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
    /// 三角形索引数量
    index_count: u32,
    /// 逻辑屏幕尺寸
    logical_size: [f32; 2],
}

impl Renderer {
    /// 创建渲染器并初始化管线、缓冲与资源
    async fn new(window: &winit::window::Window) -> Result<Self> {
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
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .first()
            .copied()
            .unwrap_or(wgpu::TextureFormat::Bgra8Unorm);
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
        let bga = bga::BgaRenderer::new(&device, format);
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
            bga,
            index_count: 6,
            logical_size,
        })
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
    fn draw_frame(&self, instances: &[Instance]) -> Result<()> {
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
            self.bga
                .draw(&mut rpass, &self.quad_vb, &self.idx_buf, self.index_count);
        }
        self.queue.submit(Some(encoder.finish()));
        frame.present();
        Ok(())
    }

    /// 根据给定路径加载并更新 BGA 图片
    fn update_bga_from_path(&mut self, path: &Path) -> Result<()> {
        self.bga
            .update_image_from_path(&self.device, &self.queue, &self.screen_buffer, path)
    }
}

impl RenderBackend for Renderer {
    fn resize(&mut self, width: u32, height: u32) {
        self.resize_surface(width, height);
    }

    fn draw(&self, instances: &[Instance]) -> Result<()> {
        self.draw_frame(instances)
    }

    fn update_bga_image_from_path(&mut self, path: &Path) -> Result<()> {
        self.update_bga_from_path(path)
    }
}

/// 视觉区域高度（像素）
pub const VISIBLE_HEIGHT: f32 = 600.0;

/// 右侧BGA区域与轨道之间的间隔（像素）
const RIGHT_PANEL_GAP: f32 = 16.0;

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

impl ApplicationHandler for Handler {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attrs = winit::window::Window::default_attributes()
            .with_title("Nebula Tunes")
            .with_inner_size(LogicalSize::new(
                (total_width() + RIGHT_PANEL_GAP + VISIBLE_HEIGHT) as f64 + 64.0,
                VISIBLE_HEIGHT as f64 + 64.0,
            ));
        let window = match event_loop.create_window(attrs) {
            Ok(w) => w,
            Err(_) => return,
        };
        let renderer = match futures_lite::future::block_on(Renderer::new(&window)) {
            Ok(r) => r,
            Err(_) => return,
        };
        let rx = match self.visual_rx.take() {
            Some(r) => r,
            None => return,
        };
        self.app = Some(App {
            window,
            renderer: Some(Box::new(renderer)),
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
                    let Some(renderer) = app.renderer.as_mut() else {
                        return;
                    };
                    renderer.resize(size.width, size.height);
                    app.window.request_redraw();
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
                    let Some(renderer) = app.renderer.as_mut() else {
                        return;
                    };
                    loop {
                        match app.visual_rx.try_recv() {
                            Ok(msg) => match msg {
                                VisualMsg::Instances(instances) => {
                                    app.latest_instances = instances;
                                }
                                VisualMsg::Bga(path) => {
                                    let _ = renderer.update_bga_image_from_path(&path);
                                }
                            },
                            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => break,
                        }
                    }
                    let _ = renderer.draw(&app.latest_instances);
                }
            }
            _ => {}
        }
    }
    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(app) = self.app.as_mut() {
            app.window.request_redraw();
        }
    }
}
