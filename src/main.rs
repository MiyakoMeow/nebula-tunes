//! # Nebula Tunes 主程序

#![warn(missing_docs)]
#![warn(clippy::must_use_candidate)]
#![warn(clippy::must_use_unit)]
#![warn(clippy::redundant_clone)]
#![warn(clippy::redundant_closure_for_method_calls)]
#![warn(clippy::redundant_else)]
#![warn(clippy::redundant_feature_names)]

use std::path::PathBuf;

use bevy::prelude::*;
use clap::Parser;

mod test_archive_plugin;
use test_archive_plugin::TestArchivePlugin;

fn main() {
    let args = ExecArgs::parse();
    // 测试模式下使用 MinimalPlugins，否则使用 DefaultPlugins
    if args.test_archive_path.is_some() {
        App::new()
            .insert_resource(args)
            .add_plugins(MinimalPlugins)
            .add_plugins(TestArchivePlugin)
            .run();
        return;
    };
    // 正常模式下使用 DefaultPlugins
    App::new()
        .insert_resource(args)
        .add_plugins(DefaultPlugins)
        .run();
}

#[derive(Parser, Resource)]
struct ExecArgs {
    #[arg(long)]
    test_archive_path: Option<PathBuf>,
}
