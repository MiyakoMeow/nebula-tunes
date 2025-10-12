//! # 图形渲染模块
//!
//! 本模块提供了基于wgpu的图形渲染功能，用于创建和管理WebGPU渲染上下文、
//! 渲染管线以及相关的渲染资源。

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;
use winit::window::Window;

/// 顶点结构体
///
/// 定义了每个顶点的属性，包括位置和颜色信息
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct Vertex {
    position: [f32; 2], // 顶点位置 (x, y)
    color: [f32; 4],    // 顶点颜色 (r, g, b, a)
}

/// 图形渲染状态结构体
///
/// 包含了WebGPU渲染所需的各种资源和状态信息，用于管理整个渲染管线。
pub struct State {
    /// 应用程序窗口，使用Arc包装以支持多所有权
    window: Arc<Window>,
    /// WebGPU设备，用于创建各种GPU资源
    device: wgpu::Device,
    /// 命令队列，用于向GPU提交渲染命令
    queue: wgpu::Queue,
    /// 窗口的物理尺寸（像素单位）
    size: winit::dpi::PhysicalSize<u32>,
    /// 渲染表面，用于显示渲染结果
    surface: wgpu::Surface<'static>,
    /// 表面纹理格式，定义了渲染目标的颜色格式
    surface_format: wgpu::TextureFormat,
    /// 渲染管线，包含了顶点着色器和片段着色器的配置
    render_pipeline: wgpu::RenderPipeline,
    /// 应用程序启动时间，用于动画计时
    start_time: std::time::Instant,
    /// 统一缓冲区，用于向着色器传递时间数据
    uniform_buffer: wgpu::Buffer,
    /// 统一绑定组，用于将缓冲区绑定到着色器
    uniform_bind_group: wgpu::BindGroup,
    /// 顶点缓冲区，存储三角形顶点数据
    vertex_buffer: wgpu::Buffer,
    /// 顶点数量
    num_vertices: u32,
}

impl State {
    /// 创建新的State实例
    ///
    /// 这个函数会初始化WebGPU渲染所需的所有资源，包括：
    /// - WebGPU实例和适配器
    /// - 逻辑设备和命令队列
    /// - 渲染表面和配置
    /// - 着色器模块
    /// - 渲染管线
    /// - 统一缓冲区和绑定组
    ///
    /// # 参数
    /// * `window` - 应用程序窗口，使用Arc包装
    ///
    /// # 返回
    /// 返回一个完全初始化的State实例
    pub async fn new(window: Arc<Window>) -> State {
        // 创建WebGPU实例，这是WebGPU API的入口点
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());

        // 请求适配器（物理GPU设备），这会选择最适合的GPU
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .unwrap();

        // 从适配器创建逻辑设备和命令队列
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .unwrap();

        // 获取窗口的当前尺寸
        let size = window.inner_size();

        // 从窗口创建渲染表面，这是显示渲染结果的地方
        let surface = instance.create_surface(window.clone()).unwrap();

        // 获取表面的能力信息，包括支持的纹理格式
        let cap = surface.get_capabilities(&adapter);
        let surface_format = cap.formats[0];

