//! 游戏页面构建器：用于创建 `GamePage` 实例

use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use bms_rs::chart_process::prelude::*;

use crate::chart::bms::BgaFileType;
use crate::game_page::{GamePage, JudgeParams};
use crate::media::BgaDecodeCache;
use crate::pages::{Page, PageBuilder, PageId};

/// 游戏页面构建器
pub struct GamePageBuilder {
    /// BMS 处理器
    processor: BmsProcessor,
    /// 音频路径映射
    audio_paths: HashMap<WavId, PathBuf>,
    /// BGA 图片路径映射
    bmp_paths: HashMap<BmpId, PathBuf>,
    /// BGA 文件类型映射
    bmp_types: HashMap<BmpId, BgaFileType>,
    /// BGA 解码缓存
    bga_cache: Arc<BgaDecodeCache>,
    /// 判定参数
    judge: JudgeParams,
}

impl GamePageBuilder {
    /// 创建新的游戏页面构建器
    ///
    /// # 参数
    ///
    /// * `processor`: BMS 谱面处理器
    /// * `audio_paths`: 音频 ID 到路径的映射
    /// * `bmp_paths`: BGA 图片 ID 到路径的映射
    /// * `bmp_types`: BGA 图片 ID 到文件类型的映射
    /// * `bga_cache`: BGA 解码缓存
    /// * `judge`: 判定参数
    #[must_use]
    pub const fn new(
        processor: BmsProcessor,
        audio_paths: HashMap<WavId, PathBuf>,
        bmp_paths: HashMap<BmpId, PathBuf>,
        bmp_types: HashMap<BmpId, BgaFileType>,
        bga_cache: Arc<BgaDecodeCache>,
        judge: JudgeParams,
    ) -> Self {
        Self {
            processor,
            audio_paths,
            bmp_paths,
            bmp_types,
            bga_cache,
            judge,
        }
    }

    /// 构建游戏页面实例（消耗 self）
    ///
    /// # Errors
    ///
    /// 永不返回错误
    #[must_use]
    pub fn build_once(self) -> Box<dyn Page> {
        Box::new(GamePage::new(
            self.processor,
            self.audio_paths,
            self.bmp_paths,
            self.bmp_types,
            self.bga_cache,
            self.judge,
        ))
    }
}

impl PageBuilder for GamePageBuilder {
    /// 构建游戏页面实例
    ///
    /// # Errors
    ///
    /// 总是返回错误，因为 `BmsProcessor` 不实现 `Clone`。请使用 `GamePageBuilder::build_once` 代替。
    fn build(&self) -> Result<Box<dyn Page>> {
        Err(anyhow::anyhow!(
            "GamePageBuilder::build() cannot be called. Use GamePageBuilder::build_once() instead because BmsProcessor doesn't implement Clone."
        ))
    }

    /// 获取页面 ID
    fn page_id(&self) -> PageId {
        PageId::Game
    }
}
