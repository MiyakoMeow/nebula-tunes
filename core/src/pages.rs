//! 页面抽象层：统一管理不同界面状态
//!
//! 提供页面生命周期管理、事件处理和渲染抽象，为多页面系统（标题、设置、游戏等）提供基础架构。

use anyhow::Result;
use std::any::Any;
use std::collections::HashMap;
use std::sync::mpsc;
use std::time::Instant;

use crate::Instance;
use crate::loops::{InputMsg, VisualMsg};

/// 页面标识符
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PageId {
    /// 游戏主界面
    Game,
    /// 标题界面
    Title,
    /// 设置界面
    Settings,
    /// 结算界面
    Result,
    /// 歌曲选择界面
    SongSelect,
}

/// 页面配置
#[derive(Debug, Clone)]
pub struct PageConfig {
    /// 是否启用页面实例缓存
    pub cache_enabled: bool,
}

impl Default for PageConfig {
    fn default() -> Self {
        Self {
            cache_enabled: true,
        }
    }
}

/// 页面上下文：包含页面运行所需的所有共享资源
pub struct PageContext {
    /// 视觉消息发送端（用于渲染更新）
    pub visual_tx: std::sync::mpsc::SyncSender<VisualMsg>,
    /// 音频消息发送端（用于音频播放控制）
    pub audio_tx: std::sync::mpsc::SyncSender<crate::loops::audio::Msg>,
    /// 窗口尺寸（宽度，高度）
    pub window_size: (f32, f32),
}

/// 页面生命周期事件
#[derive(Debug, Clone, PartialEq)]
pub enum PageEvent {
    /// 页面进入
    Enter,
    /// 页面离开
    Leave,
    /// 页面暂停（进入子页面）
    Pause,
    /// 页面恢复（从子页面返回）
    Resume,
    /// 窗口尺寸变化
    Resize {
        /// 新宽度
        width: u32,
        /// 新高度
        height: u32,
    },
}

/// 页面转换结果
pub enum PageTransition {
    /// 保持当前页面
    Stay,
    /// 切换到指定页面
    Switch(PageId),
    /// 推入子页面（模态）
    Push(PageId),
    /// 弹出当前页面（返回上一层）
    Pop,
    /// 退出应用
    Exit,
}

/// 页面 trait：定义页面的完整生命周期
pub trait Page: Any {
    /// 获取页面 ID
    fn id(&self) -> PageId;

    /// 页面初始化（首次创建时调用）
    ///
    /// 用于初始化页面状态、加载资源等
    ///
    /// # Errors
    ///
    /// 默认实现永不返回错误
    fn on_init(&mut self, ctx: &PageContext) -> anyhow::Result<()> {
        let _ = ctx;
        Ok(())
    }

    /// 页面进入（每次切换到该页面时调用）
    ///
    /// # Errors
    ///
    /// 默认实现永不返回错误
    fn on_enter(&mut self, ctx: &PageContext) -> anyhow::Result<()> {
        let _ = ctx;
        Ok(())
    }

    /// 页面离开（每次切换离开时调用）
    ///
    /// # Errors
    ///
    /// 默认实现永不返回错误
    fn on_leave(&mut self, ctx: &PageContext) -> anyhow::Result<()> {
        let _ = ctx;
        Ok(())
    }

    /// 处理输入事件
    ///
    /// 返回是否需要进一步处理
    ///
    /// # Errors
    ///
    /// 默认实现永不返回错误
    fn on_input(&mut self, msg: &InputMsg, ctx: &PageContext) -> anyhow::Result<bool> {
        let _ = (msg, ctx);
        Ok(false)
    }

    /// 更新页面状态（每帧调用）
    ///
    /// 返回页面转换请求
    ///
    /// # Errors
    ///
    /// 如果页面更新失败，返回错误
    fn on_update(&mut self, dt: f32, ctx: &PageContext) -> anyhow::Result<PageTransition>;

    /// 渲染页面（每帧调用）
    ///
    /// 返回要渲染的实例列表
    fn on_render(&mut self, ctx: &PageContext) -> Vec<Instance>;

