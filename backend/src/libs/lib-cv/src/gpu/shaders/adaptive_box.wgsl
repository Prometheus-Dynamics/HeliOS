struct BoxBlurParams {
    width: u32,
    height: u32,
    radius: u32,
    inv_kernel: f32,
};

@group(0) @binding(0)
var boxInput : texture_2d<f32>;

@group(0) @binding(1)
var boxOutput : texture_storage_2d<rgba8unorm, write>;

@group(0) @binding(2)
var<uniform> boxParams : BoxBlurParams;

fn luminance(value: vec4<f32>) -> f32 {
    return dot(value.rgb, vec3<f32>(0.299, 0.587, 0.114));
}

@compute @workgroup_size(8, 8, 1)
fn box_horizontal_main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= boxParams.width || id.y >= boxParams.height) {
        return;
    }

    let x = i32(id.x);
    let y = i32(id.y);
    let radius = i32(boxParams.radius);
    var sum = 0.0;

    for (var offset = -radius; offset <= radius; offset = offset + 1) {
        let sample_x = clamp(x + offset, 0, i32(boxParams.width) - 1);
        let sample = textureLoad(boxInput, vec2<i32>(sample_x, y), 0);
        sum = sum + luminance(sample);
    }

    let mean = clamp(sum * boxParams.inv_kernel, 0.0, 1.0);
    textureStore(boxOutput, vec2<i32>(x, y), vec4<f32>(mean, mean, mean, 1.0));
}

@compute @workgroup_size(8, 8, 1)
fn box_vertical_main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= boxParams.width || id.y >= boxParams.height) {
        return;
    }

    let x = i32(id.x);
    let y = i32(id.y);
    let radius = i32(boxParams.radius);
    var sum = 0.0;

    for (var offset = -radius; offset <= radius; offset = offset + 1) {
        let sample_y = clamp(y + offset, 0, i32(boxParams.height) - 1);
        let sample = textureLoad(boxInput, vec2<i32>(x, sample_y), 0);
        sum = sum + luminance(sample);
    }

    let mean = clamp(sum * boxParams.inv_kernel, 0.0, 1.0);
    textureStore(boxOutput, vec2<i32>(x, y), vec4<f32>(mean, mean, mean, 1.0));
}
