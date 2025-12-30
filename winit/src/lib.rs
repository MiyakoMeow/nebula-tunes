//! # Nebula Tunes - winit 平台实现
//!
//! 提供 winit 窗口系统与事件循环的桌面平台实现

mod app;

use anyhow::Result;
use std::sync::{Arc, mpsc};

use nebula_tunes::loops::{ControlMsg, RawInputMsg, VisualMsg, visual};

/// 运行 winit 事件循环并驱动渲染与输入分发
///
/// # 参数
///
/// - `visual_rx`: 视觉消息接收端
/// - `control_tx`: 控制消息发送端
/// - `raw_input_tx`: 原始输入消息发送端
/// - `bga_cache`: BGA 解码缓存
///
/// # Errors
///
/// - winit 事件循环创建失败
pub fn run(
    visual_rx: mpsc::Receiver<VisualMsg>,
    control_tx: mpsc::SyncSender<ControlMsg>,
    raw_input_tx: mpsc::SyncSender<RawInputMsg>,
    bga_cache: Arc<visual::BgaDecodeCache>,
) -> Result<()> {
    app::run_internal(visual_rx, control_tx, raw_input_tx, bga_cache)
}
