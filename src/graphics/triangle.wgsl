struct TimeUniform {
    time: f32,
}

@group(0) @binding(0)
var<uniform> time_data: TimeUniform;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    let positions = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 0.5),
        vec2<f32>(-0.5, -0.5),
        vec2<f32>(0.5, -0.5)
    );

    let colors = array<vec4<f32>, 3>(
        vec4<f32>(1.0, 0.0, 0.0, 1.0),
        vec4<f32>(0.0, 1.0, 0.0, 1.0),
        vec4<f32>(0.0, 0.0, 1.0, 1.0)
    );

    let pos = positions[vertex_index];
    let color = colors[vertex_index];

    // Create animation - make triangle move in a circle
    let offset_x = sin(time_data.time) * 0.3;
    let offset_y = cos(time_data.time) * 0.3;

    var output: VertexOutput;
    output.position = vec4<f32>(pos.x + offset_x, pos.y + offset_y, 0.0, 1.0);
    output.color = color;
    return output;
}

@fragment
fn fs_main(@location(0) color: vec4<f32>) -> @location(0) vec4<f32> {
    return color;
}
