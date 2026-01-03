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

use bevy::{
    asset::{AssetPlugin, UnapprovedPathMode, io::AssetSourceBuilder},
    prelude::*,
};
use bevy_kira_audio::AudioPlugin;
use clap::Parser;

use plugins::{AudioManagerPlugin, BMSProcessorPlugin, NoteRendererPlugin, TimeSystemPlugin};
use resources::ExecArgs;

fn main() {
    let args = ExecArgs::parse();
    let mut app = App::new();

    app.register_asset_source("fs", AssetSourceBuilder::platform_default(".", None))
        .insert_resource(args)
        .add_plugins(DefaultPlugins.set(AssetPlugin {
            unapproved_path_mode: UnapprovedPathMode::Deny,
            ..Default::default()
        }))
        .add_plugins(AudioPlugin)
        .add_plugins(TimeSystemPlugin)
        .add_plugins(BMSProcessorPlugin)
        .add_plugins(AudioManagerPlugin)
        .add_plugins(NoteRendererPlugin)
        .run();
}
