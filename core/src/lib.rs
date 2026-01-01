//! Nebula Tunes library target.

pub mod chart;
pub mod config;
pub mod entry;
pub mod filesystem;
pub mod game_page;
pub mod logging;
pub mod loops;
pub mod media;
pub mod pages;
pub mod title_page;

// 导出常用类型
pub use game_page::JudgeParams;

use bms_rs::bms::prelude::Key;
use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Clone, Copy, Zeroable, Pod, PartialEq)]
/// 单个矩形实例（位置、大小、颜色）
pub struct Instance {
    /// 中心坐标（x, y）
    pub pos: [f32; 2],
    /// 尺寸（宽, 高）
    pub size: [f32; 2],
    /// 颜色（RGBA）
    pub color: [f32; 4],
}

// 手动实现 Hash，因为 f32 不支持 Hash
impl std::hash::Hash for Instance {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // 使用 bytemuck 将实例转换为字节进行哈希
        let bytes: &[u8] = bytemuck::bytes_of(self);
        state.write(bytes);
    }
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
