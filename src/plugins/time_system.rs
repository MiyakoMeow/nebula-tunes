//! 时间管理插件
//!
//! 提供全局时间戳管理和更新

use bevy::prelude::*;
use gametime::TimeStamp;

use crate::resources::NowStamp;

/// 时间管理插件
pub struct TimeSystemPlugin;

impl Plugin for TimeSystemPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NowStamp>()
            .add_systems(Update, update_now_stamp);
    }
}

/// 更新当前时间戳
fn update_now_stamp(mut now_stamp: ResMut<NowStamp>) {
    now_stamp.0 = TimeStamp::now();
}
