//! 页面管理器：协调页面生命周期和转换

use anyhow::Result;
use std::collections::HashMap;
use std::sync::mpsc;
use std::time::Instant;

use crate::Instance;
use crate::pages::{Page, PageBuilder, PageContext, PageEvent, PageId, PageTransition};

/// 页面管理器
pub struct PageManager {
    /// 当前活动页面
    current_page: Option<Box<dyn Page>>,
    /// 页面栈（用于模态对话框）
    page_stack: Vec<Box<dyn Page>>,
    /// 页面构建器注册表
    builders: HashMap<PageId, Box<dyn PageBuilder>>,
    /// 视觉消息发送端
    visual_tx: mpsc::SyncSender<crate::loops::VisualMsg>,
    /// 音频消息发送端
    audio_tx: mpsc::SyncSender<crate::loops::audio::Msg>,
    /// 窗口尺寸
    window_size: (f32, f32),
    /// 上一帧时间
    last_frame_time: Instant,
}

impl PageManager {
    /// 创建新的页面管理器
    #[must_use]
    pub fn new(
        visual_tx: mpsc::SyncSender<crate::loops::VisualMsg>,
        audio_tx: mpsc::SyncSender<crate::loops::audio::Msg>,
    ) -> Self {
        Self {
            current_page: None,
            page_stack: Vec::new(),
            builders: HashMap::new(),
            visual_tx,
            audio_tx,
            window_size: (1920.0, 1080.0),
            last_frame_time: Instant::now(),
        }
    }

    /// 注册页面构建器
    ///
    /// # Errors
    ///
    /// 如果页面 ID 已存在，返回错误
    pub fn register_builder(&mut self, builder: Box<dyn PageBuilder>) -> Result<()> {
        let id = builder.page_id();
        self.builders.insert(id, builder);
        Ok(())
    }

    /// 直接设置当前页面（跳过构建器）
    ///
    /// 初始化并进入页面。
    ///
    /// # Errors
    ///
    /// 如果页面初始化或进入失败，返回错误
    pub fn set_current_page(&mut self, mut page: Box<dyn Page>) -> Result<()> {
        // 初始化并进入页面
        let ctx = self.create_context();
        page.on_init(&ctx)?;
        page.on_enter(&ctx)?;

        self.current_page = Some(page);
        Ok(())
    }

    /// 切换到指定页面
    ///
    /// # Errors
    ///
    /// 如果页面构建器未注册，返回错误
    pub fn switch_to(&mut self, id: PageId) -> Result<()> {
        // 离开当前页面
        {
            let ctx = self.create_context();
            if let Some(page) = self.current_page.as_mut() {
                page.on_leave(&ctx)?;
            }
        }

        // 创建新页面
        let builder = self
            .builders
            .get(&id)
            .ok_or_else(|| anyhow::anyhow!("页面构建器未注册: {:?}", id))?;
        let mut new_page = builder.build()?;

        // 初始化并进入新页面
        {
            let ctx = self.create_context();
            new_page.on_init(&ctx)?;
            new_page.on_enter(&ctx)?;
        }

        self.current_page = Some(new_page);
        Ok(())
    }

    /// 推入子页面（模态）
    ///
    /// # Errors
    ///
    /// 如果页面构建器未注册，返回错误
    pub fn push_page(&mut self, id: PageId) -> Result<()> {
        // 暂停当前页面
        {
            let ctx = self.create_context();
            if let Some(page) = self.current_page.as_mut() {
                page.on_event(PageEvent::Pause, &ctx)?;
            }
        }

        // 将当前页面推入栈
        if let Some(page) = self.current_page.take() {
            self.page_stack.push(page);
        }

        // 创建并进入新页面
        let builder = self
            .builders
            .get(&id)
            .ok_or_else(|| anyhow::anyhow!("页面构建器未注册: {:?}", id))?;
        let mut new_page = builder.build()?;

        let ctx = self.create_context();
        new_page.on_init(&ctx)?;
        new_page.on_enter(&ctx)?;

        self.current_page = Some(new_page);
        Ok(())
    }

    /// 弹出当前页面（返回上一层）
    ///
    /// # Errors
    ///
    /// 如果没有父页面可以返回，返回错误
    pub fn pop_page(&mut self) -> Result<()> {
        // 离开当前页面
        {
            let ctx = self.create_context();
            if let Some(mut page) = self.current_page.take() {
                page.on_leave(&ctx)?;
                page.on_cleanup(&ctx)?;
                // 注意：page 在这里被 drop
            }
        }

        // 恢复上一个页面
        if let Some(mut prev_page) = self.page_stack.pop() {
            let ctx = self.create_context();
            prev_page.on_event(PageEvent::Resume, &ctx)?;
            prev_page.on_enter(&ctx)?;
            self.current_page = Some(prev_page);
        }

        Ok(())
    }

    /// 处理输入事件
    ///
    /// # Errors
    ///
    /// 如果页面处理输入失败，返回错误
    pub fn handle_input(&mut self, msg: &crate::loops::InputMsg) -> Result<()> {
        let ctx = self.create_context();

        if let Some(page) = self.current_page.as_mut() {
            let _ = page.on_input(msg, &ctx)?;
        }

        Ok(())
    }

    /// 更新当前页面
    ///
    /// 返回是否应该继续运行
    ///
    /// # Errors
    ///
    /// 如果页面更新或页面转换失败，返回错误
    pub fn update(&mut self) -> Result<bool> {
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame_time).as_secs_f32();
        self.last_frame_time = now;

        let ctx = self.create_context();

        if let Some(page) = self.current_page.as_mut() {
            match page.on_update(dt, &ctx)? {
                PageTransition::Stay => Ok(true),
                PageTransition::Switch(id) => {
                    self.switch_to(id)?;
                    Ok(true)
                }
                PageTransition::Push(id) => {
                    self.push_page(id)?;
                    Ok(true)
                }
                PageTransition::Pop => {
                    self.pop_page()?;
                    Ok(true)
                }
                PageTransition::Exit => Ok(false),
            }
        } else {
            Ok(true)
        }
    }

    /// 渲染当前页面
    pub fn render(&mut self) -> Vec<Instance> {
        let ctx = self.create_context();

        self.current_page
            .as_mut()
            .map(|page| page.on_render(&ctx))
            .unwrap_or_default()
    }

    /// 处理窗口尺寸变化
    ///
    /// # Errors
    ///
    /// 如果页面处理尺寸变化失败，返回错误
    #[allow(clippy::cast_precision_loss)]
    pub fn resize(&mut self, width: u32, height: u32) -> Result<()> {
        self.window_size = (width as f32, height as f32);

        let ctx = self.create_context();
        if let Some(page) = self.current_page.as_mut() {
            page.on_event(PageEvent::Resize { width, height }, &ctx)?;
        }

        Ok(())
    }

    /// 创建页面上下文
    fn create_context(&self) -> PageContext {
        PageContext {
            visual_tx: self.visual_tx.clone(),
            audio_tx: self.audio_tx.clone(),
            window_size: self.window_size,
        }
    }
}
