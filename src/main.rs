//! # Nebula Tunes 主程序

#![warn(missing_docs)]
#![warn(clippy::must_use_candidate)]
#![warn(clippy::must_use_unit)]
#![warn(clippy::redundant_clone)]
#![warn(clippy::redundant_closure_for_method_calls)]
#![warn(clippy::redundant_else)]
#![warn(clippy::redundant_feature_names)]

mod config;
mod filesystem;
mod loops;

use std::{collections::HashMap, path::Path, path::PathBuf};

use anyhow::Result;
use async_fs as afs;
use bms_rs::{bms::prelude::*, chart_process::prelude::*};
use bytemuck::{Pod, Zeroable};
use chardetng::EncodingDetector;
use clap::Parser;
use gametime::TimeSpan;
use tokio::sync::mpsc;
use winit::event_loop::EventLoop;

use crate::config::load_sys_config;
use crate::loops::{InputMsg, audio, main_loop, visual};

#[derive(Parser)]
struct ExecArgs {
    #[arg(long)]
    bms_path: Option<PathBuf>,
}

fn key_to_lane(key: Key) -> Option<usize> {
    match key {
        Key::Scratch(_) => Some(0),
        Key::Key(n) => match n {
            1..=7 => Some(n as usize),
            _ => None,
        },
        _ => None,
    }
}

#[repr(C)]
#[derive(Clone, Copy, Zeroable, Pod)]
/// 单个矩形实例（位置、大小、颜色）
pub struct Instance {
    pos: [f32; 2],
    size: [f32; 2],
    color: [f32; 4],
}

async fn load_bms_and_collect_paths(
    bms_path: PathBuf,
    travel: TimeSpan,
) -> Result<(BmsProcessor, HashMap<WavId, PathBuf>)> {
    let bms_bytes = afs::read(&bms_path).await?;
    let mut det = EncodingDetector::new();
    det.feed(&bms_bytes, true);
    let enc = det.guess(None, true);
    let (bms_str, _, _) = enc.decode(&bms_bytes);
    let BmsOutput { bms, warnings: _ } = bms_rs::bms::parse_bms(&bms_str, default_config());
    let bms = bms.unwrap();
    // print bms info
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

#[tokio::main]
async fn main() -> Result<()> {
    let sys = load_sys_config(Path::new("config_sys.toml"))?;
    let args = ExecArgs::parse();
    let event_loop = EventLoop::new()?;
    let (pre_processor, pre_audio_paths) = if let Some(bms_path) = args.bms_path {
        let (p, ap) = load_bms_and_collect_paths(bms_path, sys.judge.visible_travel).await?;
        (Some(p), ap)
    } else {
        (None, HashMap::new())
    };
    let (control_tx, control_rx) = mpsc::channel::<loops::ControlMsg>(1);
    let (visual_tx, visual_rx) = mpsc::channel::<Vec<Instance>>(2);
    let (input_tx, input_rx) = mpsc::channel::<InputMsg>(64);
    let (audio_tx, audio_rx) = mpsc::channel::<audio::AudioMsg>(64);
    let (audio_event_tx, audio_event_rx) = mpsc::channel::<audio::AudioEvent>(1);
    let _audio_handle = tokio::spawn(audio::run_audio_loop(audio_rx, audio_event_tx));
    let _main_handle = tokio::spawn(main_loop::run_main_loop(
        pre_processor,
        pre_audio_paths,
        control_rx,
        visual_tx,
        input_rx,
        main_loop::JudgeParams {
            travel: sys.judge.visible_travel,
            windows: sys.judge.windows(),
        },
        audio_tx,
        audio_event_rx,
    ));
    let mut handler = visual::Handler::new(visual_rx, control_tx, input_tx, sys.keys.lanes.clone());
    event_loop.run_app(&mut handler)?;
    Ok(())
}
