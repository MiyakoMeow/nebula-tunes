//! 页面抽象层：统一管理不同界面状态
//!
//! 提供页面生命周期管理、事件处理和渲染抽象，为多页面系统（标题、设置、游戏等）提供基础架构。

pub use crate::pages_manager::PageManager;

use std::any::Any;

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
