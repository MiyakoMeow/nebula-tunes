//! BGA 图像处理模块
//!
//! 负责图像解码、缓存和背景移除处理

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use async_fs as fs;
use futures_lite::future;
use image::{ImageBuffer, Luma};
use imageproc::region_labelling::{Connectivity, connected_components};
use tracing::info;

use crate::loops::BgaLayer;

/// 已解码的图片数据
#[allow(clippy::module_name_repetitions)]
pub struct DecodedImage {
    /// RGBA8 像素缓冲
    pub rgba: Vec<u8>,
    /// 宽度
    pub width: u32,
    /// 高度
    pub height: u32,
}

/// 解码后的缓存变体
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DecodeVariant {
    /// 原始 RGBA
    Raw,
    /// 去除背景后的 RGBA
    RemoveBackground,
}

/// 缓存键：路径 + 解码变体
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct CacheKey {
    /// 文件路径
    path: PathBuf,
    /// 解码变体
    variant: DecodeVariant,
}

/// BGA 图片解码缓存（跨线程共享）
pub struct BgaDecodeCache {
    /// (路径, 变体) 到已解码图片的映射
    inner: Mutex<HashMap<CacheKey, Arc<DecodedImage>>>,
}

impl BgaDecodeCache {
    /// 创建空缓存
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// 查询指定变体的缓存条目
    #[must_use]
    pub(crate) fn get_variant(
        &self,
        variant: DecodeVariant,
        path: &Path,
    ) -> Option<Arc<DecodedImage>> {
        let key = CacheKey {
            path: path.to_path_buf(),
            variant,
        };
        self.inner.lock().ok()?.get(&key).cloned()
    }

    /// 写入指定变体的缓存条目并返回共享引用
    pub(crate) fn insert_variant(
        &self,
        variant: DecodeVariant,
        path: PathBuf,
        rgba: Vec<u8>,
        width: u32,
        height: u32,
    ) -> Arc<DecodedImage> {
        let decoded = Arc::new(DecodedImage {
            rgba,
            width,
            height,
        });
        if let Ok(mut map) = self.inner.lock() {
            map.insert(CacheKey { path, variant }, decoded.clone());
        }
        decoded
    }
}

/// 将指定图层映射到预处理变体
pub const fn layer_to_variant(layer: BgaLayer) -> DecodeVariant {
    match layer {
        BgaLayer::Layer | BgaLayer::Layer2 => DecodeVariant::RemoveBackground,
        BgaLayer::Bga | BgaLayer::Poor => DecodeVariant::Raw,
    }
}

