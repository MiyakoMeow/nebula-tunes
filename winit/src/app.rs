//! winit 窗口与事件循环入口

use std::sync::{Arc, mpsc};

use anyhow::Result;
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, MouseScrollDelta, Touch, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::PhysicalKey,
    window::WindowId,
};

use nebula_tunes::entry::VisualApp;
use nebula_tunes::loops::{
    ControlMsg, KeyState, MouseButton, RawInputMsg, RawKeyCode, TouchPhase, VisualMsg, visual,
};

/// 将 winit `KeyCode` 转换为配置文件格式的字符串
fn key_code_to_string(code: winit::keyboard::KeyCode) -> String {
    serde_json::to_string(&code)
        .map(|s| s.trim_matches('"').to_string())
        .unwrap_or_else(|_| format!("{:?}", code))
}

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
    /// 原始输入消息发送端
    raw_input_tx: mpsc::SyncSender<RawInputMsg>,
    /// BGA 解码缓存（用于创建渲染器并复用预加载结果）
    bga_cache: Arc<visual::BgaDecodeCache>,
    /// 光标位置缓存
    cursor_position: (f64, f64),
}

impl Handler {
    /// 创建视觉事件处理器
    const fn new(
        visual_rx: mpsc::Receiver<VisualMsg>,
        control_tx: mpsc::SyncSender<ControlMsg>,
        raw_input_tx: mpsc::SyncSender<RawInputMsg>,
        bga_cache: Arc<visual::BgaDecodeCache>,
    ) -> Self {
        Self {
            app: None,
            visual_rx: Some(visual_rx),
            control_tx,
            raw_input_tx,
            bga_cache,
            cursor_position: (0.0, 0.0),
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

        let size = window.inner_size();
        let gpu_ctx = match visual::init_gpu(&window, (size.width, size.height)) {
            Ok(ctx) => ctx,
            Err(e) => {
                tracing::error!("GPU initialization failed: {:?}", e);
                return;
            }
        };

        let rx = match self.visual_rx.take() {
            Some(r) => r,
            None => return,
        };

        let renderer = match visual::Renderer::new(gpu_ctx, self.bga_cache.clone()) {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("Renderer creation failed: {:?}", e);
                return;
            }
        };

        self.app = Some(App {
            window,
            app: VisualApp::new(renderer, rx, self.control_tx.clone()),
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
                if let PhysicalKey::Code(code) = event.physical_key {
                    let key_str = key_code_to_string(code);

                    let state = match event.state {
                        ElementState::Pressed => KeyState::Pressed,
                        ElementState::Released => KeyState::Released,
                    };

                    let raw_msg = RawInputMsg::Key {
                        code: RawKeyCode(key_str),
                        state,
                    };

                    let _ = self.raw_input_tx.try_send(raw_msg);
                }
            }
            // 鼠标移动事件
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_position = (position.x, position.y);
                let raw_msg = RawInputMsg::Mouse {
                    button: None,
                    state: KeyState::Pressed,
                    position: (position.x, position.y),
                    delta: None,
                };
                let _ = self.raw_input_tx.try_send(raw_msg);
            }
            // 鼠标按钮事件
            WindowEvent::MouseInput { state, button, .. } => {
                let key_state = match state {
                    ElementState::Pressed => KeyState::Pressed,
                    ElementState::Released => KeyState::Released,
                };
                let mouse_button = match button {
                    winit::event::MouseButton::Left => MouseButton::Left,
                    winit::event::MouseButton::Right => MouseButton::Right,
                    winit::event::MouseButton::Middle => MouseButton::Middle,
                    winit::event::MouseButton::Other(code) => MouseButton::Other(code),
                    winit::event::MouseButton::Back => MouseButton::Other(3),
                    winit::event::MouseButton::Forward => MouseButton::Other(4),
                };
                let raw_msg = RawInputMsg::Mouse {
                    button: Some(mouse_button),
                    state: key_state,
                    position: self.cursor_position,
                    delta: None,
                };
                let _ = self.raw_input_tx.try_send(raw_msg);
            }
            // 鼠标滚轮事件
            WindowEvent::MouseWheel { delta, .. } => {
                let (dx, dy) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (x, y),
                    MouseScrollDelta::PixelDelta(pos) => (pos.x as f32, pos.y as f32),
                };
                let raw_msg = RawInputMsg::Mouse {
                    button: None,
                    state: KeyState::Pressed,
                    position: self.cursor_position,
                    delta: Some((dx, dy)),
                };
                let _ = self.raw_input_tx.try_send(raw_msg);
            }
            // 触控输入事件
            WindowEvent::Touch(Touch {
                id,
                location,
                phase,
                ..
            }) => {
                let touch_phase = match phase {
                    winit::event::TouchPhase::Started => TouchPhase::Started,
                    winit::event::TouchPhase::Moved => TouchPhase::Moved,
                    winit::event::TouchPhase::Ended => TouchPhase::Ended,
                    winit::event::TouchPhase::Cancelled => TouchPhase::Cancelled,
                };
                let raw_msg = RawInputMsg::Touch {
                    id,
                    position: (location.x, location.y),
                    phase: touch_phase,
                };
                let _ = self.raw_input_tx.try_send(raw_msg);
            }
            // 游戏手柄连接事件（winit 0.30 暂不支持 WindowEvent::Gamepad）
            // TODO: 添加手柄轮询逻辑或监听连接状态变化
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
    raw_input_tx: mpsc::SyncSender<RawInputMsg>,
    bga_cache: Arc<visual::BgaDecodeCache>,
) -> Result<()> {
    let event_loop = EventLoop::new()?;
    let mut handler = Handler::new(visual_rx, control_tx, raw_input_tx, bga_cache);
    event_loop.run_app(&mut handler)?;
    Ok(())
}
