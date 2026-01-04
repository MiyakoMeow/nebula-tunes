//! 音频触发插件
//!
//! 桥接 BMS 处理和音频播放，将 Message 转换为 Message

use bevy::prelude::*;
use bms_rs::chart_process::prelude::WavId;

use crate::plugins::audio_manager::AudioPlayMessage;
use crate::schedule::AudioSchedule;

/// 音符触发消息
///
/// 当 BMS 处理器检测到音符触发时发送此消息
#[derive(Message, Clone, Debug)]
pub struct TriggeredNoteEvent {
    /// 音频 ID
    pub wav_id: WavId,
    /// 是否为 BGM
    pub is_bgm: bool,
}

/// 音频触发插件
///
/// 负责将 `TriggeredNoteEvent` 转换为 `AudioPlayMessage`
pub struct AudioTriggerPlugin;

impl Plugin for AudioTriggerPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<TriggeredNoteEvent>()
            .add_systems(AudioSchedule, convert_events_to_messages);
    }
}

/// 将触发消息转换为音频播放消息
fn convert_events_to_messages(
    mut triggered_events: MessageReader<TriggeredNoteEvent>,
    mut audio_messages: MessageWriter<AudioPlayMessage>,
) {
    for event in triggered_events.read() {
        audio_messages.write(AudioPlayMessage {
            wav_id: event.wav_id,
            is_bgm: event.is_bgm,
        });
    }
}
