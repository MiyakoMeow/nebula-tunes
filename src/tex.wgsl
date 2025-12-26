struct Screen {
    size: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> screen: Screen;

@group(0) @binding(1)
var texture0: texture_2d<f32>;

@group(0) @binding(2)
var sampler0: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(
    @location(0) vpos: vec2<f32>,
    @location(1) ipos: vec2<f32>,
    @location(2) isize: vec2<f32>,
) -> VsOut {
    var out: VsOut;
    let world = vpos * isize + ipos;
    let ndc = vec2<f32>(
        world.x / (screen.size.x * 0.5),
        world.y / (screen.size.y * 0.5),
    );
    out.pos = vec4<f32>(ndc, 0.0, 1.0);
    out.uv = vpos * 0.5 + vec2<f32>(0.5, 0.5);
    return out;
}

@fragment
fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let color = textureSample(texture0, sampler0, uv);
    return color;
}
