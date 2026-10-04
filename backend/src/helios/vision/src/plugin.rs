//! Daedalus nodes for the vision stages.
//!
//! The graph carries a camera frame (`styx:framelease`) to the stages that
//! need full-resolution pixels; they read its luma plane in place
//! ([`luma_view`]). `vision.downscale` uses the ISP's half-size pyramid
//! companion when the frame has one. Stage outputs are structured types with
//! stable keys so they can be inspected and stored.

use std::sync::Arc;

use daedalus::{
    DaedalusToValue, DaedalusTypeExpr, adapt,
    data::{model::Value, to_value::ToValue},
    macros::{NodeConfig, node, plugin},
    runtime::{NodeError, plugins::PluginRegistry},
    transport::TransportError,
    type_key,
};
use styx::imports::framelease::FrameLease;

use crate::aruco::{self, DecodeConfig, Dictionary};
use crate::geometry::Point;
use crate::image::{BinaryImage, GrayImage, GrayView};
use crate::quads::{self, Quad};
use crate::threshold::{self, ThresholdConfig};

/// Key of the Styx frame carrier, shared with the engine's frame glue.
pub const FRAMELEASE_TYPE_KEY: &str = "styx:framelease";
pub const GRAY_TYPE_KEY: &str = "helios:gray8";
pub const BINARY_TYPE_KEY: &str = "helios:binary";

/// A grayscale image on a graph edge.
#[type_key("helios:gray8")]
#[derive(Clone, Debug)]
pub struct Gray(pub Arc<GrayImage>);

/// A binary mask on a graph edge.
#[type_key("helios:binary")]
#[derive(Clone, Debug)]
pub struct Binary(pub Arc<BinaryImage>);

#[derive(Clone, Debug, PartialEq, DaedalusTypeExpr, DaedalusToValue)]
#[daedalus(type_key = "helios:point")]
pub struct PointValue {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, PartialEq, DaedalusTypeExpr, DaedalusToValue)]
#[daedalus(type_key = "helios:quad")]
pub struct QuadValue {
    /// Clockwise in image coordinates.
    pub corners: Vec<PointValue>,
}

#[derive(Clone, Debug, Default, PartialEq, DaedalusTypeExpr, DaedalusToValue)]
#[daedalus(type_key = "helios:quads")]
pub struct QuadList {
    /// Size of the image the quads were found in; the decoder rescales
    /// corners when it decodes on a larger image.
    pub image_width: i64,
    pub image_height: i64,
    pub quads: Vec<QuadValue>,
}

#[derive(Clone, Debug, PartialEq, DaedalusTypeExpr, DaedalusToValue)]
#[daedalus(type_key = "helios:aruco_marker")]
pub struct MarkerValue {
    pub dictionary: String,
    pub id: i64,
    /// Printed top-left corner first, then clockwise.
    pub corners: Vec<PointValue>,
    pub center: PointValue,
    pub hamming: i64,
}

#[derive(Clone, Debug, Default, PartialEq, DaedalusTypeExpr, DaedalusToValue)]
#[daedalus(type_key = "helios:aruco_markers")]
pub struct MarkerList {
    pub markers: Vec<MarkerValue>,
}

fn point(p: Point) -> PointValue {
    PointValue { x: p[0] as f64, y: p[1] as f64 }
}

fn from_point(p: &PointValue) -> Point {
    [p.x as f32, p.y as f32]
}

impl From<&aruco::Marker> for MarkerValue {
    fn from(marker: &aruco::Marker) -> Self {
        Self {
            dictionary: marker.dictionary.to_string(),
            id: marker.id as i64,
            corners: marker.corners.iter().copied().map(point).collect(),
            center: point(marker.center()),
            hamming: marker.hamming as i64,
        }
    }
}

