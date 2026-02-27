struct ConvolutionParams {
    factor: f32,
    bias: f32,
    _pad: vec2<f32>,
    kernel: array<vec4<f32>, 3>,
};

@group(0) @binding(0)
var convInput : texture_2d<f32>;

@group(0) @binding(1)
var convOutput : texture_storage_2d<rgba8unorm, write>;

@group(0) @binding(2)
var<uniform> convParams : ConvolutionParams;

fn luminance(value: vec4<f32>) -> f32 {
    return dot(value.rgb, vec3<f32>(0.299, 0.587, 0.114)) * 255.0;
}

@compute @workgroup_size(8, 8, 1)
fn convolution_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let dims = textureDimensions(convInput);
    if (id.x >= dims.x || id.y >= dims.y) {
        return;
    }

    let x = i32(id.x);
    let y = i32(id.y);
    var acc = 0.0;
    for (var ky = -1; ky <= 1; ky = ky + 1) {
        for (var kx = -1; kx <= 1; kx = kx + 1) {
            let sample_x = clamp(x + kx, 0, i32(dims.x) - 1);
            let sample_y = clamp(y + ky, 0, i32(dims.y) - 1);
            let row = u32(ky + 1);
            let col = u32(kx + 1);
            let weight = convParams.kernel[row][col];
            acc = acc + luminance(textureLoad(convInput, vec2<i32>(sample_x, sample_y), 0)) * weight;
        }
    }

    let value = acc * convParams.factor + convParams.bias;
    let normalized = clamp(value / 255.0, 0.0, 1.0);
    textureStore(convOutput, vec2<i32>(x, y), vec4<f32>(normalized, normalized, normalized, 1.0));
}
