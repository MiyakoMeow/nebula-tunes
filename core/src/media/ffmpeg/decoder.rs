//! 视频解码器抽象和实现
//!
//! 提供 `VideoDecoder` trait 和具体实现（FFmpeg、WASM 等）

use anyhow::Result;

/// 解码后的视频帧
#[derive(Debug, Clone)]
pub struct DecodedFrame {
    /// RGBA8 像素数据
    pub rgba: Vec<u8>,
    /// 宽度
    pub width: u32,
    /// 高度
    pub height: u32,
    /// 时间戳（秒）
    pub timestamp: f64,
    /// 帧索引
    pub frame_index: u64,
}

/// 视频解码器 trait
///
/// 所有视频解码器实现都需要实现此 trait
#[allow(dead_code)]
pub trait VideoDecoder {
    /// 解码下一帧
    ///
    /// 返回 Some(DecodedFrame) 表示成功解码一帧
    /// 返回 None 表示已到达文件末尾
    ///
    /// # Errors
    ///
    /// 如果解码失败，返回错误。
    fn decode_next_frame(&mut self) -> Result<Option<DecodedFrame>>;

    /// 跳转到指定帧
    ///
    /// # Errors
    ///
    /// 如果跳转失败，返回错误。
    #[allow(dead_code)]
    fn seek_to_frame(&mut self, frame_idx: u64) -> Result<()>;

    /// 获取当前帧索引
    #[allow(dead_code)]
    fn current_frame_index(&self) -> u64;

    /// 获取视频宽度
    #[allow(dead_code)]
    fn width(&self) -> u32;

    /// 获取视频高度
    #[allow(dead_code)]
    fn height(&self) -> u32;

    /// 获取帧率（FPS）
    #[allow(dead_code)]
    fn fps(&self) -> f64;
}
