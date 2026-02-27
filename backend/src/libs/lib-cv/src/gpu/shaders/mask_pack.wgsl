struct MaskPackParams {
    width: u32,
    height: u32,
    roi_x: u32,
    roi_y: u32,
    roi_w: u32,
    roi_h: u32,
    packs_w: u32,
    _pad: u32,
};

@group(0) @binding(0)
var maskInput : texture_2d<f32>;

@group(0) @binding(1)
var<storage, read_write> packedOut : array<u32>;

@group(0) @binding(2)
var<uniform> params : MaskPackParams;

fn luminance(value: vec4<f32>) -> f32 {
    return dot(value.rgb, vec3<f32>(0.299, 0.587, 0.114));
}

@compute @workgroup_size(8, 8, 1)
fn pack_mask_main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= params.packs_w || id.y >= params.roi_h) {
        return;
    }

    let base_x = params.roi_x + id.x * 4u;
    let y = params.roi_y + id.y;

    var packed: u32 = 0u;
    for (var i: u32 = 0u; i < 4u; i = i + 1u) {
        let x = base_x + i;
        var v: u32 = 0u;
        if (x < params.roi_x + params.roi_w && x < params.width && y < params.height) {
            let px = luminance(textureLoad(maskInput, vec2<i32>(i32(x), i32(y)), 0));
            v = select(0u, 255u, px > 0.0);
        }
        packed = packed | ((v & 0xFFu) << (i * 8u));
    }

    let idx = id.y * params.packs_w + id.x;
    packedOut[idx] = packed;
}
