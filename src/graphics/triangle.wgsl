//! # 三角形着色器
//!
//! 这个WGSL着色器文件定义了一个彩色三角形。
//! 顶点着色器直接使用从CPU传递的顶点位置，
//! 片段着色器则简单地输出顶点颜色。

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
/// 从顶点缓冲区读取顶点位置和颜色，直接传递给片段着色器。
///
/// # 参数
/// * `input` - 从顶点缓冲区读取的顶点属性
///
/// # 返回
/// 返回包含位置和颜色的VertexOutput结构体
@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    // 构建输出结构体，直接使用输入的位置
    var output: VertexOutput;
    output.position = vec4<f32>(input.position.x, input.position.y, 0.0, 1.0);
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
