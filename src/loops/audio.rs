//! 音频播放循环
//!
//! - 使用异步文件读取缓存音频数据
//! - 支持预加载全部音频资源、每秒输出一次进度
//! - 预加载完成后才开始播放队列

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use anyhow::Result;
use rodio::{Source, buffer::SamplesBuffer, decoder::Decoder, stream::OutputStream};

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
    fn cached_buffer(&mut self, path: &Path) -> Result<Arc<SamplesBuffer>> {
        if let Some(buf) = self.decoded.get(path) {
            return Ok(buf.clone());
        }
        let bytes = fs::read(path)?;
        let buffer = decode_bytes(bytes)?;
        let arc = Arc::new(buffer);
        self.decoded.insert(path.to_path_buf(), arc.clone());
        Ok(arc)
    }
}

/// 音频循环：预加载与播放
///
/// - 从 `rx` 接收预加载或播放请求
/// - 预加载期间每秒输出一次 `已加载/总数`
/// - 完成后向 `ready_tx` 发送 `PreloadFinished`
pub fn run_audio_loop(rx: mpsc::Receiver<Msg>, ready_tx: mpsc::SyncSender<Event>) {
    let mut audio = match Audio::new() {
        Ok(a) => a,
        Err(_) => return,
    };
    while let Ok(msg) = rx.recv() {
        match msg {
            Msg::PreloadAll { files } => {
                let total = files.len() as u32;
                let loaded = Arc::new(AtomicU32::new(0));
                let done = Arc::new(AtomicBool::new(false));
                let loaded_for_log = loaded.clone();
                let done_for_log = done.clone();
                let logger = thread::spawn(move || {
                    loop {
                        thread::sleep(Duration::from_secs(1));
                        let c = loaded_for_log.load(Ordering::Relaxed);
                        println!("音频预加载进度：{}/{}", c, total);
                        if done_for_log.load(Ordering::Relaxed) {
                            break;
                        }
                    }
                });

                let (work_tx, work_rx) = std::sync::mpsc::channel::<PathBuf>();
                let work_rx = Arc::new(Mutex::new(work_rx));
                let (result_tx, result_rx) =
                    std::sync::mpsc::channel::<(PathBuf, Option<Arc<SamplesBuffer>>)>();
                let workers = thread::available_parallelism()
                    .map(std::num::NonZero::get)
                    .unwrap_or(1)
                    .clamp(1, 8);

                let mut handles = Vec::with_capacity(workers);
                for _ in 0..workers {
                    let work_rx = work_rx.clone();
                    let result_tx = result_tx.clone();
                    let loaded = loaded.clone();
                    handles.push(thread::spawn(move || {
                        loop {
                            let path = {
                                let Ok(work_rx) = work_rx.lock() else {
                                    break;
                                };
                                match work_rx.recv() {
                                    Ok(p) => p,
                                    Err(_) => break,
                                }
                            };
                            let bytes = match fs::read(&path) {
                                Ok(b) => b,
                                Err(_) => {
                                    loaded.fetch_add(1, Ordering::Relaxed);
                                    let _ = result_tx.send((path, None));
                                    continue;
                                }
                            };
                            let buffer = match decode_bytes(bytes) {
                                Ok(b) => b,
                                Err(_) => {
                                    loaded.fetch_add(1, Ordering::Relaxed);
                                    let _ = result_tx.send((path, None));
                                    continue;
                                }
                            };
                            loaded.fetch_add(1, Ordering::Relaxed);
                            let _ = result_tx.send((path, Some(Arc::new(buffer))));
                        }
                    }));
                }

                for path in files {
                    let _ = work_tx.send(path);
                }
                drop(work_tx);
                drop(result_tx);

                for _ in 0..total {
                    let Ok((p, buf)) = result_rx.recv() else {
                        break;
                    };
                    if let Some(buf) = buf {
                        audio.decoded.insert(p, buf);
                    }
                }

                for h in handles {
                    let _ = h.join();
                }
                done.store(true, Ordering::Relaxed);
                let _ = logger.join();
                let _ = ready_tx.send(Event::PreloadFinished);
                println!("音频预加载完成");
            }
            Msg::Play(path) => {
                if let Ok(buf) = audio.cached_buffer(&path) {
                    audio.stream.mixer().add((*buf).clone());
                }
            }
        }
    }
}
