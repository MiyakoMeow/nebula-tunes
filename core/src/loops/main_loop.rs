//! 主循环：推进 BMS 处理器并分发事件
//!
//! - 以固定间隔推进 `BmsProcessor::update`
//! - 将音频事件通过通道分发给音频循环
//! - 构建视觉实例列表并发送给视觉循环
//! - 通过 `PageManager` 管理页面生命周期和切换

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};

use bms_rs::chart_process::prelude::*;
use tracing::debug;

use crate::chart::bms::BgaFileType;
use crate::game_page::JudgeParams;
use crate::game_page_builder::GamePageBuilder;
use crate::loops::audio::{Event, Msg};
use crate::loops::visual::{BgaDecodeCache, preload_bga_files};
use crate::loops::{ControlMsg, RawInputMsg, VisualMsg};
use crate::pages_manager::PageManager;

/// 运行主循环
///
/// # 参数
///
/// * `processor` - 谱面处理器
/// * `audio_paths` - 音频 ID 到路径的映射
/// * `bmp_paths` - BGA 图片 ID 到路径的映射
/// * `bmp_types` - BGA 图片 ID 到文件类型的映射
/// * `bga_cache` - BGA 解码缓存
/// * `control_rx` - 启动控制消息接收端
/// * `visual_tx` - 视觉实例帧发送端
/// * `raw_input_rx` - 原始输入消息接收端
/// * `key_codes` - 按键代码字符串列表
/// * `judge` - 判定参数
/// * `audio_tx` - 音频播放请求发送端
/// * `audio_event_rx` - 音频事件接收端
#[allow(clippy::too_many_arguments)]
pub fn run(
    processor: Option<BmsProcessor>,
    audio_paths: HashMap<WavId, PathBuf>,
    bmp_paths: HashMap<BmpId, PathBuf>,
    bmp_types: HashMap<BmpId, BgaFileType>,
    bga_cache: Arc<BgaDecodeCache>,
    control_rx: mpsc::Receiver<ControlMsg>,
    visual_tx: mpsc::SyncSender<VisualMsg>,
    raw_input_rx: mpsc::Receiver<RawInputMsg>,
    key_codes: Vec<String>,
    judge: JudgeParams,
    audio_tx: mpsc::SyncSender<Msg>,
    audio_event_rx: mpsc::Receiver<Event>,
) {
    // 等待启动信号
    match control_rx.recv() {
        Ok(ControlMsg::Start) => {}
        Err(_) => return,
    }

    // 创建按键映射器
    let key_map = crate::loops::key_map::KeyMap::new(key_codes);

    // 预加载音频文件
    let files: Vec<PathBuf> = audio_paths.values().cloned().collect();
    let _ = audio_tx.send(Msg::PreloadAll { files });

    // 预加载 BGA 文件
    let bmp_files: Vec<PathBuf> = bmp_paths.values().cloned().collect();
    let bga_cache_for_thread = bga_cache.clone();
    let bga_preload = thread::spawn(move || preload_bga_files(bga_cache_for_thread, bmp_files));

    // 等待音频预加载完成
    match audio_event_rx.recv() {
        Ok(Event::PreloadFinished) => {}
        Err(_) => return,
    }
    let _ = bga_preload.join();

    // 创建 PageManager
    let mut page_manager = PageManager::new(visual_tx.clone(), audio_tx);

    // 如果有 processor，直接创建游戏页面并设置
    if let Some(proc) = processor {
        let game_page =
            GamePageBuilder::new(proc, audio_paths, bmp_paths, bmp_types, bga_cache, judge)
                .build_once();

        // 直接设置当前页面
        let _ = page_manager.set_current_page(game_page);
    }

    // 主循环
    let tick = Duration::from_millis(16);
    let mut next_tick = Instant::now();
    let mut last_log_sec: u64 = 0;

    loop {
        // 计算下一帧时间
        let Some(t) = next_tick.checked_add(tick) else {
            next_tick = Instant::now();
            continue;
        };
        next_tick = t;

        let now_instant = Instant::now();
        if let Some(wait) = next_tick.checked_duration_since(now_instant) {
            thread::sleep(wait);
        } else {
            next_tick = now_instant;
        }

        // 处理原始输入
        while let Ok(raw_msg) = raw_input_rx.try_recv() {
            if let Some(input_msg) = key_map.convert(raw_msg) {
                let _ = page_manager.handle_input(&input_msg);
            }
        }

        // 更新页面
        let should_continue = match page_manager.update() {
            Ok(continue_flag) => continue_flag,
            Err(e) => {
                debug!(error = %e, "页面更新失败");
                false
            }
        };

        if !should_continue {
            break;
        }

        // 获取渲染实例并发送
        let instances = page_manager.render();
        let _ = visual_tx.try_send(VisualMsg::Instances(instances));

        // 每秒输出性能日志
        let now = std::time::SystemTime::now();
        let duration = now
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let sec = duration.as_secs();

        if sec != last_log_sec {
            debug!(elapsed_sec = sec, "主循环运行中");
            last_log_sec = sec;
        }
    }
}
