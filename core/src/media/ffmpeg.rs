//! 视频解码和渲染模块
//!
//! 提供视频 BGA 支持

mod decoder;
mod frame_queue;
mod texture_manager;

mod ffmpeg_impl;

pub use decoder::{DecodedFrame, VideoDecoder};
pub use ffmpeg_impl::FFmpegVideoDecoder;
pub use frame_queue::FrameQueue;
pub use texture_manager::TextureManager;
