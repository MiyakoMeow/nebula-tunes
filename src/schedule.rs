//! 自定义 Schedule 定义
//!
//! 用于分离 BMS 处理、音频播放

use bevy::ecs::schedule::ScheduleLabel;

/// BMS 逻辑处理 Schedule
///
/// 负责 BMS 文件解析、状态更新和消息生成
#[derive(ScheduleLabel, Debug, Hash, PartialEq, Eq, Clone)]
pub struct LogicSchedule;

/// 音频处理 Schedule
///
/// 负责音频播放控制
#[derive(ScheduleLabel, Debug, Hash, PartialEq, Eq, Clone)]
pub struct AudioSchedule;
