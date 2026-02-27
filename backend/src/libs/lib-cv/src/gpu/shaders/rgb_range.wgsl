struct RangeParams {
    count: u32,
    _pad: vec3<u32>,
};

struct RgbRange {
    r_min: i32,
    r_max: i32,
    g_min: i32,
    g_max: i32,
    b_min: i32,
    b_max: i32,
    _pad: vec2<i32>,
};

@group(0) @binding(0)
var rgbInput : texture_2d<f32>;

@group(0) @binding(1)
var rgbOutput : texture_storage_2d<rgba8unorm, write>;

@group(0) @binding(2)
var<storage, read> rgbRanges : array<RgbRange>;

@group(0) @binding(3)
var<uniform> rgbParams : RangeParams;

@compute @workgroup_size(8, 8, 1)
fn rgb_mask_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let dims = textureDimensions(rgbInput);
    if (id.x >= dims.x || id.y >= dims.y) {
        return;
    }

    let color = textureLoad(rgbInput, vec2<i32>(i32(id.x), i32(id.y)), 0);
    let r = color.r * 255.0;
    let g = color.g * 255.0;
    let b = color.b * 255.0;

    var matched = false;
    for (var i: u32 = 0u; i < rgbParams.count; i = i + 1u) {
        let range = rgbRanges[i];
        if (r >= f32(range.r_min) && r <= f32(range.r_max) &&
            g >= f32(range.g_min) && g <= f32(range.g_max) &&
            b >= f32(range.b_min) && b <= f32(range.b_max)) {
            matched = true;
            break;
        }
    }

    let v = select(0.0, 1.0, matched);
    textureStore(rgbOutput, vec2<i32>(i32(id.x), i32(id.y)), vec4<f32>(v, v, v, 1.0));
}
