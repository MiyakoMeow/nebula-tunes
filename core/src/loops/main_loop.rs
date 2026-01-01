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
use crate::game_page::{GamePageBuilder, JudgeParams};
use crate::loops::audio::Msg;
use crate::loops::{ControlMsg, InputMsg, RawInputMsg, SystemKey, VisualMsg};
use crate::media::BgaDecodeCache;
use crate::pages::Page;
use crate::pages::PageManager;

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
) {
    // 等待启动信号
    match control_rx.recv() {
        Ok(ControlMsg::Start) => {}
        Ok(ControlMsg::FileSelected(_)) => {
            // 不应该在启动前收到文件选择消息
            return;
        }
        Err(_) => return,
    }

    // 创建按键映射器
    let key_map = crate::loops::key_map::KeyMap::new(key_codes);

    // 创建 PageManager
    let mut page_manager = PageManager::new(visual_tx.clone(), audio_tx.clone());

    // 配置 GamePage 不缓存（每次重新开始）
    page_manager.set_page_config(
        crate::pages::PageId::Game,
        crate::pages::PageConfig {
            cache_enabled: false,
        },
    );

    // 如果有 processor，直接创建游戏页面并设置
    if let Some(proc) = processor {
        let game_page = GamePageBuilder::new(
            proc,
            audio_paths,
            bmp_paths,
            bmp_types,
            bga_cache.clone(),
            judge.clone(),
        )
        .build_once();

        // 直接设置当前页面
        let _ = page_manager.set_current_page(game_page);
    } else {
        // 没有 processor，创建 Title 页面
        let title_page = crate::title_page::TitlePageBuilder::new().build_once();
        let _ = page_manager.set_current_page(title_page);
    }

    // 等待页面初始化完成（包括预加载）
    // 注意：预加载现在在 GamePage::on_init() 中执行

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
                let consumed = page_manager
                    .handle_input_consumed(&input_msg)
                    .unwrap_or(false);

                // 检查是否是 ESC 键且未被消费
                if !consumed && let InputMsg::SystemKey(SystemKey::Escape) = input_msg {
                    // 退出主循环
                    return;
                }
            }
        }

        // 处理控制消息（文件选择结果）
        while let Ok(ctrl_msg) = control_rx.try_recv() {
            match ctrl_msg {
                ControlMsg::FileSelected(Some(bms_path)) => {
                    // 加载 BMS 并切换到游戏页面
                    match load_bms_in_main_loop(bms_path, &judge, &bga_cache, &visual_tx, &audio_tx)
                    {
                        Ok(game_page) => {
                            let _ = page_manager.set_current_page(game_page);
                        }
                        Err(e) => {
                            debug!(error = %e, "加载 BMS 失败");
                        }
                    }
                }
                ControlMsg::FileSelected(None) => {
                    // 用户取消选择，保持在 Title 页面
                }
                ControlMsg::Start => {
                    // 启动消息，已在循环开始前处理
                }
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

/// 在主循环中加载 BMS 并创建游戏页面
fn load_bms_in_main_loop(
    bms_path: PathBuf,
    judge: &JudgeParams,
    bga_cache: &Arc<BgaDecodeCache>,
    _visual_tx: &mpsc::SyncSender<VisualMsg>,
    _audio_tx: &mpsc::SyncSender<Msg>,
) -> anyhow::Result<Box<dyn Page>> {
    use futures_lite::future;

    // 使用 block_on 将异步加载转为同步
    let (processor, audio_paths, bmp_paths, bmp_types) = future::block_on(
        crate::chart::bms::load_bms_and_collect_paths(bms_path, judge.travel),
    )?;

    // 创建游戏页面
    let game_page = GamePageBuilder::new(
        processor,
        audio_paths,
        bmp_paths,
        bmp_types,
        bga_cache.clone(),
        judge.clone(),
    )
    .build_once();

    Ok(game_page)
}
