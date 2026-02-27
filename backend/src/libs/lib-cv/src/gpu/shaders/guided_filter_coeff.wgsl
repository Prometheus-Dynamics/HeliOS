struct GuidedParams {
    radius: u32,
    epsilon: f32,
    _pad: vec2<f32>,
};

@group(0) @binding(0)
var guidedInput : texture_2d<f32>;

@group(0) @binding(1)
var guidedCoeff : texture_storage_2d<rgba16float, write>;

@group(0) @binding(2)
var<uniform> guidedParams : GuidedParams;

fn luminance(px: vec4<f32>) -> f32 {
    return dot(px.rgb, vec3<f32>(0.299, 0.587, 0.114));
}

@compute @workgroup_size(8, 8, 1)
fn guided_coeff_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let dims = textureDimensions(guidedInput);
    if (id.x >= dims.x || id.y >= dims.y) {
        return;
    }

    let x = i32(id.x);
    let y = i32(id.y);
    let radius = i32(guidedParams.radius);

    var sum_i = 0.0;
    var sum_ii = 0.0;
    var count = 0.0;
    for (var dy = -radius; dy <= radius; dy = dy + 1) {
        for (var dx = -radius; dx <= radius; dx = dx + 1) {
            let sx = clamp(x + dx, 0, i32(dims.x) - 1);
            let sy = clamp(y + dy, 0, i32(dims.y) - 1);
            let i = luminance(textureLoad(guidedInput, vec2<i32>(sx, sy), 0));
            sum_i = sum_i + i;
            sum_ii = sum_ii + i * i;
            count = count + 1.0;
        }
    }

    let mean_i = sum_i / max(count, 1.0);
    let mean_ii = sum_ii / max(count, 1.0);
    let var_i = max(mean_ii - mean_i * mean_i, 0.0);
    let a = var_i / (var_i + guidedParams.epsilon);
    let b = mean_i - a * mean_i;

    textureStore(guidedCoeff, vec2<i32>(x, y), vec4<f32>(a, b, 0.0, 0.0));
}
