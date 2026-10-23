//! Textured scenery for the simulated camera: boxes (a quad is a thin box) with procedural
//! textures, placed in WPILib's frame besides the tags. A feature tracker needs corners to
//! follow, and the tags are not always in view, so a scenario can add this (`--scenery`).
//!
//! `scenery.json`:
//!
//! ```json
//! {
//!   "textures": { "speckle": { "kind": "noise", "seed": 7, "cell_m": 0.02, "filter": "nearest" } },
//!   "boxes": [ { "name": "wall", "centre": [1.0, 3.2, 0.9], "size": [6.0, 0.1, 2.4],
//!                "yaw": 0, "texture": "speckle" } ]
//! }
//! ```
//!
//! - `centre` (metres) and `size` (metres, along the box's own X forward, Y left, Z up) in WPILib;
//!   `yaw` (degrees, about Z, default 0) turns the box about its centre.
//! - A `noise` texture is a 256x256 image of black and white cells (high contrast: a corner in
//!   every cell's corner), `cell_m` metres a cell (default 0.02), so the image repeats every
//!   32 cells. `seed` picks the pattern (default 0). `filter` is `nearest` (crisp cells, the
//!   default) or `linear` (blurred edges). Every box with the same texture name shares one image.
//! - Each face of a box is textured at the texel density of `cell_m`, so a big wall and a small
//!   pillar show the same cell size.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::geometry::{Mat3, Vec3, add, mul_v, rotation_from_rpy_degrees, to_gltf};
use crate::glb::{Builder, Filter, MeshAccessors, TARGET_ARRAY_BUFFER};
use crate::scene::encode_png;

/// The texture image's side (pixels) and a noise cell's side (pixels): a 32x32 grid.
const TEXTURE_PX: usize = 256;
const CELL_PX: usize = 8;
const GRID: usize = TEXTURE_PX / CELL_PX;
const DEFAULT_CELL_M: f64 = 0.02;
/// The two levels of a noise cell (near black and near white).
const DARK: u8 = 10;
const LIGHT: u8 = 245;

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum TextureKind {
    Noise,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TextureFile {
    kind: TextureKind,
    #[serde(default)]
    seed: u64,
    #[serde(default)]
    cell_m: Option<f64>,
    #[serde(default)]
    filter: Option<Filter>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BoxFile {
    name: Option<String>,
    centre: Vec3,
    size: Vec3,
    #[serde(default)]
    yaw: f64,
    texture: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SceneryFile {
    #[serde(default)]
    textures: BTreeMap<String, TextureFile>,
    #[serde(default)]
    boxes: Vec<BoxFile>,
}

/// A procedural texture, validated.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Texture {
    pub seed: u64,
    pub cell_m: f64,
    pub filter: Filter,
}

impl Texture {
    /// The metres one repeat of the image spans.
    pub fn repeat_m(&self) -> f64 {
        GRID as f64 * self.cell_m
    }
}

/// One box of scenery, validated.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneryBox {
    pub name: String,
    pub centre: Vec3,
    pub size: Vec3,
    pub yaw_deg: f64,
    pub texture: String,
}

/// The scenery of a scenario: named textures and the boxes that use them.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Scenery {
    pub textures: BTreeMap<String, Texture>,
    pub boxes: Vec<SceneryBox>,
}

/// One face of a box: four corners (WPILib, counter-clockwise seen from outside) and its normal.
#[derive(Debug, Clone, PartialEq)]
pub struct Quad {
    pub corners: [Vec3; 4],
    pub normal: Vec3,
    /// The corners' texture coordinates (the image's top-left is (0, 0)).
    pub uvs: [[f32; 2]; 4],
}

impl Scenery {
    /// Reads and checks a `scenery.json`'s text.
    pub fn parse(text: &str) -> Result<Self> {
        let file: SceneryFile = serde_json::from_str(text).context("the scenery file is not JSON of the expected shape")?;
        let mut textures = BTreeMap::new();
        for (name, texture) in file.textures {
            let cell_m = texture.cell_m.unwrap_or(DEFAULT_CELL_M);
            ensure!(cell_m.is_finite() && cell_m > 0.0, "texture {name}: cell_m must be a positive length");
            let TextureKind::Noise = texture.kind;
            textures.insert(name, Texture { seed: texture.seed, cell_m, filter: texture.filter.unwrap_or(Filter::Nearest) });
        }
        let mut boxes = Vec::with_capacity(file.boxes.len());
        for (index, b) in file.boxes.into_iter().enumerate() {
            let name = b.name.unwrap_or_else(|| format!("box{index}"));
            ensure!(textures.contains_key(&b.texture), "box {name}: no texture `{}`", b.texture);
            ensure!(b.size.iter().all(|v| v.is_finite() && *v > 0.0), "box {name}: each size must be a positive length");
            ensure!(b.centre.iter().all(|v| v.is_finite()), "box {name}: centre must be finite");
            ensure!(b.yaw.is_finite(), "box {name}: yaw must be finite");
            boxes.push(SceneryBox { name, centre: b.centre, size: b.size, yaw_deg: b.yaw, texture: b.texture });
        }
        Ok(Self { textures, boxes })
    }

