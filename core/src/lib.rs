//! Nebula Tunes library target used for WASM compilation checks.

pub mod chart;
pub mod config;
pub mod entry;
pub mod filesystem;
pub mod logging;
pub mod loops;
pub mod media;

use bms_rs::bms::prelude::Key;
use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Clone, Copy, Zeroable, Pod)]
/// 单个矩形实例（位置、大小、颜色）
pub struct Instance {
    /// 中心坐标（x, y）
    pos: [f32; 2],
    /// 尺寸（宽, 高）
    size: [f32; 2],
    /// 颜色（RGBA）
    color: [f32; 4],
}

/// 将按键映射到轨道索引
pub(crate) const fn key_to_lane(key: Key) -> Option<usize> {
    match key {
        Key::Scratch(_) => Some(0),
        Key::Key(n) => match n {
            1..=7 => Some(n as usize),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(target_os = "wasi")]
/// WASM 构建冒烟检查入口
///
/// # Errors
///
/// - `getrandom` 获取随机数失败
pub fn wasm_smoke_checks() -> Result<(), getrandom::Error> {
    let _ = bms_rs::bms::default_config();
    let mut buf = [0u8; 16];
    getrandom::fill(&mut buf)?;
    Ok(())
}
