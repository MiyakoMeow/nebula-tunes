//! BMS文件处理插件
//!
//! 负责BMS文件的异步加载、解析和处理

use std::{path::Path, path::PathBuf, time::Duration};

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

use crate::filesystem;
use crate::plugins::audio_manager::AudioPlayMessage;
use crate::resources::{ExecArgs, NowStamp};

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
    /// 是否已开始播放
    pub started: bool,
    /// 是否已警告缺失音频
    pub warned_missing: bool,
}

/// BMS处理插件
pub struct BMSProcessorPlugin;

impl Plugin for BMSProcessorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load_bms_file)
            .add_systems(Update, poll_bms_load_task)
            .add_systems(Update, process_chart_events);
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
    let bms = bms.unwrap();

    // 生成基础BPM
    let base_bpm = StartBpmGenerator
        .generate(&bms)
        .unwrap_or(BaseBpm(120.0.into()));

    // 创建处理器
    let processor = BmsProcessor::new::<KeyLayoutBeat>(
        &bms,
        VisibleRangePerBpm::new(
            &base_bpm,
            TimeSpan::from_duration(Duration::from_secs_f32(0.6)),
        ),
    );

    // 收集音频文件路径
    let bms_dir = bms_path.parent().unwrap_or(Path::new(".")).to_path_buf();
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
    asset_server: Res<AssetServer>,
    task_res: Option<ResMut<BmsLoadTask>>,
) {
    let Some(mut task) = task_res else {
        return;
    };

    if let Some(result) = check_ready(&mut task.0) {
        match result {
            Ok((processor, audio_paths)) => {
                // 加载音频资源
                let mut audio_handles = HashMap::new();

                // 由于Bevy Asset API的'static生命周期要求,需要构建完整路径字符串
                for (id, chosen) in audio_paths.iter() {
                    let path_str = chosen.to_string_lossy().to_string();
                    let asset_str = format!("fs://{}", path_str);
                    let ap = AssetPath::parse(&asset_str);
                    let handle: Handle<KiraAudioSource> = asset_server.load_override(ap);
                    audio_handles.insert(*id, handle);
                }

                // 创建处理器资源
                commands.insert_resource(BmsProcessorResource {
                    processor,
                    audio_handles,
                    audio_paths,
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

/// 处理图表事件并发送音频播放消息
fn process_chart_events(
    status: Option<ResMut<BmsProcessorResource>>,
    mut audio_messages: MessageWriter<AudioPlayMessage>,
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

    // 更新处理器并发送音频消息
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
            audio_messages.write(AudioPlayMessage {
                wav_id: *wav,
                is_bgm,
            });
        }
    }
}
