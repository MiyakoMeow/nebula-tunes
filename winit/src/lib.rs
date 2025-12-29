//! # Nebula Tunes - winit 平台实现
//!
//! 提供 winit 窗口系统与事件循环的桌面平台实现

#![cfg(not(target_arch = "wasm32"))]

mod app;

use anyhow::Result;
use std::sync::{Arc, mpsc};
use winit::keyboard::KeyCode;

use nebula_tunes::loops::{ControlMsg, InputMsg, VisualMsg, visual};

/// 将配置文件中的按键代码字符串转换为 `winit::KeyCode`
fn parse_key_code(s: &str) -> Option<KeyCode> {
    // winit 的 KeyCode 实现了 FromStr，但需要检查是否支持
    // 这里我们使用 serde 的反序列化功能
    serde_json::from_str::<KeyCode>(format!("\"{}\"", s).as_str()).ok()
}

/// 运行 winit 事件循环并驱动渲染与输入分发
///
/// # 参数
///
/// - `key_codes`: 按键代码字符串列表（从配置文件读取）
/// - 其他参数与内部实现保持一致
///
/// # Errors
///
/// - winit 事件循环创建失败
/// - 按键代码字符串解析失败（仅警告，不会中断）
pub fn run(
    visual_rx: mpsc::Receiver<VisualMsg>,
    control_tx: mpsc::SyncSender<ControlMsg>,
    input_tx: mpsc::SyncSender<InputMsg>,
    key_codes: Vec<String>,
    bga_cache: Arc<visual::BgaDecodeCache>,
) -> Result<()> {
    // 将字符串转换为 winit::KeyCode
    let mut parsed_codes = Vec::new();
    for code_str in key_codes {
        match parse_key_code(&code_str) {
            Some(code) => parsed_codes.push(code),
            None => {
                tracing::warn!("无效的按键代码: {}", code_str);
                // 使用默认值或跳过
            }
        }
    }

    app::run_internal(visual_rx, control_tx, input_tx, parsed_codes, bga_cache)
}
