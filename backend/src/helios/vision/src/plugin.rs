//! Daedalus nodes for marker detection, one node per Eidos stage.
//!
//! The graph carries a camera frame (`styx:framelease`) into the stages; they
//! read it in place. `vision.aruco_mask` thresholds the frame's half-size
//! luma (the ISP's pyramid companion when the frame has one),
//! `vision.find_quads` traces candidate quads on the mask, `aruco.decode`
//! reads the marker bits on the full frame and `vision.refine_corners` moves
//! the corners onto the sub-pixel marker edges. Each node keeps its Eidos
//! stage, and that stage's scratch, between frames. Stage outputs are
//! structured types with stable keys so they can be inspected and stored.

use std::sync::Arc;

use daedalus::{
    DaedalusToValue, DaedalusTypeExpr,
    data::to_value::ToValue,
    macros::{NodeConfig, node, plugin},
    runtime::{NodeError, plugins::PluginRegistry},
    type_key,
};
use eidos_aruco::{
    ArucoDetections, ArucoDictionaryKind, ArucoMaskPrep, ArucoMaskPrepArgs, ArucoMaskPrepConfig, ArucoQuad, ArucoQuads, BitGridSampleMode, BitGridSamplerConfig, CandidateQuadFinder,
    CandidateQuadFinderArgs, CandidateQuadFinderConfig, DecodeQuadsArgs, PointI, QuadDetectionDecoder, QuadDetectionDecoderConfig, dictionary, scale_coord_between_levels,
};
use eidos_core::{ColorSpace, FourCc, MaskFrame, MediaFormat};
use styx::imports::framelease::FrameLease;

use crate::geometry::Point;
use crate::image::GrayView;
use crate::refine::refine_corners;

/// Key of the Styx frame carrier (owned by Styx).
pub const FRAMELEASE_TYPE_KEY: &str = styx::core::daedalus::FRAME_TYPE_KEY;
pub const MASK_TYPE_KEY: &str = "eidos:mask";

/// A binary mask on a graph edge (Eidos `MaskFrame`, 0 or 255 per pixel).
#[type_key("eidos:mask")]
#[derive(Clone)]
pub struct Mask(pub Arc<MaskFrame>);

impl std::fmt::Debug for Mask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let resolution = self.0.as_frame().meta().format.resolution;
        write!(f, "Mask({}x{})", resolution.width, resolution.height)
    }
}

