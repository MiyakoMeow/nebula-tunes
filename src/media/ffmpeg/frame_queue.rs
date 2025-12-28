//! 视频帧队列管理
//!
//! 用于预解码和管理视频帧，确保播放流畅

use super::DecodedFrame;
use std::collections::VecDeque;
use std::sync::Arc;

/// 视频帧队列
///
/// 预解码 2-3 帧以应对解码延迟
pub struct FrameQueue {
    /// 帧队列
    frames: VecDeque<Arc<DecodedFrame>>,
    /// 队列容量
    capacity: usize,
}

impl FrameQueue {
    /// 创建新的帧队列
    ///
    /// capacity: 队列容量，建议 2-3 帧
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            frames: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// 推入一帧到队列
    ///
    /// 如果队列已满，移除最旧的帧
    pub fn push(&mut self, frame: DecodedFrame) {
        if self.frames.len() >= self.capacity {
            self.frames.pop_front();
        }
        self.frames.push_back(Arc::new(frame));
    }

    /// 根据时间戳获取最近的帧
    ///
    /// 用于视频时间同步
    #[must_use]
    pub fn get_frame_at_time(&self, timestamp: f64) -> Option<Arc<DecodedFrame>> {
        self.frames
            .iter()
            .min_by(|a, b| {
                (a.timestamp - timestamp)
                    .abs()
                    .partial_cmp(&(b.timestamp - timestamp).abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .cloned()
    }

    /// 获取最新的帧
    #[must_use]
    #[allow(dead_code)]
    pub fn latest_frame(&self) -> Option<Arc<DecodedFrame>> {
        self.frames.back().cloned()
    }

    /// 获取队列长度
    #[must_use]
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// 检查队列是否为空
    #[must_use]
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// 清空队列
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.frames.clear();
    }
}

impl Default for FrameQueue {
    fn default() -> Self {
        Self::new(3)
    }
}
