//! BMS文件处理插件
//!
//! 负责BMS文件的异步加载、解析和处理

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::Result;
use async_fs as afs;
use bevy::{
    asset::AssetPath,
    platform::collections::HashMap,
    prelude::*,
    tasks::{IoTaskPool, Task, futures::check_ready},
};
use bevy_kira_audio::AudioSource as KiraAudioSource;
use bms_rs::{bms::prelude::*, chart_process::prelude::*};
use chardetng::EncodingDetector;
use gametime::TimeSpan;

use crate::schedule::LogicSchedule;

use crate::filesystem;
use crate::resources::{ExecArgs, NowStamp};

/// 系统集合
#[derive(SystemSet, Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum BmsSystemSet {
    /// 加载BMS文件
    BmsLoad,
    /// 处理BMS事件
    EventProcess,
}

/// 音频系统集合
#[derive(SystemSet, Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum AudioSystemSet {
    /// 音频播放
    AudioPlay,
}

/// BGM通道标记
#[derive(Resource)]
pub struct BgmChannel;

/// 音效通道标记
#[derive(Resource)]
pub struct SfxChannel;

/// BMS加载任务资源
#[derive(Resource)]
pub struct BmsLoadTask(pub Task<Result<(BmsProcessor, HashMap<WavId, PathBuf>)>>);

/// BMS处理器资源
#[derive(Resource)]
pub struct BmsProcessorResource {
    /// BMS处理器
    pub processor: BmsProcessor,
    /// 音频文件路径映射
    pub audio_paths: HashMap<WavId, PathBuf>,
    /// 音频资源句柄
    pub audio_handles: HashMap<WavId, Handle<KiraAudioSource>>,
    /// 待加载的音频ID列表
    pending_audio_loads: Vec<WavId>,
    /// 是否已开始播放
    pub started: bool,
    /// 是否已警告缺失音频
    pub warned_missing: bool,
}

/// BMS处理插件
pub struct BMSProcessorPlugin;

impl Plugin for BMSProcessorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load_bms_file.in_set(BmsSystemSet::BmsLoad))
            .add_systems(
                LogicSchedule,
                (
                    poll_bms_load_task,
                    batch_load_audio_assets,
                    update_processor_state,
                )
                    .chain()
                    .in_set(BmsSystemSet::EventProcess),
            );
    }
}

/// 启动BMS文件加载
fn load_bms_file(mut commands: Commands, args: Res<ExecArgs>) {
    let Some(bms_path) = args.bms_path.clone() else {
        return;
    };
    let pool = IoTaskPool::get();
    let task = pool.spawn(load_bms_and_collect_paths(bms_path));
    commands.insert_resource(BmsLoadTask(task));
}

/// 异步加载BMS文件并收集音频路径
async fn load_bms_and_collect_paths(
    bms_path: PathBuf,
) -> Result<(BmsProcessor, HashMap<WavId, PathBuf>)> {
    // 读取BMS文件
    let bms_bytes = afs::read(&bms_path).await?;

    // 检测字符编码
    let mut det = EncodingDetector::new();
    det.feed(&bms_bytes, true);
    let enc = det.guess(None, true);
    let (bms_str, _, _) = enc.decode(&bms_bytes);

    // 解析BMS文件
    let BmsOutput { bms, warnings: _ } = bms_rs::bms::parse_bms(&bms_str, default_config());
    let bms = bms?;

    // 生成基础BPM
    let base_bpm = StartBpmGenerator
        .generate(&bms)
        .unwrap_or_else(|| BaseBpm(120.0.into()));

    // 创建处理器
    let processor = BmsProcessor::new::<KeyLayoutBeat>(
        &bms,
        VisibleRangePerBpm::new(
            &base_bpm,
            TimeSpan::from_duration(Duration::from_secs_f32(0.6)),
        ),
    );

    // 收集音频文件路径
    let bms_dir = bms_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let mut audio_paths: HashMap<WavId, PathBuf> = HashMap::new();

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

    Ok((processor, audio_paths))
}

/// 轮询BMS加载任务状态
fn poll_bms_load_task(
    mut commands: Commands,
    _asset_server: Res<AssetServer>,
    task_res: Option<ResMut<BmsLoadTask>>,
) {
    let Some(mut task) = task_res else {
        return;
    };

    if let Some(result) = check_ready(&mut task.0) {
        match result {
            Ok((processor, audio_paths)) => {
                // 收集所有音频ID,稍后分批加载
                let all_audio_ids: Vec<_> = audio_paths.keys().copied().collect();

                // 创建处理器资源（音频句柄为空,稍后分批加载）
                commands.insert_resource(BmsProcessorResource {
                    processor,
                    audio_handles: HashMap::new(),
                    audio_paths,
                    pending_audio_loads: all_audio_ids,
                    started: false,
                    warned_missing: false,
                });
            }
            Err(e) => {
                eprintln!("{}", e);
            }
        }
        commands.remove_resource::<BmsLoadTask>();
    }
}

/// 更新处理器状态并发送触发消息
fn update_processor_state(
    status: Option<ResMut<BmsProcessorResource>>,
    mut triggered_events: MessageWriter<crate::plugins::audio_trigger::TriggeredNoteEvent>,
    now_stamp: Res<NowStamp>,
) {
    let Some(mut status) = status else {
        return;
    };
    if !status.started {
        return;
    }

    // 先收集音频句柄
    let audio_ids: Vec<_> = status.audio_handles.keys().copied().collect();

    // 更新处理器并发送触发事件
    for evp in status.processor.update(now_stamp.0) {
        let (wav, is_bgm) = match evp.event() {
            ChartEvent::Bgm { wav_id: Some(wav) } => (wav, true),
            ChartEvent::Note {
                wav_id: Some(wav), ..
            } => (wav, false),
            _ => continue,
        };

        // 检查音频是否存在
        if audio_ids.contains(wav) {
            // 发送触发消息（而不是音频播放消息）
            triggered_events.write(crate::plugins::audio_trigger::TriggeredNoteEvent {
                wav_id: *wav,
                is_bgm,
            });
        }
    }
}

/// 分批加载音频资源
fn batch_load_audio_assets(
    status: Option<ResMut<BmsProcessorResource>>,
    asset_server: Res<AssetServer>,
) {
    let Some(mut status) = status else {
        return;
    };

    // 每帧加载最多10个音频文件
    const BATCH_SIZE: usize = 10;
    let mut loaded_count = 0;

    // 从待加载列表中取出音频ID
    while !status.pending_audio_loads.is_empty() && loaded_count < BATCH_SIZE {
        let id = status.pending_audio_loads.remove(0);
        if let Some(path) = status.audio_paths.get(&id) {
            let path_str = path.to_string_lossy().to_string();
            let asset_str = format!("fs://{}", path_str);
            let ap = AssetPath::parse(&asset_str);
            let handle: Handle<KiraAudioSource> = asset_server.load_override(ap);
            status.audio_handles.insert(id, handle);
            loaded_count += 1;
        }
    }
}
