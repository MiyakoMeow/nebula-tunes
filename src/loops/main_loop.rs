//! 主循环：推进 BMS 处理器并分发事件
//!
//! - 以固定间隔推进 `BmsProcessor::update`
//! - 将音频事件通过通道分发给音频循环
//! - 构建视觉实例列表并发送给视觉循环

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use bms_rs::chart_process::prelude::*;
use bms_rs::chart_process::types::PlayheadEvent;
use gametime::{TimeSpan, TimeStamp};

use crate::loops::audio::{Event, Msg};
use crate::loops::visual::{
    BgaDecodeCache, base_instances, build_instances_for_processor_with_state,
};
use crate::loops::{BgaLayer as VisualBgaLayer, ControlMsg, InputMsg, VisualMsg};

/// 预先解码所有 BGA 图片到缓存，并每秒输出一次进度
fn preload_bga_files(cache: Arc<BgaDecodeCache>, files: Vec<PathBuf>) {
    let mut unique = HashSet::new();
    for p in files {
        unique.insert(p);
    }
    let paths: Vec<PathBuf> = unique.into_iter().collect();
    let total = paths.len() as u32;
    if total == 0 {
        println!("BGA预加载进度：0/0");
        println!("BGA预加载完成");
        return;
    }
    let loaded = Arc::new(AtomicU32::new(0));
    let done = Arc::new(AtomicBool::new(false));

    let loaded_for_log = loaded.clone();
    let done_for_log = done.clone();
    let logger = thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_secs(1));
            let c = loaded_for_log.load(Ordering::Relaxed);
            println!("BGA预加载进度：{}/{}", c, total);
            if done_for_log.load(Ordering::Relaxed) {
                break;
            }
        }
    });

    let (work_tx, work_rx) = std::sync::mpsc::channel::<PathBuf>();
    let work_rx = Arc::new(std::sync::Mutex::new(work_rx));
    let workers = thread::available_parallelism()
        .map(std::num::NonZero::get)
        .unwrap_or(1)
        .clamp(1, 8);

    let mut handles = Vec::with_capacity(workers);
    for _ in 0..workers {
        let work_rx = work_rx.clone();
        let cache = cache.clone();
        let loaded = loaded.clone();
        handles.push(thread::spawn(move || {
            loop {
                let path = {
                    let Ok(work_rx) = work_rx.lock() else {
                        break;
                    };
                    match work_rx.recv() {
                        Ok(p) => p,
                        Err(_) => break,
                    }
                };
                if cache.get(path.as_path()).is_some() {
                    loaded.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
                let decoded = (|| -> Option<(Vec<u8>, u32, u32)> {
                    let bytes = std::fs::read(&path).ok()?;
                    let img = image::load_from_memory(&bytes).ok()?;
                    let rgba = img.to_rgba8();
                    let w = rgba.width();
                    let h = rgba.height();
                    Some((rgba.into_raw(), w, h))
                })();
                if let Some((rgba, w, h)) = decoded {
                    let _ = cache.insert(path, rgba, w, h);
                }
                loaded.fetch_add(1, Ordering::Relaxed);
            }
        }));
    }

    for path in paths {
        let _ = work_tx.send(path);
    }
    drop(work_tx);

    for h in handles {
        let _ = h.join();
    }
    done.store(true, Ordering::Relaxed);
    let _ = logger.join();
    println!("BGA预加载完成");
}

/// 判定配置参数
pub struct JudgeParams {
    /// 可见区域的时间跨度
    pub travel: TimeSpan,
    /// 各判定等级的时间窗口（从严到宽）
    pub windows: [TimeSpan; 4],
}

/// 游戏运行时状态
struct GameState {
    /// 当前 8 轨按键按下状态
    pressed: [bool; 8],
    /// 血条值 [0.0, 1.0]
    gauge: f32,
    /// 连击数
    combo: u32,
}

