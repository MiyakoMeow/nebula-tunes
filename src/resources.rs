//! 共享资源定义
//!
//! 定义所有跨插件使用的Resource类型

use std::path::PathBuf;

use bevy::prelude::*;
use clap::Parser;
use gametime::TimeStamp;

/// 命令行参数
#[derive(Parser, Resource)]
#[command(author, version, about, long_about = None)]
pub struct ExecArgs {
    /// BMS文件路径
    #[arg(long)]
    pub bms_path: Option<PathBuf>,
}

/// 当前时间戳
#[derive(Resource, Clone, Copy, Debug)]
pub struct NowStamp(pub TimeStamp);

impl Default for NowStamp {
    fn default() -> Self {
        Self(TimeStamp::start())
    }
}
