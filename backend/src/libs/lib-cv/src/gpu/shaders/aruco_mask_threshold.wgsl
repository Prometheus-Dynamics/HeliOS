struct MaskThresholdParams {
    offset: f32,
    invert: u32,
    _pad: vec2<u32>,
};

@group(0) @binding(0)
var maskInput : texture_2d<f32>;

@group(0) @binding(1)
var maskMean : texture_2d<f32>;

@group(0) @binding(2)
var maskOutput : texture_storage_2d<rgba8unorm, write>;

@group(0) @binding(3)
var<uniform> maskParams : MaskThresholdParams;

fn luminance(value: vec4<f32>) -> f32 {
    return dot(value.rgb, vec3<f32>(0.299, 0.587, 0.114));
}

@compute @workgroup_size(8, 8, 1)
fn mask_threshold_r8_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let dims = textureDimensions(maskInput);
    if (id.x >= dims.x || id.y >= dims.y) {
        return;
    }

    let coords = vec2<i32>(i32(id.x), i32(id.y));
    let px = luminance(textureLoad(maskInput, coords, 0));
    let mean = luminance(textureLoad(maskMean, coords, 0));
    let threshold = mean - maskParams.offset;
    var v = select(0.0, 1.0, px > threshold);
    if (maskParams.invert != 0u) {
        v = 1.0 - v;
    }
    textureStore(maskOutput, coords, vec4<f32>(v, v, v, 1.0));
}