/// 运行主循环
///
/// - `processor`：谱面处理器（可选）
/// - `audio_paths`：音频 ID 到路径的映射
/// - `control_rx`：启动控制消息接收端
/// - `visual_tx`：视觉实例帧发送端
/// - `audio_tx`：音频播放请求发送端
#[allow(clippy::too_many_arguments)]
pub fn run(
    mut processor: Option<BmsProcessor>,
    audio_paths: HashMap<WavId, PathBuf>,
    bmp_paths: HashMap<BmpId, PathBuf>,
    bga_cache: Arc<BgaDecodeCache>,
    control_rx: mpsc::Receiver<ControlMsg>,
    visual_tx: mpsc::SyncSender<VisualMsg>,
    input_rx: mpsc::Receiver<InputMsg>,
    judge: JudgeParams,
    audio_tx: mpsc::SyncSender<Msg>,
    audio_event_rx: mpsc::Receiver<Event>,
) {
    match control_rx.recv() {
        Ok(ControlMsg::Start) => {}
        Err(_) => return,
    }
    let files: Vec<PathBuf> = audio_paths.values().cloned().collect();
    let _ = audio_tx.send(Msg::PreloadAll { files });

    let bmp_files: Vec<PathBuf> = bmp_paths.values().cloned().collect();
    let bga_preload = thread::spawn(move || preload_bga_files(bga_cache, bmp_files));

    match audio_event_rx.recv() {
        Ok(Event::PreloadFinished) => {}
        Err(_) => return,
    }
    let _ = bga_preload.join();

    if let Some(p) = processor.as_mut() {
        p.start_play(TimeStamp::now());
    }
    let mut state = GameState {
        pressed: [false; 8],
        gauge: 0.5,
        combo: 0,
    };
    let mut last_log_sec: u64 = 0;
    let mut audio_plays_this_sec: u32 = 0;
    let tick = Duration::from_millis(16);
    let mut next_tick = Instant::now();
    loop {
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

        let now = TimeStamp::now();
        let Some(p) = processor.as_mut() else {
            let _ = visual_tx.try_send(VisualMsg::Instances(base_instances()));
            continue;
        };
        loop {
            match input_rx.try_recv() {
                Ok(InputMsg::KeyDown(idx)) => {
                    if let Some(flag) = state.pressed.get_mut(idx) {
                        *flag = true;
                    }
                    let mut best: Option<(PlayheadEvent, f32)> = None;
                    for (ev, ratio) in p.visible_events() {
                        let ChartEvent::Note {
                            side,
                            key,
                            wav_id: _,
                            ..
                        } = ev.event()
                        else {
                            continue;
                        };
                        if *side != PlayerSide::Player1 {
                            continue;
                        }
                        let Some(lane) = crate::key_to_lane(*key) else {
                            continue;
                        };
                        if lane != idx {
                            continue;
                        }
                        let r = ratio.as_f64() as f32;
                        if !(0.0..=1.0).contains(&r) {
                            continue;
                        }
                        if let Some((_, br)) = &best {
                            if r < *br {
                                best = Some((ev.clone(), r));
                            }
                        } else {
                            best = Some((ev.clone(), r));
                        }
                    }
                    let Some((ev, r)) = best else {
                        continue;
                    };
                    let nanos = (judge.travel.as_nanos() as f64 * r as f64).max(0.0) as u64;
                    let dt = TimeSpan::from_duration(std::time::Duration::from_nanos(nanos));
                    let judge = if dt.as_nanos() <= judge.windows[0].as_nanos() {
                        4
                    } else if dt.as_nanos() <= judge.windows[1].as_nanos() {
                        3
                    } else if dt.as_nanos() <= judge.windows[2].as_nanos() {
                        2
                    } else if dt.as_nanos() <= judge.windows[3].as_nanos() {
                        1
                    } else {
                        0
                    };
                    match judge {
                        4 | 3 => {
                            state.combo = state.combo.saturating_add(1);
                            state.gauge = (state.gauge + 0.02).min(1.0);
                            if let ChartEvent::Note { wav_id, .. } = ev.event()
                                && let Some(wav_id) = wav_id.as_ref()
                                && let Some(path) = audio_paths.get(wav_id)
                                && audio_tx.try_send(Msg::Play(path.clone())).is_ok()
                            {
                                audio_plays_this_sec = audio_plays_this_sec.saturating_add(1);
                            }
                        }
                        2 => {
                            state.combo = state.combo.saturating_add(1);
                            state.gauge = (state.gauge + 0.01).min(1.0);
                            if let ChartEvent::Note { wav_id, .. } = ev.event()
                                && let Some(wav_id) = wav_id.as_ref()
                                && let Some(path) = audio_paths.get(wav_id)
                                && audio_tx.try_send(Msg::Play(path.clone())).is_ok()
                            {
                                audio_plays_this_sec = audio_plays_this_sec.saturating_add(1);
                            }
                        }
                        1 => {
                            state.combo = 0;
                            state.gauge = (state.gauge - 0.03).max(0.0);
                            let _ = visual_tx.try_send(VisualMsg::BgaPoorTrigger);
                        }
                        _ => {
                            state.combo = 0;
                            state.gauge = (state.gauge - 0.05).max(0.0);
                            let _ = visual_tx.try_send(VisualMsg::BgaPoorTrigger);
                        }
                    }
                }
                Ok(InputMsg::KeyUp(idx)) => {
                    if let Some(flag) = state.pressed.get_mut(idx) {
                        *flag = false;
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
        }
        let events: Vec<PlayheadEvent> = p.update(now).collect();
        for ev in &events {
            if let ChartEvent::Note {
                side,
                key,
                wav_id: _,
                ..
            } = ev.event()
            {
                if *side != PlayerSide::Player1 {
                    continue;
                }
                let Some(_idx) = crate::key_to_lane(*key) else {
                    continue;
                };
            }
            if let ChartEvent::BgaChange { layer, bmp_id } = ev.event()
                && let Some(bmp_id) = bmp_id.as_ref()
                && let Some(path) = bmp_paths.get(bmp_id)
            {
                let mapped_layer = match layer {
                    BgaLayer::Base => VisualBgaLayer::Bga,
                    BgaLayer::Overlay => VisualBgaLayer::Layer,
                    BgaLayer::Overlay2 => VisualBgaLayer::Layer2,
                    BgaLayer::Poor => VisualBgaLayer::Poor,
                    _ => VisualBgaLayer::Bga,
                };
                let _ = visual_tx.try_send(VisualMsg::BgaChange {
                    layer: mapped_layer,
                    path: path.clone(),
                });
            }
            if let ChartEvent::Bgm { wav_id } = ev.event()
                && let Some(wav_id) = wav_id.as_ref()
                && let Some(path) = audio_paths.get(wav_id)
                && audio_tx.try_send(Msg::Play(path.clone())).is_ok()
            {
                audio_plays_this_sec = audio_plays_this_sec.saturating_add(1);
            }
        }
        let instances = build_instances_for_processor_with_state(p, state.pressed, state.gauge);
        let _ = visual_tx.try_send(VisualMsg::Instances(instances));
        let Some(start) = p.started_at() else {
            continue;
        };
        let elapsed = now.checked_elapsed_since(start).unwrap_or(TimeSpan::ZERO);
        let sec = (elapsed.as_nanos().max(0) as u64) / 1_000_000_000;
        if sec != last_log_sec {
            let visible = p.visible_events().count();
            let mut min_r: f32 = 1.0;
            let mut max_r: f32 = 0.0;
            for (_, r) in p.visible_events() {
                let rf = r.as_f64() as f32;
                if rf < min_r {
                    min_r = rf;
                }
                if rf > max_r {
                    max_r = rf;
                }
            }
            println!(
                "elapsed={}s visible={} ratio=[{:.2},{:.2}] audio={}",
                sec, visible, min_r, max_r, audio_plays_this_sec
            );
            audio_plays_this_sec = 0;
            last_log_sec = sec;
        }
    }
}
