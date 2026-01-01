//! 标题菜单页面

use crate::Instance;
use crate::loops::InputMsg;
use crate::pages::{Page, PageBuilder, PageContext, PageId, PageTransition};
use anyhow::Result;

/// 标题页面
pub struct TitlePage {
    /// 实例缓冲区
    instance_buffer: Vec<Instance>,
}

impl Default for TitlePage {
    fn default() -> Self {
        Self::new()
    }
}

impl TitlePage {
    /// 创建新的标题页面
    #[must_use]
    pub fn new() -> Self {
        Self {
            instance_buffer: Vec::with_capacity(128),
        }
    }

    /// 渲染标题界面
    fn render_title_screen(&mut self, window_size: (f32, f32)) -> Vec<Instance> {
        self.instance_buffer.clear();

        let center_x = window_size.0 / 2.0;
        let center_y = window_size.1 / 2.0;

        // 渲染标题 "NEBULA TUNES"（每个字符用一个矩形表示）
        self.render_pixel_text(
            center_x,
            center_y - 100.0,
            "NEBULA TUNES",
            40.0,
            [0.8, 0.9, 1.0, 1.0],
        );

        // 渲染操作提示
        self.render_pixel_text(
            center_x,
            center_y + 50.0,
            "Press ENTER to select file",
            20.0,
            [0.7, 0.7, 0.7, 1.0],
        );
        self.render_pixel_text(
            center_x,
            center_y + 90.0,
            "Press ESC to exit",
            20.0,
            [0.7, 0.7, 0.7, 1.0],
        );

        std::mem::take(&mut self.instance_buffer)
    }

    /// 使用矩形渲染像素风格文字
    #[allow(clippy::cast_precision_loss)]
    fn render_pixel_text(&mut self, center_x: f32, y: f32, text: &str, size: f32, color: [f32; 4]) {
        // 简化实现：每个字符用一个矩形表示
        let char_width = size * 0.5;
        let char_spacing = size * 0.6;

        let total_width = text.chars().count() as f32 * char_spacing;
        let start_x = center_x - total_width / 2.0;

        for (i, _ch) in text.chars().enumerate() {
            self.instance_buffer.push(Instance {
                pos: [start_x + i as f32 * char_spacing, y],
                size: [char_width, size * 0.7],
                color,
            });
        }
    }
}

impl Page for TitlePage {
    fn id(&self) -> PageId {
        PageId::Title
    }

    fn on_update(&mut self, _dt: f32, _ctx: &PageContext) -> Result<PageTransition> {
        Ok(PageTransition::Stay)
    }

    fn on_render(&mut self, ctx: &PageContext) -> Vec<Instance> {
        self.render_title_screen(ctx.window_size)
    }

    fn on_input(&mut self, msg: &InputMsg, _ctx: &PageContext) -> Result<bool> {
        match msg {
            InputMsg::SystemKey(crate::loops::SystemKey::Enter) => {
                // 请求打开文件选择器
                let _ = _ctx
                    .visual_tx
                    .try_send(crate::loops::VisualMsg::RequestFileOpen);
                Ok(true)
            }
            InputMsg::SystemKey(crate::loops::SystemKey::Escape) => {
                // 请求退出应用
                Ok(false)
            }
            _ => Ok(false),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// 标题页面构建器
pub struct TitlePageBuilder;

impl Default for TitlePageBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl TitlePageBuilder {
    /// 创建新的构建器
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// 构建页面实例（消耗 self）
    #[must_use]
    pub fn build_once(self) -> Box<dyn Page> {
        Box::new(TitlePage::new())
    }
}

impl PageBuilder for TitlePageBuilder {
    fn build(&self) -> Result<Box<dyn Page>> {
        Ok(Box::new(TitlePage::new()))
    }

    fn page_id(&self) -> PageId {
        PageId::Title
    }
}
