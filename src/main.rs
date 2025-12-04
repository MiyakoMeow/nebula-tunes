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

mod archive_plugin;
use archive_plugin::{ArchivePlugin, ScanArchives};

fn main() {
    App::new()
        .add_plugins(MinimalPlugins)
        .add_plugins(ArchivePlugin)
        .add_systems(Startup, parse_args)
        .run();
}

#[derive(Parser)]
struct Args {
    #[arg(long)]
    test_archive_path: Option<PathBuf>,
}

fn parse_args(mut scan_archives_writer: MessageWriter<ScanArchives>) {
    let args = Args::parse();
    if let Some(path) = args.test_archive_path {
        scan_archives_writer.write(ScanArchives(path));
    }
}
