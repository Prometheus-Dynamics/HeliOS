struct ClaheParams {
    width: u32,
    height: u32,
    tile_grid: u32,
    _pad0: u32,
    clip_limit: f32,
    _pad1: vec3<u32>,
};

@group(0) @binding(0)
var claheInput: texture_2d<f32>;

@group(0) @binding(1)
var claheOutput: texture_storage_2d<rgba8unorm, write>;

@group(0) @binding(2)
var<uniform> params: ClaheParams;

var<workgroup> hist: array<atomic<u32>, 256>;
var<workgroup> bins: array<u32, 256>;
var<workgroup> lut: array<u32, 256>;

fn luma_u8(px: vec4<f32>) -> u32 {
    let r = u32(clamp(px.r * 255.0 + 0.5, 0.0, 255.0));
    let g = u32(clamp(px.g * 255.0 + 0.5, 0.0, 255.0));
    let b = u32(clamp(px.b * 255.0 + 0.5, 0.0, 255.0));
    return (77u * r + 150u * g + 29u * b + 128u) >> 8u;
}

@compute @workgroup_size(16, 16, 1)
fn clahe_main(
    @builtin(workgroup_id) wg: vec3<u32>,
    @builtin(local_invocation_id) lid: vec3<u32>,
) {
    let width = params.width;
    let height = params.height;
    if (width == 0u || height == 0u) {
        return;
    }

    let grid = max(params.tile_grid, 1u);
    let tile_w = min(max((width + grid - 1u) / grid, 4u), width);
    let tile_h = min(max((height + grid - 1u) / grid, 4u), height);
    let tiles_x = max((width + tile_w - 1u) / tile_w, 1u);
    let tiles_y = max((height + tile_h - 1u) / tile_h, 1u);

    if (wg.x >= tiles_x || wg.y >= tiles_y) {
        return;
    }

    let lane = lid.y * 16u + lid.x;
    atomicStore(&hist[lane], 0u);
    bins[lane] = 0u;
    lut[lane] = lane;
    workgroupBarrier();

    let x0 = wg.x * tile_w;
    let y0 = wg.y * tile_h;
    let x1 = min((wg.x + 1u) * tile_w, width);
    let y1 = min((wg.y + 1u) * tile_h, height);
    let tw = x1 - x0;
    let th = y1 - y0;

    var yy = lid.y;
    loop {
        if (yy >= th) {
            break;
        }
        var xx = lid.x;
        loop {
            if (xx >= tw) {
                break;
            }
            let gx = x0 + xx;
            let gy = y0 + yy;
            let px = textureLoad(claheInput, vec2<i32>(i32(gx), i32(gy)), 0);
            let lv = luma_u8(px);
            atomicAdd(&hist[lv], 1u);
            xx = xx + 16u;
        }
        yy = yy + 16u;
    }

    workgroupBarrier();

    if (lane == 0u) {
        let tile_pixels = max(tw * th, 1u);
        var clip_value: u32 = tile_pixels;
        if (params.clip_limit > 0.0) {
            let avg_per_bin = f32(tile_pixels) / 256.0;
            let scaled = max(params.clip_limit, 1.0) * avg_per_bin;
            clip_value = max(u32(round(scaled)), 1u);
        }

        var excess = 0u;
        for (var i = 0u; i < 256u; i = i + 1u) {
            var c = atomicLoad(&hist[i]);
            if (c > clip_value) {
                excess = excess + (c - clip_value);
                c = clip_value;
            }
            bins[i] = c;
        }

        let redist = excess / 256u;
        let rem = excess % 256u;
        for (var i = 0u; i < 256u; i = i + 1u) {
            bins[i] = bins[i] + redist + select(0u, 1u, i < rem);
        }

        var cdf_min = 0u;
        for (var i = 0u; i < 256u; i = i + 1u) {
            if (bins[i] != 0u) {
                cdf_min = bins[i];
                break;
            }
        }
        let denom = max(tile_pixels - cdf_min, 1u);
        var cumulative = 0u;
        for (var i = 0u; i < 256u; i = i + 1u) {
            cumulative = cumulative + bins[i];
            let v = select(0u, cumulative - cdf_min, cumulative > cdf_min);
            lut[i] = (v * 255u) / denom;
        }
    }

    workgroupBarrier();

    yy = lid.y;
    loop {
        if (yy >= th) {
            break;
        }
        var xx = lid.x;
        loop {
            if (xx >= tw) {
                break;
            }
            let gx = x0 + xx;
            let gy = y0 + yy;
            let px = textureLoad(claheInput, vec2<i32>(i32(gx), i32(gy)), 0);
            let lv = luma_u8(px);
            let out_v = f32(lut[lv]) / 255.0;
            textureStore(claheOutput, vec2<i32>(i32(gx), i32(gy)), vec4<f32>(out_v, out_v, out_v, 1.0));
            xx = xx + 16u;
        }
        yy = yy + 16u;
    }
}
