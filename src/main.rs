//! # Nebula Tunes 主程序

#![warn(missing_docs)]
#![warn(clippy::must_use_candidate)]
#![warn(clippy::must_use_unit)]
#![warn(clippy::redundant_clone)]
#![warn(clippy::redundant_closure_for_method_calls)]
#![warn(clippy::redundant_else)]
#![warn(clippy::redundant_feature_names)]

use bevy::prelude::*;

mod archive_plugin;
use archive_plugin::ZipArchivePlugin;

fn main() {
    App::new()
        .add_plugins(MinimalPlugins)
        .add_plugins(ZipArchivePlugin)
        .run();
}
