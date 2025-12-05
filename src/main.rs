//! # Nebula Tunes 主程序

#![warn(missing_docs)]
#![warn(clippy::must_use_candidate)]
#![warn(clippy::must_use_unit)]
#![warn(clippy::redundant_clone)]
#![warn(clippy::redundant_closure_for_method_calls)]
#![warn(clippy::redundant_else)]
#![warn(clippy::redundant_feature_names)]

use std::{
    path::Path,
    path::PathBuf,
    time::{Duration, SystemTime},
};

use bevy::{
    asset::{AssetPath, io::AssetSourceBuilder},
    audio::{AudioPlayer, AudioSource, PlaybackSettings},
    platform::collections::HashMap,
    prelude::*,
};
use bms_rs::{bms::prelude::*, chart_process::prelude::*};
use chardetng::EncodingDetector;
use clap::Parser;

mod test_archive_plugin;
use test_archive_plugin::TestArchivePlugin;

fn main() {
    let args = ExecArgs::parse();
    // 测试模式下使用 MinimalPlugins，否则使用 DefaultPlugins
    if args.test_archive_path.is_some() {
        App::new()
            .insert_resource(args)
            .add_plugins(MinimalPlugins)
            .add_plugins(TestArchivePlugin)
            .run();
        return;
    };
    // 正常模式下使用 DefaultPlugins
    let mut app = App::new();
    let bms_dir = args
        .bms_path
        .as_ref()
        .and_then(|p| p.parent().map(Path::to_path_buf));
    if let Some(dir) = bms_dir.as_ref() {
        app.register_asset_source(
            "bms",
            AssetSourceBuilder::platform_default(&dir.to_string_lossy(), None),
        );
    }
    app.insert_resource(args)
        .add_plugins(DefaultPlugins)
        .add_systems(Startup, load_bms_file)
        .add_systems(Update, (start_when_audio_ready, process_chart_events))
        .run();
}

#[derive(Parser, Resource)]
struct ExecArgs {
    #[arg(long)]
    test_archive_path: Option<PathBuf>,
    #[arg(long)]
    bms_path: Option<PathBuf>,
}

#[derive(Resource)]
struct BmsProcessStatus {
    processor: BmsProcessor,
    audio_handles: HashMap<WavId, Handle<AudioSource>>,
    audio_paths: HashMap<WavId, PathBuf>,
    started: bool,
}

fn load_bms_file(mut commands: Commands, asset_server: Res<AssetServer>, args: Res<ExecArgs>) {
    let Some(bms_path) = args.bms_path.as_ref() else {
        return;
    };
    let bms_bytes = std::fs::read(bms_path).unwrap();
    // Parse Bms
    let mut det = EncodingDetector::new();
    det.feed(&bms_bytes, true);
    let enc = det.guess(None, true);
    let (bms_str, _, _) = enc.decode(&bms_bytes);
    let BmsOutput { bms, warnings: _ } = bms_rs::bms::parse_bms(&bms_str, default_config());
    let bms = bms.unwrap();
    // Setup Processor
    let base_bpm = StartBpmGenerator
        .generate(&bms)
        .unwrap_or(BaseBpm(120.0.into()));
    let processor =
        BmsProcessor::new::<KeyLayoutBeat>(&bms, base_bpm, Duration::from_secs_f32(0.6));
    // Load audio
    let mut audio_handles = HashMap::new();
    let mut audio_paths = HashMap::new();
    let bms_dir = bms_path.parent().unwrap_or(Path::new("."));
    for (id, audio_path) in processor.audio_files() {
        let audio_abs_path = bms_dir.join(audio_path);
        let ap = AssetPath::from_path(&audio_abs_path);
        let handle: Handle<AudioSource> = asset_server.load(ap);
        audio_handles.insert(id, handle);
        audio_paths.insert(id, audio_abs_path);
    }
    commands.insert_resource(BmsProcessStatus {
        processor,
        audio_handles,
        audio_paths,
        started: false,
    });
}

fn start_when_audio_ready(mut status: ResMut<BmsProcessStatus>, assets: Res<Assets<AudioSource>>) {
    if status.started {
        return;
    }
    let mut missing: Vec<WavId> = Vec::new();
    for (id, handle) in &status.audio_handles {
        if assets.get(handle).is_none() {
            missing.push(*id);
        }
    }
    if missing.is_empty() {
        status.processor.start_play(SystemTime::now());
        status.started = true;
    } else {
        for id in missing {
            if let Some(p) = status.audio_paths.get(&id) {
                eprintln!("音频未载入: #WAV{:03} -> {}", id.0, p.to_string_lossy());
            } else {
                eprintln!("音频未载入: #WAV{:03}", id.0);
            }
        }
    }
}

fn process_chart_events(
    mut commands: Commands,
    mut status: ResMut<BmsProcessStatus>,
    assets: Res<Assets<AudioSource>>,
) {
    if !status.started {
        return;
    }
    let now = SystemTime::now();
    let handles = status.audio_handles.clone();
    let paths = status.audio_paths.clone();
    for evp in status.processor.update(now) {
        match evp.event() {
            ChartEvent::Note {
                wav_id: Some(wav), ..
            }
            | ChartEvent::Bgm { wav_id: Some(wav) } => {
                if let Some(handle) = handles.get(wav) {
                    if assets.get(handle).is_some() {
                        commands
                            .spawn((AudioPlayer::new(handle.clone()), PlaybackSettings::DESPAWN));
                    } else if let Some(p) = paths.get(wav) {
                        eprintln!("音频尚未就绪: #WAV{:03} -> {}", wav.0, p.to_string_lossy());
                    } else {
                        eprintln!("音频尚未就绪: #WAV{:03}", wav.0);
                    }
                } else {
                    eprintln!("缺少音频句柄: #WAV{:03}", wav.0);
                }
            }
            _ => {}
        }
    }
}
