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

mod archive_plugin;
use archive_plugin::{ArchivePlugin, ScanArchives};

fn main() {
    App::new()
        .add_plugins(MinimalPlugins)
        .add_plugins(ArchivePlugin)
        .add_systems(Startup, send_scan_message)
        .run();
}

fn send_scan_message(mut writer: MessageWriter<ScanArchives>) {
    let Some(path_env) = std::env::args_os().nth(1) else {
        return;
    };
    let path: PathBuf = PathBuf::from(path_env);
    writer.write(ScanArchives(path));
}
