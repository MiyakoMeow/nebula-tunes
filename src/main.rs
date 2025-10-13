//! # Nebula Tunes 主程序
//!
//! 这是一个基于wgpu和winit的简单图形应用程序，展示了一个带有动画效果的彩色长方形。
#![warn(missing_docs)]
#![warn(clippy::must_use_candidate)]
#![warn(clippy::must_use_unit)]

mod graphics;

use std::sync::Arc;

use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId},
};

use self::graphics::State;

/// 应用程序结构体
///
/// 包含了应用程序的主要状态信息，使用Option包装State以支持延迟初始化。
#[derive(Default)]
struct App {
    state: Option<State>,
}

/// 为App实现ApplicationHandler trait
///
/// 这个trait定义了应用程序如何响应不同的事件循环事件。
impl ApplicationHandler for App {
    /// 当应用程序恢复时调用
    ///
    /// 这个方法在应用程序启动或从后台恢复时调用，用于初始化窗口和渲染状态。
    ///
    /// # 参数
    /// * `event_loop` - 活动事件循环引用
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // 创建窗口对象
        let window = Arc::new(
            event_loop
                .create_window(Window::default_attributes())
                .unwrap(),
        );

        // 异步创建渲染状态，使用pollster阻塞等待完成
        let state = pollster::block_on(State::new(window.clone()));
        self.state = Some(state);

        // 请求重绘以开始渲染循环
        window.request_redraw();
    }

    /// 处理窗口事件
    ///
    /// 这个方法处理所有窗口相关的事件，包括关闭、调整大小和重绘请求。
    ///
    /// # 参数
    /// * `event_loop` - 活动事件循环引用
    /// * `_id` - 窗口ID（未使用）
    /// * `event` - 窗口事件
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let state = self.state.as_mut().expect("状态未初始化");
        match event {
            WindowEvent::CloseRequested => {
                println!("关闭按钮被按下，正在停止应用程序");
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                // 执行渲染
                state.render();
                // 发出新的重绘请求事件，创建连续的渲染循环
                state.get_window().request_redraw();
            }
            WindowEvent::Resized(size) => {
                // 重新配置表面尺寸。我们不在此处重新渲染，
                // 因为这个事件总是会跟随一个重绘请求。
                state.resize(size);
            }
            _ => (), // 忽略其他事件
        }
    }
}

/// 应用程序入口点
///
/// 这个函数设置并启动整个应用程序的事件循环。
fn main() {
    // wgpu使用`log`进行所有日志记录，因此我们使用`env_logger`crate初始化日志器。
    //
    // 要更改日志级别，请设置`RUST_LOG`环境变量。更多信息请参阅`env_logger`文档。
    env_logger::init();

    // 创建事件循环
    let event_loop = EventLoop::new().unwrap();

    // 当当前循环迭代完成时，无论是否有新事件可用，都立即开始新的迭代。
    // 对于希望以最快速度渲染的应用程序（如游戏）是首选。
    event_loop.set_control_flow(ControlFlow::Poll);

    // 当当前循环迭代完成时，挂起线程直到另一个事件到达。
    // 有助于在没有事情发生时保持CPU利用率低，这在应用程序可能在后台空闲时是首选。
    // event_loop.set_control_flow(ControlFlow::Wait);

    // 创建应用程序实例
    let mut app = App::default();

    // 运行应用程序事件循环
    event_loop.run_app(&mut app).unwrap();
}