    /// 处理页面事件
    ///
    /// # Errors
    ///
    /// 默认实现永不返回错误
    fn on_event(&mut self, event: PageEvent, ctx: &PageContext) -> anyhow::Result<()> {
        let _ = (event, ctx);
        Ok(())
    }

    /// 清理页面资源
    ///
    /// # Errors
    ///
    /// 默认实现永不返回错误
    fn on_cleanup(&mut self, ctx: &PageContext) -> anyhow::Result<()> {
        let _ = ctx;
        Ok(())
    }

    /// 向下转型为具体类型（用于访问页面特有方法）
    fn as_any(&self) -> &dyn Any;

    /// 向下转型为具体类型（用于访问页面特有方法）
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// 页面构建器 trait：用于创建页面实例
pub trait PageBuilder: Any {
    /// 创建新页面实例
    ///
    /// # Errors
    ///
    /// 如果页面创建失败，返回错误
    fn build(&self) -> anyhow::Result<Box<dyn Page>>;

    /// 获取页面 ID
    fn page_id(&self) -> PageId;
}

/// 页面管理器：协调页面生命周期和转换
pub struct PageManager {
    /// 页面实例缓存（统一管理所有页面）
    cached_pages: HashMap<PageId, Box<dyn Page>>,
    /// 当前活动页面 ID
    current_page_id: Option<PageId>,
    /// 页面栈（存储 ID，实例从 `cached_pages` 获取）
    page_stack: Vec<PageId>,
    /// 页面配置
    page_configs: HashMap<PageId, PageConfig>,
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
            cached_pages: HashMap::new(),
            current_page_id: None,
            page_stack: Vec::new(),
            page_configs: HashMap::new(),
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

    /// 设置页面配置
    pub fn set_page_config(&mut self, id: PageId, config: PageConfig) {
        self.page_configs.insert(id, config);
    }

    /// 获取页面配置（如果未设置则返回默认配置）
    fn get_page_config(&self, id: PageId) -> PageConfig {
        self.page_configs.get(&id).cloned().unwrap_or_default()
    }

    /// 处理页面离开（根据配置决定是否清理）
    fn handle_page_leave(&mut self, page_id: PageId) -> Result<()> {
        // 先获取配置和上下文
        let config = self.get_page_config(page_id);
        let should_cleanup = !config.cache_enabled;
        let ctx = self.create_context();

        if let Some(page) = self.cached_pages.get_mut(&page_id) {
            page.on_leave(&ctx)?;
            if should_cleanup {
                page.on_cleanup(&ctx)?;
            }
        }
        Ok(())
    }

    /// 创建新页面实例
    fn create_page(&self, id: PageId) -> Result<Box<dyn Page>> {
        let builder = self
            .builders
            .get(&id)
            .ok_or_else(|| anyhow::anyhow!("页面构建器未注册: {:?}", id))?;
        builder.build()
    }

    /// 直接设置当前页面（跳过构建器）
    ///
    /// 初始化并进入页面。
    ///
    /// # Errors
    ///
    /// 如果页面初始化或进入失败，返回错误
    pub fn set_current_page(&mut self, mut page: Box<dyn Page>) -> Result<()> {
        let page_id = page.id();

        // 离开当前页面
        if let Some(current_id) = self.current_page_id {
            self.handle_page_leave(current_id)?;
        }

        // 初始化并进入新页面
        let ctx = self.create_context();
        page.on_init(&ctx)?;
        page.on_enter(&ctx)?;

        // 存储到缓存中
        self.cached_pages.insert(page_id, page);
        self.current_page_id = Some(page_id);
        Ok(())
    }

