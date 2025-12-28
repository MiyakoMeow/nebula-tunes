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
/// - 使用自定义格式：时间（不含日期）+ 模块路径 + 日志级别 + 消息
#[cfg(not(target_arch = "wasm32"))]
fn init_desktop_logging() {
    use tracing_subscriber::fmt::time::FormatTime;
    use tracing_subscriber::{EnvFilter, fmt};

    // 自定义时间格式化器：只显示 HH:MM:SS.微秒
    struct CustomTime;

    impl FormatTime for CustomTime {
        fn format_time(&self, w: &mut fmt::format::Writer<'_>) -> std::fmt::Result {
            // 获取当前时间（UTC）
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();

            let total_secs = now.as_secs();
            let micros = now.subsec_micros();

            // 计算时分秒
            let h = (total_secs / 3600) % 24;
            let m = (total_secs / 60) % 60;
            let s = total_secs % 60;

            write!(w, "{:02}:{:02}:{:02}.{:06}", h, m, s, micros)
        }
    }

    // 从环境变量 RUST_LOG 读取日志级别，默认为 info
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    // 配置订阅者 - 使用自定义时间格式
    fmt()
        .with_env_filter(env_filter)
        .with_target(true) // 显示模块路径
        .with_thread_ids(false) // 不显示线程 ID
        .with_file(false) // 不显示文件名
        .with_line_number(false) // 不显示行号
        .with_ansi(true) // 保留颜色输出
        .with_timer(CustomTime) // 使用自定义时间格式
        .compact() // 紧凑格式，去除多余空行
        .init();
}

/// WASM 平台日志初始化
///
/// 使用 `tracing-wasm` 配置简化的日志系统
#[cfg(target_arch = "wasm32")]
fn init_wasm_logging() {
    tracing_wasm::set_as_global_default();
}
