//! 音频播放循环
//!
//! 独立维护音频输出流与缓存，按队列消费播放请求。

use std::{collections::HashMap, path::Path, path::PathBuf, sync::Arc};

use anyhow::Result;
use rodio::{Sink, stream::OutputStream};
use tokio::sync::mpsc;

struct Audio {
    stream: OutputStream,
    sinks: Vec<Sink>,
    cache: HashMap<PathBuf, Arc<[u8]>>,
}

impl Audio {
    fn new() -> Result<Self> {
        let stream = rodio::OutputStreamBuilder::open_default_stream()?;
        Ok(Self {
            stream,
            sinks: Vec::new(),
            cache: HashMap::new(),
        })
    }

    fn cached_bytes(&mut self, path: &Path) -> Result<Arc<[u8]>> {
        if let Some(b) = self.cache.get(path) {
            return Ok(b.clone());
        }
        let bytes = std::fs::read(path)?;
        let arc: Arc<[u8]> = Arc::from(bytes);
        self.cache.insert(path.to_path_buf(), arc.clone());
        Ok(arc)
    }

    fn play_file(&mut self, path: &Path) -> Result<()> {
        let bytes = self.cached_bytes(path)?;
        let cursor = std::io::Cursor::new(bytes);
        let sink = rodio::play(self.stream.mixer(), cursor)?;
        self.sinks.push(sink);
        Ok(())
    }

    fn cleanup(&mut self) {
        self.sinks.retain(|s| !s.empty());
    }
}

/// 异步音频播放循环
///
/// - 从 `rx` 读取待播放的文件路径
/// - 执行播放并清理已完成的 `Sink`
pub async fn run_audio_loop(mut rx: mpsc::Receiver<PathBuf>) {
    let mut audio = match Audio::new() {
        Ok(a) => a,
        Err(_) => return,
    };
    while let Some(path) = rx.recv().await {
        let _ = audio.play_file(&path);
        audio.cleanup();
    }
}
