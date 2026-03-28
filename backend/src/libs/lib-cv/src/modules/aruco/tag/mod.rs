//! ArUco tag family definitions and decoding utilities.

pub mod dictionary;

mod decode_cell_means;
mod decode_grid;
mod decoding;
mod family;

pub(crate) fn compact_runtime_scratch_after_frame() {
    decode_grid::compact_tag_scratch_after_frame();
}

// Used by family data modules to share the decode implementation.
pub(super) fn decode(code: &[Vec<u8>], family: &dyn ArucoTagDecoding) -> Option<ArucoTagDecode> {
    decode_grid::decode(code, family)
}

pub use decode_cell_means::{
    DecodeFromCellMeansError, DecodeFromCellMeansOk, decode_from_cell_means, decode_from_cell_means_result, decode_from_cell_means_result_detailed, decode_from_cell_means_result_detailed_with_tuning,
};
pub use decode_grid::{decode_marker_grid, decode_marker_grid_from_cell_means};
pub use decoding::{ArucoTagDecode, ArucoTagDecoding};
pub use family::{ArucoTagDecodeTuning, ArucoTagFamily, ArucoTagFamilyKind};
