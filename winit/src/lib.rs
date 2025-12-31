//! # Nebula Tunes - winit 平台实现
//!
//! 提供 winit 窗口系统与事件循环的桌面平台实现
//!
//! ## 架构说明
//!
//! 此 crate 是纯平台层，负责：
//!
//! - **窗口创建与管理**：使用 winit 创建应用窗口
//! - **事件循环**：运行主事件循环，分发窗口事件
//! - **输入收集**：收集键盘、鼠标、触控、手柄等原始输入
//!
//! GPU 渲染相关的逻辑已移至 `nebula_tunes::loops::visual` 模块，保持平台无关性。
//!
//! ## 支持的输入类型
//!
//! - 键盘输入（物理按键）
//! - 鼠标输入（移动、点击、滚轮）
//! - 触控输入（多点触控）
//! - 游戏手柄（连接状态）

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
/// - `raw_input_tx`: 原始输入消息发送端（键盘、鼠标、触控、手柄）
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
