//! 事件循环模块入口
//!
//! 提供三个子模块：
//! - `audio`：音频播放循环
//! - `main_loop`：节拍推进与事件分发循环
//! - `visual`：事件线程上的渲染循环

pub mod audio;
pub mod main_loop;
pub mod visual;

use std::path::PathBuf;

/// 控制主循环启动的消息
pub enum ControlMsg {
    /// 触发主循环开始
    Start,
}

/// 输入事件消息
pub enum InputMsg {
    /// 某轨道按键按下（索引）
    KeyDown(usize),
    /// 某轨道按键抬起（索引）
    KeyUp(usize),
}

/// 视觉循环消息
pub enum VisualMsg {
    /// 更新实例列表
    Instances(Vec<crate::Instance>),
    /// 切换BGA图片
    Bga(PathBuf),
}
