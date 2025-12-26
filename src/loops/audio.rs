//! 音频播放循环
//!
//! - 使用异步文件读取缓存音频数据
//! - 支持预加载全部音频资源、每秒输出一次进度
//! - 预加载完成后才开始播放队列

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::Duration,
};

use anyhow::Result;
use async_fs as afs;
use rodio::{Source, buffer::SamplesBuffer, decoder::Decoder, stream::OutputStream};
use tokio::sync::mpsc;

/// 将原始字节数据解码为可播放的采样缓冲
fn decode_bytes(bytes: Vec<u8>) -> Result<SamplesBuffer> {
    let decoder = Decoder::new(std::io::Cursor::new(bytes))?;
    let channels = decoder.channels();
    let sample_rate = decoder.sample_rate();
    let samples: Vec<f32> = decoder.collect();
    Ok(SamplesBuffer::new(channels, sample_rate, samples))
}

/// 音频循环消息
pub enum Msg {
    /// 预加载所有音频文件
    PreloadAll {
        /// 要预加载的文件路径列表
        files: Vec<PathBuf>,
    },
    /// 播放单个音频文件
    Play(PathBuf),
}

/// 音频循环事件
pub enum Event {
    /// 预加载完成
    PreloadFinished,
}

/// 音频后端与解码缓存管理
struct Audio {
    /// 音频输出流
    stream: OutputStream,
    /// 路径到已解码采样缓冲的缓存
    decoded: HashMap<PathBuf, Arc<SamplesBuffer>>,
}

impl Audio {
    /// 创建音频输出流并初始化缓存
    fn new() -> Result<Self> {
        let stream = rodio::OutputStreamBuilder::open_default_stream()?;
        Ok(Self {
            stream,
            decoded: HashMap::new(),
        })
    }

    /// 获取指定路径的采样缓冲，若未缓存则读取并解码后加入缓存
    async fn cached_buffer(&mut self, path: &Path) -> Result<Arc<SamplesBuffer>> {
        if let Some(buf) = self.decoded.get(path) {
            return Ok(buf.clone());
        }
        let bytes = afs::read(path).await?;
        let buffer = decode_bytes(bytes)?;
        let arc = Arc::new(buffer);
        self.decoded.insert(path.to_path_buf(), arc.clone());
        Ok(arc)
    }
}

/// 异步音频循环：预加载与播放
///
/// - 从 `rx` 接收预加载或播放请求
/// - 预加载期间每秒输出一次 `已加载/总数`
/// - 完成后向 `ready_tx` 发送 `PreloadFinished`
pub async fn run_audio_loop(mut rx: mpsc::Receiver<Msg>, ready_tx: mpsc::Sender<Event>) {
    let mut audio = match Audio::new() {
        Ok(a) => a,
        Err(_) => return,
    };
    while let Some(msg) = rx.recv().await {
        match msg {
            Msg::PreloadAll { files } => {
                let total = files.len() as u32;
                let loaded = Arc::new(AtomicU32::new(0));
                let done = Arc::new(AtomicBool::new(false));
                let loaded_for_log = loaded.clone();
                let done_for_log = done.clone();
                let logger = tokio::spawn(async move {
                    let mut ticker = tokio::time::interval(Duration::from_secs(1));
                    loop {
                        ticker.tick().await;
                        let c = loaded_for_log.load(Ordering::Relaxed);
                        println!("音频预加载进度：{}/{}", c, total);
                        if done_for_log.load(Ordering::Relaxed) {
                            break;
                        }
                    }
                });
                let mut handles = Vec::with_capacity(files.len());
                for path in files {
                    let loaded_cl = loaded.clone();
                    handles.push(tokio::spawn(async move {
                        let bytes = match afs::read(&path).await {
                            Ok(b) => b,
                            Err(_) => {
                                loaded_cl.fetch_add(1, Ordering::Relaxed);
                                return (path, None);
                            }
                        };
                        let buffer = match decode_bytes(bytes) {
                            Ok(b) => b,
                            Err(_) => {
                                loaded_cl.fetch_add(1, Ordering::Relaxed);
                                return (path, None);
                            }
                        };
                        loaded_cl.fetch_add(1, Ordering::Relaxed);
                        (path, Some(Arc::new(buffer)))
                    }));
                }
                for h in handles {
                    if let Ok((p, Some(buf))) = h.await {
                        audio.decoded.insert(p, buf);
                    }
                }
                done.store(true, Ordering::Relaxed);
                let _ = logger.await;
                let _ = ready_tx.try_send(Event::PreloadFinished);
                println!("音频预加载完成");
            }
            Msg::Play(path) => {
                if let Ok(buf) = audio.cached_buffer(&path).await {
                    audio.stream.mixer().add((*buf).clone());
                }
            }
        }
    }
}
