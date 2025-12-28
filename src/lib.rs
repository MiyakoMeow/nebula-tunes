//! Nebula Tunes library target used for WASM compilation checks.

pub(crate) mod chart;
pub mod config;
pub(crate) mod entry;
pub(crate) mod filesystem;
pub mod logging;
pub(crate) mod loops;
pub(crate) mod media;

#[cfg(not(target_arch = "wasm32"))]
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    thread,
};

#[cfg(not(target_arch = "wasm32"))]
use anyhow::Result;
use bms_rs::bms::prelude::Key;
use bytemuck::{Pod, Zeroable};
#[cfg(not(target_arch = "wasm32"))]
use futures_lite::future;

#[cfg(not(target_arch = "wasm32"))]
use crate::config::load_sys;
#[cfg(not(target_arch = "wasm32"))]
use crate::loops::{InputMsg, VisualMsg, audio, main_loop, visual};

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

#[cfg(not(target_arch = "wasm32"))]
/// 运行 Nebula Tunes 主程序入口
///
/// # Errors
///
/// - 读取或解析系统配置失败
/// - 加载谱面与资源索引失败
/// - winit/wgpu 初始化失败
pub fn run(bms_path: Option<PathBuf>) -> Result<()> {
    logging::init_logging();
    let sys = load_sys(Path::new("config_sys.toml"))?;
    let (pre_processor, pre_audio_paths, pre_bmp_paths, pre_bmp_types) =
        if let Some(bms_path) = bms_path {
            let (p, ap, bp, bt) = future::block_on(chart::bms::load_bms_and_collect_paths(
                bms_path,
                sys.judge.visible_travel,
            ))?;
            (Some(p), ap, bp, Some(bt))
        } else {
            (None, HashMap::new(), HashMap::new(), None)
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
            pre_bmp_types.unwrap_or_default(),
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

#[cfg(target_os = "wasi")]
/// WASM 构建冒烟检查入口
///
/// # Errors
///
/// - `getrandom` 获取随机数失败
pub fn wasm_smoke_checks() -> Result<(), getrandom::Error> {
    let _ = bms_rs::bms::default_config();
    let mut buf = [0u8; 16];
    getrandom::fill(&mut buf)?;
    Ok(())
}
