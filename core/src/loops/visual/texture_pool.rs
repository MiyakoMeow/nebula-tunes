//! 纹理池管理模块
//!
//! 提供纹理重用和 LRU 缓存机制，减少 GPU 资源创建开销

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// 纹理池键（尺寸 + 格式）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PoolKey {
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
}

/// 纹理池条目
struct PoolEntry {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    last_used: Instant,
}

/// BGA 纹理池
///
/// 通过重用相同尺寸的纹理来减少 GPU 资理创建开销
pub struct BgaTexturePool {
    /// 按尺寸分组的纹理池
    pool: HashMap<PoolKey, Vec<PoolEntry>>,
    /// 最大池大小（每个尺寸）
    max_per_size: usize,
    /// 纹理过期时间
    expire_after: Duration,
}

impl BgaTexturePool {
    /// 创建纹理池
    ///
    /// # 参数
    ///
    /// - `max_per_size`: 每个尺寸的最大缓存数量
    /// - `expire_after`: 纹理过期时间，超过此时间未使用的纹理将被清理
    #[must_use]
    #[allow(dead_code)] // 公共 API，供未来使用
    pub fn new(max_per_size: usize, expire_after: Duration) -> Self {
        Self {
            pool: HashMap::new(),
            max_per_size,
            expire_after,
        }
    }

    /// 获取或创建纹理
    ///
    /// 优先从池中重用现有纹理，如果没有可用的则创建新纹理
    pub fn acquire(
        &mut self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
    ) -> (wgpu::Texture, wgpu::TextureView) {
        let key = PoolKey {
            width,
            height,
            format,
        };
        let now = Instant::now();

        // 清理过期条目并尝试复用
        if let Some(entries) = self.pool.get_mut(&key) {
            // 移除过期的纹理
            entries.retain(|e| now.duration_since(e.last_used) < self.expire_after);

            // 尝试复用最新释放的纹理
            if let Some(entry) = entries.pop() {
                return (entry.texture, entry.view);
            }
        }

        // 没有可复用的纹理，创建新纹理
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("bga-pooled-texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        (texture, view)
    }

    /// 归还纹理到池中
    ///
    /// 如果池未满，纹理将被缓存以供后续复用；否则将被丢弃
    pub fn release(
        &mut self,
        texture: wgpu::Texture,
        view: wgpu::TextureView,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
    ) {
        let key = PoolKey {
            width,
            height,
            format,
        };
        let entries = self.pool.entry(key).or_default();

        // 仅在池未满时缓存
        if entries.len() < self.max_per_size {
            entries.push(PoolEntry {
                texture,
                view,
                last_used: Instant::now(),
            });
        }
        // 超过限制则丢弃（GPU 会自动回收）
    }

    /// 清理所有过期的纹理
    ///
    /// 可定期调用此方法以释放不再使用的纹理
    #[allow(dead_code)] // 公共 API，供未来使用
    pub fn cleanup_expired(&mut self) {
        let now = Instant::now();
        for entries in self.pool.values_mut() {
            entries.retain(|e| now.duration_since(e.last_used) < self.expire_after);
        }
    }

    /// 清空所有缓存的纹理
    #[allow(dead_code)] // 公共 API，供未来使用
    pub fn clear(&mut self) {
        self.pool.clear();
    }
}

impl Default for BgaTexturePool {
    fn default() -> Self {
        // 默认配置：每个尺寸最多缓存 4 个纹理，过期时间为 10 秒
        Self {
            pool: HashMap::new(),
            max_per_size: 4,
            expire_after: Duration::from_secs(10),
        }
    }
}
