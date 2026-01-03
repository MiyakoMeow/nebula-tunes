//! # Nebula Tunes 主程序
//!
//! BMS播放器主入口,负责插件注册和初始化

#![warn(missing_docs)]
#![warn(clippy::must_use_candidate)]
#![warn(clippy::must_use_unit)]
#![warn(clippy::redundant_clone)]
#![warn(clippy::redundant_closure_for_method_calls)]
#![warn(clippy::redundant_else)]
#![warn(clippy::redundant_feature_names)]

mod components;
mod filesystem;
mod plugins;
mod resources;
mod schedule;

use bevy::{
    app::MainScheduleOrder,
    asset::{AssetPlugin, UnapprovedPathMode, io::AssetSourceBuilder},
    ecs::schedule::{ExecutorKind, Schedule},
    prelude::*,
};
use bevy_kira_audio::AudioPlugin;
use clap::Parser;

use plugins::{
    AudioManagerPlugin, AudioTriggerPlugin, BMSProcessorPlugin, NoteRendererPlugin,
    TimeSystemPlugin,
};
use resources::ExecArgs;
use schedule::{AudioSchedule, LogicSchedule, RenderSchedule};

fn main() {
    let args = ExecArgs::parse();
    let mut app = App::new();

    app.register_asset_source("fs", AssetSourceBuilder::platform_default(".", None))
        .insert_resource(args)
        .add_plugins(DefaultPlugins.set(AssetPlugin {
            unapproved_path_mode: UnapprovedPathMode::Deny,
            ..Default::default()
        }))
        .add_plugins(AudioPlugin);

    // 配置自定义 Schedule
    configure_schedules(&mut app);

    app.add_plugins(TimeSystemPlugin)
        .add_plugins(BMSProcessorPlugin)
        .add_plugins(AudioTriggerPlugin)
        .add_plugins(AudioManagerPlugin)
        .add_plugins(NoteRendererPlugin)
        .run();
}

/// 配置自定义 Schedule 和执行顺序
fn configure_schedules(app: &mut App) {
    // 创建并添加 Schedule（单线程执行）
    let mut logic_schedule = Schedule::new(LogicSchedule);
    logic_schedule.set_executor_kind(ExecutorKind::SingleThreaded);
    app.add_schedule(logic_schedule);

    let mut audio_schedule = Schedule::new(AudioSchedule);
    audio_schedule.set_executor_kind(ExecutorKind::SingleThreaded);
    app.add_schedule(audio_schedule);

    let mut render_schedule = Schedule::new(RenderSchedule);
    render_schedule.set_executor_kind(ExecutorKind::SingleThreaded);
    app.add_schedule(render_schedule);

    // 配置执行顺序
    let mut main_order = app.world_mut().resource_mut::<MainScheduleOrder>();
    main_order.insert_after(bevy::app::First, LogicSchedule);
    main_order.insert_after(LogicSchedule, AudioSchedule);
    main_order.insert_after(AudioSchedule, bevy::app::Update);
    main_order.insert_after(bevy::app::Update, RenderSchedule);
}
