struct RangeParams {
    count: u32,
    _pad: vec3<u32>,
};

struct HsvRange {
    h_min: f32,
    h_max: f32,
    s_min: f32,
    s_max: f32,
    v_min: f32,
    v_max: f32,
    _pad: vec2<f32>,
};

@group(0) @binding(0)
var hsvInput : texture_2d<f32>;

@group(0) @binding(1)
var hsvOutput : texture_storage_2d<rgba8unorm, write>;

@group(0) @binding(2)
var<storage, read> hsvRanges : array<HsvRange>;

@group(0) @binding(3)
var<uniform> hsvParams : RangeParams;

fn to_hsv(c: vec3<f32>) -> vec3<f32> {
    let maxc = max(max(c.r, c.g), c.b);
    let minc = min(min(c.r, c.g), c.b);
    let delta = maxc - minc;

    var h = 0.0;
    if (delta != 0.0) {
        if (maxc == c.r) {
            h = 60.0 * ((c.g - c.b) / delta % 6.0);
        } else if (maxc == c.g) {
            h = 60.0 * (((c.b - c.r) / delta) + 2.0);
        } else {
            h = 60.0 * (((c.r - c.g) / delta) + 4.0);
        }
    }
    if (h < 0.0) {
        h = h + 360.0;
    }

    let s = select(0.0, delta / maxc, maxc != 0.0);
    let v = maxc;
    return vec3<f32>(h, s, v);
}

@compute @workgroup_size(8, 8, 1)
fn hsv_mask_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let dims = textureDimensions(hsvInput);
    if (id.x >= dims.x || id.y >= dims.y) {
        return;
    }

    let color = textureLoad(hsvInput, vec2<i32>(i32(id.x), i32(id.y)), 0).rgb;
    let hsv = to_hsv(color);

    var matched = false;
    for (var i: u32 = 0u; i < hsvParams.count; i = i + 1u) {
        let range = hsvRanges[i];
        if (hsv.x >= range.h_min && hsv.x <= range.h_max &&
            hsv.y >= range.s_min && hsv.y <= range.s_max &&
            hsv.z >= range.v_min && hsv.z <= range.v_max) {
            matched = true;
            break;
        }
    }

    let v = select(0.0, 1.0, matched);
    textureStore(hsvOutput, vec2<i32>(i32(id.x), i32(id.y)), vec4<f32>(v, v, v, 1.0));
}
