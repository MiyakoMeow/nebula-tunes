use std::{
    path::PathBuf,
    sync::mpsc,
    thread::{self, JoinHandle},
};

use crate::Instance;
use crate::loops::{VisualMsg, visual};

/// 视觉应用：负责驱动渲染器与处理视觉消息
pub(crate) struct VisualApp {
    /// 绑定到窗口表面的渲染器
    window_renderer: visual::Renderer,
    /// 视觉消息接收端
    visual_rx: mpsc::Receiver<VisualMsg>,
    /// 最新一帧的实例列表
    latest_instances: Vec<Instance>,
    /// BGA 解码请求发送端（发送图片路径）
    bga_decode_tx: Option<mpsc::Sender<PathBuf>>,
    /// BGA 解码结果接收端（rgba, w, h）
    bga_decoded_rx: mpsc::Receiver<(Vec<u8>, u32, u32)>,
    /// BGA 解码线程句柄
    bga_decode_thread: Option<JoinHandle<()>>,
}

impl VisualApp {
    /// 创建视觉应用并启动 BGA 解码线程
    pub(crate) fn new(
        window_renderer: visual::Renderer,
        visual_rx: mpsc::Receiver<VisualMsg>,
    ) -> Self {
        let (bga_decode_tx, bga_decode_rx) = mpsc::channel::<PathBuf>();
        let (bga_decoded_tx, bga_decoded_rx) = mpsc::channel::<(Vec<u8>, u32, u32)>();
        let bga_decode_thread = thread::spawn(move || {
            loop {
                let Ok(mut path) = bga_decode_rx.recv() else {
                    break;
                };
                loop {
                    match bga_decode_rx.try_recv() {
                        Ok(new_path) => path = new_path,
                        Err(mpsc::TryRecvError::Empty) => break,
                        Err(mpsc::TryRecvError::Disconnected) => return,
                    }
                }
                let decoded = (|| -> Option<(Vec<u8>, u32, u32)> {
                    let bytes = std::fs::read(path).ok()?;
                    let img = image::load_from_memory(&bytes).ok()?;
                    let rgba = img.to_rgba8();
                    let w = rgba.width();
                    let h = rgba.height();
                    Some((rgba.into_raw(), w, h))
                })();
                if let Some(decoded) = decoded {
                    let _ = bga_decoded_tx.send(decoded);
                }
            }
        });

        Self {
            window_renderer,
            visual_rx,
            latest_instances: visual::base_instances(),
            bga_decode_tx: Some(bga_decode_tx),
            bga_decoded_rx,
            bga_decode_thread: Some(bga_decode_thread),
        }
    }

    /// 处理窗口大小变化
    pub(crate) fn resize(&mut self, width: u32, height: u32) {
        self.window_renderer.resize(width, height);
    }

    /// 执行一次渲染：消费消息、更新资源并绘制
    pub(crate) fn redraw(&mut self) {
        loop {
            match self.bga_decoded_rx.try_recv() {
                Ok((rgba, w, h)) => {
                    let _ = self
                        .window_renderer
                        .update_bga_image_from_rgba(rgba.as_slice(), w, h);
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
        }

        loop {
            match self.visual_rx.try_recv() {
                Ok(msg) => match msg {
                    VisualMsg::Instances(instances) => {
                        self.latest_instances = instances;
                    }
                    VisualMsg::Bga(path) => {
                        if let Some(tx) = &self.bga_decode_tx {
                            let _ = tx.send(path);
                        }
                    }
                },
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
        }

        let _ = self.window_renderer.draw(&self.latest_instances);
    }
}

impl Drop for VisualApp {
    fn drop(&mut self) {
        let _ = self.bga_decode_tx.take();
        if let Some(handle) = self.bga_decode_thread.take() {
            let _ = handle.join();
        }
    }
}
