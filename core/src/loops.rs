//! 事件循环模块入口
//!
//! 提供四个子模块：
//! - `audio`：音频播放循环
//! - `key_map`：按键映射模块
//! - `main_loop`：节拍推进与事件分发循环
//! - `visual`：事件线程上的渲染循环

pub mod audio;
pub mod key_map;
pub mod main_loop;
pub mod visual;

use std::path::PathBuf;

/// 控制主循环启动的消息
pub enum ControlMsg {
    /// 触发主循环开始
    Start,
}

/// 原始按键代码（平台无关表示）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawKeyCode(pub String);

/// 原始按键状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyState {
    /// 按键按下
    Pressed,
    /// 按键释放
    Released,
}

/// 原始输入消息（从 winit 传递到 core）
pub enum RawInputMsg {
    /// 键盘输入事件
    Key {
        /// 按键代码
        code: RawKeyCode,
        /// 按键状态
        state: KeyState,
    },
}

/// 输入事件消息
pub enum InputMsg {
    /// 某轨道按键按下（索引）
    KeyDown(usize),
    /// 某轨道按键抬起（索引）
    KeyUp(usize),
}

/// BGA 图层类型
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BgaLayer {
    /// BGA 主图层
    Bga,
    /// LAYER 叠加图层
    Layer,
    /// LAYER2 叠加图层
    Layer2,
    /// POOR 图层（默认隐藏，通过事件触发）
    Poor,
}

/// 视觉循环消息
pub enum VisualMsg {
    /// 更新实例列表
    Instances(Vec<crate::Instance>),
    /// 切换指定图层的 BGA 图片
    BgaChange {
        /// 目标图层
        layer: BgaLayer,
        /// 图片路径
        path: PathBuf,
    },
    /// 触发显示 POOR 图层
    BgaPoorTrigger,
    /// 播放视频 BGA
    VideoPlay {
        /// 目标图层
        layer: BgaLayer,
        /// 视频路径
        path: PathBuf,
        /// 是否循环播放
        loop_play: bool,
    },
    /// 更新视频帧
    #[allow(dead_code)]
    VideoFrame {
        /// 目标图层
        layer: BgaLayer,
        /// 解码后的帧数据
        frame: crate::loops::visual::DecodedFrame,
    },
    /// 停止视频播放
    #[allow(dead_code)]
    VideoStop {
        /// 目标图层
        layer: BgaLayer,
    },
    /// 跳转到指定时间戳
    #[allow(dead_code)]
    VideoSeek {
        /// 目标图层
        layer: BgaLayer,
        /// 时间戳（秒）
        timestamp: f64,
    },
}
