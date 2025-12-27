//! # Nebula Tunes 主程序

mod chart;
mod config;
mod entry;
mod filesystem;
mod loops;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    thread,
};

use anyhow::Result;
use bms_rs::bms::prelude::Key;
use bytemuck::{Pod, Zeroable};
use clap::Parser;
use futures_lite::future;

use crate::config::load_sys;
use crate::loops::{InputMsg, VisualMsg, audio, main_loop, visual};

#[derive(Parser)]
/// 命令行参数
struct ExecArgs {
    #[arg(long)]
    /// 指定要加载的 BMS 文件路径
    bms_path: Option<PathBuf>,
}

/// 将按键映射到轨道索引
const fn key_to_lane(key: Key) -> Option<usize> {
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
    /// 中心坐标（x, y）
    pos: [f32; 2],
    /// 尺寸（宽, 高）
    size: [f32; 2],
    /// 颜色（RGBA）
    color: [f32; 4],
}

fn main() -> Result<()> {
    let sys = load_sys(Path::new("config_sys.toml"))?;
    let args = ExecArgs::parse();
    let (pre_processor, pre_audio_paths, pre_bmp_paths) = if let Some(bms_path) = args.bms_path {
        let (p, ap, bp) = future::block_on(chart::bms::load_bms_and_collect_paths(
            bms_path,
            sys.judge.visible_travel,
        ))?;
        (Some(p), ap, bp)
    } else {
        (None, HashMap::new(), HashMap::new())
    };
    let (control_tx, control_rx) = mpsc::sync_channel::<loops::ControlMsg>(1);
    let (visual_tx, visual_rx) = mpsc::sync_channel::<VisualMsg>(2);
    let (input_tx, input_rx) = mpsc::sync_channel::<InputMsg>(64);
    let (audio_tx, audio_rx) = mpsc::sync_channel::<audio::Msg>(64);
    let (audio_event_tx, audio_event_rx) = mpsc::sync_channel::<audio::Event>(1);
    let bga_cache = Arc::new(visual::BgaDecodeCache::new());
    let _audio_thread = thread::spawn(move || {
        audio::run_audio_loop(audio_rx, audio_event_tx);
    });
    let bga_cache_for_main = bga_cache.clone();
    let _main_thread = thread::spawn(move || {
        main_loop::run(
            pre_processor,
            pre_audio_paths,
            pre_bmp_paths,
            bga_cache_for_main,
            control_rx,
            visual_tx,
            input_rx,
            main_loop::JudgeParams {
                travel: sys.judge.visible_travel,
                windows: sys.judge.windows(),
            },
            audio_tx,
            audio_event_rx,
        );
    });
    entry::winit::run(visual_rx, control_tx, input_tx, sys.keys.lanes, bga_cache)?;
    Ok(())
}
