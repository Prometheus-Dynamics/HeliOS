struct BlurParams {
    width: u32,
    height: u32,
    radius: u32,
    kernel_len: u32,
};

@group(0) @binding(0)
var blurInput : texture_2d<f32>;

@group(0) @binding(1)
var blurOutput : texture_storage_2d<rgba8unorm, write>;

@group(0) @binding(2)
var<storage, read> blurWeights : array<f32>;

@group(0) @binding(3)
var<uniform> blurParams : BlurParams;

@compute @workgroup_size(8, 8, 1)
fn blur_horizontal_main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= blurParams.width || id.y >= blurParams.height) {
        return;
    }

    let x = i32(id.x);
    let y = i32(id.y);
    let radius = i32(blurParams.radius);
    var accum = vec4<f32>(0.0);

    for (var offset = -radius; offset <= radius; offset = offset + 1) {
        let kernel_index = u32(offset + radius);
        if (kernel_index >= blurParams.kernel_len) {
            continue;
        }
        let weight = blurWeights[kernel_index];
        let sample_x = clamp(x + offset, 0, i32(blurParams.width) - 1);
        let sample = textureLoad(blurInput, vec2<i32>(sample_x, y), 0);
        accum = accum + sample * weight;
    }

    textureStore(blurOutput, vec2<i32>(x, y), clamp(accum, vec4<f32>(0.0), vec4<f32>(1.0)));
}

@compute @workgroup_size(8, 8, 1)
fn blur_vertical_main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= blurParams.width || id.y >= blurParams.height) {
        return;
    }

    let x = i32(id.x);
    let y = i32(id.y);
    let radius = i32(blurParams.radius);
    var accum = vec4<f32>(0.0);

    for (var offset = -radius; offset <= radius; offset = offset + 1) {
        let kernel_index = u32(offset + radius);
        if (kernel_index >= blurParams.kernel_len) {
            continue;
        }
        let weight = blurWeights[kernel_index];
        let sample_y = clamp(y + offset, 0, i32(blurParams.height) - 1);
        let sample = textureLoad(blurInput, vec2<i32>(x, sample_y), 0);
        accum = accum + sample * weight;
    }

    textureStore(blurOutput, vec2<i32>(x, y), clamp(accum, vec4<f32>(0.0), vec4<f32>(1.0)));
}
