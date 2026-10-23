//! A glTF 2.0 binary (`.glb`) writer: one buffer, PNG images embedded as buffer views, and the
//! scene's nodes, meshes, cameras and animations as JSON. Only what the simulator needs.
//!
//! The reader in the tests checks what the writer makes: the header and chunks, every buffer
//! view and accessor within the binary chunk, and the JSON's references.

use serde_json::{Value, json};

pub const TARGET_ARRAY_BUFFER: u32 = 34962;
const TARGET_ELEMENT_ARRAY_BUFFER: u32 = 34963;
const COMPONENT_UNSIGNED_SHORT: u32 = 5123;
const COMPONENT_FLOAT: u32 = 5126;
/// NEAREST magnification and minification, REPEAT wrapping.
const SAMPLER_NEAREST_REPEAT: usize = 0;
const FILTER_NEAREST: u32 = 9728;
const WRAP_REPEAT: u32 = 10497;

#[derive(Default)]
pub struct Builder {
    bin: Vec<u8>,
    buffer_views: Vec<Value>,
    accessors: Vec<Value>,
    images: Vec<Value>,
    textures: Vec<Value>,
    materials: Vec<Value>,
    meshes: Vec<Value>,
    nodes: Vec<Value>,
    cameras: Vec<Value>,
    animations: Vec<Value>,
}

/// One vertex attribute set for a mesh: accessor indices.
pub struct MeshAccessors {
    pub positions: usize,
    pub normals: usize,
    pub uvs: usize,
    pub indices: usize,
}

impl Builder {
    /// Appends `bytes` (4-byte aligned) as a buffer view and returns its index.
    fn view(&mut self, bytes: &[u8], target: Option<u32>) -> usize {
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        let offset = self.bin.len();
        self.bin.extend_from_slice(bytes);
        let mut view = json!({ "buffer": 0, "byteOffset": offset, "byteLength": bytes.len() });
        if let Some(target) = target {
            view["target"] = json!(target);
        }
        self.buffer_views.push(view);
        self.buffer_views.len() - 1
    }

    /// A float accessor of `width` components per element (SCALAR, VEC2, VEC3 or VEC4).
    pub fn floats(&mut self, data: &[f32], width: usize, target: Option<u32>) -> usize {
        assert!(width > 0 && data.len().is_multiple_of(width), "float data is whole elements");
        let bytes: Vec<u8> = data.iter().flat_map(|v| v.to_le_bytes()).collect();
        let view = self.view(&bytes, target);
        let kind = match width {
            1 => "SCALAR",
            2 => "VEC2",
            3 => "VEC3",
            _ => "VEC4",
        };
        let count = data.len() / width;
        let mut accessor = json!({ "bufferView": view, "componentType": COMPONENT_FLOAT, "count": count, "type": kind });
        // Bounds per component, as the spec requires for POSITION and animation input.
        let mut min = vec![f64::INFINITY; width];
        let mut max = vec![f64::NEG_INFINITY; width];
        for element in data.chunks(width) {
            for (c, v) in element.iter().enumerate() {
                min[c] = min[c].min(f64::from(*v));
                max[c] = max[c].max(f64::from(*v));
            }
        }
        if count > 0 {
            accessor["min"] = json!(min);
            accessor["max"] = json!(max);
        }
        self.accessors.push(accessor);
        self.accessors.len() - 1
    }

    /// A u16 index accessor (SCALAR).
    pub fn indices(&mut self, data: &[u16]) -> usize {
        let bytes: Vec<u8> = data.iter().flat_map(|v| v.to_le_bytes()).collect();
        let view = self.view(&bytes, Some(TARGET_ELEMENT_ARRAY_BUFFER));
        self.accessors.push(json!({ "bufferView": view, "componentType": COMPONENT_UNSIGNED_SHORT, "count": data.len(), "type": "SCALAR" }));
        self.accessors.len() - 1
    }

    /// A PNG image (embedded) and a texture of it with the scene's sampler.
    pub fn texture_png(&mut self, png: &[u8]) -> usize {
        let view = self.view(png, None);
        self.images.push(json!({ "bufferView": view, "mimeType": "image/png" }));
        self.textures.push(json!({ "source": self.images.len() - 1, "sampler": SAMPLER_NEAREST_REPEAT }));
        self.textures.len() - 1
    }

