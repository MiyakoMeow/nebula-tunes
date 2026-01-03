//! 音频管理插件
//!
//! 负责音频资源的加载、管理和播放控制

use bevy::prelude::*;
use bevy_kira_audio::{AudioApp, AudioChannel, AudioControl};

use crate::plugins::bms_processor::{AudioSystemSet, BmsProcessorResource};
use crate::resources::NowStamp;
use crate::schedule::AudioSchedule;

// 导入trait以访问BmsProcessor的方法
use bms_rs::chart_process::ChartProcessor;
use num_traits::ToPrimitive;

/// 音频播放消息
#[derive(Message, Clone)]
pub struct AudioPlayMessage {
    /// 音频ID
    pub wav_id: bms_rs::chart_process::prelude::WavId,
    /// 是否为BGM
    pub is_bgm: bool,
}

/// 音频管理插件
pub struct AudioManagerPlugin;

impl Plugin for AudioManagerPlugin {
    fn build(&self, app: &mut App) {
        app.add_audio_channel::<crate::plugins::bms_processor::BgmChannel>()
            .add_audio_channel::<crate::plugins::bms_processor::SfxChannel>()
            .add_message::<AudioPlayMessage>()
            .add_systems(
                AudioSchedule,
                (start_when_audio_ready, handle_audio_messages)
                    .chain()
                    .in_set(AudioSystemSet::AudioPlay),
            )
            .add_systems(AudioSchedule, print_playback_status);
    }
}

/// 等待音频资源就绪后开始播放
fn start_when_audio_ready(
    status: Option<ResMut<BmsProcessorResource>>,
    assets: Res<Assets<bevy_kira_audio::AudioSource>>,
    now_stamp: Res<NowStamp>,
) {
    let Some(mut status) = status else {
        return;
    };
    if status.started {
        return;
    }

    // 检查所有音频是否已加载
    let mut missing: Vec<bms_rs::chart_process::prelude::WavId> = Vec::new();
    for (id, handle) in &status.audio_handles {
        if assets.get(handle).is_none() {
            missing.push(*id);
        }
    }

    if missing.is_empty() {
        // 所有音频已加载,开始播放
        println!("✓ 所有音频资源已加载完成,开始播放");
        status.processor.start_play(now_stamp.0);
        status.started = true;
    } else if !status.warned_missing {
        // 警告缺失的音频
        for id in missing {
            if let Some(p) = status.audio_paths.get(&id) {
                eprintln!("音频未载入: #WAV{:03} -> {}", id.0, p.to_string_lossy());
            } else {
                eprintln!("音频未载入: #WAV{:03}", id.0);
            }
        }
        status.warned_missing = true;
    }
}

/// 处理音频播放消息
fn handle_audio_messages(
    status: Option<Res<BmsProcessorResource>>,
    assets: Res<Assets<bevy_kira_audio::AudioSource>>,
    bgm_channel: Res<AudioChannel<crate::plugins::bms_processor::BgmChannel>>,
    sfx_channel: Res<AudioChannel<crate::plugins::bms_processor::SfxChannel>>,
    mut messages: MessageReader<AudioPlayMessage>,
) {
    let Some(status) = status else {
        return;
    };
    if !status.started {
        return;
    }

    for message in messages.read() {
        let Some(handle) = status.audio_handles.get(&message.wav_id) else {
            continue;
        };
        if assets.get(handle).is_none() {
            continue;
        }

        if message.is_bgm {
            bgm_channel.play(handle.clone());
        } else {
            sfx_channel.play(handle.clone());
        }
    }
}

/// 播放状态资源
#[derive(Resource, Default)]
struct PlaybackStatusTimer {
    last_print: f32,
}

/// 打印播放状态
fn print_playback_status(
    status: Option<Res<crate::plugins::bms_processor::BmsProcessorResource>>,
    time: Res<Time>,
    mut timer: Local<PlaybackStatusTimer>,
) {
    let Some(status) = status else {
        return;
    };

    if !status.started {
        return;
    }

    // 每秒打印一次
    timer.last_print += time.delta_secs();
    if timer.last_print >= 1.0 {
        timer.last_print = 0.0;

        if let Some(_started_at) = status.processor.started_at() {
            let elapsed = time.elapsed().as_secs_f64();
            let playback_ratio = status.processor.playback_ratio();
            let bpm = status.processor.current_bpm();

            println!(
                "▶ 播放中 | 时间: {:.1}s | 播放比例: {:.3} | BPM: {:.1}",
                elapsed,
                playback_ratio.to_f64().unwrap_or(0.0),
                bpm.to_f64().unwrap_or(120.0)
            );
        }
    }
}
