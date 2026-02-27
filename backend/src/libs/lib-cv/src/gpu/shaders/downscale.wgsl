struct DownscaleParams {
    in_width: u32,
    in_height: u32,
    out_width: u32,
    out_height: u32,
    factor: u32,
    _pad: vec3<u32>,
};

@group(0) @binding(0)
var downscaleInput : texture_2d<f32>;

@group(0) @binding(1)
var downscaleOutput : texture_storage_2d<rgba8unorm, write>;

@group(0) @binding(2)
var<uniform> downscaleParams : DownscaleParams;

@compute @workgroup_size(8, 8, 1)
fn downscale_main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= downscaleParams.out_width || id.y >= downscaleParams.out_height) {
        return;
    }

    let start_x = id.x * downscaleParams.factor;
    let start_y = id.y * downscaleParams.factor;
    let end_x = min(start_x + downscaleParams.factor, downscaleParams.in_width);
    let end_y = min(start_y + downscaleParams.factor, downscaleParams.in_height);

    var accum = vec4<f32>(0.0);
    var count = 0u;

    for (var y = start_y; y < end_y; y = y + 1u) {
        for (var x = start_x; x < end_x; x = x + 1u) {
            let sample = textureLoad(downscaleInput, vec2<i32>(i32(x), i32(y)), 0);
            accum = accum + sample;
            count = count + 1u;
        }
    }

    if (count == 0u) {
        return;
    }

    let out = clamp(accum / f32(count), vec4<f32>(0.0), vec4<f32>(1.0));
    textureStore(downscaleOutput, vec2<i32>(i32(id.x), i32(id.y)), out);
}
