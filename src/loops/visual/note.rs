//! 音符与轨道实例构建模块
//!
//! 提供基础轨道实例和根据谱面与状态生成的实例列表。

use bms_rs::chart_process::ChartProcessor;
use bms_rs::chart_process::prelude::*;

use crate::Instance;
use crate::key_to_lane;

/// 获取指定轨道的基础颜色
const fn lane_color(idx: usize) -> [f32; 4] {
    const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
    const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    const BLUE: [f32; 4] = [0.2, 0.6, 1.0, 1.0];
    match idx % 8 {
        0 => RED,
        1 => WHITE,
        2 => BLUE,
        3 => WHITE,
        4 => BLUE,
        5 => WHITE,
        6 => BLUE,
        _ => WHITE,
    }
}

/// 构建基础轨道与面板实例
pub fn base_instances() -> Vec<Instance> {
    let mut instances: Vec<Instance> = Vec::with_capacity(1024);
    for i in 0..super::LANE_COUNT {
        instances.push(Instance {
            pos: [super::lane_x(i), 0.0],
            size: [super::LANE_WIDTH, super::VISIBLE_HEIGHT],
            color: [0.15, 0.15, 0.18, 1.0],
        });
    }
    instances.push(Instance {
        pos: [
            -((super::RIGHT_PANEL_GAP + super::VISIBLE_HEIGHT) / 2.0),
            -super::VISIBLE_HEIGHT / 2.0 + 2.0,
        ],
        size: [super::total_width(), 4.0],
        color: [0.9, 0.9, 0.9, 1.0],
    });
    instances
}

/// 根据处理器可见事件与当前状态构建实例列表
pub fn build_instances_for_processor_with_state(
    p: &mut BmsProcessor,
    pressed: [bool; 8],
    gauge: f32,
) -> Vec<Instance> {
    let mut instances = base_instances();
    if p.started_at().is_some() {
        for (ev, ratio) in p.visible_events() {
            let ChartEvent::Note { side, key, .. } = ev.event() else {
                continue;
            };
            if *side != PlayerSide::Player1 {
                continue;
            };
            let Some(idx) = key_to_lane(*key) else {
                continue;
            };
            let x = super::lane_x(idx);
            let r = (ratio.as_f64() as f32).clamp(0.0, 1.0);
            let y = -super::VISIBLE_HEIGHT / 2.0 + r * super::VISIBLE_HEIGHT;
            instances.push(Instance {
                pos: [x, y],
                size: [super::LANE_WIDTH - 4.0, super::NOTE_HEIGHT],
                color: lane_color(idx),
            });
        }
    }
    for (i, pressed_flag) in pressed.into_iter().enumerate() {
        if pressed_flag {
            instances.push(Instance {
                pos: [super::lane_x(i), -super::VISIBLE_HEIGHT / 2.0 + 24.0],
                size: [super::LANE_WIDTH - 8.0, 24.0],
                color: [1.0, 1.0, 1.0, 0.25],
            });
        }
    }
    let gw = super::total_width();
    let gy = super::VISIBLE_HEIGHT / 2.0 - 20.0;
    instances.push(Instance {
        pos: [
            -((super::RIGHT_PANEL_GAP + super::VISIBLE_HEIGHT) / 2.0),
            gy,
        ],
        size: [gw, 8.0],
        color: [0.3, 0.3, 0.35, 1.0],
    });
    instances.push(Instance {
        pos: [
            -((super::RIGHT_PANEL_GAP + super::VISIBLE_HEIGHT) / 2.0)
                + (-gw / 2.0 + (gw * gauge) / 2.0),
            gy,
        ],
        size: [gw * gauge, 8.0],
        color: [0.2, 0.8, 0.4, 1.0],
    });
    instances
}
