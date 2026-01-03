//! 音符渲染插件
//!
//! 负责音符的可视化渲染和场景管理

use std::collections::HashMap;

use bevy::prelude::*;
use bms_rs::{bms::prelude::*, chart_process::prelude::*};
use num_traits::ToPrimitive;

use crate::components::{NoteMarker, NoteState, PooledNote};
use crate::plugins::bms_processor::BmsProcessorResource;
use crate::resources::NowStamp;
use crate::schedule::RenderSchedule;

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
/// 对象池初始大小
const POOL_INITIAL_SIZE: usize = 500;

/// 音符池状态
#[derive(Resource, Default)]
pub struct NotePoolState {
    /// 可用的实体池
    available: Vec<Entity>,
    /// 活跃音符: `ChartEventId` -> Entity
    active: HashMap<ChartEventId, Entity>,
    /// 实体到事件ID的反向映射
    entity_to_event: HashMap<Entity, ChartEventId>,
}

/// 图谱视觉状态
#[derive(Resource, Default)]
pub struct ChartVisualState {
    /// 音符事件ID到实体的映射（保留用于兼容）
    pub notes: HashMap<ChartEventId, Entity>,
}

/// 音符渲染插件
pub struct NoteRendererPlugin;

impl Plugin for NoteRendererPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NotePoolState>()
            .init_resource::<ChartVisualState>()
            .add_systems(Startup, (setup_note_scene, initialize_note_pool))
            .add_systems(RenderSchedule, render_visible_chart)
            .add_systems(RenderSchedule, print_pool_stats);
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

/// 初始化音符对象池
fn initialize_note_pool(mut commands: Commands, mut pool: ResMut<NotePoolState>) {
    println!("✓ 初始化音符对象池: {} 个实体", POOL_INITIAL_SIZE);

    for _ in 0..POOL_INITIAL_SIZE {
        let entity = commands
            .spawn((
                Sprite {
                    color: Color::srgb(0.3, 0.7, 1.0),
                    custom_size: Some(Vec2::new(LANE_WIDTH - 4.0, NOTE_HEIGHT)),
                    ..Default::default()
                },
                Transform::from_xyz(0.0, 0.0, 2.0),
                GlobalTransform::default(),
                Visibility::Hidden,
                InheritedVisibility::default(),
                NoteMarker,
                PooledNote {
                    state: NoteState::Hidden,
                    event_id: None,
                },
            ))
            .id();
        pool.available.push(entity);
    }
}

/// 渲染可见音符（使用对象池）
fn render_visible_chart(
    status: Option<ResMut<BmsProcessorResource>>,
    mut pool: ResMut<NotePoolState>,
    mut vis: ResMut<ChartVisualState>,
    mut q_notes: Query<(&mut Transform, &mut Visibility, &mut PooledNote), With<NoteMarker>>,
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

        let event_id = playhead_event.id();

        // 检查音符是否已经在活跃列表中
        if let Some(&entity) = pool.active.get(&event_id) {
            // 更新现有音符的位置和可见性
            if let Ok((mut tf, mut v, mut note)) = q_notes.get_mut(entity) {
                tf.translation.x = x;
                tf.translation.y = y;
                *v = Visibility::Visible;
                note.state = NoteState::Active;
            }
            alive.push(event_id);
            continue;
        }

        // 从对象池中获取一个可用实体
        if let Some(&entity) = pool.available.last() {
            pool.available.pop();

            // 更新实体组件
            if let Ok((mut tf, mut v, mut note)) = q_notes.get_mut(entity) {
                tf.translation.x = x;
                tf.translation.y = y;
                *v = Visibility::Visible;
                note.state = NoteState::Active;
                note.event_id = Some(event_id);
            }

            // 加入活跃列表
            pool.active.insert(event_id, entity);
            pool.entity_to_event.insert(entity, event_id);
            vis.notes.insert(event_id, entity);
            alive.push(event_id);
        }
    }

    // 回收过时音符到对象池
    let obsolete: Vec<ChartEventId> = pool
        .active
        .keys()
        .filter(|id| !alive.contains(id))
        .cloned()
        .collect();

    for event_id in obsolete {
        if let Some(&entity) = pool.active.get(&event_id) {
            // 隐藏音符
            if let Ok((_, mut v, mut note)) = q_notes.get_mut(entity) {
                *v = Visibility::Hidden;
                note.state = NoteState::Hidden;
                note.event_id = None;
            }

            // 从活跃列表移除，加入可用池
            pool.available.push(entity);
            pool.active.remove(&event_id);
            pool.entity_to_event.remove(&entity);
            vis.notes.remove(&event_id);
        }
    }
}

/// 打印对象池统计信息
fn print_pool_stats(pool: Res<NotePoolState>, time: Res<Time>, mut timer: Local<f32>) {
    // 每5秒打印一次统计信息
    *timer += time.delta_secs();
    if *timer >= 5.0 {
        *timer = 0.0;

        let usage =
            (POOL_INITIAL_SIZE - pool.available.len()) as f32 / POOL_INITIAL_SIZE as f32 * 100.0;

        println!(
            "📊 对象池状态 | 活跃: {} | 可用: {} | 使用率: {:.1}%",
            pool.active.len(),
            pool.available.len(),
            usage
        );
    }
}
