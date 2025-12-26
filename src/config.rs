use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use serde::{Deserialize, Deserializer};
use winit::keyboard::KeyCode;

#[derive(Deserialize, Clone)]
pub struct SysConfig {
    pub keys: KeysConfig,
    pub judge: JudgeConfig,
}

#[derive(Deserialize, Clone)]
pub struct KeysConfig {
    pub lanes: Vec<KeyCode>,
}

#[derive(Deserialize, Clone)]
pub struct JudgeConfig {
    #[serde(rename = "visible_travel_ms", deserialize_with = "de_timespan_ms")]
    pub visible_travel: gametime::TimeSpan,
    pub preset: String,
}

pub trait JudgePreset {
    fn windows(&self) -> [gametime::TimeSpan; 4];
}

pub struct LR2Preset;
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

impl JudgeConfig {
    pub fn preset_impl(&self) -> Box<dyn JudgePreset> {
        match self.preset.as_str() {
            "LR2" => Box::new(LR2Preset),
            "Standard" => Box::new(StandardPreset),
            _ => Box::new(LR2Preset),
        }
    }
    pub fn windows(&self) -> [gametime::TimeSpan; 4] {
        self.preset_impl().windows()
    }
}

pub fn load_sys_config(path: &Path) -> Result<SysConfig> {
    let s = std::fs::read_to_string(path)?;
    let cfg: SysConfig = toml::from_str(&s)?;
    Ok(cfg)
}

fn de_timespan_ms<'de, D>(deserializer: D) -> Result<gametime::TimeSpan, D::Error>
where
    D: Deserializer<'de>,
{
    let ms = f32::deserialize(deserializer)?;
    let dur = Duration::from_secs_f64(ms as f64 / 1000.0);
    Ok(gametime::TimeSpan::from_duration(dur))
}
