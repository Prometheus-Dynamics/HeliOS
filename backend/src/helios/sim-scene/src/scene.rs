//! The scene Styx renders with `scene=gltf`: the field's AprilTags (36h11, each a quad facing its
//! tag's `+X`), a floor (z = 0, a 1 m checker), the camera node (static, or looped along a
//! trajectory), and the ground truth points `truth:tag<ID>/<corner>` at each black square's
//! corners (top-left, top-right, bottom-right, bottom-left as printed, seen from the front).

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use eidos_aruco::{ArucoDictionaryKind, BitGrid, dictionary};
use helios_field::FieldLayout;
use serde::Deserialize;

use crate::geometry::{
    Mat3, Vec3, add, camera_node_rotation, matrix_to_quat, mul_v, quat_to_matrix, rotation_from_rpy_degrees, to_gltf,
};
use crate::glb::{Builder, Filter, MeshAccessors, TARGET_ARRAY_BUFFER};
use crate::scenery::{self, Scenery};

/// A tag's image: 10 cells across (a 1-cell white quiet zone, the 8-cell black square with its
/// 1-cell black border, the 6x6 data inside), 16 pixels a cell.
const CELLS: usize = 10;
const BLACK_CELLS: usize = 8;
const PIXELS_PER_CELL: usize = 16;
/// The floor's checker: one texel is one metre, so the texture is 2x2 and its UVs run 0.5 per metre.
const FLOOR_HALF_M: f64 = 30.0;
const IDENTITY_ROTATION: [f64; 4] = [0.0, 0.0, 0.0, 1.0];
/// The tag's corners in the tag frame as (Y, Z) signs, top-left, top-right, bottom-right and
/// bottom-left as printed (seen from the front: +Y is to the right, +Z up).
const CORNER_SIGNS: [(f64, f64); 4] = [(-1.0, 1.0), (1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)];
pub const CORNER_NAMES: [&str; 4] = ["tl", "tr", "br", "bl"];
/// The quad's UVs for the corners above (the image's top-left is UV (0, 0)).
const CORNER_UVS: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];

/// The lens and the image size, as HeliOS's calibration JSON (`tools/sim/ov9782-calibration.json`).
#[derive(Debug, Deserialize)]
pub struct Calibration {
    pub width: u32,
    pub height: u32,
    pub model: String,
    pub fx: f64,
    pub fy: f64,
    pub cx: f64,
    pub cy: f64,
    #[serde(default)]
    pub distortion: Distortion,
}

#[derive(Debug, Default, Deserialize)]
pub struct Distortion {
    #[serde(default)]
    pub k1: f64,
    #[serde(default)]
    pub k2: f64,
    #[serde(default)]
    pub k3: f64,
    #[serde(default)]
    pub k4: f64,
    #[serde(default)]
    pub k5: f64,
    #[serde(default)]
    pub k6: f64,
    #[serde(default)]
    pub p1: f64,
    #[serde(default)]
    pub p2: f64,
}

impl Calibration {
    /// Styx's lens keys for this calibration (`lens=...,fx=...`), for its `scene=gltf` spec.
    pub fn lens_spec(&self) -> Result<String> {
        let d = &self.distortion;
        let common = format!(
            "lens_width={},lens_height={},fx={},fy={},cx={},cy={}",
            self.width, self.height, self.fx, self.fy, self.cx, self.cy
        );
        match self.model.as_str() {
            "fisheye" => {
                ensure!(d.k5 == 0.0 && d.k6 == 0.0 && d.p1 == 0.0 && d.p2 == 0.0, "a fisheye lens has k1..k4 only");
                Ok(format!("lens=fisheye,{common},k1={},k2={},k3={},k4={}", d.k1, d.k2, d.k3, d.k4))
            }
            "pinhole" => Ok(format!(
                "lens=pinhole,{common},k1={},k2={},k3={},k4={},k5={},k6={},p1={},p2={}",
                d.k1, d.k2, d.k3, d.k4, d.k5, d.k6, d.p1, d.p2
            )),
            other => bail!("calibration model `{other}` is neither fisheye nor pinhole"),
        }
    }

    /// The vertical field of view of the whole pixel array (the glTF camera's `yfov`, for the
    /// viewers that read it; Styx uses the lens keys).
    fn yfov_rad(&self) -> f64 {
        2.0 * (f64::from(self.height) / (2.0 * self.fy)).atan()
    }
}

