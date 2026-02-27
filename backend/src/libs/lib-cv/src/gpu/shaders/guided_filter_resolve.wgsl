struct GuidedParams {
    radius: u32,
    epsilon: f32,
    _pad: vec2<f32>,
};

@group(0) @binding(0)
var resolveInput : texture_2d<f32>;

@group(0) @binding(1)
var resolveCoeff : texture_2d<f32>;

@group(0) @binding(2)
var<uniform> resolveParams : GuidedParams;

@group(0) @binding(3)
var resolveOutput : texture_storage_2d<rgba8unorm, write>;

fn luminance(px: vec4<f32>) -> f32 {
    return dot(px.rgb, vec3<f32>(0.299, 0.587, 0.114));
}

@compute @workgroup_size(8, 8, 1)
fn guided_resolve_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let dims = textureDimensions(resolveInput);
    if (id.x >= dims.x || id.y >= dims.y) {
        return;
    }

    let x = i32(id.x);
    let y = i32(id.y);
    let radius = i32(resolveParams.radius);

    var sum_a = 0.0;
    var sum_b = 0.0;
    var count = 0.0;
    for (var dy = -radius; dy <= radius; dy = dy + 1) {
        for (var dx = -radius; dx <= radius; dx = dx + 1) {
            let sx = clamp(x + dx, 0, i32(dims.x) - 1);
            let sy = clamp(y + dy, 0, i32(dims.y) - 1);
            let coeff = textureLoad(resolveCoeff, vec2<i32>(sx, sy), 0);
            sum_a = sum_a + coeff.r;
            sum_b = sum_b + coeff.g;
            count = count + 1.0;
        }
    }

    let mean_a = sum_a / max(count, 1.0);
    let mean_b = sum_b / max(count, 1.0);
    let i = luminance(textureLoad(resolveInput, vec2<i32>(x, y), 0));
    let value = clamp(mean_a * i + mean_b, 0.0, 1.0);
    textureStore(resolveOutput, vec2<i32>(x, y), vec4<f32>(value, value, value, 1.0));
}
