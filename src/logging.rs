//! 日志系统初始化模块
//!
//! 提供统一的日志初始化接口，支持桌面和 WASM 平台

/// 初始化全局日志系统
///
/// # 平台差异
///
/// - **桌面平台**：使用 `tracing-subscriber`，支持环境变量 `RUST_LOG` 控制日志级别
/// - **WASM 平台**：使用 `tracing-wasm`，简化的日志配置
///
/// # 使用方式
///
/// ```bash
/// # 桌面平台环境变量控制
/// RUST_LOG=info cargo run          # info 及以上级别
/// RUST_LOG=debug cargo run         # debug 及以上级别
/// RUST_LOG=warn cargo run          # 仅警告和错误
/// ```
pub fn init_logging() {
    #[cfg(not(target_arch = "wasm32"))]
    {
        init_desktop_logging();
    }

    #[cfg(target_arch = "wasm32")]
    {
        init_wasm_logging();
    }
}

/// 桌面平台日志初始化
///
/// 使用 `tracing-subscriber` 配置日志系统：
/// - 从环境变量 `RUST_LOG` 读取日志级别
/// - 使用 fmt 层格式化输出
/// - 支持颜色输出和时间戳
#[cfg(not(target_arch = "wasm32"))]
fn init_desktop_logging() {
    use tracing_subscriber::{EnvFilter, fmt};

    // 从环境变量 RUST_LOG 读取日志级别，默认为 info
    // 如果环境变量未设置，则使用 info 级别
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    // 配置订阅者
    fmt()
        .with_env_filter(env_filter)
        .with_target(true) // 显示目标模块路径
        .with_thread_ids(false) // 不显示线程 ID（避免过多输出）
        .with_file(false) // 不显示文件名（保持输出简洁）
        .with_line_number(false) // 不显示行号（保持输出简洁）
        .pretty() // 使用美化输出（带颜色）
        .init();
}

/// WASM 平台日志初始化
///
/// 使用 `tracing-wasm` 配置简化的日志系统
#[cfg(target_arch = "wasm32")]
fn init_wasm_logging() {
    tracing_wasm::set_as_global_default();
}