/// One camera pose: WPILib position (m) and rotation (degrees, roll, pitch, yaw).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Key {
    pub t: f64,
    pub position: Vec3,
    pub rpy_deg: [f64; 3],
}

/// The camera: still, or along a trajectory (looped, with the first key's time as 0).
#[derive(Debug, Clone, PartialEq)]
pub enum CameraPlan {
    Still(Key),
    Path(Vec<Key>),
}

#[derive(Deserialize)]
struct PoseFile {
    x: f64,
    y: f64,
    z: f64,
    #[serde(default)]
    roll: f64,
    #[serde(default)]
    pitch: f64,
    #[serde(default)]
    yaw: f64,
}

#[derive(Deserialize)]
struct KeyFile {
    t: f64,
    #[serde(flatten)]
    pose: PoseFile,
}

#[derive(Deserialize)]
struct CameraFile {
    pose: Option<PoseFile>,
    trajectory: Option<Vec<KeyFile>>,
}

impl CameraPlan {
    /// Reads `{"pose": {...}}` or `{"trajectory": [{"t": ..., ...}, ...]}` (times in seconds,
    /// increasing; the camera loops over them).
    pub fn parse(text: &str) -> Result<Self> {
        let file: CameraFile = serde_json::from_str(text).context("the camera file is not JSON of the expected shape")?;
        let key = |t: f64, p: &PoseFile| Key { t, position: [p.x, p.y, p.z], rpy_deg: [p.roll, p.pitch, p.yaw] };
        match (file.pose, file.trajectory) {
            (Some(pose), None) => Ok(Self::Still(key(0.0, &pose))),
            (None, Some(keys)) => {
                ensure!(!keys.is_empty(), "the trajectory has no keys");
                let start = keys[0].t;
                let keys: Vec<Key> = keys.iter().map(|k| key(k.t - start, &k.pose)).collect();
                ensure!(keys.windows(2).all(|w| w[1].t > w[0].t), "the trajectory's times must increase");
                if keys.len() == 1 { Ok(Self::Still(keys[0])) } else { Ok(Self::Path(keys)) }
            }
            _ => bail!("the camera file needs exactly one of `pose` and `trajectory`"),
        }
    }

    pub fn read(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("in {}", path.display()))
    }
}

/// The WPILib corner of tag `tag` at `half` from its centre, as the tag frame's (Y, Z).
fn tag_corner_wpilib(tag: &helios_field::FieldTag, half: f64, signs: (f64, f64)) -> Vec3 {
    let p = tag.field_from_tag.translation;
    let q = tag.field_from_tag.rotation;
    let rotation = quat_to_matrix(q.w, q.x, q.y, q.z);
    add([p.x, p.y, p.z], mul_v(&rotation, [0.0, signs.0 * half, signs.1 * half]))
}

/// The quad's half side (the 10-cell image) and the black square's (8 cells) for a tag of side `s`.
fn quad_half(side: f64) -> f64 {
    side * CELLS as f64 / (2.0 * BLACK_CELLS as f64)
}

/// The tag's image: the black square (8 cells, its border included) with the data bits as the
/// marker has them (`bit(column, row)`, a set bit white), in a white quiet zone of one cell.
fn tag_cell_is_white(grid: &BitGrid, column: usize, row: usize) -> bool {
    if column == 0 || column == CELLS - 1 || row == 0 || row == CELLS - 1 {
        true
    } else if column == 1 || column == CELLS - 2 || row == 1 || row == CELLS - 2 {
        false
    } else {
        grid.bit(column - 2, row - 2)
    }
}

pub(crate) fn encode_png(width: u32, height: u32, rgb: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(rgb)?;
    writer.finish()?;
    Ok(out)
}

fn tag_png(grid: &BitGrid) -> Result<Vec<u8>> {
    ensure!(grid.size() as usize == CELLS - 4, "a 36h11 marker is 6x6 cells");
    let size = CELLS * PIXELS_PER_CELL;
    let mut rgb = Vec::with_capacity(size * size * 3);
    for py in 0..size {
        for px in 0..size {
            let white = tag_cell_is_white(grid, px / PIXELS_PER_CELL, py / PIXELS_PER_CELL);
            let value = if white { 255 } else { 0 };
            rgb.extend_from_slice(&[value; 3]);
        }
    }
    encode_png(size as u32, size as u32, &rgb)
}

