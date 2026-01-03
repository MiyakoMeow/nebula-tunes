//! 插件模块
//!
//! 包含所有功能插件的实现

pub mod audio_manager;
pub mod bms_processor;
pub mod note_renderer;
pub mod time_system;

pub use audio_manager::AudioManagerPlugin;
pub use bms_processor::BMSProcessorPlugin;
pub use note_renderer::NoteRendererPlugin;
pub use time_system::TimeSystemPlugin;
