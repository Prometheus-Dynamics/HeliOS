@group(0) @binding(0)
var diffA : texture_2d<f32>;

@group(0) @binding(1)
var diffB : texture_2d<f32>;

@group(0) @binding(3)
var diffOutput : texture_storage_2d<rgba8unorm, write>;

@compute @workgroup_size(8, 8, 1)
fn diff_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let dims = textureDimensions(diffA);
    if (id.x >= dims.x || id.y >= dims.y) {
        return;
    }
    let coords = vec2<i32>(i32(id.x), i32(id.y));
    let a = textureLoad(diffA, coords, 0);
    let b = textureLoad(diffB, coords, 0);
    let l = clamp(a.rgb - b.rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    textureStore(diffOutput, coords, vec4<f32>(l, 1.0));
}