#[derive(Clone, Debug, PartialEq, DaedalusTypeExpr, DaedalusToValue)]
#[daedalus(type_key = "helios:point")]
pub struct PointValue {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, PartialEq, DaedalusTypeExpr, DaedalusToValue)]
#[daedalus(type_key = "helios:quad")]
pub struct QuadValue {
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

fn point(x: f64, y: f64) -> PointValue {
    PointValue { x, y }
}

fn center(corners: &[PointValue]) -> PointValue {
    let n = corners.len().max(1) as f64;
    point(corners.iter().map(|p| p.x).sum::<f64>() / n, corners.iter().map(|p| p.y).sum::<f64>() / n)
}

/// Dictionary names accepted by `aruco.decode`.
pub fn dictionary_kind(name: &str) -> Option<ArucoDictionaryKind> {
    use ArucoDictionaryKind::*;
    Some(match name {
        "4x4_50" => Aruco4x4_50,
        "4x4_100" => Aruco4x4_100,
        "4x4_250" => Aruco4x4_250,
        "4x4_1000" => Aruco4x4_1000,
        "5x5_50" => Aruco5x5_50,
        "5x5_100" => Aruco5x5_100,
        "5x5_250" => Aruco5x5_250,
        "5x5_1000" => Aruco5x5_1000,
        "6x6_50" => Aruco6x6_50,
        "6x6_100" => Aruco6x6_100,
        "6x6_250" => Aruco6x6_250,
        "6x6_1000" => Aruco6x6_1000,
        "7x7_50" => Aruco7x7_50,
        "7x7_100" => Aruco7x7_100,
        "7x7_250" => Aruco7x7_250,
        "7x7_1000" => Aruco7x7_1000,
        "original" => ArucoOriginal,
        "mip_36h12" => ArucoMip36h12,
        "16h5" => AprilTag16h5,
        "25h9" => AprilTag25h9,
        "36h10" => AprilTag36h10,
        "36h11" => AprilTag36h11,
        _ => return None,
    })
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

fn eidos_error(error: impl std::fmt::Display) -> NodeError {
    NodeError::InvalidInput(error.to_string())
}

#[derive(Clone, Debug, NodeConfig)]
pub struct MaskParams {
    /// Pyramid level to threshold: 0 is the full frame, 1 the half-size plane.
    #[port(default = 1, min = 0, max = 1, policy = "clamp")]
    pub level: i64,
    /// Box-mean window radius in pixels (the window is 2 * radius + 1).
    #[port(default = 3, min = 1, max = 64, policy = "clamp")]
    pub radius: i64,
    /// Darker than the local mean by more than this is foreground.
    #[port(default = 7, min = -64, max = 64, policy = "clamp")]
    pub offset: i64,
}

#[derive(Default)]
pub struct MaskState {
    prep: Option<(ArucoMaskPrep, (usize, i16))>,
    mask: Option<Arc<MaskFrame>>,
    /// Half-size luma computed on the CPU when the frame has no ISP companion.
    half: Option<FrameLease>,
}

/// Adaptive (box-mean) threshold of the frame's luma into a mask, on the
/// ISP's half-size companion when present. Eidos `ArucoMaskPrep`.
#[node(id = "vision.aruco_mask", inputs("frame", config = MaskParams), outputs("mask"), state(MaskState))]
pub fn aruco_mask(frame: &FrameLease, params: MaskParams, state: &mut MaskState) -> Result<Mask, NodeError> {
    let key = (params.radius as usize, params.offset as i16);
    if state.prep.as_ref().is_none_or(|(_, current)| *current != key) {
        let mut config = ArucoMaskPrepConfig::default();
        config.adaptive.radius = key.0;
        config.adaptive.offset = key.1;
        state.prep = Some((ArucoMaskPrep::new(config), key));
    }
    let source: &FrameLease = match (params.level, frame.pyramid_level(1)) {
        (0, _) => frame,
        (_, Some(half)) => half,
        (_, None) => {
            let half = luma_view(frame).map_err(NodeError::InvalidInput)?.downscale2();
            let resolution = eidos_core::Resolution::new(half.width() as u32, half.height() as u32).ok_or_else(|| eidos_error("frame too small to halve"))?;
            if state.half.as_ref().is_none_or(|f| f.meta().format.resolution != resolution) {
                let format = MediaFormat::new(FourCc::R8, resolution, ColorSpace::Unknown);
                state.half = Some(FrameLease::allocate_host_owned(format, frame.meta().timestamp).map_err(eidos_error)?);
            }
            let target = state.half.as_mut().expect("allocated above");
            target.copy_slice_to_visible_plane(0, half.data()).map_err(eidos_error)?;
            state.half.as_ref().expect("allocated above")
        }
    };
    let resolution = source.meta().format.resolution;
    // Reuse last frame's mask when nothing downstream still holds it.
    let reusable = state.mask.as_mut().and_then(Arc::get_mut).is_some_and(|mask| mask.as_frame().meta().format.resolution == resolution);
    if !reusable {
        let format = MediaFormat::new(FourCc::R8, resolution, ColorSpace::Unknown);
        state.mask = Some(Arc::new(MaskFrame::from_frame_unchecked(FrameLease::allocate_host_owned(format, source.meta().timestamp).map_err(eidos_error)?)));
    }
    let mask = state.mask.as_mut().and_then(Arc::get_mut).expect("unique after allocation");
    state.prep.as_mut().expect("set above").0.apply(ArucoMaskPrepArgs { src: source, dst: mask }).map_err(eidos_error)?;
    Ok(Mask(state.mask.clone().expect("set above")))
}

#[derive(Clone, Debug, NodeConfig)]
pub struct QuadParams {
    /// Smallest candidate width and height, in mask pixels.
    #[port(default = 4, min = 1, max = 1000, policy = "clamp")]
    pub min_size: i64,
    /// Polygon approximation tolerance, thousandths of the contour perimeter.
    #[port(default = 50, min = 1, max = 500, policy = "clamp")]
    pub approx_permille: i64,
}

#[derive(Default)]
pub struct QuadState {
    finder: Option<(CandidateQuadFinder, (u32, u32))>,
}

/// Convex quadrilateral candidates traced on a mask. Eidos `CandidateQuadFinder`.
#[node(id = "vision.find_quads", inputs("mask", config = QuadParams), outputs("quads"), state(QuadState))]
pub fn find_quads(mask: &Mask, params: QuadParams, state: &mut QuadState) -> Result<QuadList, NodeError> {
    let key = (params.min_size as u32, params.approx_permille as u32);
    if state.finder.as_ref().is_none_or(|(_, current)| *current != key) {
        let config = CandidateQuadFinderConfig { min_width: key.0, min_height: key.0, douglas_peucker_fraction_x1000: key.1, ..CandidateQuadFinderConfig::default() };
        state.finder = Some((CandidateQuadFinder::new(config), key));
    }
    let (quads, _) = state.finder.as_mut().expect("set above").0.apply(CandidateQuadFinderArgs { mask: &mask.0 }).map_err(eidos_error)?;
    let resolution = mask.0.as_frame().meta().format.resolution;
    Ok(QuadList {
        image_width: resolution.width.get() as i64,
        image_height: resolution.height.get() as i64,
        quads: quads.items.iter().map(|q| QuadValue { corners: q.corners.iter().map(|c| point(c.x as f64, c.y as f64)).collect() }).collect(),
    })
}

#[derive(Clone, Debug, NodeConfig)]
pub struct DecodeParams {
    /// Marker dictionary, e.g. `4x4_50`, `36h11`, `16h5` (see `dictionary_kind`).
    #[port(default = "4x4_50")]
    pub dictionary: String,
    /// Bit errors to correct; -1 uses the dictionary's default.
    #[port(default = -1, min = -1, max = 8, policy = "clamp")]
    pub max_correction: i64,
}

#[derive(Default)]
pub struct DecodeState {
    decoder: Option<(QuadDetectionDecoder, (ArucoDictionaryKind, i64))>,
    quads: ArucoQuads,
    detections: ArucoDetections,
}

/// Decode marker ids from quads on the frame's full-resolution luma, read in
/// place. Eidos `QuadDetectionDecoder` with 3x3-mean bit sampling.
#[node(id = "aruco.decode", inputs("frame", "quads", config = DecodeParams), outputs("markers"), state(DecodeState))]
pub fn decode(frame: &FrameLease, list: &QuadList, params: DecodeParams, state: &mut DecodeState) -> Result<MarkerList, NodeError> {
    let kind = dictionary_kind(&params.dictionary).ok_or_else(|| NodeError::InvalidInput(format!("unknown marker dictionary '{}'", params.dictionary)))?;
    let key = (kind, params.max_correction);
    if state.decoder.as_ref().is_none_or(|(_, current)| *current != key) {
        let dict = dictionary(kind);
        let max_correction_bits = u8::try_from(params.max_correction).unwrap_or(dict.max_correction_bits);
        let sampler = BitGridSamplerConfig { marker_size: dict.marker_size, threshold: 128, border_bits: 1, sample_mode: BitGridSampleMode::Mean3x3 };
        state.decoder = Some((QuadDetectionDecoder::new_with_config(dict, QuadDetectionDecoderConfig { max_correction_bits, sampler }), key));
    }
    let resolution = frame.meta().format.resolution;
    let (width, height) = (resolution.width.get() as usize, resolution.height.get() as usize);
    let (from_width, from_height) = (list.image_width.max(1) as usize, list.image_height.max(1) as usize);
    state.quads.items.clear();
    for quad in list.quads.iter().filter(|q| q.corners.len() == 4) {
        let corner = |i: usize| {
            let p = &quad.corners[i];
            PointI { x: scale_coord_between_levels(p.x.round() as i32, width, from_width), y: scale_coord_between_levels(p.y.round() as i32, height, from_height) }
        };
        state.quads.items.push(ArucoQuad { corners: [corner(0), corner(1), corner(2), corner(3)] });
    }
    state.decoder.as_mut().expect("set above").0.apply_into(DecodeQuadsArgs { frame, quads: &state.quads }, &mut state.detections).map_err(eidos_error)?;
    let markers = state
        .detections
        .items
        .iter()
        .map(|d| {
            let corners: Vec<PointValue> = d.quad.corners.iter().map(|c| point(c.x as f64, c.y as f64)).collect();
            MarkerValue { dictionary: params.dictionary.clone(), id: d.id as i64, center: center(&corners), corners, hamming: d.hamming_distance as i64 }
        })
        .collect();
    Ok(MarkerList { markers })
}

/// Move marker corners onto the sub-pixel marker edges (line fits along each
/// side on the full-resolution luma). Markers whose edges cannot be fitted
/// keep their corners.
#[node(id = "vision.refine_corners", inputs("frame", "markers"), outputs("markers"))]
pub fn refine_marker_corners(frame: &FrameLease, markers: &MarkerList) -> Result<MarkerList, NodeError> {
    let gray = luma_view(frame).map_err(NodeError::InvalidInput)?;
    let mut out = markers.clone();
    for marker in out.markers.iter_mut().filter(|m| m.corners.len() == 4) {
        let coarse: [Point; 4] = std::array::from_fn(|i| [marker.corners[i].x as f32, marker.corners[i].y as f32]);
        if let Some(refined) = refine_corners(gray, coarse) {
            marker.corners = refined.iter().map(|p| point(p[0] as f64, p[1] as f64)).collect();
            marker.center = center(&marker.corners);
        }
    }
    Ok(out)
}

/// What a mask looks like to host inspection.
#[derive(Clone, Debug, DaedalusToValue)]
struct MaskSummary {
    width: i64,
    height: i64,
}

/// Frame ports take `FrameLease`, whose type key (`styx:framelease`) comes from
/// Styx's `StyxFramesPlugin`; hosts install that plugin before this one.
fn install(registry: &mut PluginRegistry) -> daedalus::runtime::plugins::PluginResult<()> {
    registry.register_value_serializer::<Mask, _>(|mask| {
        let resolution = mask.0.as_frame().meta().format.resolution;
        MaskSummary { width: resolution.width.get() as i64, height: resolution.height.get() as i64 }.to_value()
    });
    Ok(())
}

/// HeliOS vision nodes: marker detection on camera frames with Eidos.
#[plugin(id = "helios.vision", install = install, types(Mask), values(QuadList, MarkerList), nodes(aruco_mask, find_quads, decode, refine_marker_corners))]
pub struct VisionPlugin;

#[cfg(feature = "dylib")]
daedalus::export_plugin!(VisionPlugin);
