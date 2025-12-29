//! winit 窗口与事件循环入口

#![cfg(not(target_arch = "wasm32"))]
use std::{
    collections::HashMap,
    sync::{Arc, mpsc},
};

use anyhow::Result;
use futures_lite::future;
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::WindowId,
};

use nebula_tunes::entry::VisualApp;
use nebula_tunes::loops::{ControlMsg, InputMsg, VisualMsg, visual};

/// 视觉应用状态
struct App {
    /// 窗口实例
    window: winit::window::Window,
    /// 视觉渲染应用
    app: VisualApp,
}

/// 视觉事件处理器
struct Handler {
    /// 可选的视觉应用状态
    app: Option<App>,
    /// 视觉消息接收端
    visual_rx: Option<mpsc::Receiver<VisualMsg>>,
    /// 控制消息发送端
    control_tx: mpsc::SyncSender<ControlMsg>,
    /// 输入消息发送端
    input_tx: mpsc::SyncSender<InputMsg>,
    /// 键位到轨道索引映射
    key_map: HashMap<KeyCode, usize>,
    /// BGA 解码缓存（用于创建渲染器并复用预加载结果）
    bga_cache: Arc<visual::BgaDecodeCache>,
}

impl Handler {
    /// 创建视觉事件处理器并建立键位映射
    fn new(
        visual_rx: mpsc::Receiver<VisualMsg>,
        control_tx: mpsc::SyncSender<ControlMsg>,
        input_tx: mpsc::SyncSender<InputMsg>,
        key_codes: Vec<KeyCode>,
        bga_cache: Arc<visual::BgaDecodeCache>,
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
            bga_cache,
        }
    }
}

impl ApplicationHandler for Handler {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attrs = winit::window::Window::default_attributes()
            .with_title("Nebula Tunes")
            .with_inner_size(LogicalSize::new(
                (visual::total_width() + visual::RIGHT_PANEL_GAP + visual::VISIBLE_HEIGHT) as f64
                    + 64.0,
                visual::VISIBLE_HEIGHT as f64 + 64.0,
            ));
        let window = match event_loop.create_window(attrs) {
            Ok(w) => w,
            Err(_) => return,
        };

        let renderer = match (|| -> Result<visual::Renderer> {
            let instance = wgpu::Instance::default();
            let surface = unsafe {
                instance.create_surface_unsafe(
                    wgpu::SurfaceTargetUnsafe::from_window(&window)
                        .map_err(|e| anyhow::anyhow!(e.to_string()))?,
                )
            }?;
            let adapter =
                future::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    force_fallback_adapter: false,
                    compatible_surface: Some(&surface),
                }))
                .map_err(|e| anyhow::anyhow!("request_adapter failed: {:?}", e))?;
            let (device, queue) =
                future::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    experimental_features: wgpu::ExperimentalFeatures::disabled(),
                    memory_hints: wgpu::MemoryHints::default(),
                    trace: wgpu::Trace::Off,
                    label: None,
                }))?;
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
            visual::Renderer::new(surface, device, queue, config, self.bga_cache.clone())
        })() {
            Ok(r) => r,
            Err(_) => return,
        };

        let rx = match self.visual_rx.take() {
            Some(r) => r,
            None => return,
        };
        self.app = Some(App {
            window,
            app: VisualApp::new(renderer, rx),
        });
        let _ = self.control_tx.try_send(ControlMsg::Start);
    }

    fn window_event(&mut self, _event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::Resized(size) => {
                if let Some(app) = self.app.as_mut() {
                    app.app.resize(size.width, size.height);
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
                    app.app.redraw();
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

/// 运行 winit 事件循环并驱动渲染与输入分发（内部实现）
pub fn run_internal(
    visual_rx: mpsc::Receiver<VisualMsg>,
    control_tx: mpsc::SyncSender<ControlMsg>,
    input_tx: mpsc::SyncSender<InputMsg>,
    key_codes: Vec<KeyCode>,
    bga_cache: Arc<visual::BgaDecodeCache>,
) -> Result<()> {
    let event_loop = EventLoop::new()?;
    let mut handler = Handler::new(visual_rx, control_tx, input_tx, key_codes, bga_cache);
    event_loop.run_app(&mut handler)?;
    Ok(())
}
