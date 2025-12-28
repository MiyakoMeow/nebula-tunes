//! BMS 解析与处理器创建

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::Result;
use async_fs as afs;
use bms_rs::{bms::prelude::*, chart_process::prelude::*};
use chardetng::EncodingDetector;
use gametime::TimeSpan;

use crate::filesystem;

/// BGA 文件类型
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BgaFileType {
    /// 图像文件
    Image,
    /// 视频文件
    Video,
}

/// 加载 BMS 文件并收集音频/BGA 资源路径映射
///
/// # Errors
///
/// - 读取 BMS 文件失败
/// - 编码探测或解码失败
/// - BMS 解析失败
pub(crate) async fn load_bms_and_collect_paths(
    bms_path: PathBuf,
    travel: TimeSpan,
) -> Result<(
    BmsProcessor,
    HashMap<WavId, PathBuf>,
    HashMap<BmpId, PathBuf>,
    HashMap<BmpId, BgaFileType>,
)> {
    let bms_bytes = afs::read(&bms_path).await?;
    let mut det = EncodingDetector::new();
    det.feed(&bms_bytes, true);
    let enc = det.guess(None, true);
    let (bms_str, _, _) = enc.decode(&bms_bytes);
    let BmsOutput { bms, warnings: _ } = bms_rs::bms::parse_bms(&bms_str, default_config());
    let Ok(bms) = bms else {
        anyhow::bail!("failed to parse BMS")
    };
    println!("Title: {:?}", bms.music_info.title);
    println!("Artist: {:?}", bms.music_info.artist);
    let base_bpm = StartBpmGenerator
        .generate(&bms)
        .unwrap_or(BaseBpm(120.0.into()));
    println!("BaseBpm: {}", base_bpm.value());
    let processor =
        BmsProcessor::new::<KeyLayoutBeat>(&bms, VisibleRangePerBpm::new(&base_bpm, travel));
    let bms_dir = bms_path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let mut audio_paths: HashMap<WavId, PathBuf> = HashMap::new();
    let mut bmp_paths: HashMap<BmpId, PathBuf> = HashMap::new();
    let mut bmp_types: HashMap<BmpId, BgaFileType> = HashMap::new();
    let child_list: Vec<PathBuf> = processor
        .audio_files()
        .into_values()
        .map(std::path::Path::to_path_buf)
        .collect();
    let index = filesystem::choose_paths_by_ext_async(
        &bms_dir,
        &child_list,
        &["flac", "wav", "ogg", "mp3"],
    )
    .await;
    for (id, audio_path) in processor.audio_files().into_iter() {
        let stem = audio_path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(std::string::ToString::to_string);
        let base = bms_dir.join(audio_path);
        let chosen = stem.and_then(|s| index.get(&s).cloned()).unwrap_or(base);
        audio_paths.insert(id, chosen);
    }
    let bmp_list: Vec<PathBuf> = processor
        .bmp_files()
        .into_values()
        .map(std::path::Path::to_path_buf)
        .collect();
    let bmp_index = filesystem::choose_paths_by_ext_async(
        &bms_dir,
        &bmp_list,
        &[
            "bmp", "jpg", "jpeg", "png", "mp4", "avi", "mpeg", "webm", "mkv", "wmv",
        ],
    )
    .await;
    // 视频文件扩展名
    const VIDEO_EXTS: &[&str] = &["mp4", "avi", "mpeg", "webm", "mkv", "wmv"];
    for (id, bmp_path) in processor.bmp_files().into_iter() {
        let stem = bmp_path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(std::string::ToString::to_string);
        let base = bms_dir.join(bmp_path);
        let chosen = stem
            .and_then(|s| bmp_index.get(&s).cloned())
            .unwrap_or(base);

        // 判断文件类型
        let file_type = chosen
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| {
                if VIDEO_EXTS.contains(&ext.to_lowercase().as_str()) {
                    BgaFileType::Video
                } else {
                    BgaFileType::Image
                }
            })
            .unwrap_or(BgaFileType::Image);

        bmp_paths.insert(id, chosen);
        bmp_types.insert(id, file_type);
    }
    Ok((processor, audio_paths, bmp_paths, bmp_types))
}