    /// An unlit-looking material: the texture as base colour, double-sided (a tag is seen from
    /// the front, but a floor's back can be in view too).
    pub fn material(&mut self, texture: usize) -> usize {
        self.materials.push(json!({
            "name": format!("material-{texture}"),
            "pbrMetallicRoughness": {
                "baseColorTexture": { "index": texture },
                "baseColorFactor": [1.0, 1.0, 1.0, 1.0],
                "metallicFactor": 0.0,
                "roughnessFactor": 1.0,
            },
            "doubleSided": true,
        }));
        self.materials.len() - 1
    }

    pub fn mesh(&mut self, name: &str, accessors: &MeshAccessors, material: usize) -> usize {
        self.meshes.push(json!({
            "name": name,
            "primitives": [{
                "attributes": { "POSITION": accessors.positions, "NORMAL": accessors.normals, "TEXCOORD_0": accessors.uvs },
                "indices": accessors.indices,
                "material": material,
                "mode": 4,
            }],
        }));
        self.meshes.len() - 1
    }

    pub fn camera(&mut self, yfov_rad: f64, aspect_ratio: f64) -> usize {
        self.cameras.push(json!({
            "name": "camera",
            "type": "perspective",
            "perspective": { "yfov": yfov_rad, "aspectRatio": aspect_ratio, "znear": 0.01, "zfar": 1000.0 },
        }));
        self.cameras.len() - 1
    }

    /// A node with an optional translation (metres), rotation (`[x, y, z, w]`), mesh or camera.
    pub fn node(&mut self, name: &str, translation: [f64; 3], rotation: [f64; 4], mesh: Option<usize>, camera: Option<usize>) -> usize {
        let mut node = json!({ "name": name, "translation": translation, "rotation": rotation });
        if let Some(mesh) = mesh {
            node["mesh"] = json!(mesh);
        }
        if let Some(camera) = camera {
            node["camera"] = json!(camera);
        }
        self.nodes.push(node);
        self.nodes.len() - 1
    }

    /// Looped LINEAR translation and rotation animations of `node`: `times` in seconds (strictly
    /// increasing, starting at 0), one translation and one rotation per time.
    pub fn animate(&mut self, node: usize, times: &[f32], translations: &[[f32; 3]], rotations: &[[f32; 4]]) {
        assert!(times.len() == translations.len() && times.len() == rotations.len() && !times.is_empty());
        let input = self.floats(times, 1, None);
        let translation_output = self.floats(&translations.concat(), 3, None);
        let rotation_output = self.floats(&rotations.concat(), 4, None);
        let samplers = vec![
            json!({ "input": input, "output": translation_output, "interpolation": "LINEAR" }),
            json!({ "input": input, "output": rotation_output, "interpolation": "LINEAR" }),
        ];
        let channels = vec![
            json!({ "sampler": 0, "target": { "node": node, "path": "translation" } }),
            json!({ "sampler": 1, "target": { "node": node, "path": "rotation" } }),
        ];
        self.animations.push(json!({ "channels": channels, "samplers": samplers }));
    }

    /// The `.glb` bytes: a 12-byte header, the JSON chunk (padded with spaces) and the binary
    /// chunk (padded with zeros). The scene's root nodes are `roots`.
    pub fn finish(mut self, roots: &[usize]) -> Vec<u8> {
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        let mut document = json!({
            "asset": { "version": "2.0", "generator": "helios-sim-scene" },
            "scene": 0,
            "scenes": [{ "nodes": roots }],
            "nodes": self.nodes,
            "meshes": self.meshes,
            "cameras": self.cameras,
            "materials": self.materials,
            "textures": self.textures,
            "images": self.images,
            "samplers": [{ "magFilter": FILTER_NEAREST, "minFilter": FILTER_NEAREST, "wrapS": WRAP_REPEAT, "wrapT": WRAP_REPEAT }],
            "accessors": self.accessors,
            "bufferViews": self.buffer_views,
            "buffers": [{ "byteLength": self.bin.len() }],
        });
        if !self.animations.is_empty() {
            document["animations"] = json!(self.animations);
        }
        let mut json_bytes = serde_json::to_vec(&document).expect("the document is JSON");
        while !json_bytes.len().is_multiple_of(4) {
            json_bytes.push(b' ');
        }
        let total = 12 + 8 + json_bytes.len() + 8 + self.bin.len();
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2_u32.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(&json_bytes);
        out.extend_from_slice(&(self.bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&self.bin);
        out
    }
}

#[cfg(test)]
pub mod reader {
    //! A reader for what [`Builder::finish`] writes, checking the spec's structure.
    use serde_json::Value;

