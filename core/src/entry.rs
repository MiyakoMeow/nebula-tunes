//! 程序入口模块

use std::sync::mpsc;

use crate::Instance;
use crate::loops::{VisualMsg, visual};

/// 视觉应用：负责驱动渲染器与处理视觉消息
pub struct VisualApp {
    /// 绑定到窗口表面的渲染器
    window_renderer: visual::Renderer,
    /// 视觉消息接收端
    visual_rx: mpsc::Receiver<VisualMsg>,
    /// 最新一帧的实例列表
    latest_instances: Vec<Instance>,
}

impl VisualApp {
    /// 创建视觉应用
    pub fn new(window_renderer: visual::Renderer, visual_rx: mpsc::Receiver<VisualMsg>) -> Self {
        Self {
            window_renderer,
            visual_rx,
            latest_instances: visual::base_instances(),
        }
    }

    /// 处理窗口大小变化
    pub fn resize(&mut self, width: u32, height: u32) {
        self.window_renderer.resize(width, height);
    }

    /// 执行一次渲染：消费消息、更新资源并绘制
    pub fn redraw(&mut self) {
        while let Ok(msg) = self.visual_rx.try_recv() {
            match msg {
                VisualMsg::Instances(instances) => {
                    self.latest_instances = instances;
                }
                VisualMsg::BgaChange { layer, path } => {
                    self.window_renderer.request_bga_decode(layer, path);
                }
                VisualMsg::BgaPoorTrigger => {
                    self.window_renderer.trigger_poor();
                }
                // 视频消息处理
                VisualMsg::VideoPlay {
                    layer,
                    path,
                    loop_play,
                } => {
                    self.window_renderer.start_video(layer, path, loop_play);
                }
                VisualMsg::VideoFrame { layer, frame } => {
                    self.window_renderer
                        .update_video_frame_internal(layer, frame);
                }
                VisualMsg::VideoStop { layer } => {
                    self.window_renderer.stop_video(layer);
                }
                VisualMsg::VideoSeek { layer, timestamp } => {
                    self.window_renderer.seek_video(layer, timestamp);
                }
            }
        }

        // 处理解码线程发送的视频帧消息
        while let Ok((layer, frame)) = self.window_renderer.video_frame_rx.try_recv() {
            self.window_renderer
                .update_video_frame_internal(layer, frame);
        }

        let _ = self.window_renderer.draw(&self.latest_instances);
    }
}
