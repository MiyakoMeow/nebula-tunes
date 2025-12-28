//! 视频纹理管理
//!
//! 管理视频帧的 GPU 纹理上传和缓存

use super::DecodedFrame;
use anyhow::Result;
use wgpu;

/// 视频纹理管理器
///
/// 使用双缓冲或多缓冲机制，避免解码和渲染冲突
pub struct TextureManager {
    /// 纹理缓冲区
    textures: Vec<Option<wgpu::Texture>>,
    /// 纹理视图缓冲区
    views: Vec<Option<wgpu::TextureView>>,
    /// 当前使用的纹理索引
    current_index: usize,
    /// 纹理宽度
    width: u32,
    /// 纹理高度
    height: u32,
}

impl TextureManager {
    /// 创建纹理管理器
    ///
    /// count: 缓冲区数量，建议 2-3 个纹理（双缓冲或三缓冲）
    pub fn new(device: &wgpu::Device, width: u32, height: u32, count: usize) -> Self {
        let mut textures = Vec::with_capacity(count);
        let mut views = Vec::with_capacity(count);

        for _ in 0..count {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("video-frame-texture"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            textures.push(Some(texture));
            views.push(Some(view));
        }

        Self {
            textures,
            views,
            current_index: 0,
            width,
            height,
        }
    }

    /// 上传帧数据到纹理
    ///
    /// 返回当前纹理的视图
    pub fn upload_frame(
        &mut self,
        queue: &wgpu::Queue,
        frame: &DecodedFrame,
    ) -> Result<&wgpu::TextureView> {
        // 检查尺寸是否匹配
        if frame.width != self.width || frame.height != self.height {
            anyhow::bail!(
                "帧尺寸不匹配: 期望 {}x{}, 实际 {}x{}",
                self.width,
                self.height,
                frame.width,
                frame.height
            );
        }

        let idx = self.current_index;
        if let (Some(texture_opt), Some(view_opt)) = (self.textures.get(idx), self.views.get(idx))
            && let (Some(texture), Some(_view)) = (texture_opt.as_ref(), view_opt.as_ref())
        {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &frame.rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * frame.width),
                    rows_per_image: Some(frame.height),
                },
                wgpu::Extent3d {
                    width: frame.width,
                    height: frame.height,
                    depth_or_array_layers: 1,
                },
            );
        }

        // 切换到下一个纹理
        self.current_index = (self.current_index + 1) % self.textures.len();

        self.views
            .get(idx)
            .and_then(|v| v.as_ref())
            .ok_or_else(|| anyhow::anyhow!("无法获取纹理视图"))
    }

    /// 获取当前纹理视图（不上传新帧）
    #[must_use]
    #[allow(dead_code)]
    pub fn current_view(&self) -> Option<&wgpu::TextureView> {
        let idx = (self.current_index + self.textures.len() - 1) % self.textures.len();
        self.views.get(idx).and_then(|v| v.as_ref())
    }

    /// 重置到初始状态
    #[allow(dead_code)]
    pub const fn reset(&mut self) {
        self.current_index = 0;
    }
}
