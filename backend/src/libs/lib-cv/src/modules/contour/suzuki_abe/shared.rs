#![allow(unsafe_code)]

use imageproc::contours::BorderType;
use imageproc::point::Point;
use memchr::memchr;
use smallvec::SmallVec;
use std::mem::size_of;
use std::sync::{Mutex, OnceLock};

fn row_offsets_scratch() -> &'static Mutex<Vec<usize>> {
    static ROW_OFFSETS_SCRATCH: OnceLock<Mutex<Vec<usize>>> = OnceLock::new();
    ROW_OFFSETS_SCRATCH.get_or_init(|| Mutex::new(Vec::new()))
}

const ROW_OFFSETS_RETAIN_CAP: usize = 2048;

pub(super) type ContourPoints = SmallVec<[Point<i32>; 32]>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompactContour {
    pub start: usize,
    pub len: usize,
    pub chain_len: u32,
    pub border_type: BorderType,
    pub parent: Option<usize>,
    pub min_x: i32,
    pub min_y: i32,
    pub max_x: i32,
    pub max_y: i32,
}

impl CompactContour {
    #[inline(always)]
    pub fn points<'a>(&self, point_store: &'a [Point<i32>]) -> &'a [Point<i32>] {
        &point_store[self.start..self.start + self.len]
    }

    #[inline(always)]
    pub fn bbox_dims(&self) -> (u32, u32) {
        ((self.max_x - self.min_x).unsigned_abs().saturating_add(1), (self.max_y - self.min_y).unsigned_abs().saturating_add(1))
    }
}

#[inline(always)]
pub(super) fn compact_contours_bytes(point_store_capacity: usize, contours_capacity: usize) -> usize {
    point_store_capacity * size_of::<Point<i32>>() + contours_capacity * size_of::<CompactContour>()
}

pub(super) fn ensure_scratch_len(buffer: &mut Vec<i32>, len: usize) {
    if buffer.len() != len {
        buffer.resize(len, 0);
        crate::diagnostics::report_scratch_high_water("contour.suzuki_image_values", buffer.capacity() * size_of::<i32>());
    } else {
        buffer.fill(0);
    }
}

pub(super) fn with_row_offsets<R>(width: usize, height: usize, f: impl FnOnce(&[usize]) -> R) -> R {
    let mut rows = row_offsets_scratch().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if rows.len() != height {
        rows.resize(height, 0);
        crate::diagnostics::report_scratch_high_water("contour.row_offsets", rows.capacity() * size_of::<usize>());
    }
    for (y, slot) in rows.iter_mut().enumerate().take(height) {
        *slot = y * width;
    }
    f(&rows)
}

pub(super) fn compact_shared_scratch_after_frame() {
    let mut rows = row_offsets_scratch().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if rows.capacity() > ROW_OFFSETS_RETAIN_CAP {
        *rows = Vec::with_capacity(ROW_OFFSETS_RETAIN_CAP);
    } else {
        rows.clear();
    }
}

pub(super) fn release_shared_scratch_on_idle() {
    let mut rows = row_offsets_scratch().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    *rows = Vec::new();
}

#[inline(always)]
pub(super) fn find_next_255(row: &[u8], start: usize) -> Option<usize> {
    if start >= row.len() {
        return None;
    }
    let len = row.len() - start;
    let ptr = unsafe { row.as_ptr().add(start) };
    // SAFETY: caller ensures start <= row.len().
    let slice = unsafe { core::slice::from_raw_parts(ptr, len) };
    memchr(255, slice).map(|offset| offset + start)
}

pub(super) const NEIGHBOR_FORWARD: [[usize; 8]; 8] = [
    [0, 1, 2, 3, 4, 5, 6, 7],
    [1, 2, 3, 4, 5, 6, 7, 0],
    [2, 3, 4, 5, 6, 7, 0, 1],
    [3, 4, 5, 6, 7, 0, 1, 2],
    [4, 5, 6, 7, 0, 1, 2, 3],
    [5, 6, 7, 0, 1, 2, 3, 4],
    [6, 7, 0, 1, 2, 3, 4, 5],
    [7, 0, 1, 2, 3, 4, 5, 6],
];

