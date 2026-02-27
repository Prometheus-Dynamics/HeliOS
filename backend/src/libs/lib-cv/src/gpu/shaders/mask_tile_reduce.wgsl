struct MaskTileParams {
    width: u32,
    height: u32,
    tile: u32,
    tiles_w: u32,
    tiles_h: u32,
    _pad: vec3<u32>,
};

@group(0) @binding(0)
var maskInput : texture_2d<f32>;

@group(0) @binding(1)
var<storage, read_write> tileOut : array<u32>;

@group(0) @binding(2)
var<uniform> params : MaskTileParams;

fn luminance(value: vec4<f32>) -> f32 {
    return dot(value.rgb, vec3<f32>(0.299, 0.587, 0.114));
}

@compute @workgroup_size(8, 8, 1)
fn tile_reduce_main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= params.tiles_w || id.y >= params.tiles_h) {
        return;
    }

    let x0 = id.x * params.tile;
    let y0 = id.y * params.tile;
    let x1 = min(x0 + params.tile, params.width);
    let y1 = min(y0 + params.tile, params.height);

    var on: u32 = 0u;
    var yy = y0;
    loop {
        if (yy >= y1 || on != 0u) {
            break;
        }
        var xx = x0;
        loop {
            if (xx >= x1) {
                break;
            }
            let px = luminance(textureLoad(maskInput, vec2<i32>(i32(xx), i32(yy)), 0));
            if (px > 0.0) {
                on = 1u;
                break;
            }
            xx = xx + 1u;
        }
        yy = yy + 1u;
    }

    let idx = id.y * params.tiles_w + id.x;
    tileOut[idx] = on;
}
