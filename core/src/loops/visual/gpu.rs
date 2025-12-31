//! GPU 初始化与表面配置模块
//!
//! 负责创建 wgpu 实例、设备、队列和配置表面

use anyhow::Result;
use futures_lite::future;
use wgpu;

/// GPU 初始化结果
pub struct GpuContext {
    /// wgpu 表面
    pub surface: wgpu::Surface<'static>,
    /// GPU 设备
    pub device: wgpu::Device,
    /// 命令队列
    pub queue: wgpu::Queue,
    /// 表面配置
    pub config: wgpu::SurfaceConfiguration,
}

/// 初始化 GPU 上下文
///
/// # 参数
///
/// - `window`: 窗口引用（用于创建表面）
/// - `size`: 初始表面尺寸 (width, height)
///
/// # 返回
///
/// 返回包含表面、设备、队列和配置的 GPU 上下文
///
/// # Errors
///
/// - 适配器请求失败
/// - 设备创建失败
/// - 表面创建失败
pub fn init_gpu<W>(window: &W, size: (u32, u32)) -> Result<GpuContext>
where
    W: wgpu::WindowHandle,
{
    let instance = wgpu::Instance::default();
    let surface = unsafe {
        instance.create_surface_unsafe(
            wgpu::SurfaceTargetUnsafe::from_window(window)
                .map_err(|e| anyhow::anyhow!(e.to_string()))?,
        )?
    };

    let adapter = future::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: Some(&surface),
    }))
    .map_err(|e| anyhow::anyhow!("request_adapter failed: {:?}", e))?;

    let (device, queue) = future::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
        label: None,
    }))?;

    let caps = surface.get_capabilities(&adapter);
    let format = caps
        .formats
        .first()
        .copied()
        .unwrap_or(wgpu::TextureFormat::Bgra8Unorm);
    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width: size.0,
        height: size.1,
        present_mode: wgpu::PresentMode::FifoRelaxed,
        alpha_mode: wgpu::CompositeAlphaMode::Opaque,
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    };

    Ok(GpuContext {
        surface,
        device,
        queue,
        config,
    })
}