    pub struct Glb {
        pub json: Value,
        pub bin: Vec<u8>,
    }

    fn u32_at(bytes: &[u8], at: usize) -> Result<u32, String> {
        let slice = bytes.get(at..at + 4).ok_or_else(|| format!("no u32 at {at}"))?;
        Ok(u32::from_le_bytes(slice.try_into().expect("four bytes")))
    }

    pub fn parse(bytes: &[u8]) -> Result<Glb, String> {
        if bytes.get(0..4) != Some(b"glTF") {
            return Err("no glTF magic".into());
        }
        if u32_at(bytes, 4)? != 2 {
            return Err("not glTF 2.0".into());
        }
        if u32_at(bytes, 8)? as usize != bytes.len() {
            return Err("the header's length is not the file's".into());
        }
        let json_len = u32_at(bytes, 12)? as usize;
        if bytes.get(16..20) != Some(b"JSON") || !json_len.is_multiple_of(4) {
            return Err("the first chunk is not an aligned JSON chunk".into());
        }
        let json: Value = serde_json::from_slice(bytes.get(20..20 + json_len).ok_or("JSON chunk past the end")?)
            .map_err(|e| format!("JSON: {e}"))?;
        let bin_at = 20 + json_len;
        let bin_len = u32_at(bytes, bin_at)? as usize;
        if bytes.get(bin_at + 4..bin_at + 8) != Some(b"BIN\0") || bin_at + 8 + bin_len != bytes.len() {
            return Err("the second chunk is not the binary chunk to the end".into());
        }
        let bin = bytes[bin_at + 8..].to_vec();
        check_references(&json, &bin)?;
        Ok(Glb { json, bin })
    }

    /// Every buffer view inside the binary chunk, every accessor inside its view, every index
    /// that names an element of its array.
    fn check_references(json: &Value, bin: &[u8]) -> Result<(), String> {
        let array = |key: &str| json.get(key).and_then(Value::as_array).cloned().unwrap_or_default();
        let views = array("bufferViews");
        for view in &views {
            let offset = view["byteOffset"].as_u64().unwrap_or(0) as usize;
            let length = view["byteLength"].as_u64().ok_or("a buffer view has no byteLength")? as usize;
            if offset + length > bin.len() {
                return Err(format!("a buffer view ends at {} past the binary chunk ({})", offset + length, bin.len()));
            }
        }
        let accessors = array("accessors");
        for accessor in &accessors {
            let view = accessor["bufferView"].as_u64().ok_or("an accessor has no bufferView")? as usize;
            let view = views.get(view).ok_or("an accessor names no buffer view")?;
            let count = accessor["count"].as_u64().unwrap_or(0) as usize;
            let width = match accessor["type"].as_str().unwrap_or("") {
                "SCALAR" => 1,
                "VEC2" => 2,
                "VEC3" => 3,
                "VEC4" => 4,
                other => return Err(format!("accessor type {other}")),
            };
            let size = if accessor["componentType"] == 5126 || accessor["componentType"] == 5125 { 4 } else { 2 };
            if count * width * size > view["byteLength"].as_u64().unwrap_or(0) as usize {
                return Err("an accessor is longer than its buffer view".into());
            }
        }
        let count = |key: &str| array(key).len();
        let nodes = array("nodes");
        for node in &nodes {
            if let Some(mesh) = node.get("mesh").and_then(Value::as_u64)
                && mesh as usize >= count("meshes")
            {
                return Err("a node names no mesh".into());
            }
            if let Some(camera) = node.get("camera").and_then(Value::as_u64)
                && camera as usize >= count("cameras")
            {
                return Err("a node names no camera".into());
            }
        }
        for mesh in array("meshes") {
            for primitive in mesh["primitives"].as_array().into_iter().flatten() {
                for index in primitive["attributes"].as_object().into_iter().flat_map(|a| a.values()) {
                    if index.as_u64().unwrap_or(u64::MAX) as usize >= accessors.len() {
                        return Err("an attribute names no accessor".into());
                    }
                }
            }
        }
        for animation in array("animations") {
            for channel in animation["channels"].as_array().into_iter().flatten() {
                let node = channel["target"]["node"].as_u64().unwrap_or(u64::MAX) as usize;
                if node >= nodes.len() {
                    return Err("an animation channel names no node".into());
                }
            }
        }
        Ok(())
    }
}
