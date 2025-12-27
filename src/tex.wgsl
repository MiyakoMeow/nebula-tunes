struct Screen {
    size: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> screen: Screen;

struct BgaParams {
    rects: array<vec4<f32>, 4>,
    flags: vec4<u32>,
}

@group(0) @binding(1)
var<uniform> params: BgaParams;

@group(0) @binding(2)
var tex_bga: texture_2d<f32>;
@group(0) @binding(3)
var tex_layer: texture_2d<f32>;
@group(0) @binding(4)
var tex_layer2: texture_2d<f32>;
@group(0) @binding(5)
var tex_poor: texture_2d<f32>;

@group(0) @binding(6)
var sampler0: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) world: vec2<f32>,
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
    out.uv = vec2<f32>(
        vpos.x + 0.5,
        1.0 - (vpos.y + 0.5),
    );
    out.world = world;
    return out;
}

fn inside01(v: vec2<f32>) -> bool {
    return v.x >= 0.0 && v.x <= 1.0 && v.y >= 0.0 && v.y <= 1.0;
}

fn sample_layer(tex: texture_2d<f32>, world: vec2<f32>, rect: vec4<f32>, enabled: bool) -> vec4<f32> {
    if !enabled {
        return vec4<f32>(0.0);
    }
    let pos = rect.xy;
    let size = rect.zw;
    if size.x <= 0.0 || size.y <= 0.0 {
        return vec4<f32>(0.0);
    }
    let local = (world - pos) / size + vec2<f32>(0.5, 0.5);
    if !inside01(local) {
        return vec4<f32>(0.0);
    }
    let uv = vec2<f32>(local.x, 1.0 - local.y);
    return textureSample(tex, sampler0, uv);
}

fn over(bottom: vec4<f32>, top: vec4<f32>) -> vec4<f32> {
    let bottom_pm = bottom.rgb * bottom.a;
    let top_pm = top.rgb * top.a;
    let out_a = top.a + bottom.a * (1.0 - top.a);
    let out_pm = top_pm + bottom_pm * (1.0 - top.a);
    if out_a <= 0.0 {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(out_pm / out_a, out_a);
}

@fragment
fn fs_main(@location(1) world: vec2<f32>) -> @location(0) vec4<f32> {
    let c0 = sample_layer(tex_bga, world, params.rects[0], params.flags.x == 1u);
    let c1 = sample_layer(tex_layer, world, params.rects[1], params.flags.y == 1u);
    let c2 = sample_layer(tex_layer2, world, params.rects[2], params.flags.z == 1u);
    let c3 = sample_layer(tex_poor, world, params.rects[3], params.flags.w == 1u);
    var out = vec4<f32>(0.0);
    out = over(out, c0);
    out = over(out, c1);
    out = over(out, c2);
    out = over(out, c3);
    return out;
}
