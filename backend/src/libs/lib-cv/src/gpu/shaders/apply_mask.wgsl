@group(0) @binding(0)
var applyInput : texture_2d<f32>;

@group(0) @binding(1)
var applyMask : texture_2d<f32>;

@group(0) @binding(2)
var applyOutput : texture_storage_2d<rgba8unorm, write>;

@compute @workgroup_size(8, 8, 1)
fn apply_mask_main(@builtin(global_invocation_id) id : vec3<u32>) {
    let dims = textureDimensions(applyInput);
    if (id.x >= dims.x || id.y >= dims.y) {
        return;
    }

    let coords = vec2<i32>(i32(id.x), i32(id.y));
    let color = textureLoad(applyInput, coords, 0);
    let mask = textureLoad(applyMask, coords, 0);
    let keep = select(0.0, 1.0, mask.r > 0.5);
    textureStore(applyOutput, coords, vec4<f32>(color.rgb * keep, color.a));
}