pub(super) const NEIGHBOR_REVERSE: [[usize; 8]; 8] = [
    [7, 6, 5, 4, 3, 2, 1, 0],
    [0, 7, 6, 5, 4, 3, 2, 1],
    [1, 0, 7, 6, 5, 4, 3, 2],
    [2, 1, 0, 7, 6, 5, 4, 3],
    [3, 2, 1, 0, 7, 6, 5, 4],
    [4, 3, 2, 1, 0, 7, 6, 5],
    [5, 4, 3, 2, 1, 0, 7, 6],
    [6, 5, 4, 3, 2, 1, 0, 7],
];

pub(super) const OPPOSITE: [usize; 8] = [4, 5, 6, 7, 0, 1, 2, 3];

#[derive(Copy, Clone)]
pub(super) struct NeighborStep {
    pub(super) idx: usize,
    pub(super) lin: isize,
    pub(super) dx: i32,
    pub(super) dy: i32,
}

const ZERO_STEP: NeighborStep = NeighborStep { idx: 0, lin: 0, dx: 0, dy: 0 };

#[inline(always)]
pub(super) fn build_neighbor_steps(offsets_lin: &[isize; 8], offsets: &[Point<i32>; 8], order_table: &[[usize; 8]; 8]) -> [[NeighborStep; 8]; 8] {
    let mut steps = [[ZERO_STEP; 8]; 8];
    let mut start = 0usize;
    while start < 8 {
        let mut i = 0usize;
        while i < 8 {
            let idx = order_table[start][i];
            let diff = offsets[idx];
            steps[start][i] = NeighborStep { idx, lin: offsets_lin[idx], dx: diff.x, dy: diff.y };
            i += 1;
        }
        start += 1;
    }
    steps
}

const fn right_edge_table() -> [[bool; 8]; 8] {
    let mut table = [[false; 8]; 8];
    let mut start = 0usize;
    while start < 8 {
        let i_east = (4 + 8 - start) & 7;
        let mut next = 0usize;
        while next < 8 {
            let i_target = (next + 8 - start) & 7;
            table[start][next] = i_east > i_target;
            next += 1;
        }
        start += 1;
    }
    table
}

pub(super) const RIGHT_EDGE: [[bool; 8]; 8] = right_edge_table();

#[inline(always)]
pub(super) fn first_neighbor_inner_fwd(base: isize, start: usize, steps_fwd: &[[NeighborStep; 8]; 8], image_data: &[u8], curr: Point<i32>) -> Option<(usize, Point<i32>)> {
    let order = unsafe { steps_fwd.get_unchecked(start) };
    macro_rules! check {
        ($step:expr) => {{
            let step = $step;
            let nidx = (base + step.lin) as usize;
            if unsafe { *image_data.get_unchecked(nidx) } != 0 {
                return Some((step.idx, Point::new(curr.x + step.dx, curr.y + step.dy)));
            }
        }};
    }
    check!(&order[0]);
    check!(&order[1]);
    check!(&order[2]);
    check!(&order[3]);
    check!(&order[4]);
    check!(&order[5]);
    check!(&order[6]);
    check!(&order[7]);
    None
}

#[inline(always)]
pub(super) fn first_neighbor_outer_fwd(
    start: usize,
    steps_fwd: &[[NeighborStep; 8]; 8],
    image_data: &[u8],
    width: usize,
    width_i32: i32,
    height_i32: i32,
    curr: Point<i32>,
) -> Option<(usize, Point<i32>)> {
    let order = unsafe { steps_fwd.get_unchecked(start) };
    macro_rules! check {
        ($step:expr) => {{
            let step = $step;
            let nx = curr.x + step.dx;
            let ny = curr.y + step.dy;
            if nx >= 0 && ny >= 0 && nx < width_i32 && ny < height_i32 {
                let nidx = ny as usize * width + nx as usize;
                if unsafe { *image_data.get_unchecked(nidx) } != 0 {
                    return Some((step.idx, Point::new(nx, ny)));
                }
            }
        }};
    }
    check!(&order[0]);
    check!(&order[1]);
    check!(&order[2]);
    check!(&order[3]);
    check!(&order[4]);
    check!(&order[5]);
    check!(&order[6]);
    check!(&order[7]);
    None
}

