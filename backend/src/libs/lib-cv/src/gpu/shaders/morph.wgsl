struct MorphParams {
    radius: u32,
    norm: i32,
    op: u32,
    _pad: u32,
};

@group(0) @binding(0)
var morphInput : texture_2d<f32>;

@group(0) @binding(1)
var morphOutput : texture_storage_2d<rgba8unorm, write>;

@group(0) @binding(2)
var<uniform> morphParams : MorphParams;

fn within_kernel(dx: i32, dy: i32, radius: i32) -> bool {
    if (radius <= 0) {
        return dx == 0 && dy == 0;
    }
    switch morphParams.norm {
        case 1: { // L1
            return abs(dx) + abs(dy) <= radius;
        }
        case 2: { // L2
            return dx * dx + dy * dy <= radius * radius;
        }
        default: { // Linf / box
            return true;
        }
    }
}

@compute @workgroup_size(8, 8, 1)
fn morph_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let dims = textureDimensions(morphInput);
    if (id.x >= dims.x || id.y >= dims.y) {
        return;
    }

    let x = i32(id.x);
    let y = i32(id.y);
    let radius = i32(morphParams.radius);
    let op = morphParams.op;

    var accum = select(0.0, 1.0, op == 0u); // 0: erode (min), 1: dilate (max)

    for (var dy = -radius; dy <= radius; dy = dy + 1) {
        for (var dx = -radius; dx <= radius; dx = dx + 1) {
            if (!within_kernel(dx, dy, radius)) {
                continue;
            }
            let sx = clamp(x + dx, 0, i32(dims.x) - 1);
            let sy = clamp(y + dy, 0, i32(dims.y) - 1);
            let l = dot(textureLoad(morphInput, vec2<i32>(sx, sy), 0).rgb, vec3<f32>(0.299, 0.587, 0.114));
            if (op == 0u) {
                accum = min(accum, l);
            } else {
                accum = max(accum, l);
            }
        }
    }

    textureStore(morphOutput, vec2<i32>(x, y), vec4<f32>(accum, accum, accum, 1.0));
}