fn floor_png() -> Result<Vec<u8>> {
    // Two light and two dark texels on the diagonal (a 1 m checker with NEAREST filtering).
    let light = [200_u8; 3];
    let dark = [90_u8; 3];
    let rgb: Vec<u8> = [light, dark, dark, light].concat();
    encode_png(2, 2, &rgb)
}

/// Builds the `.glb` for a field layout, a camera plan, the camera's lens and the scenery (textured
/// boxes besides the tags; `Scenery::default()` for none).
pub fn build(layout: &FieldLayout, camera: &CameraPlan, calibration: &Calibration, scenery: &Scenery) -> Result<Vec<u8>> {
    ensure!(calibration.width > 0 && calibration.height > 0 && calibration.fy > 0.0, "the calibration has no size or focal length");
    let mut glb = Builder::default();
    let mut roots = Vec::new();
    let dictionary = dictionary(ArucoDictionaryKind::AprilTag36h11);
    let mut ids = BTreeSet::new();
    let mut truth = Vec::new();

    for tag in &layout.tags {
        ensure!(ids.insert(tag.id), "tag {} is in the layout twice", tag.id);
        ensure!(tag.side_m.is_finite() && tag.side_m > 0.0, "tag {} has no size", tag.id);
        let grid = BitGrid::from_marker(dictionary, tag.id, 0)
            .with_context(|| format!("tag {} is not in AprilTag 36h11", tag.id))?;
        let texture = glb.texture_png(&tag_png(&grid)?, Filter::Nearest);
        let material = glb.material(texture);

        let half = quad_half(tag.side_m);
        let normal = to_gltf(mul_v(&quad_rotation(tag), [1.0, 0.0, 0.0]));
        let mut positions = Vec::with_capacity(12);
        let mut normals = Vec::with_capacity(12);
        let mut uvs = Vec::with_capacity(8);
        for (signs, uv) in CORNER_SIGNS.iter().zip(CORNER_UVS) {
            let p = to_gltf(tag_corner_wpilib(tag, half, *signs));
            positions.extend(p.map(|v| v as f32));
            normals.extend(normal.map(|v| v as f32));
            uvs.extend(uv);
        }
        // Counter-clockwise seen from the front (bottom-left, bottom-right, top-right, top-left).
        let indices = [3_u16, 2, 1, 3, 1, 0];
        let accessors = MeshAccessors {
            positions: glb.floats(&positions, 3, Some(TARGET_ARRAY_BUFFER)),
            normals: glb.floats(&normals, 3, Some(TARGET_ARRAY_BUFFER)),
            uvs: glb.floats(&uvs, 2, Some(TARGET_ARRAY_BUFFER)),
            indices: glb.indices(&indices),
        };
        let mesh = glb.mesh(&format!("tag{}", tag.id), &accessors, material);
        roots.push(glb.node(&format!("tag{}", tag.id), [0.0; 3], IDENTITY_ROTATION, Some(mesh), None));

        for (name, signs) in CORNER_NAMES.iter().zip(CORNER_SIGNS) {
            let p = tag_corner_wpilib(tag, tag.side_m / 2.0, signs);
            truth.push((format!("truth:tag{}/{name}", tag.id), to_gltf(p)));
        }
    }

    // The floor: a square at z = 0, its UVs one texel per metre of the 2x2 checker.
    let floor = [(-FLOOR_HALF_M, -FLOOR_HALF_M), (FLOOR_HALF_M, -FLOOR_HALF_M), (FLOOR_HALF_M, FLOOR_HALF_M), (-FLOOR_HALF_M, FLOOR_HALF_M)];
    let floor_positions: Vec<f32> = floor.iter().flat_map(|&(x, y)| to_gltf([x, y, 0.0]).map(|v| v as f32)).collect();
    let floor_normals: Vec<f32> = [0.0_f32, 1.0, 0.0].repeat(4);
    let floor_uvs: Vec<f32> = floor.iter().flat_map(|&(x, y)| [(x * 0.5) as f32, (y * 0.5) as f32]).collect();
    let floor_texture = glb.texture_png(&floor_png()?, Filter::Nearest);
    let floor_material = glb.material(floor_texture);
    let floor_accessors = MeshAccessors {
        positions: glb.floats(&floor_positions, 3, Some(TARGET_ARRAY_BUFFER)),
        normals: glb.floats(&floor_normals, 3, Some(TARGET_ARRAY_BUFFER)),
        uvs: glb.floats(&floor_uvs, 2, Some(TARGET_ARRAY_BUFFER)),
        indices: glb.indices(&[0, 1, 2, 0, 2, 3]),
    };
    let floor_mesh = glb.mesh("floor", &floor_accessors, floor_material);
    roots.push(glb.node("floor", [0.0; 3], IDENTITY_ROTATION, Some(floor_mesh), None));

    // The scenery: textured boxes for a feature tracker to follow where no tag is in view.
    roots.extend(scenery::add_to(&mut glb, scenery)?);

    // The camera: its own perspective (the lens keys are what Styx renders with).
    let camera_index = glb.camera(calibration.yfov_rad(), f64::from(calibration.width) / f64::from(calibration.height));
    match camera {
        CameraPlan::Still(key) => {
            let rotation = matrix_to_quat(&camera_node_rotation(&rotation_from_rpy_degrees(key.rpy_deg[0], key.rpy_deg[1], key.rpy_deg[2])));
            roots.push(glb.node("camera", to_gltf(key.position), rotation, None, Some(camera_index)));
        }
        CameraPlan::Path(keys) => {
            let mut rotations: Vec<[f64; 4]> = Vec::with_capacity(keys.len());
            for key in keys {
                let mut q = matrix_to_quat(&camera_node_rotation(&rotation_from_rpy_degrees(key.rpy_deg[0], key.rpy_deg[1], key.rpy_deg[2])));
                // The shortest way between neighbours: a quaternion's sign is its rotation's.
                if let Some(previous) = rotations.last()
                    && dot4(previous, &q) < 0.0
                {
                    q = q.map(|v| -v);
                }
                rotations.push(q);
            }
            let first = keys[0];
            let node = glb.node("camera", to_gltf(first.position), rotations[0], None, Some(camera_index));
            let times: Vec<f32> = keys.iter().map(|k| k.t as f32).collect();
            let translations: Vec<[f32; 3]> = keys.iter().map(|k| to_gltf(k.position).map(|v| v as f32)).collect();
            let rotations: Vec<[f32; 4]> = rotations.iter().map(|q| q.map(|v| v as f32)).collect();
            glb.animate(node, &times, &translations, &rotations);
            roots.push(node);
        }
    }

    for (name, point) in truth {
        roots.push(glb.node(&name, point, IDENTITY_ROTATION, None, None));
    }
    Ok(glb.finish(&roots))
}