    /// 切换到指定页面
    ///
    /// # Errors
    ///
    /// 如果页面构建器未注册，返回错误
    pub fn switch_to(&mut self, id: PageId) -> Result<()> {
        // 先获取配置
        let config = self.get_page_config(id);
        let cache_enabled = config.cache_enabled;

        // 离开当前页面
        if let Some(current_id) = self.current_page_id {
            self.handle_page_leave(current_id)?;
        }

        if cache_enabled {
            // 缓存模式：尝试从缓存获取
            if let Some(cached) = self.cached_pages.get_mut(&id) {
                // 从缓存恢复，只触发 on_enter
                // 注意：先获取锁，然后创建 context，避免借用冲突
                let ctx = PageContext {
                    visual_tx: self.visual_tx.clone(),
                    audio_tx: self.audio_tx.clone(),
                    window_size: self.window_size,
                };
                cached.on_enter(&ctx)?;
                self.current_page_id = Some(id);
                return Ok(());
            }

            // 缓存未命中，需要创建
            let mut new_page = self.create_page(id)?;
            let ctx = self.create_context();
            new_page.on_init(&ctx)?;
            new_page.on_enter(&ctx)?;

            self.cached_pages.insert(id, new_page);
            self.current_page_id = Some(id);
        } else {
            // 不缓存模式：先清理旧实例（如果存在）
            {
                let ctx = self.create_context();
                if let Some(mut old_page) = self.cached_pages.remove(&id) {
                    let _ = old_page.on_cleanup(&ctx);
                    // old_page 在这里被 drop
                }
            }

            // 创建新页面
            let mut new_page = self.create_page(id)?;
            let ctx = self.create_context();
            new_page.on_init(&ctx)?;
            new_page.on_enter(&ctx)?;

            self.cached_pages.insert(id, new_page);
            self.current_page_id = Some(id);
        }

        Ok(())
    }

    /// 推入子页面（模态）
    ///
    /// # Errors
    ///
    /// 如果页面构建器未注册，返回错误
    pub fn push_page(&mut self, id: PageId) -> Result<()> {
        // 获取当前页面 ID（如果有）
        let current_id = self.current_page_id;

        // 暂停当前页面
        if let Some(cid) = current_id {
            let ctx = self.create_context();
            if let Some(page) = self.cached_pages.get_mut(&cid) {
                page.on_event(PageEvent::Pause, &ctx)?;
            }
            // 将当前页面 ID 推入栈
            self.page_stack.push(cid);
        }

        // 切换到新页面（复用 switch_to 的逻辑）
        self.switch_to(id)?;
        Ok(())
    }

    /// 弹出当前页面（返回上一层）
    ///
    /// # Errors
    ///
    /// 如果没有父页面可以返回，返回错误
    pub fn pop_page(&mut self) -> Result<()> {
        // 离开当前页面
        if let Some(current_id) = self.current_page_id {
            self.handle_page_leave(current_id)?;
        }

        // 从栈中恢复上一个页面 ID
        if let Some(prev_id) = self.page_stack.pop() {
            self.current_page_id = Some(prev_id);

            // 恢复上一个页面
            let ctx = self.create_context();
            if let Some(page) = self.cached_pages.get_mut(&prev_id) {
                page.on_event(PageEvent::Resume, &ctx)?;
                page.on_enter(&ctx)?;
            }
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

        if let Some(page_id) = self.current_page_id
            && let Some(page) = self.cached_pages.get_mut(&page_id)
        {
            let _ = page.on_input(msg, &ctx)?;
        }

        Ok(())
    }

    /// 处理输入事件并返回是否被消费
    ///
    /// # Errors
    ///
    /// 如果页面处理输入失败，返回错误
    pub fn handle_input_consumed(&mut self, msg: &crate::loops::InputMsg) -> Result<bool> {
        let ctx = self.create_context();

        if let Some(page_id) = self.current_page_id
            && let Some(page) = self.cached_pages.get_mut(&page_id)
        {
            return page.on_input(msg, &ctx);
        }

        Ok(false)
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

        if let Some(page_id) = self.current_page_id {
            if let Some(page) = self.cached_pages.get_mut(&page_id) {
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
        } else {
            Ok(true)
        }
    }

    /// 渲染当前页面
    pub fn render(&mut self) -> Vec<Instance> {
        let ctx = self.create_context();

        if let Some(page_id) = self.current_page_id {
            self.cached_pages
                .get_mut(&page_id)
                .map(|page| page.on_render(&ctx))
                .unwrap_or_default()
        } else {
            Vec::new()
        }
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
        if let Some(page_id) = self.current_page_id
            && let Some(page) = self.cached_pages.get_mut(&page_id)
        {
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
