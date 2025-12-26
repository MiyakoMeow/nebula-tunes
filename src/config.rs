//! 系统配置定义与解析

use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use serde::{Deserialize, Deserializer};
use winit::keyboard::KeyCode;

/// 系统运行时配置
#[derive(Deserialize, Clone)]
pub struct Sys {
    /// 键位映射配置
    pub keys: Keys,
    /// 判定与可见区域配置
    pub judge: Judge,
}

/// 键位配置
#[derive(Deserialize, Clone)]
pub struct Keys {
    /// 8 轨对应的按键代码列表
    pub lanes: Vec<KeyCode>,
}

/// 判定配置
#[derive(Deserialize, Clone)]
pub struct Judge {
    #[serde(rename = "visible_travel_ms", deserialize_with = "de_timespan_ms")]
    /// 可见区域的时间跨度（毫秒）
    pub visible_travel: gametime::TimeSpan,
    /// 判定预设名（如 LR2、Standard）
    pub preset: String,
}

/// 判定预设接口
pub trait JudgePreset {
    /// 返回四档判定窗口
    fn windows(&self) -> [gametime::TimeSpan; 4];
}

/// LR2 判定预设
pub struct LR2Preset;
/// 标准判定预设
pub struct StandardPreset;

impl JudgePreset for LR2Preset {
    fn windows(&self) -> [gametime::TimeSpan; 4] {
        [
            gametime::TimeSpan::from_duration(Duration::from_millis(16)),
            gametime::TimeSpan::from_duration(Duration::from_millis(36)),
            gametime::TimeSpan::from_duration(Duration::from_millis(80)),
            gametime::TimeSpan::from_duration(Duration::from_millis(120)),
        ]
    }
}

impl JudgePreset for StandardPreset {
    fn windows(&self) -> [gametime::TimeSpan; 4] {
        [
            gametime::TimeSpan::from_duration(Duration::from_millis(16)),
            gametime::TimeSpan::from_duration(Duration::from_millis(36)),
            gametime::TimeSpan::from_duration(Duration::from_millis(80)),
            gametime::TimeSpan::from_duration(Duration::from_millis(120)),
        ]
    }
}

impl Judge {
    /// 根据预设名称创建判定窗配置实现
    #[must_use]
    pub fn preset_impl(&self) -> Box<dyn JudgePreset> {
        match self.preset.as_str() {
            "LR2" => Box::new(LR2Preset),
            "Standard" => Box::new(StandardPreset),
            _ => Box::new(LR2Preset),
        }
    }
    /// 获取四档判定时间窗口
    #[must_use]
    pub fn windows(&self) -> [gametime::TimeSpan; 4] {
        self.preset_impl().windows()
    }
}

/// 从 TOML 字符串解析系统配置
///
/// # Errors
///
/// - TOML 解析失败
/// - 配置字段反序列化失败
pub fn parse_sys_str(s: &str) -> Result<Sys> {
    let cfg: Sys = toml::from_str(s)?;
    Ok(cfg)
}

/// 从指定路径加载系统配置（TOML）
///
/// # Errors
///
/// - 读取文件失败
/// - TOML 解析失败
/// - 配置字段反序列化失败
#[cfg(any(not(target_arch = "wasm32"), target_os = "wasi"))]
pub fn load_sys(path: &Path) -> Result<Sys> {
    let s = std::fs::read_to_string(path)?;
    parse_sys_str(&s)
}

/// 从指定路径加载系统配置（TOML）
///
/// # Errors
///
/// - WASM 目标不支持直接读取本地文件
#[cfg(all(target_arch = "wasm32", not(target_os = "wasi")))]
pub fn load_sys(_path: &Path) -> Result<Sys> {
    anyhow::bail!("load_sys is not available on wasm")
}

/// 反序列化毫秒为 `TimeSpan`
fn de_timespan_ms<'de, D>(deserializer: D) -> Result<gametime::TimeSpan, D::Error>
where
    D: Deserializer<'de>,
{
    let ms = f32::deserialize(deserializer)?;
    let dur = Duration::from_secs_f64(ms as f64 / 1000.0);
    Ok(gametime::TimeSpan::from_duration(dur))
}
