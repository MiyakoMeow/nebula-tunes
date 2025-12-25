//! 主循环：推进 BMS 处理器并分发事件
//!
//! - 以固定间隔推进 `BmsProcessor::update`
//! - 将音频事件通过通道分发给音频循环
//! - 构建视觉实例列表并发送给视觉循环

use std::{collections::HashMap, path::PathBuf, time::Duration};

use bms_rs::chart_process::prelude::*;
use bms_rs::chart_process::types::PlayheadEvent;
use gametime::{TimeSpan, TimeStamp};
use tokio::sync::mpsc;

use crate::Instance;
use crate::loops::ControlMsg;
use crate::loops::audio::{AudioEvent, AudioMsg};
use crate::loops::visual::{base_instances, build_instances_for_processor};

/// 运行主循环
///
/// - `processor`：谱面处理器（可选）
/// - `audio_paths`：音频 ID 到路径的映射
/// - `control_rx`：启动控制消息接收端
/// - `visual_tx`：视觉实例帧发送端
/// - `audio_tx`：音频播放请求发送端
pub async fn run_main_loop(
    mut processor: Option<BmsProcessor>,
    audio_paths: HashMap<WavId, PathBuf>,
    mut control_rx: mpsc::Receiver<ControlMsg>,
    visual_tx: mpsc::Sender<Vec<Instance>>,
    audio_tx: mpsc::Sender<AudioMsg>,
    mut audio_event_rx: mpsc::Receiver<AudioEvent>,
) {
    match control_rx.recv().await {
        Some(ControlMsg::Start) => {}
        None => return,
    }
    // 预加载所有音频资源，并等待完成事件
    let files: Vec<PathBuf> = audio_paths.values().cloned().collect();
    let _ = audio_tx.send(AudioMsg::PreloadAll { files }).await;
    match audio_event_rx.recv().await {
        Some(AudioEvent::PreloadFinished) => {}
        None => return,
    }
    if let Some(p) = processor.as_mut() {
        p.start_play(TimeStamp::now());
    }
    let mut last_log_sec: u64 = 0;
    let mut audio_plays_this_sec: u32 = 0;
    let mut ticker = tokio::time::interval(Duration::from_millis(16));
    loop {
        ticker.tick().await;
        let now = TimeStamp::now();
        if let Some(p) = processor.as_mut() {
            let events: Vec<PlayheadEvent> = p.update(now).collect();
            for ev in &events {
                if let ChartEvent::Note {
                    side, key, wav_id, ..
                } = ev.event()
                {
                    if *side != PlayerSide::Player1 {
                        continue;
                    }
                    let Some(_idx) = crate::key_to_lane(*key) else {
                        continue;
                    };
                    if let Some(wav_id) = wav_id.as_ref()
                        && let Some(path) = audio_paths.get(wav_id)
                        && audio_tx.try_send(AudioMsg::Play(path.clone())).is_ok()
                    {
                        audio_plays_this_sec = audio_plays_this_sec.saturating_add(1);
                    }
                }
                if let ChartEvent::Bgm { wav_id } = ev.event()
                    && let Some(wav_id) = wav_id.as_ref()
                    && let Some(path) = audio_paths.get(wav_id)
                    && audio_tx.try_send(AudioMsg::Play(path.clone())).is_ok()
                {
                    audio_plays_this_sec = audio_plays_this_sec.saturating_add(1);
                }
            }
            let instances = build_instances_for_processor(p);
            let _ = visual_tx.try_send(instances);
            let Some(start) = p.started_at() else {
                continue;
            };
            let elapsed = now.checked_elapsed_since(start).unwrap_or(TimeSpan::ZERO);
            let sec = (elapsed.as_nanos().max(0) as u64) / 1_000_000_000;
            if sec != last_log_sec {
                let visible = p.visible_events().count();
                println!(
                    "elapsed={}s visible={} audio={}",
                    sec, visible, audio_plays_this_sec
                );
                audio_plays_this_sec = 0;
                last_log_sec = sec;
            }
        } else {
            let _ = visual_tx.try_send(base_instances());
        }
    }
}