    pub fn read(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("in {}", path.display()))
    }
}

/// A face of a box in the box's own axes: its outward `normal`, the in-plane axes `u` and `v`
/// (u x v = normal), and the half extents along each.
struct Face {
    normal: Vec3,
    u: Vec3,
    v: Vec3,
    half_normal: f64,
    half_u: f64,
    half_v: f64,
}

/// The six faces of a box with `size`, in the box's own axes.
fn box_faces(size: Vec3) -> [Face; 6] {
    let [sx, sy, sz] = size;
    let (x, y, z) = ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
    let neg = |v: Vec3| v.map(|c| -c);
    let face = |normal, u, v, half_normal, half_u, half_v| Face { normal, u, v, half_normal, half_u, half_v };
    [
        face(x, y, z, sx / 2.0, sy / 2.0, sz / 2.0),
        face(neg(x), z, y, sx / 2.0, sz / 2.0, sy / 2.0),
        face(y, z, x, sy / 2.0, sz / 2.0, sx / 2.0),
        face(neg(y), x, z, sy / 2.0, sx / 2.0, sz / 2.0),
        face(z, x, y, sz / 2.0, sx / 2.0, sy / 2.0),
        face(neg(z), y, x, sz / 2.0, sy / 2.0, sx / 2.0),
    ]
}

/// The six faces of a scenery box, in WPILib, with texture coordinates at the texel density of
/// `repeat_m` metres per image repeat.
pub fn box_quads(b: &SceneryBox, repeat_m: f64) -> Vec<Quad> {
    let rotation: Mat3 = rotation_from_rpy_degrees(0.0, 0.0, b.yaw_deg);
    box_faces(b.size)
        .into_iter()
        .map(|f| {
            let at = |su: f64, sv: f64| {
                let local = [0, 1, 2].map(|i| f.normal[i] * f.half_normal + f.u[i] * su * f.half_u + f.v[i] * sv * f.half_v);
                add(b.centre, mul_v(&rotation, local))
            };
            // Counter-clockwise seen from outside: (-,-), (+,-), (+,+), (-,+) in (u, v).
            let signs = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
            let corners = signs.map(|(su, sv)| at(su, sv));
            let uvs = signs.map(|(su, sv)| [((su + 1.0) * f.half_u / repeat_m) as f32, ((sv + 1.0) * f.half_v / repeat_m) as f32]);
            Quad { corners, normal: mul_v(&rotation, f.normal), uvs }
        })
        .collect()
}

fn splitmix64(value: u64) -> u64 {
    let mut z = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Whether a noise cell is light: a hash of the seed and the cell's index.
fn cell_is_light(seed: u64, index: usize) -> bool {
    splitmix64(seed ^ splitmix64(index as u64)) & 1 == 1
}

/// The noise texture's pixels (RGB, 256x256): each 8x8 cell light or dark, by the seed.
pub fn noise_rgb(seed: u64) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(TEXTURE_PX * TEXTURE_PX * 3);
    for py in 0..TEXTURE_PX {
        for px in 0..TEXTURE_PX {
            let cell = (py / CELL_PX) * GRID + px / CELL_PX;
            let value = if cell_is_light(seed, cell) { LIGHT } else { DARK };
            rgb.extend_from_slice(&[value; 3]);
        }
    }
    rgb
}

