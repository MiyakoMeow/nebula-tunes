//! # 三角形着色器
//!
//! 这个WGSL着色器文件定义了一个带有动画效果的彩色三角形。
//! 顶点着色器根据时间让三角形在屏幕上做圆周运动，
//! 片段着色器则简单地输出顶点颜色。

/// 时间统一变量结构体
///
/// 用于向GPU着色器传递当前动画时间
struct TimeUniform {
    time: f32,  // 当前时间（秒）
}

/// 绑定组0，绑定点0的统一变量
///
/// 这个变量从CPU传递时间数据到GPU着色器
@group(0) @binding(0)
var<uniform> time_data: TimeUniform;

/// 顶点着色器输出结构体
///
/// 包含了顶点的最终位置和颜色信息
struct VertexOutput {
    @builtin(position) position: vec4<f32>,  // 顶点在裁剪空间中的位置
    @location(0) color: vec4<f32>,          // 传递给片段着色器的颜色
}

/// 顶点着色器主函数
///
/// 使用内置顶点索引来确定顶点位置和颜色，并根据时间创建动画效果。
///
/// # 参数
/// * `vertex_index` - 内置顶点索引（0、1或2）
///
/// # 返回
/// 返回包含位置和颜色的VertexOutput结构体
@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    // 定义三角形的三个顶点位置（在裁剪空间中）
    let positions = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 0.5),    // 顶部顶点
        vec2<f32>(-0.5, -0.5),  // 左下顶点
        vec2<f32>(0.5, -0.5)    // 右下顶点
    );

    // 定义每个顶点的颜色（红、绿、蓝）
    let colors = array<vec4<f32>, 3>(
        vec4<f32>(1.0, 0.0, 0.0, 1.0),  // 红色
        vec4<f32>(0.0, 1.0, 0.0, 1.0),  // 绿色
        vec4<f32>(0.0, 0.0, 1.0, 1.0)   // 蓝色
    );

    // 根据顶点索引获取对应的位置和颜色
    let pos = positions[vertex_index];
    let color = colors[vertex_index];

    // 创建动画效果 - 让三角形做圆周运动
    let offset_x = sin(time_data.time) * 0.3;  // X轴偏移，使用正弦函数
    let offset_y = cos(time_data.time) * 0.3;  // Y轴偏移，使用余弦函数

    // 构建输出结构体
    var output: VertexOutput;
    output.position = vec4<f32>(pos.x + offset_x, pos.y + offset_y, 0.0, 1.0);
    output.color = color;
    return output;
}

/// 片段着色器主函数
///
/// 这个片段着色器非常简单，只是将顶点着色器传递的颜色直接输出。
/// 在实际应用中，这里可以添加纹理采样、光照计算等复杂的片段处理逻辑。
///
/// # 参数
/// * `color` - 从顶点着色器插值得到的颜色
///
/// # 返回
/// 返回最终的片段颜色
@fragment
fn fs_main(@location(0) color: vec4<f32>) -> @location(0) vec4<f32> {
    return color;  // 直接返回输入颜色
}
