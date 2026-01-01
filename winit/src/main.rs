//! # Nebula Tunes 主程序

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    thread,
};

use anyhow::Result;
use clap::Parser;
use futures_lite::future;

use nebula_tunes::{
    JudgeParams,
    chart::bms::load_bms_and_collect_paths,
    config::load_sys,
    logging,
    loops::{ControlMsg, RawInputMsg, VisualMsg, audio, main_loop, visual},
};

#[derive(Parser)]
/// 命令行参数
struct ExecArgs {
    #[arg(long)]
    /// 指定要加载的 BMS 文件路径
    bms_path: Option<PathBuf>,
}

fn main() -> Result<()> {
    let args = ExecArgs::parse();
    logging::init_logging();
    let sys = load_sys(Path::new("config_sys.toml"))?;
    let (pre_processor, pre_audio_paths, pre_bmp_paths, pre_bmp_types) =
        if let Some(bms_path) = args.bms_path {
            let (p, ap, bp, bt) = future::block_on(load_bms_and_collect_paths(
                bms_path,
                sys.judge.visible_travel,
            ))?;
            (Some(p), ap, bp, Some(bt))
        } else {
            (None, HashMap::new(), HashMap::new(), None)
        };
    let (control_tx, control_rx) = mpsc::sync_channel::<ControlMsg>(1);
    let (visual_tx, visual_rx) = mpsc::sync_channel::<VisualMsg>(2);
    let (raw_input_tx, raw_input_rx) = mpsc::sync_channel::<RawInputMsg>(64);
    let (audio_tx, audio_rx) = mpsc::sync_channel::<audio::Msg>(64);
    let (audio_event_tx, audio_event_rx) = mpsc::sync_channel::<audio::Event>(1);
    let bga_cache = Arc::new(visual::BgaDecodeCache::new());

    let _audio_thread = thread::spawn(move || {
        audio::run_audio_loop(audio_rx, audio_event_tx);
    });
    let bga_cache_for_main = bga_cache.clone();
    // 准备按键配置
    let key_strings: Vec<String> = sys.keys.lanes.into_iter().map(|k| k.0).collect();
    let _main_thread = thread::spawn(move || {
        main_loop::run(
            pre_processor,
            pre_audio_paths,
            pre_bmp_paths,
            pre_bmp_types.unwrap_or_default(),
            bga_cache_for_main,
            control_rx,
            visual_tx,
            raw_input_rx,
            key_strings,
            JudgeParams {
                travel: sys.judge.visible_travel,
                windows: sys.judge.windows(),
            },
            audio_tx,
            audio_event_rx,
        );
    });

    nebula_tunes_winit::run(visual_rx, control_tx, raw_input_tx, bga_cache)?;
    Ok(())
}