#[derive(Clone, Debug, NodeConfig)]
pub struct ThresholdParams {
    /// Averaging window side in pixels.
    #[port(default = 23, min = 3, max = 255, policy = "clamp")]
    pub window: i64,
    /// Darker than the local mean by more than this is foreground.
    #[port(default = 7, min = -64, max = 64, policy = "clamp")]
    pub offset: i64,
}

#[derive(Clone, Debug, NodeConfig)]
pub struct DownscaleParams {
    /// Shrink factor: 1, 2 or 4 (rounded down to a power of two).
    #[port(default = 2, min = 1, max = 4, policy = "clamp")]
    pub factor: i64,
}

#[derive(Clone, Debug, NodeConfig)]
pub struct QuadParams {
    /// Minimum quad perimeter, in thousandths of the larger image side.
    #[port(default = 30, min = 1, max = 4000, policy = "clamp")]
    pub min_perimeter_permille: i64,
    /// Maximum quad perimeter, in thousandths of the larger image side.
    #[port(default = 4000, min = 1, max = 4000, policy = "clamp")]
    pub max_perimeter_permille: i64,
}

#[derive(Clone, Debug, NodeConfig)]
pub struct DecodeParams {
    /// Marker dictionary: `4x4_50` or `36h11`.
    #[port(default = "4x4_50")]
    pub dictionary: String,
    /// Minimum grey-level difference between the darkest and brightest cells.
    #[port(default = 20, min = 0, max = 255, policy = "clamp")]
    pub min_contrast: i64,
    /// Bit errors to correct; -1 uses the dictionary's default.
    #[port(default = -1, min = -1, max = 8, policy = "clamp")]
    pub max_correction: i64,
}

/// The luma plane of a camera frame, read in place.
pub fn luma_view(frame: &FrameLease) -> Result<GrayView<'_>, String> {
    if !frame.has_luma_plane() {
        return Err(format!("frame format {} has no luma plane", frame.meta().format.code));
    }
    let resolution = frame.meta().format.resolution;
    let planes = frame.planes();
    let plane = planes.first().ok_or("frame has no planes")?;
    GrayView::new(resolution.width.get() as usize, resolution.height.get() as usize, plane.stride(), plane.data()).map_err(|error| error.to_string())
}

/// Copy a camera frame's luma plane into a grayscale image.
#[adapt(id = "helios.vision.framelease_to_gray", from = "styx:framelease", to = "helios:gray8", kind = daedalus::transport::AdapterKind::Materialize)]
pub fn framelease_to_gray(frame: &FrameLease) -> Result<Gray, TransportError> {
    let view = luma_view(frame).map_err(TransportError::Unsupported)?;
    Ok(Gray(Arc::new(view.to_image())))
}

/// Shrink a frame's luma by averaging blocks, so quad search runs on fewer
/// pixels. The first halving comes from the ISP when the frame carries a
/// half-size pyramid companion; otherwise it is computed from the frame in
/// place.
#[node(id = "vision.downscale", inputs("frame", config = DownscaleParams), outputs("gray"))]
pub fn downscale(frame: &FrameLease, params: DownscaleParams) -> Result<Gray, NodeError> {
    let full = luma_view(frame).map_err(NodeError::InvalidInput)?;
    if params.factor < 2 {
        return Ok(Gray(Arc::new(full.to_image())));
    }
    let mut out = match frame.pyramid_level(1) {
        Some(half) => luma_view(half).map_err(NodeError::InvalidInput)?.to_image(),
        None => full.downscale2(),
    };
    let mut factor = 2;
    while factor * 2 <= params.factor {
        out = out.downscale2();
        factor *= 2;
    }
    Ok(Gray(Arc::new(out)))
}

#[node(id = "vision.adaptive_threshold", inputs("gray", config = ThresholdParams), outputs("binary"))]
pub fn adaptive_threshold(gray: &Gray, params: ThresholdParams) -> Result<Binary, NodeError> {
    let config = ThresholdConfig { window: params.window as usize, offset: params.offset as i32 };
    Ok(Binary(Arc::new(threshold::adaptive_threshold(&gray.0, &config))))
}