/// 去除背景（黑色背景转透明）
fn remove_background(rgba_buf: &mut [u8], width: u32, height: u32) {
    let width_usize = width as usize;
    let mask = ImageBuffer::from_fn(width, height, |x, y| {
        let base = ((y as usize) * width_usize + (x as usize)) * 4;
        let is_black = rgba_buf
            .get(base..base + 4)
            .and_then(|px| <[u8; 4]>::try_from(px).ok())
            .is_some_and(|[r, g, b, a]| r == 0 && g == 0 && b == 0 && a != 0);
        Luma([u8::from(is_black)])
    });

    let labels = connected_components(&mask, Connectivity::Four, Luma([0u8]));
    let corners = [
        (0u32, 0u32),
        (width - 1, 0u32),
        (0u32, height - 1),
        (width - 1, height - 1),
    ];
    let mut targets = [0u32; 4];
    let mut targets_len = 0usize;
    for (x, y) in corners {
        let label = *labels.get_pixel(x, y).0.first().unwrap_or(&0);
        if label == 0 || targets.iter().take(targets_len).any(|v| *v == label) {
            continue;
        }
        let Some(slot) = targets.get_mut(targets_len) else {
            break;
        };
        *slot = label;
        targets_len += 1;
    }

    if targets_len != 0 {
        for (x, y, p) in labels.enumerate_pixels() {
            let label = *p.0.first().unwrap_or(&0);
            if label == 0 || !targets.iter().take(targets_len).any(|v| *v == label) {
                continue;
            }

            let base = ((y as usize) * width_usize + (x as usize)) * 4;
            if let Some(px) = rgba_buf.get_mut(base..base + 4) {
                px.copy_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
}

/// 从文件路径读取并解码图片为 RGBA8 缓冲
async fn decode_image_async(path: &Path) -> Option<(Vec<u8>, u32, u32)> {
    let bytes = fs::read(path).await.ok()?;
    let img = image::load_from_memory(&bytes).ok()?;
    let rgba = img.to_rgba8();
    let w = rgba.width();
    let h = rgba.height();
    Some((rgba.into_raw(), w, h))
}

/// 对 RGBA 缓冲按变体进行预处理
fn preprocess_rgba(mut rgba: Vec<u8>, width: u32, height: u32, variant: DecodeVariant) -> Vec<u8> {
    if variant == DecodeVariant::RemoveBackground && width != 0 && height != 0 {
        remove_background(&mut rgba, width, height);
    }
    rgba
}

/// 解码图片并写入缓存（缓存命中则直接返回）
pub fn decode_and_cache(
    cache: &BgaDecodeCache,
    layer: BgaLayer,
    path: PathBuf,
) -> Option<Arc<DecodedImage>> {
    let want = layer_to_variant(layer);
    if let Some(decoded) = cache.get_variant(want, path.as_path()) {
        return Some(decoded);
    }

    if want == DecodeVariant::RemoveBackground
        && let Some(raw) = cache.get_variant(DecodeVariant::Raw, path.as_path())
    {
        let rgba = preprocess_rgba(raw.rgba.clone(), raw.width, raw.height, want);
        return Some(cache.insert_variant(want, path, rgba, raw.width, raw.height));
    }

    let (raw_rgba, w, h) = future::block_on(decode_image_async(path.as_path()))?;
    let raw = cache.insert_variant(DecodeVariant::Raw, path.clone(), raw_rgba.clone(), w, h);
    let processed = preprocess_rgba(raw_rgba, w, h, DecodeVariant::RemoveBackground);
    let processed = cache.insert_variant(DecodeVariant::RemoveBackground, path, processed, w, h);
    Some(match want {
        DecodeVariant::Raw => raw,
        DecodeVariant::RemoveBackground => processed,
    })
}

/// 确保指定路径的两种预处理变体都已进入缓存
pub fn ensure_preprocessed(cache: &BgaDecodeCache, path: PathBuf) {
    let raw_exists = cache
        .get_variant(DecodeVariant::Raw, path.as_path())
        .is_some();
    let processed_exists = cache
        .get_variant(DecodeVariant::RemoveBackground, path.as_path())
        .is_some();
    if raw_exists && processed_exists {
        return;
    }

    if !processed_exists && let Some(raw) = cache.get_variant(DecodeVariant::Raw, path.as_path()) {
        let rgba = preprocess_rgba(
            raw.rgba.clone(),
            raw.width,
            raw.height,
            DecodeVariant::RemoveBackground,
        );
        let _ = cache.insert_variant(
            DecodeVariant::RemoveBackground,
            path,
            rgba,
            raw.width,
            raw.height,
        );
        return;
    }

    let Some((raw_rgba, w, h)) = future::block_on(decode_image_async(path.as_path())) else {
        return;
    };
    if !raw_exists {
        let _ = cache.insert_variant(DecodeVariant::Raw, path.clone(), raw_rgba.clone(), w, h);
    }
    if !processed_exists {
        let rgba = preprocess_rgba(raw_rgba, w, h, DecodeVariant::RemoveBackground);
        let _ = cache.insert_variant(DecodeVariant::RemoveBackground, path, rgba, w, h);
    }
}

/// 预先解码所有 BGA 图片到缓存，并每秒输出一次进度
pub fn preload_bga_files(cache: Arc<BgaDecodeCache>, files: Vec<PathBuf>) {
    let paths: Vec<PathBuf> = files
        .into_iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let total = u32::try_from(paths.len()).unwrap_or(u32::MAX);
    if total == 0 {
        info!(current = 0, total = 0, "BGA预加载进度");
        info!("BGA预加载完成");
        return;
    }

    let loaded = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let done = Arc::new(std::sync::atomic::AtomicBool::new(false));

    let loaded_for_log = loaded.clone();
    let done_for_log = done.clone();
    let logger = thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_secs(1));
            let c = loaded_for_log.load(std::sync::atomic::Ordering::Relaxed);
            info!(current = c, total, "BGA预加载进度");
            if done_for_log.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
        }
    });

    let (work_tx, work_rx) = std::sync::mpsc::channel::<PathBuf>();
    let work_rx = Arc::new(std::sync::Mutex::new(work_rx));
    let workers = thread::available_parallelism()
        .map(std::num::NonZero::get)
        .unwrap_or(1)
        .clamp(1, 8);

    let mut handles = Vec::with_capacity(workers);
    for _ in 0..workers {
        let work_rx = work_rx.clone();
        let cache = cache.clone();
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
                ensure_preprocessed(cache.as_ref(), path);
                loaded.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }));
    }

    for path in paths {
        let _ = work_tx.send(path);
    }
    drop(work_tx);

    for h in handles {
        let _ = h.join();
    }
    done.store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = logger.join();
    info!("BGA预加载完成");
}