#[inline(always)]
pub(super) fn next_neighbor_inner_rev(base: isize, start: usize, steps_rev: &[[NeighborStep; 8]; 8], image_data: &[u8], pos_x: i32, pos_y: i32) -> (usize, Point<i32>) {
    let order = unsafe { steps_rev.get_unchecked(start) };
    macro_rules! check {
        ($step:expr) => {{
            let step = $step;
            let nidx = (base + step.lin) as usize;
            if unsafe { *image_data.get_unchecked(nidx) } != 0 {
                return (step.idx, Point::new(pos_x + step.dx, pos_y + step.dy));
            }
        }};
    }
    check!(&order[0]);
    check!(&order[1]);
    check!(&order[2]);
    check!(&order[3]);
    check!(&order[4]);
    check!(&order[5]);
    check!(&order[6]);
    check!(&order[7]);
    debug_assert!(false, "contour scan missed a neighbor");
    (order[7].idx, Point::new(pos_x, pos_y))
}

#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub(super) fn next_neighbor_outer_rev(
    start: usize,
    steps_rev: &[[NeighborStep; 8]; 8],
    image_data: &[u8],
    width: usize,
    width_i32: i32,
    height_i32: i32,
    pos_x: i32,
    pos_y: i32,
) -> (usize, Point<i32>) {
    let order = unsafe { steps_rev.get_unchecked(start) };
    macro_rules! check {
        ($step:expr) => {{
            let step = $step;
            let nx = pos_x + step.dx;
            let ny = pos_y + step.dy;
            if nx >= 0 && ny >= 0 && nx < width_i32 && ny < height_i32 {
                let nidx = ny as usize * width + nx as usize;
                if unsafe { *image_data.get_unchecked(nidx) } != 0 {
                    return (step.idx, Point::new(nx, ny));
                }
            }
        }};
    }
    check!(&order[0]);
    check!(&order[1]);
    check!(&order[2]);
    check!(&order[3]);
    check!(&order[4]);
    check!(&order[5]);
    check!(&order[6]);
    check!(&order[7]);
    debug_assert!(false, "contour scan missed a neighbor");
    (start, Point::new(pos_x, pos_y))
}

#[derive(Clone)]
pub(super) struct Neighborhood {
    pub(super) offsets: [Point<i32>; 8],
    pub(super) start: usize,
}

impl Default for Neighborhood {
    fn default() -> Self {
        Self {
            offsets: [
                Point { x: -1, y: 0 },  // w
                Point { x: -1, y: -1 }, // nw
                Point { x: 0, y: -1 },  // n
                Point { x: 1, y: -1 },  // ne
                Point { x: 1, y: 0 },   // e
                Point { x: 1, y: 1 },   // se
                Point { x: 0, y: 1 },   // s
                Point { x: -1, y: 1 },  // sw
            ],
            start: 0,
        }
    }
}

impl Neighborhood {
    #[inline(always)]
    pub(super) fn rotate_to_value(&mut self, value: Point<i32>) {
        self.start = offset_index(value);
    }
}

#[inline(always)]
pub(super) fn offset_index(value: Point<i32>) -> usize {
    debug_assert!((-1..=1).contains(&value.x));
    debug_assert!((-1..=1).contains(&value.y));
    const TABLE: [usize; 9] = [
        1, 2, 3, // y = -1
        0, 8, 4, // y = 0 (8 is invalid for 0,0)
        7, 6, 5, // y = 1
    ];
    let idx = ((value.y + 1) * 3 + (value.x + 1)) as usize;
    let mapped = TABLE[idx];
    if mapped == 8 {
        unreachable!("invalid neighbor offset");
    }
    mapped
}