        // 从WGSL源文件创建着色器模块
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("graphics/triangle.wgsl").into()),
        });

        // 定义三角形的三个顶点数据
        let vertices = [
            Vertex {
                position: [0.0, 0.5],        // 顶部顶点
                color: [1.0, 0.0, 0.0, 1.0], // 红色
            },
            Vertex {
                position: [-0.5, -0.5],      // 左下顶点
                color: [0.0, 1.0, 0.0, 1.0], // 绿色
            },
            Vertex {
                position: [0.5, -0.5],       // 右下顶点
                color: [0.0, 0.0, 1.0, 1.0], // 蓝色
            },
        ];

        // 定义顶点缓冲区布局
        let vertex_buffer_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x2, // 位置属性
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x4, // 颜色属性
                },
            ],
        };

        // 创建绑定组布局，定义了着色器中uniform变量的布局
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Bind Group Layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,                             // 绑定点0
                visibility: wgpu::ShaderStages::VERTEX, // 只在顶点着色器中可见
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform, // 统一缓冲区类型
                    has_dynamic_offset: false,            // 不使用动态偏移
                    min_binding_size: None,               // 不限制最小绑定大小
                },
                count: None,
            }],
        });

        // 创建渲染管线布局，定义了整个渲染管线的资源布局
        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Render Pipeline Layout"),
                bind_group_layouts: &[&bind_group_layout], // 使用之前创建的绑定组布局
                push_constant_ranges: &[],                 // 没有推送常量
            });

        // 创建渲染管线，这是渲染操作的核心配置
        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            cache: None,
            // 顶点着色器配置
            vertex: wgpu::VertexState {
                module: &shader,                  // 使用之前创建的着色器模块
                entry_point: Some("vs_main"),     // 顶点着色器的入口函数名
                buffers: &[vertex_buffer_layout], // 使用顶点缓冲区布局
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            // 片段着色器配置
            fragment: Some(wgpu::FragmentState {
                module: &shader,              // 使用同一着色器模块
                entry_point: Some("fs_main"), // 片段着色器的入口函数名
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,                 // 渲染目标的纹理格式
                    blend: Some(wgpu::BlendState::REPLACE), // 禁用混合，直接替换像素
                    write_mask: wgpu::ColorWrites::ALL,     // 写入所有颜色通道
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            // 图元装配配置
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList, // 渲染三角形列表
                strip_index_format: None,                        // 不使用索引条带
                front_face: wgpu::FrontFace::Ccw,                // 逆时针为正面
                cull_mode: Some(wgpu::Face::Back),               // 剔除背面三角形
                unclipped_depth: false,                          // 启用深度裁剪
                polygon_mode: wgpu::PolygonMode::Fill,           // 填充整个多边形
                conservative: false,                             // 不使用保守光栅化
            },
            depth_stencil: None, // 禁用深度和模板测试
            // 多采样抗锯齿配置
            multisample: wgpu::MultisampleState {
                count: 1,                         // 1倍采样（无抗锯齿）
                mask: !0,                         // 采样所有子像素
                alpha_to_coverage_enabled: false, // 禁用alpha到覆盖
            },
            multiview: None, // 禁用多视图渲染
        });

        // 创建统一缓冲区，用于存储时间数据
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Uniform Buffer"),
            size: std::mem::size_of::<f32>() as u64, // 存储一个f32类型的时间值
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, // 用作uniform和允许CPU写入
            mapped_at_creation: false, // 创建时不映射到CPU内存
        });

        // 创建统一绑定组，将缓冲区绑定到着色器的绑定点0
        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Uniform Bind Group"),
            layout: &bind_group_layout, // 使用之前创建的绑定组布局
            entries: &[wgpu::BindGroupEntry {
                binding: 0, // 绑定到着色器的绑定点0
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &uniform_buffer, // 绑定之前创建的统一缓冲区
                    offset: 0,               // 从缓冲区开头开始
                    size: None,              // 绑定整个缓冲区
                }),
            }],
        });

        // 创建顶点缓冲区
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Vertex Buffer"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        // 创建State实例，包含所有初始化的WebGPU资源
        let state = State {
            window,
            device,
            queue,
            size,
            surface,
            surface_format,
            render_pipeline,
            start_time: std::time::Instant::now(), // 记录应用程序启动时间
            uniform_buffer,
            uniform_bind_group,
            vertex_buffer,
            num_vertices: vertices.len() as u32,
        };

        // 首次配置渲染表面
        state.configure_surface();

        state
    }

    /// 获取窗口引用
    ///
    /// # 返回
    /// 返回对窗口的不可变引用
    pub fn get_window(&self) -> &Window {
        &self.window
    }

    /// 配置渲染表面
    ///
    /// 这个方法配置渲染表面的参数，包括尺寸、格式和显示模式。
    /// 通常在窗口大小改变时调用。
    pub fn configure_surface(&self) {
        // 创建表面配置
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT, // 用作渲染附件
            format: self.surface_format,                   // 表面纹理格式
            // 请求与sRGB格式纹理视图的兼容性
            view_formats: vec![self.surface_format.add_srgb_suffix()],
            alpha_mode: wgpu::CompositeAlphaMode::Auto, // 自动选择alpha模式
            width: self.size.width,                     // 表面宽度
            height: self.size.height,                   // 表面高度
            desired_maximum_frame_latency: 2,           // 期望的最大帧延迟
            present_mode: wgpu::PresentMode::AutoVsync, // 自动垂直同步
        };
        // 应用配置到表面
        self.surface.configure(&self.device, &surface_config);
    }

    /// 处理窗口大小调整
    ///
    /// 当窗口大小发生变化时，更新内部尺寸记录并重新配置渲染表面。
    ///
    /// # 参数
    /// * `new_size` - 新的窗口物理尺寸
    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        self.size = new_size;

        // 重新配置表面以匹配新的尺寸
        self.configure_surface();
    }

    /// 执行一帧渲染
    ///
    /// 这个方法执行完整的渲染循环，包括：
    /// - 更新动画时间
    /// - 更新统一缓冲区
    /// - 获取表面纹理
    /// - 创建命令编码器
    /// - 执行渲染过程
    /// - 提交渲染命令
    pub fn render(&mut self) {
        // 计算动画经过的时间
        let elapsed = self.start_time.elapsed();
        let time = elapsed.as_secs_f32();

        // 更新统一缓冲区，将时间数据写入GPU
        self.queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[time]));

        // 获取当前交换链纹理用于渲染
        let surface_texture = self
            .surface
            .get_current_texture()
            .expect("获取下一个交换链纹理失败");
        let texture_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor {
                // 如果不使用add_srgb_suffix()，图像可能不是"伽马校正"的
                format: Some(self.surface_format.add_srgb_suffix()),
                ..Default::default()
            });

        // 创建命令编码器，用于记录渲染命令
        let mut encoder = self.device.create_command_encoder(&Default::default());

        // 创建渲染过程，将清除屏幕
        let mut renderpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &texture_view,  // 渲染目标纹理视图
                depth_slice: None,    // 无深度附件
                resolve_target: None, // 无多采样解析目标
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), // 清除为黑色
                    store: wgpu::StoreOp::Store,                   // 存储渲染结果
                },
            })],
            depth_stencil_attachment: None, // 无深度模板附件
            timestamp_writes: None,         // 无时间戳写入
            occlusion_query_set: None,      // 无遮挡查询
        });

        // 设置渲染管线和绑定组
        renderpass.set_pipeline(&self.render_pipeline); // 使用之前创建的渲染管线
        renderpass.set_bind_group(0, &self.uniform_bind_group, &[]); // 绑定统一缓冲区
        renderpass.set_vertex_buffer(0, self.vertex_buffer.slice(..)); // 绑定顶点缓冲区

        // 绘制三角形
        renderpass.draw(0..self.num_vertices, 0..1);

        // 结束渲染过程
        drop(renderpass);

        // 将命令提交到队列执行
        self.queue.submit([encoder.finish()]);
        self.window.pre_present_notify(); // 窗口呈现前通知
        surface_texture.present(); // 呈现纹理到屏幕
    }
}
