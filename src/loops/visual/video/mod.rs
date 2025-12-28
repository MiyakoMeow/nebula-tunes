//! 视频解码和渲染模块
//!
//! 提供视频 BGA 支持

mod decoder;
mod frame_queue;
mod texture_manager;

#[cfg(not(target_arch = "wasm32"))]
mod ffmpeg_impl;

pub use decoder::{DecodedFrame, VideoDecoder};