/// Adds the scenery's meshes to `glb` (one mesh per box, one material per texture used).
/// Returns the root nodes.
pub fn add_to(glb: &mut Builder, scenery: &Scenery) -> Result<Vec<usize>> {
    let mut materials: BTreeMap<&str, usize> = BTreeMap::new();
    let mut roots = Vec::with_capacity(scenery.boxes.len());
    for b in &scenery.boxes {
        let texture = scenery.textures.get(&b.texture).with_context(|| format!("box {}: no texture `{}`", b.name, b.texture))?;
        let material = match materials.get(b.texture.as_str()) {
            Some(material) => *material,
            None => {
                let png = encode_png(TEXTURE_PX as u32, TEXTURE_PX as u32, &noise_rgb(texture.seed))?;
                let image = glb.texture_png(&png, texture.filter);
                let material = glb.material(image);
                materials.insert(b.texture.as_str(), material);
                material
            }
        };
        let quads = box_quads(b, texture.repeat_m());
        let mut positions = Vec::with_capacity(quads.len() * 12);
        let mut normals = Vec::with_capacity(quads.len() * 12);
        let mut uvs = Vec::with_capacity(quads.len() * 8);
        let mut indices: Vec<u16> = Vec::with_capacity(quads.len() * 6);
        for (face, quad) in quads.iter().enumerate() {
            let base = (face * 4) as u16;
            for (corner, uv) in quad.corners.iter().zip(quad.uvs) {
                positions.extend(to_gltf(*corner).map(|v| v as f32));
                normals.extend(to_gltf(quad.normal).map(|v| v as f32));
                uvs.extend(uv);
            }
            indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        let accessors = MeshAccessors {
            positions: glb.floats(&positions, 3, Some(TARGET_ARRAY_BUFFER)),
            normals: glb.floats(&normals, 3, Some(TARGET_ARRAY_BUFFER)),
            uvs: glb.floats(&uvs, 2, Some(TARGET_ARRAY_BUFFER)),
            indices: glb.indices(&indices),
        };
        let mesh = glb.mesh(&format!("scenery:{}", b.name), &accessors, material);
        roots.push(glb.node(&format!("scenery:{}", b.name), [0.0; 3], [0.0, 0.0, 0.0, 1.0], Some(mesh), None));
    }
    Ok(roots)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glb::reader;

    const SCENERY: &str = r#"{
        "textures": {
            "speckle": { "kind": "noise", "seed": 7, "cell_m": 0.02 },
            "fine": { "kind": "noise", "seed": 9, "cell_m": 0.01, "filter": "linear" }
        },
        "boxes": [
            { "name": "wall", "centre": [1.0, 3.2, 0.9], "size": [6.0, 0.1, 2.4], "texture": "speckle" },
            { "centre": [2.5, 2.0, 0.5], "size": [0.25, 0.25, 1.0], "yaw": 90, "texture": "fine" }
        ]
    }"#;

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn sub(a: Vec3, b: Vec3) -> Vec3 {
        [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
    }

    fn cross(a: Vec3, b: Vec3) -> Vec3 {
        [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
    }

    fn area(quad: &Quad) -> f64 {
        // Two triangles of the quad: the cross product of its diagonals' halves.
        let c = quad.corners;
        let n = cross(sub(c[1], c[0]), sub(c[2], c[0]));
        let m = cross(sub(c[2], c[0]), sub(c[3], c[0]));
        0.5 * (n.iter().map(|v| v * v).sum::<f64>().sqrt() + m.iter().map(|v| v * v).sum::<f64>().sqrt())
    }

    #[test]
    fn parses_with_defaults_and_names_the_unnamed_boxes() {
        let scenery = Scenery::parse(SCENERY).expect("valid scenery");
        assert_eq!(scenery.textures["speckle"], Texture { seed: 7, cell_m: 0.02, filter: Filter::Nearest });
        assert_eq!(scenery.textures["fine"].filter, Filter::Linear);
        assert_eq!(scenery.boxes.len(), 2);
        assert_eq!(scenery.boxes[1].name, "box1");
        assert_eq!(scenery.boxes[1].yaw_deg, 90.0);
        assert_eq!(scenery.boxes[0].yaw_deg, 0.0);
    }

    #[test]
    fn refuses_what_it_does_not_understand() {
        let cases = [
            (r#"{"boxes": [], "texure": {}}"#, "unknown field"),
            (r#"{"textures": {"t": {"kind": "marble"}}}"#, "unknown kind"),
            (r#"{"textures": {"t": {"kind": "noise", "filter": "cubic"}}}"#, "unknown filter"),
            (r#"{"textures": {"t": {"kind": "noise", "cell_m": 0}}}"#, "zero cell"),
            (r#"{"textures": {"t": {"kind": "noise"}}, "boxes": [{"centre": [0,0,0], "size": [1,0,1], "texture": "t"}]}"#, "zero size"),
            (r#"{"textures": {"t": {"kind": "noise"}}, "boxes": [{"centre": [0,0,0], "size": [1,1,1], "texture": "u"}]}"#, "no such texture"),
        ];
        for (text, what) in cases {
            assert!(Scenery::parse(text).is_err(), "{what} must be refused");
        }
    }

    #[test]
    fn a_box_has_six_outward_faces_with_its_surface_area() {
        let b = SceneryBox { name: "b".into(), centre: [1.0, 2.0, 3.0], size: [2.0, 1.0, 0.5], yaw_deg: 0.0, texture: "t".into() };
        let quads = box_quads(&b, 0.5);
        assert_eq!(quads.len(), 6);
        let total: f64 = quads.iter().map(area).sum();
        assert!(approx(total, 2.0 * (2.0 * 1.0 + 1.0 * 0.5 + 2.0 * 0.5)), "surface {total}");
        for quad in &quads {
            let centre = [0, 1, 2].map(|i| quad.corners.iter().map(|c| c[i]).sum::<f64>() / 4.0);
            // Outward: the face's centre lies along its normal from the box's centre.
            assert!(sub(centre, b.centre).iter().zip(quad.normal).map(|(d, n)| d * n).sum::<f64>() > 0.0);
            // The corners wind counter-clockwise from the normal's side: (c1-c0) x (c2-c1) is along n.
            let winding = cross(sub(quad.corners[1], quad.corners[0]), sub(quad.corners[2], quad.corners[1]));
            assert!(winding.iter().zip(quad.normal).map(|(w, n)| w * n).sum::<f64>() > 0.0, "winding of {:?}", quad.corners);
        }
    }

    #[test]
    fn a_yawed_box_turns_its_footprint_and_keeps_its_texel_density() {
        // 2 m along X, 1 m along Y, turned 90 degrees: its footprint is 1 m along X and 2 m along Y.
        let b = SceneryBox { name: "b".into(), centre: [0.0, 0.0, 0.0], size: [2.0, 1.0, 1.0], yaw_deg: 90.0, texture: "t".into() };
        let corners: Vec<Vec3> = box_quads(&b, 0.5).into_iter().flat_map(|q| q.corners).collect();
        let span = |axis: usize| {
            let lo = corners.iter().map(|c| c[axis]).fold(f64::INFINITY, f64::min);
            let hi = corners.iter().map(|c| c[axis]).fold(f64::NEG_INFINITY, f64::max);
            hi - lo
        };
        assert!(approx(span(0), 1.0) && approx(span(1), 2.0), "spans {} {}", span(0), span(1));
        // The top face (+Z) spans 2 m by 1 m in its own axes, whatever the yaw: 4 repeats of 0.5 m
        // along u and 2 along v.
        let top = &box_quads(&b, 0.5)[4];
        let max_u = top.uvs.iter().map(|uv| uv[0]).fold(0.0_f32, f32::max);
        let max_v = top.uvs.iter().map(|uv| uv[1]).fold(0.0_f32, f32::max);
        assert!((max_u - 4.0).abs() < 1e-6 && (max_v - 2.0).abs() < 1e-6, "uv span {max_u} x {max_v}");
    }

    #[test]
    fn the_noise_is_seeded_high_contrast_and_cells_are_square() {
        let a = noise_rgb(7);
        assert_eq!(a, noise_rgb(7), "the same seed gives the same image");
        assert_ne!(a, noise_rgb(8), "another seed gives another image");
        assert!(a.chunks(3).all(|p| p[0] == p[1] && p[1] == p[2] && (p[0] == DARK || p[0] == LIGHT)), "black or white only");
        let light = a.chunks(3).filter(|p| p[0] == LIGHT).count();
        let fraction = light as f64 / (TEXTURE_PX * TEXTURE_PX) as f64;
        assert!((0.35..=0.65).contains(&fraction), "about half the cells are light: {fraction}");
        // Every cell is one colour: the pixel at a cell's corner and its far corner agree.
        let at = |x: usize, y: usize| a[(y * TEXTURE_PX + x) * 3];
        for cell_y in 0..GRID {
            for cell_x in 0..GRID {
                let (x0, y0) = (cell_x * CELL_PX, cell_y * CELL_PX);
                let value = at(x0, y0);
                assert_eq!(at(x0 + CELL_PX - 1, y0 + CELL_PX - 1), value, "cell ({cell_x}, {cell_y}) is one colour");
            }
        }
    }

    #[test]
    fn the_scenery_adds_one_image_per_texture_and_one_mesh_per_box() {
        let scenery = Scenery::parse(SCENERY).expect("scenery");
        let mut glb = Builder::default();
        let roots = add_to(&mut glb, &scenery).expect("meshes");
        let bytes = glb.finish(&roots);
        let parsed = reader::parse(&bytes).expect("a valid glb");
        let images = parsed.json["images"].as_array().expect("images");
        assert_eq!(images.len(), 2, "one image per texture used");
        let samplers = parsed.json["samplers"].as_array().expect("samplers");
        assert!(samplers.iter().any(|s| s["magFilter"] == 9728), "a nearest sampler");
        assert!(samplers.iter().any(|s| s["magFilter"] == 9729), "a linear sampler");
        let names: Vec<&str> = parsed.json["nodes"].as_array().unwrap().iter().filter_map(|n| n["name"].as_str()).collect();
        assert!(names.contains(&"scenery:wall") && names.contains(&"scenery:box1"), "{names:?}");
        let wall_mesh = parsed.json["meshes"].as_array().unwrap().iter().find(|m| m["name"] == "scenery:wall").expect("the wall's mesh");
        let positions = wall_mesh["primitives"][0]["attributes"]["POSITION"].as_u64().unwrap() as usize;
        assert_eq!(parsed.json["accessors"][positions]["count"], 24, "six faces of four corners");
    }
}
