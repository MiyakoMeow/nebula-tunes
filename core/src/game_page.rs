//! 游戏页面：将现有的 `main_loop` 逻辑迁移到页面中

use anyhow::Result;
use bms_rs::chart_process::prelude::*;
use bms_rs::chart_process::types::PlayheadEvent;
use gametime::TimeSpan;
use num::ToPrimitive;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::Instance;
use crate::chart::bms::BgaFileType;
use crate::loops::audio::Msg;
use crate::loops::visual::{build_instances_for_processor_with_state, preload_bga_files};
use crate::loops::{InputMsg, VisualMsg};
use crate::media::BgaDecodeCache;
use crate::pages::{Page, PageContext, PageId, PageTransition};

/// 游戏页面：实际的 BMS 游戏界面
pub struct GamePage {
    /// BMS 处理器
    processor: Option<BmsProcessor>,
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
    /// 游戏状态
    state: GameState,
    /// 复用的实例缓冲区
    instance_buffer: Vec<Instance>,
}

/// 判定参数
#[derive(Clone)]
pub struct JudgeParams {
    /// 可见区域时间跨度
    pub travel: TimeSpan,
    /// 判定窗口
    pub windows: [TimeSpan; 4],
}

/// 游戏状态
struct GameState {
    /// 按键按下状态
    pressed: [bool; 8],
    /// 血条
    gauge: f32,
    /// 连击数
    combo: u32,
}

impl GamePage {
    /// 创建新的游戏页面
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        processor: BmsProcessor,
        audio_paths: HashMap<WavId, PathBuf>,
        bmp_paths: HashMap<BmpId, PathBuf>,
        bmp_types: HashMap<BmpId, BgaFileType>,
        bga_cache: Arc<BgaDecodeCache>,
        judge: JudgeParams,
    ) -> Self {
        Self {
            processor: Some(processor),
            audio_paths,
            bmp_paths,
            bmp_types,
            bga_cache,
            judge,
            state: GameState {
                pressed: [false; 8],
                gauge: 0.5,
                combo: 0,
            },
            instance_buffer: Vec::with_capacity(2048),
        }
    }
}

impl Page for GamePage {
    fn id(&self) -> PageId {
        PageId::Game
    }

    fn on_init(&mut self, ctx: &PageContext) -> Result<()> {
        // 预加载 BGA 文件
        let bmp_files: Vec<PathBuf> = self.bmp_paths.values().cloned().collect();
        preload_bga_files(self.bga_cache.clone(), bmp_files);

        // 预加载音频文件
        let audio_files: Vec<PathBuf> = self.audio_paths.values().cloned().collect();
        let _ = ctx.audio_tx.send(Msg::PreloadAll { files: audio_files });

        Ok(())
    }

    fn on_enter(&mut self, _ctx: &PageContext) -> Result<()> {
        // 启动游戏
        if let Some(p) = self.processor.as_mut() {
            p.start_play(gametime::TimeStamp::now());
        }
        Ok(())
    }

