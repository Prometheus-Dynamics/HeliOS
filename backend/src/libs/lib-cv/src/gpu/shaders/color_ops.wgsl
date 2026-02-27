struct ColorParam {
    value: f32,
    _pad: vec3<f32>,
};

@group(0) @binding(0)
var colorInput : texture_2d<f32>;

@group(0) @binding(1)
var colorOutput : texture_storage_2d<rgba8unorm, write>;

@group(0) @binding(2)
var<uniform> colorParams : ColorParam;

fn clamp_rgb(v: vec3<f32>) -> vec3<f32> {
    return clamp(v, vec3<f32>(0.0), vec3<f32>(1.0));
}

@compute @workgroup_size(8, 8, 1)
fn brightness_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let dims = textureDimensions(colorInput);
    if (id.x >= dims.x || id.y >= dims.y) {
        return;
    }
    let coords = vec2<i32>(i32(id.x), i32(id.y));
    let px = textureLoad(colorInput, coords, 0);
    let delta = colorParams.value;
    let rgb = clamp_rgb(px.rgb + delta);
    textureStore(colorOutput, coords, vec4<f32>(rgb, px.a));
}

@compute @workgroup_size(8, 8, 1)
fn contrast_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let dims = textureDimensions(colorInput);
    if (id.x >= dims.x || id.y >= dims.y) {
        return;
    }
    let coords = vec2<i32>(i32(id.x), i32(id.y));
    let px = textureLoad(colorInput, coords, 0);
    let factor = colorParams.value;
    let rgb = clamp_rgb((px.rgb - vec3<f32>(0.5)) * factor + vec3<f32>(0.5));
    textureStore(colorOutput, coords, vec4<f32>(rgb, px.a));
}

@compute @workgroup_size(8, 8, 1)
fn saturation_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let dims = textureDimensions(colorInput);
    if (id.x >= dims.x || id.y >= dims.y) {
        return;
    }
    let coords = vec2<i32>(i32(id.x), i32(id.y));
    let px = textureLoad(colorInput, coords, 0);
    let gray = dot(px.rgb, vec3<f32>(0.299, 0.587, 0.114));
    let rgb = clamp_rgb(mix(vec3<f32>(gray), px.rgb, colorParams.value));
    textureStore(colorOutput, coords, vec4<f32>(rgb, px.a));
}

@compute @workgroup_size(8, 8, 1)
fn grayscale_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let dims = textureDimensions(colorInput);
    if (id.x >= dims.x || id.y >= dims.y) {
        return;
    }
    let coords = vec2<i32>(i32(id.x), i32(id.y));
    let px = textureLoad(colorInput, coords, 0);
    let gray = dot(px.rgb, vec3<f32>(0.299, 0.587, 0.114));
    textureStore(colorOutput, coords, vec4<f32>(gray, gray, gray, px.a));
}

fn hue_rotate(rgb: vec3<f32>, degrees: f32) -> vec3<f32> {
    let rad = radians(degrees);
    let cos_h = cos(rad);
    let sin_h = sin(rad);
    let mat = mat3x3<f32>(
        vec3<f32>(0.213 + cos_h * 0.787 - sin_h * 0.213, 0.213 - cos_h * 0.213 + sin_h * 0.143, 0.213 - cos_h * 0.213 - sin_h * 0.787),
        vec3<f32>(0.715 - cos_h * 0.715 - sin_h * 0.715, 0.715 + cos_h * 0.285 + sin_h * 0.140, 0.715 - cos_h * 0.715 + sin_h * 0.715),
        vec3<f32>(0.072 - cos_h * 0.072 + sin_h * 0.928, 0.072 - cos_h * 0.072 - sin_h * 0.283, 0.072 + cos_h * 0.928 + sin_h * 0.072)
    );
    return clamp(mat * rgb, vec3<f32>(0.0), vec3<f32>(1.0));
}

@compute @workgroup_size(8, 8, 1)
fn hue_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let dims = textureDimensions(colorInput);
    if (id.x >= dims.x || id.y >= dims.y) {
        return;
    }
    let coords = vec2<i32>(i32(id.x), i32(id.y));
    let px = textureLoad(colorInput, coords, 0);
    let rgb = hue_rotate(px.rgb, colorParams.value);
    textureStore(colorOutput, coords, vec4<f32>(rgb, px.a));
}
