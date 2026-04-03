use imageproc::point::Point;
use std::sync::{Mutex, OnceLock};

const RDP_RETAIN_POINT_CAP: usize = 4 * 1024;
const RDP_RETAIN_MARKER_CAP: usize = 4 * 1024;
const RDP_RETAIN_INDEX_CAP: usize = 4 * 1024;

#[derive(Default)]
pub(super) struct RdpScratchF32 {
    pub(super) stack: Vec<(usize, usize)>,
    pub(super) marker_epoch: Vec<u32>,
    pub(super) marker_gen: u32,
    pub(super) seg1: Vec<Point<f32>>,
    pub(super) seg2: Vec<Point<f32>>,
    pub(super) out2: Vec<Point<f32>>,
    pub(super) idxs: Vec<usize>,
    pub(super) uniq: Vec<usize>,
    pub(super) hull: Vec<usize>,
}

#[derive(Default)]
pub(super) struct RdpScratchI32 {
    pub(super) stack: Vec<(usize, usize)>,
    pub(super) marker_epoch: Vec<u32>,
    pub(super) marker_gen: u32,
    pub(super) seg1: Vec<Point<i32>>,
    pub(super) seg2: Vec<Point<i32>>,
    pub(super) out2: Vec<Point<i32>>,
}

fn rdp_scratch_f32() -> &'static Mutex<RdpScratchF32> {
    static RDP_SCRATCH_F32: OnceLock<Mutex<RdpScratchF32>> = OnceLock::new();
    RDP_SCRATCH_F32.get_or_init(|| Mutex::new(RdpScratchF32::default()))
}

fn rdp_scratch_i32() -> &'static Mutex<RdpScratchI32> {
    static RDP_SCRATCH_I32: OnceLock<Mutex<RdpScratchI32>> = OnceLock::new();
    RDP_SCRATCH_I32.get_or_init(|| Mutex::new(RdpScratchI32::default()))
}

#[inline(always)]
pub(super) fn with_rdp_scratch_f32<R>(f: impl FnOnce(&mut RdpScratchF32) -> R) -> R {
    let mut scratch = rdp_scratch_f32().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&mut scratch)
}

#[inline(always)]
pub(super) fn with_rdp_scratch_i32<R>(f: impl FnOnce(&mut RdpScratchI32) -> R) -> R {
    let mut scratch = rdp_scratch_i32().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&mut scratch)
}

#[inline(always)]
fn trim_retained_vec<T>(vec: &mut Vec<T>, retain_cap: usize) {
    vec.clear();
    if vec.capacity() > retain_cap {
        vec.shrink_to(retain_cap);
    }
}

pub(crate) fn compact_rdp_scratch_after_frame() {
    with_rdp_scratch_f32(|scratch| {
        trim_retained_vec(&mut scratch.stack, RDP_RETAIN_MARKER_CAP);
        trim_retained_vec(&mut scratch.marker_epoch, RDP_RETAIN_MARKER_CAP);
        trim_retained_vec(&mut scratch.seg1, RDP_RETAIN_POINT_CAP);
        trim_retained_vec(&mut scratch.seg2, RDP_RETAIN_POINT_CAP);
        trim_retained_vec(&mut scratch.out2, RDP_RETAIN_POINT_CAP);
        trim_retained_vec(&mut scratch.idxs, RDP_RETAIN_INDEX_CAP);
        trim_retained_vec(&mut scratch.uniq, RDP_RETAIN_INDEX_CAP);
        trim_retained_vec(&mut scratch.hull, RDP_RETAIN_INDEX_CAP);
    });
    with_rdp_scratch_i32(|scratch| {
        trim_retained_vec(&mut scratch.stack, RDP_RETAIN_MARKER_CAP);
        trim_retained_vec(&mut scratch.marker_epoch, RDP_RETAIN_MARKER_CAP);
        trim_retained_vec(&mut scratch.seg1, RDP_RETAIN_POINT_CAP);
        trim_retained_vec(&mut scratch.seg2, RDP_RETAIN_POINT_CAP);
        trim_retained_vec(&mut scratch.out2, RDP_RETAIN_POINT_CAP);
    });
}