    fn on_input(&mut self, msg: &InputMsg, ctx: &PageContext) -> Result<bool> {
        // 处理按键输入和判定逻辑
        let Some(p) = self.processor.as_mut() else {
            return Ok(false);
        };

        match msg {
            InputMsg::KeyDown(idx) => {
                if let Some(flag) = self.state.pressed.get_mut(*idx) {
                    *flag = true;
                }

                // 查找最佳匹配的音符
                let mut best: Option<(PlayheadEvent, f32)> = None;
                for (ev, ratio) in p.visible_events() {
                    let ChartEvent::Note { side, key, .. } = ev.event() else {
                        continue;
                    };

                    if *side != PlayerSide::Player1 {
                        continue;
                    }

                    let Some(lane) = crate::key_to_lane(*key) else {
                        continue;
                    };

                    if lane != *idx {
                        continue;
                    }

                    let r: f32 = ratio.start().value().to_f32().unwrap_or(0.0);
                    if !(0.0..=1.0).contains(&r) {
                        continue;
                    }

                    if let Some((_, br)) = &best {
                        if r < *br {
                            best = Some((ev.clone(), r));
                        }
                    } else {
                        best = Some((ev.clone(), r));
                    }
                }

                let Some((ev, r)) = best else {
                    return Ok(true);
                };

                // 计算判定
                #[allow(clippy::cast_precision_loss)]
                #[allow(clippy::cast_possible_truncation)]
                #[allow(clippy::cast_sign_loss)]
                let nanos =
                    u64::try_from((self.judge.travel.as_nanos() as f64 * r as f64).max(0.0) as i64)
                        .unwrap_or(u64::MAX);
                let dt = TimeSpan::from_duration(std::time::Duration::from_nanos(nanos));

                let judge_level = if dt.as_nanos() <= self.judge.windows[0].as_nanos() {
                    4
                } else if dt.as_nanos() <= self.judge.windows[1].as_nanos() {
                    3
                } else if dt.as_nanos() <= self.judge.windows[2].as_nanos() {
                    2
                } else if dt.as_nanos() <= self.judge.windows[3].as_nanos() {
                    1
                } else {
                    0
                };

                // 根据判定更新状态
                match judge_level {
                    4 | 3 => {
                        self.state.combo = self.state.combo.saturating_add(1);
                        self.state.gauge = (self.state.gauge + 0.02).min(1.0);
                        if let ChartEvent::Note { wav_id, .. } = ev.event()
                            && let Some(wav_id) = wav_id.as_ref()
                            && let Some(path) = self.audio_paths.get(wav_id)
                            && ctx.audio_tx.try_send(Msg::Play(path.clone())).is_ok()
                        {
                            // 音频播放成功
                        }
                    }
                    2 => {
                        self.state.combo = self.state.combo.saturating_add(1);
                        self.state.gauge = (self.state.gauge + 0.01).min(1.0);
                        if let ChartEvent::Note { wav_id, .. } = ev.event()
                            && let Some(wav_id) = wav_id.as_ref()
                            && let Some(path) = self.audio_paths.get(wav_id)
                            && ctx.audio_tx.try_send(Msg::Play(path.clone())).is_ok()
                        {
                            // 音频播放成功
                        }
                    }
                    1 => {
                        self.state.combo = 0;
                        self.state.gauge = (self.state.gauge - 0.03).max(0.0);
                        let _ = ctx.visual_tx.try_send(VisualMsg::BgaPoorTrigger);
                    }
                    _ => {
                        self.state.combo = 0;
                        self.state.gauge = (self.state.gauge - 0.05).max(0.0);
                        let _ = ctx.visual_tx.try_send(VisualMsg::BgaPoorTrigger);
                    }
                }

                Ok(true)
            }
            InputMsg::KeyUp(idx) => {
                if let Some(flag) = self.state.pressed.get_mut(*idx) {
                    *flag = false;
                }
                Ok(true)
            }
        }
    }

    fn on_update(&mut self, _dt: f32, ctx: &PageContext) -> Result<PageTransition> {
        // 推进处理器
        let now = gametime::TimeStamp::now();
        let Some(p) = self.processor.as_mut() else {
            return Ok(PageTransition::Stay);
        };

        let events: Vec<_> = p.update(now).collect();

        // 处理事件
        for ev in &events {
            // 跳过非玩家1的音符
            if let ChartEvent::Note { side, .. } = ev.event()
                && *side != PlayerSide::Player1
            {
                continue;
            }

            // 处理 BGA 变化
            if let ChartEvent::BgaChange { layer, bmp_id } = ev.event()
                && let Some(bmp_id) = bmp_id.as_ref()
                && let Some(path) = self.bmp_paths.get(bmp_id)
            {
                use crate::loops::BgaLayer as VisualBgaLayer;

                let mapped_layer = match layer {
                    BgaLayer::Overlay => VisualBgaLayer::Layer,
                    BgaLayer::Overlay2 => VisualBgaLayer::Layer2,
                    BgaLayer::Poor => VisualBgaLayer::Poor,
                    _ => VisualBgaLayer::Bga,
                };

                // 根据文件类型发送不同消息
                let file_type = self.bmp_types.get(bmp_id);

                match file_type {
                    Some(BgaFileType::Video) => {
                        let _ = ctx.visual_tx.try_send(VisualMsg::VideoPlay {
                            layer: mapped_layer,
                            path: path.clone(),
                            loop_play: false,
                        });
                    }
                    _ => {
                        let _ = ctx.visual_tx.try_send(VisualMsg::BgaChange {
                            layer: mapped_layer,
                            path: path.clone(),
                        });
                    }
                }
            }

            // 处理 BGM
            if let ChartEvent::Bgm { wav_id } = ev.event()
                && let Some(wav_id) = wav_id.as_ref()
                && let Some(path) = self.audio_paths.get(wav_id)
                && ctx.audio_tx.try_send(Msg::Play(path.clone())).is_ok()
            {
                // BGM 播放成功
            }
        }

        Ok(PageTransition::Stay)
    }

    fn on_render(&mut self, _ctx: &PageContext) -> Vec<Instance> {
        self.instance_buffer.clear();

        if let Some(p) = self.processor.as_mut() {
            let instances =
                build_instances_for_processor_with_state(p, self.state.pressed, self.state.gauge);
            self.instance_buffer.extend(instances);
        } else {
            self.instance_buffer
                .extend(crate::loops::visual::base_instances());
        }

        std::mem::take(&mut self.instance_buffer)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
