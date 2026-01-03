//! 跨插件组件定义
//!
//! 定义所有跨插件使用的Component类型

use bevy::prelude::*;

/// 音符标记组件
#[derive(Component)]
pub struct NoteMarker;

/// 池化音符组件
#[derive(Component)]
pub struct PooledNote {
    /// 音符状态
    pub state: NoteState,
    /// 关联的图表事件ID
    pub event_id: Option<bms_rs::chart_process::prelude::ChartEventId>,
}

/// 音符状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteState {
    /// 活跃状态(可见)
    Active,
    /// 隐藏状态
    Hidden,
}
