//! # Nebula Tunes 主程序

#![warn(missing_docs)]
#![warn(clippy::must_use_candidate)]
#![warn(clippy::must_use_unit)]
#![warn(clippy::redundant_clone)]
#![warn(clippy::redundant_closure_for_method_calls)]
#![warn(clippy::redundant_else)]
#![warn(clippy::redundant_feature_names)]

use std::{path::PathBuf, time::Duration};

use bevy::{platform::collections::HashMap, prelude::*};
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
    App::new()
        .insert_resource(args)
        .add_plugins(DefaultPlugins)
        .add_systems(Startup, load_bms_file)
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
    for (id, audio_path) in processor.audio_files() {
        let handle: Handle<AudioSource> = asset_server.load(audio_path.to_path_buf());
        audio_handles.insert(id, handle);
    }
    commands.insert_resource(BmsProcessStatus {
        processor,
        audio_handles,
    });
}
