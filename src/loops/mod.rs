//! 事件循环模块入口
//!
//! 提供三个子模块：
//! - `audio`：音频播放循环
//! - `main_loop`：节拍推进与事件分发循环
//! - `visual`：事件线程上的渲染循环

pub mod audio;
pub mod main_loop;
pub mod visual;

/// 控制主循环启动的消息
pub enum ControlMsg {
    Start,
}

/// 输入事件消息
pub enum InputMsg {
    KeyDown(usize),
    KeyUp(usize),
}
