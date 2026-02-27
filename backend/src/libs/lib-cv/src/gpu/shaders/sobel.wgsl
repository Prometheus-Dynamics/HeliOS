struct SobelParams {
    width: u32,
    height: u32,
    _pad: vec2<u32>,
};

@group(0) @binding(0)
var sobelInput : texture_2d<f32>;

@group(0) @binding(1)
var sobelOutput : texture_storage_2d<rgba8unorm, write>;

@group(0) @binding(2)
var<uniform> sobelParams : SobelParams;

fn sample_luma(coords: vec2<i32>) -> f32 {
    let px = textureLoad(sobelInput, coords, 0);
    return dot(px.rgb, vec3<f32>(0.299, 0.587, 0.114));
}

@compute @workgroup_size(8, 8, 1)
fn sobel_main(@builtin(global_invocation_id) id : vec3<u32>) {
    let dims = textureDimensions(sobelInput);
    if (id.x >= dims.x || id.y >= dims.y) {
        return;
    }

    let x = i32(id.x);
    let y = i32(id.y);
    let w = i32(dims.x);
    let h = i32(dims.y);

    var sx = array<array<i32, 3>, 3>(
        array<i32, 3>(-1, 0, 1),
        array<i32, 3>(-2, 0, 2),
        array<i32, 3>(-1, 0, 1)
    );
    var sy = array<array<i32, 3>, 3>(
        array<i32, 3>(1, 2, 1),
        array<i32, 3>(0, 0, 0),
        array<i32, 3>(-1, -2, -1)
    );

    var gx = 0.0;
    var gy = 0.0;
    for (var j = -1; j <= 1; j = j + 1) {
        for (var i = -1; i <= 1; i = i + 1) {
            let sx_i = clamp(x + i, 0, w - 1);
            let sy_j = clamp(y + j, 0, h - 1);
            let l = sample_luma(vec2<i32>(sx_i, sy_j));
            let j_idx = u32(j + 1);
            let i_idx = u32(i + 1);
            gx = gx + f32(sx[j_idx][i_idx]) * l;
            gy = gy + f32(sy[j_idx][i_idx]) * l;
        }
    }

    let mag = clamp(length(vec2<f32>(gx, gy)) / 1443.0, 0.0, 1.0);
    let v = vec4<f32>(mag, mag, mag, 1.0);
    textureStore(sobelOutput, vec2<i32>(x, y), v);
}
