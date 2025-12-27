//! # Nebula Tunes 主程序

use std::path::PathBuf;

use clap::Parser;

#[derive(Parser)]
/// 命令行参数
struct ExecArgs {
    #[arg(long)]
    /// 指定要加载的 BMS 文件路径
    bms_path: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let args = ExecArgs::parse();
    nebula_tunes::run(args.bms_path)
}