/// The tag's rotation (tag frame to the field, WPILib).
fn quad_rotation(tag: &helios_field::FieldTag) -> Mat3 {
    let q = tag.field_from_tag.rotation;
    quat_to_matrix(q.w, q.x, q.y, q.z)
}

fn dot4(a: &[f64; 4], b: &[f64; 4]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glb::reader;
    use crate::geometry::quat_to_matrix;
    use helios_field::FRC_TAG_SIDE_M;

    fn sub(a: Vec3, b: Vec3) -> Vec3 {
        [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
    }

    fn transpose(a: &Mat3) -> Mat3 {
        [0, 1, 2].map(|r| [0, 1, 2].map(|c| a[c][r]))
    }

    /// Tag 1 straight ahead (3 m, facing the camera at the origin 0.5 m up), FRC size.
    const LAYOUT: &str = r#"{
        "tags": [ { "ID": 1, "pose": { "translation": { "x": 3.0, "y": 0.0, "z": 0.5 },
                    "rotation": { "quaternion": { "W": 0.0, "X": 0.0, "Y": 0.0, "Z": 1.0 } } } } ],
        "field": { "length": 6.0, "width": 4.0 }
    }"#;

    /// A pinhole at 1280x800 with f = 500 px, the principal point at the centre.
    fn pinhole() -> Calibration {
        Calibration {
            width: 1280,
            height: 800,
            model: "pinhole".into(),
            fx: 500.0,
            fy: 500.0,
            cx: 640.0,
            cy: 400.0,
            distortion: Distortion::default(),
        }
    }

    fn still_at_origin() -> CameraPlan {
        CameraPlan::parse(r#"{"pose": {"x": 0.0, "y": 0.0, "z": 0.5, "roll": 0.0, "pitch": 0.0, "yaw": 0.0}}"#).expect("camera")
    }

    /// The camera node and the truth points of a built scene: (translation, rotation, points).
    fn scene_of(camera: &CameraPlan) -> (Vec<u8>, reader::Glb) {
        let (layout, _) = FieldLayout::parse(LAYOUT, FRC_TAG_SIDE_M).expect("layout");
        let bytes = build(&layout, camera, &pinhole(), &Scenery::default()).expect("scene");
        let glb = reader::parse(&bytes).expect("a valid glb");
        (bytes, glb)
    }

    fn node_named<'a>(glb: &'a reader::Glb, name: &str) -> &'a serde_json::Value {
        glb.json["nodes"].as_array().expect("nodes").iter().find(|n| n["name"] == name).unwrap_or_else(|| panic!("no node {name}"))
    }

    fn vec_of(value: &serde_json::Value) -> Vec3 {
        let a = value.as_array().expect("a vector");
        [a[0].as_f64().unwrap(), a[1].as_f64().unwrap(), a[2].as_f64().unwrap()]
    }

    /// A glTF camera's pixel of a world point, from its node's translation and rotation (the
    /// glTF camera looks along -Z, +Y up): OpenCV's pixel centres, x right, y down.
    fn project_gltf(camera: &serde_json::Value, p: Vec3, cal: &Calibration) -> (f64, f64) {
        let t = vec_of(&camera["translation"]);
        let r = camera["rotation"].as_array().expect("rotation");
        let q: Vec<f64> = r.iter().map(|v| v.as_f64().unwrap()).collect();
        let rotation = quat_to_matrix(q[3], q[0], q[1], q[2]);
        let local = mul_v(&transpose(&rotation), sub(p, t));
        let depth = -local[2];
        assert!(depth > 0.0, "the point is in front of the camera");
        (cal.cx + cal.fx * local[0] / depth, cal.cy - cal.fy * local[1] / depth)
    }

    #[test]
    fn the_truth_corners_project_as_printed_top_left_top_right_bottom_right_bottom_left() {
        let cal = pinhole();
        let (_, glb) = scene_of(&still_at_origin());
        let camera = node_named(&glb, "camera").clone();
        let pixel = |corner: &str| {
            let node = node_named(&glb, &format!("truth:tag1/{corner}"));
            project_gltf(&camera, vec_of(&node["translation"]), &cal)
        };
        let (tl, tr, br, bl) = (pixel("tl"), pixel("tr"), pixel("br"), pixel("bl"));
        // Left corners are left of the right ones, top corners above the bottom ones.
        assert!(tl.0 < tr.0 && bl.0 < br.0, "left before right: {tl:?} {tr:?} {br:?} {bl:?}");
        assert!(tl.1 < bl.1 && tr.1 < br.1, "top before bottom: {tl:?} {tr:?} {br:?} {bl:?}");
        // A square facing the camera: its sides are as long across as they are down.
        let width = tr.0 - tl.0;
        let height = bl.1 - tl.1;
        assert!((width - height).abs() < 0.5, "a square, {width} x {height} px");
    }

    #[test]
    fn the_truth_square_is_the_tag_and_sits_where_the_layout_puts_it() {
        let (_, glb) = scene_of(&still_at_origin());
        // The tag's quad is the 10-cell image: its corners are 1.25 tag sides apart, and the
        // truth corners are the black square's, half a side from the centre.
        let tl = vec_of(&node_named(&glb, "truth:tag1/tl")["translation"]);
        let br = vec_of(&node_named(&glb, "truth:tag1/br")["translation"]);
        let side = (sub(tl, br)[0].powi(2) + sub(tl, br)[1].powi(2) + sub(tl, br)[2].powi(2)).sqrt();
        let diagonal = FRC_TAG_SIDE_M * std::f64::consts::SQRT_2;
        assert!((side - diagonal).abs() < 1e-6, "the black square's diagonal is a side's sqrt 2: {side}");
        // The tag's centre is the square's middle: 3 m ahead (glTF x), 0.5 m up (glTF y).
        let centre = [0, 1, 2].map(|i| (tl[i] + br[i]) / 2.0);
        assert!((centre[0] - 3.0).abs() < 1e-9 && (centre[1] - 0.5).abs() < 1e-9 && centre[2].abs() < 1e-9, "centre {centre:?}");
    }

    #[test]
    fn the_glb_parses_back_with_its_tag_image_and_floor() {
        let (bytes, glb) = scene_of(&still_at_origin());
        assert!(bytes.len() > 100, "a scene with images");
        let images = glb.json["images"].as_array().expect("images");
        assert_eq!(images.len(), 2, "the tag and the floor");
        // The tag's image is the marker: the decoded PNG's data cell (0, 0) is white iff bit(0, 0).
        let view = &glb.json["bufferViews"][images[0]["bufferView"].as_u64().unwrap() as usize];
        let offset = view["byteOffset"].as_u64().unwrap() as usize;
        let length = view["byteLength"].as_u64().unwrap() as usize;
        let mut decoder = png::Decoder::new(std::io::Cursor::new(&glb.bin[offset..offset + length])).read_info().expect("png");
        let mut buffer = vec![0; decoder.output_buffer_size().expect("size")];
        let info = decoder.next_frame(&mut buffer).expect("decode");
        let size = CELLS * PIXELS_PER_CELL;
        assert_eq!((info.width as usize, info.height as usize), (size, size));
        let grid = BitGrid::from_marker(dictionary(ArucoDictionaryKind::AprilTag36h11), 1, 0).expect("marker 1");
        let at = |column: usize, row: usize| buffer[(row * size + column) * 3];
        let cell = |c: usize, r: usize| at(c * PIXELS_PER_CELL + 8, r * PIXELS_PER_CELL + 8) == 255;
        assert_eq!(cell(2, 2), grid.bit(0, 0), "data cell (0, 0)");
        assert_eq!(cell(7, 7), grid.bit(5, 5), "data cell (5, 5)");
        assert!(cell(0, 0), "the quiet zone is white");
        assert!(!cell(1, 1), "the black border is black");
    }

    #[test]
    fn a_trajectory_is_a_looped_animation_of_the_camera_node() {
        let camera = CameraPlan::parse(
            r#"{"trajectory": [
                {"t": 5, "x": 3.5, "y": 0.0, "z": 0.5, "roll": 0, "pitch": 0, "yaw": 0},
                {"t": 6, "x": 3.5, "y": 0.0, "z": 0.5, "roll": 0, "pitch": 0, "yaw": 110},
                {"t": 37, "x": 3.5, "y": 0.0, "z": 0.5, "roll": 0, "pitch": 0, "yaw": 0}]}"#,
        )
        .expect("camera");
        let (_, glb) = scene_of(&camera);
        let animation = &glb.json["animations"][0];
        assert_eq!(animation["channels"].as_array().unwrap().len(), 2);
        let input = animation["samplers"][0]["input"].as_u64().unwrap() as usize;
        // The times start at 0 and end at the trajectory's length (32 s).
        assert_eq!(glb.json["accessors"][input]["min"][0], 0.0);
        assert_eq!(glb.json["accessors"][input]["max"][0], 32.0);
        // The first key sets the camera node's pose: 3.5 m ahead (glTF x), 0.5 m up (glTF y).
        let camera_node = node_named(&glb, "camera");
        assert_eq!(vec_of(&camera_node["translation"]), [3.5, 0.5, 0.0]);
    }

    #[test]
    fn lens_keys_follow_the_model_and_refuse_keys_it_does_not_have() {
        let fisheye = Calibration {
            model: "fisheye".into(),
            distortion: Distortion { k1: -0.01, k2: 0.02, k3: -0.003, k4: 0.0005, ..Distortion::default() },
            ..pinhole()
        };
        let keys = fisheye.lens_spec().expect("fisheye");
        assert!(keys.starts_with("lens=fisheye,lens_width=1280,lens_height=800,fx=500,fy=500,cx=640,cy=400,k1=-0.01,"), "{keys}");
        assert!(!keys.contains("p1="), "a fisheye has no tangential terms: {keys}");
        let bad = Calibration { distortion: Distortion { p1: 0.001, ..Distortion::default() }, ..fisheye };
        assert!(bad.lens_spec().is_err());
        let pinhole_keys = pinhole().lens_spec().expect("pinhole");
        assert!(pinhole_keys.starts_with("lens=pinhole,") && pinhole_keys.contains(",p2=0"), "{pinhole_keys}");
    }

    #[test]
    fn a_tag_outside_the_marker_dictionary_is_refused() {
        let layout_text = LAYOUT.replace("\"ID\": 1", "\"ID\": 9999");
        let (layout, _) = FieldLayout::parse(&layout_text, FRC_TAG_SIDE_M).expect("layout");
        let error = build(&layout, &still_at_origin(), &pinhole(), &Scenery::default()).expect_err("not in 36h11");
        assert!(error.to_string().contains("tag 9999"), "{error}");
    }
}
