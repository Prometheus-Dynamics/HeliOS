struct BinaryParams {
    threshold: f32,
    _pad: vec3<f32>,
};

@group(0) @binding(0)
var binaryInput : texture_2d<f32>;

@group(0) @binding(1)
var binaryOutput : texture_storage_2d<rgba8unorm, write>;

@group(0) @binding(2)
var<uniform> binaryParams : BinaryParams;

@compute @workgroup_size(8, 8, 1)
fn binary_main(@builtin(global_invocation_id) id : vec3<u32>) {
    let dims = textureDimensions(binaryInput);
    if (id.x >= dims.x || id.y >= dims.y) {
        return;
    }

    let coords = vec2<i32>(i32(id.x), i32(id.y));
    let pixel = textureLoad(binaryInput, coords, 0);
    let luminance = dot(pixel.rgb, vec3<f32>(0.299, 0.587, 0.114));
    let value = select(0.0, 1.0, luminance > binaryParams.threshold);
    textureStore(binaryOutput, coords, vec4<f32>(value, value, value, 1.0));
}
