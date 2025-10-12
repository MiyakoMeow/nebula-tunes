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

/// 顶点着色器输入结构体
///
/// 定义了从顶点缓冲区读取的顶点属性
struct VertexInput {
    @location(0) position: vec2<f32>,  // 顶点位置
    @location(1) color: vec4<f32>,     // 顶点颜色
}

/// 顶点着色器输出结构体
///
/// 包含了顶点的最终位置和颜色信息
struct VertexOutput {
    @builtin(position) position: vec4<f32>,  // 顶点在裁剪空间中的位置
    @location(0) color: vec4<f32>,          // 传递给片段着色器的颜色
}

/// 顶点着色器主函数
///
/// 从顶点缓冲区读取顶点位置和颜色，并根据时间创建动画效果。
///
/// # 参数
/// * `input` - 从顶点缓冲区读取的顶点属性
///
/// # 返回
/// 返回包含位置和颜色的VertexOutput结构体
@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    // 创建动画效果 - 让三角形做圆周运动
    let offset_x = sin(time_data.time) * 0.3;  // X轴偏移，使用正弦函数
    let offset_y = cos(time_data.time) * 0.3;  // Y轴偏移，使用余弦函数

    // 构建输出结构体
    var output: VertexOutput;
    output.position = vec4<f32>(input.position.x + offset_x, input.position.y + offset_y, 0.0, 1.0);
    output.color = input.color;
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