#[node(id = "vision.find_quads", inputs("binary", config = QuadParams), outputs("quads"))]
pub fn find_quads(binary: &Binary, params: QuadParams) -> Result<QuadList, NodeError> {
    let config =
        quads::QuadConfig { min_perimeter_rate: params.min_perimeter_permille as f32 / 1000.0, max_perimeter_rate: params.max_perimeter_permille as f32 / 1000.0, ..quads::QuadConfig::default() };
    let found = quads::find_quads(&binary.0, &config);
    Ok(QuadList {
        image_width: binary.0.width() as i64,
        image_height: binary.0.height() as i64,
        quads: found.iter().map(|q| QuadValue { corners: q.corners.iter().copied().map(point).collect() }).collect(),
    })
}

/// Decode markers from quads on the frame's full-resolution luma, read in place.
#[node(id = "aruco.decode", inputs("frame", "quads", config = DecodeParams), outputs("markers"))]
pub fn decode(frame: &FrameLease, list: &QuadList, params: DecodeParams) -> Result<MarkerList, NodeError> {
    let gray = luma_view(frame).map_err(NodeError::InvalidInput)?;
    let dictionary = Dictionary::by_name(&params.dictionary).ok_or_else(|| NodeError::InvalidInput(format!("unknown marker dictionary '{}'", params.dictionary)))?;
    let config = DecodeConfig { dictionary, min_contrast: params.min_contrast as f32, max_correction: u32::try_from(params.max_correction).ok(), ..DecodeConfig::default() };
    let quads: Vec<Quad> = list
        .quads
        .iter()
        .filter(|q| q.corners.len() == 4)
        .map(|q| Quad { corners: [from_point(&q.corners[0]), from_point(&q.corners[1]), from_point(&q.corners[2]), from_point(&q.corners[3])] })
        .collect();
    let quads = if quads.is_empty() || list.image_width <= 0 || list.image_width as usize == gray.width() {
        quads
    } else {
        quads::scale_quads(&quads, (gray.width() as f32 / list.image_width as f32).round())
    };
    let markers = aruco::decode_quads(gray, &quads, &config);
    Ok(MarkerList { markers: markers.iter().map(MarkerValue::from).collect() })
}

/// What gray and binary images look like to host inspection.
#[derive(Clone, Debug, DaedalusToValue)]
struct ImageSummary {
    kind: String,
    width: i64,
    height: i64,
}

fn image_summary(kind: &str, width: usize, height: usize) -> Value {
    ImageSummary { kind: kind.to_string(), width: width as i64, height: height as i64 }.to_value()
}

/// Give `FrameLease` its stable key, so node ports taking a frame match the
/// `styx:framelease` payloads the engine feeds (registration is global and
/// idempotent; the engine's frame glue registers the same key).
pub fn register_frame_type() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| daedalus::data::typing::register_type::<FrameLease>(daedalus::data::model::TypeExpr::opaque(FRAMELEASE_TYPE_KEY)));
}

fn install(registry: &mut PluginRegistry) -> daedalus::runtime::plugins::PluginResult<()> {
    register_frame_type();
    registry.register_value_serializer::<Gray, _>(|gray| image_summary("gray8", gray.0.width(), gray.0.height()));
    registry.register_value_serializer::<Binary, _>(|binary| image_summary("binary", binary.0.width(), binary.0.height()));
    Ok(())
}

/// HeliOS vision nodes: CPU marker detection on camera luma.
#[plugin(
    id = "helios.vision",
    install = install,
    types(Gray, Binary),
    values(QuadList, MarkerList),
    nodes(downscale, adaptive_threshold, find_quads, decode),
    adapters(framelease_to_gray)
)]
pub struct VisionPlugin;

#[cfg(feature = "dylib")]
daedalus::export_plugin!(VisionPlugin);
