//! 音符渲染插件
//!
//! 负责音符的可视化渲染和场景管理

use std::collections::HashMap;

use bevy::prelude::*;
use bms_rs::{bms::prelude::*, chart_process::prelude::*};
use num_traits::ToPrimitive;

use crate::components::NoteMarker;
use crate::plugins::bms_processor::BmsProcessorResource;
use crate::resources::NowStamp;

/// 轨道数量
const LANE_COUNT: usize = 8;
/// 轨道宽度
const LANE_WIDTH: f32 = 60.0;
/// 轨道间距
const LANE_GAP: f32 = 8.0;
/// 可见高度
const VISIBLE_HEIGHT: f32 = 600.0;
/// 音符高度
const NOTE_HEIGHT: f32 = 12.0;

/// 图谱视觉状态
#[derive(Resource, Default)]
pub struct ChartVisualState {
    /// 音符事件ID到实体的映射
    pub notes: HashMap<ChartEventId, Entity>,
}

/// 音符渲染插件
pub struct NoteRendererPlugin;

impl Plugin for NoteRendererPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ChartVisualState>()
            .add_systems(Startup, setup_note_scene)
            .add_systems(Update, render_visible_chart);
    }
}

/// 计算总宽度
fn total_width() -> f32 {
    LANE_COUNT as f32 * LANE_WIDTH + (LANE_COUNT as f32 - 1.0) * LANE_GAP
}

/// 计算轨道X坐标
fn lane_x(idx: usize) -> f32 {
    let left = -total_width() / 2.0 + LANE_WIDTH / 2.0;
    left + idx as f32 * (LANE_WIDTH + LANE_GAP)
}

/// 将Key转换为轨道索引
fn key_to_lane(key: Key) -> Option<usize> {
    match key {
        Key::Scratch(_) => Some(0),
        Key::Key(n) => match n {
            1..=7 => Some(n as usize),
            _ => None,
        },
        _ => None,
    }
}

/// 设置音符场景
fn setup_note_scene(mut commands: Commands) {
    // 创建相机
    commands.spawn((Camera2d, Transform::default(), GlobalTransform::default()));

    // 创建轨道背景
    for i in 0..LANE_COUNT {
        commands.spawn((
            Sprite {
                color: Color::srgb(0.15, 0.15, 0.18),
                custom_size: Some(Vec2::new(LANE_WIDTH, VISIBLE_HEIGHT)),
                ..Default::default()
            },
            Transform::from_xyz(lane_x(i), 0.0, 0.0),
            GlobalTransform::default(),
            Visibility::default(),
            InheritedVisibility::default(),
        ));
    }

    // 创建判定线
    commands.spawn((
        Sprite {
            color: Color::srgb(0.9, 0.9, 0.9),
            custom_size: Some(Vec2::new(total_width(), 4.0)),
            ..Default::default()
        },
        Transform::from_xyz(0.0, -VISIBLE_HEIGHT / 2.0 + 2.0, 1.0),
        GlobalTransform::default(),
        Visibility::default(),
        InheritedVisibility::default(),
    ));
}

/// 渲染可见音符
fn render_visible_chart(
    mut commands: Commands,
    status: Option<ResMut<BmsProcessorResource>>,
    mut vis: ResMut<ChartVisualState>,
    mut q_notes: Query<(&mut Transform, &mut Visibility), With<NoteMarker>>,
    _now_stamp: Res<NowStamp>,
) {
    let Some(mut status) = status else {
        return;
    };
    if !status.started {
        return;
    }

    let mut alive: Vec<ChartEventId> = Vec::new();

    // 渲染可见音符
    for ev in status.processor.visible_events() {
        let (playhead_event, range) = ev;

        // 只处理音符事件
        let ChartEvent::Note { side, key, .. } = playhead_event.event() else {
            continue;
        };

        // 只处理P1侧
        if *side != PlayerSide::Player1 {
            continue;
        }

        // 获取轨道索引
        let Some(idx) = key_to_lane(*key) else {
            continue;
        };

        let x = lane_x(idx);
        let ratio_value = range.start().as_ref();
        let y = -VISIBLE_HEIGHT / 2.0
            + ToPrimitive::to_f64(ratio_value).unwrap_or(0.0) as f32 * VISIBLE_HEIGHT;

        // 更新或创建音符实体
        if let Some(entity) = vis.notes.get(&playhead_event.id()) {
            if let Ok((mut tf, mut v)) = q_notes.get_mut(*entity) {
                tf.translation.x = x;
                tf.translation.y = y;
                *v = Visibility::Visible;
            }
        } else {
            let entity = commands
                .spawn((
                    Sprite {
                        color: Color::srgb(0.3, 0.7, 1.0),
                        custom_size: Some(Vec2::new(LANE_WIDTH - 4.0, NOTE_HEIGHT)),
                        ..Default::default()
                    },
                    Transform::from_xyz(x, y, 2.0),
                    GlobalTransform::default(),
                    Visibility::default(),
                    InheritedVisibility::default(),
                    NoteMarker,
                ))
                .id();
            vis.notes.insert(playhead_event.id(), entity);
        }

        alive.push(playhead_event.id());
    }

    // 隐藏过时音符
    let obsolete: Vec<ChartEventId> = vis
        .notes
        .keys()
        .filter(|id| !alive.contains(id))
        .cloned()
        .collect();

    for id in obsolete {
        if let Some(&entity) = vis.notes.get(&id)
            && let Ok((_, mut v)) = q_notes.get_mut(entity)
        {
            *v = Visibility::Hidden;
        }
    }
}
