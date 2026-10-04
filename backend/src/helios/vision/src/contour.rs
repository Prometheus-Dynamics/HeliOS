//! Outer contours of foreground components.

use crate::image::BinaryImage;

/// Neighbour offsets, clockwise in image coordinates (y down), starting west.
const DIRS: [(isize, isize); 8] = [(-1, 0), (-1, -1), (0, -1), (1, -1), (1, 0), (1, 1), (0, 1), (-1, 1)];

/// Size limits applied before a component's boundary is traced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComponentFilter {
    /// Minimum bounding-box perimeter in pixels.
    pub min_box_perimeter: usize,
    /// Maximum bounding-box perimeter in pixels.
    pub max_box_perimeter: usize,
}

impl Default for ComponentFilter {
    fn default() -> Self {
        Self { min_box_perimeter: 0, max_box_perimeter: usize::MAX }
    }
}

/// Trace the outer boundary of every 8-connected foreground component whose
/// bounding box passes `filter`. Each contour is a closed list of boundary
/// pixels in clockwise order (y down), without repeating the first pixel.
///
/// Components are found on horizontal runs rather than pixels, and the
/// tracer reads the binary image directly: every foreground 8-neighbour of a
/// component's boundary pixel belongs to that component, so no label map is
/// needed.
pub fn outer_contours(binary: &BinaryImage, filter: &ComponentFilter) -> Vec<Vec<(i32, i32)>> {
    let runs = Runs::find(binary);
    let mut contours = Vec::new();
    for component in runs.components() {
        let (x0, y0, x1, y1) = component.bounds;
        let box_perimeter = 2 * ((x1 - x0 + 1) + (y1 - y0 + 1));
        if box_perimeter < filter.min_box_perimeter || box_perimeter > filter.max_box_perimeter {
            continue;
        }
        contours.push(trace(binary, component.start.0 as isize, component.start.1 as isize));
    }
    contours
}

/// A horizontal run of foreground pixels `[start, end)` on row `y`.
#[derive(Clone, Copy)]
struct Run {
    y: u32,
    start: u32,
    end: u32,
}

/// A connected component: bounding box `(x0, y0, x1, y1)` (inclusive) and its
/// top-most, left-most pixel.
struct Component {
    bounds: (usize, usize, usize, usize),
    start: (usize, usize),
}

/// Foreground runs of a binary image joined into 8-connected components with
/// union-find.
struct Runs {
    runs: Vec<Run>,
    parent: Vec<u32>,
}

impl Runs {
    fn find(binary: &BinaryImage) -> Self {
        let width = binary.width();
        let mut runs: Vec<Run> = Vec::new();
        let mut parent: Vec<u32> = Vec::new();
        let mut previous = 0..0; // runs of the row above
        for (y, row) in binary.data().chunks_exact(width.max(1)).enumerate() {
            let row_start = runs.len();
            let mut x = 0;
            while x < width {
                // Skip background eight bytes at a time.
                while x + 8 <= width && u64::from_ne_bytes(row[x..x + 8].try_into().expect("8 bytes")) == 0 {
                    x += 8;
                }
                while x < width && row[x] == 0 {
                    x += 1;
                }
                if x == width {
                    break;
                }
                let start = x;
                while x < width && row[x] != 0 {
                    x += 1;
                }
                runs.push(Run { y: y as u32, start: start as u32, end: x as u32 });
                parent.push((runs.len() - 1) as u32);
            }
            // Join with 8-connected runs of the row above.
            let mut j = previous.start;
            for i in row_start..runs.len() {
                let run = runs[i];
                while j < previous.end && runs[j].end < run.start {
                    j += 1;
                }
                let mut k = j;
                while k < previous.end && runs[k].start <= run.end {
                    union(&mut parent, i as u32, k as u32);
                    k += 1;
                }
            }
            previous = row_start..runs.len();
        }
        Self { runs, parent }
    }

    fn components(mut self) -> Vec<Component> {
        let mut index = vec![u32::MAX; self.runs.len()];
        let mut components: Vec<Component> = Vec::new();
        for i in 0..self.runs.len() {
            let root = root(&mut self.parent, i as u32) as usize;
            let run = self.runs[i];
            let (y, x0, x1) = (run.y as usize, run.start as usize, run.end as usize - 1);
            if index[root] == u32::MAX {
                // Runs come in scan order, so a component's first run holds
                // its top-most, left-most pixel.
                index[root] = components.len() as u32;
                components.push(Component { bounds: (x0, y, x1, y), start: (x0, y) });
            } else {
                let b = &mut components[index[root] as usize].bounds;
                b.0 = b.0.min(x0);
                b.2 = b.2.max(x1);
                b.3 = y;
            }
        }
        components
    }
}

fn root(parent: &mut [u32], mut a: u32) -> u32 {
    while parent[a as usize] != a {
        parent[a as usize] = parent[parent[a as usize] as usize];
        a = parent[a as usize];
    }
    a
}

fn union(parent: &mut [u32], a: u32, b: u32) {
    let (ra, rb) = (root(parent, a), root(parent, b));
    if ra != rb {
        let (lo, hi) = (ra.min(rb), ra.max(rb));
        parent[hi as usize] = lo;
    }
}

/// Moore-neighbour tracing of one component's outer boundary, starting at its
/// top-most, left-most pixel (whose west neighbour is background).
fn trace(binary: &BinaryImage, sx: isize, sy: isize) -> Vec<(i32, i32)> {
    let (width, height) = (binary.width(), binary.height());
    let is_member = |x: isize, y: isize| binary.is_set(x, y);

    let start = (sx, sy);
    let mut contour = vec![(sx as i32, sy as i32)];
    let mut current = start;
    // Direction from `current` to a known background pixel.
    let mut background = 0usize;
    let mut first_move: Option<usize> = None;
    let limit = 4 * width * height + 8;

    for _ in 0..limit {
        let mut moved = None;
        for k in 1..=8 {
            let dir = (background + k) % 8;
            let (nx, ny) = (current.0 + DIRS[dir].0, current.1 + DIRS[dir].1);
            if is_member(nx, ny) {
                moved = Some((dir, (nx, ny), (background + k - 1) % 8));
                break;
            }
        }
        let Some((dir, next, last_background)) = moved else {
            break; // isolated pixel
        };
        if current == start {
            match first_move {
                None => first_move = Some(dir),
                Some(first) if first == dir => break,
                Some(_) => {}
            }
        }
        // The last background pixel examined, seen from the new position.
        let (bx, by) = (current.0 + DIRS[last_background].0, current.1 + DIRS[last_background].1);
        let offset = (bx - next.0, by - next.1);
        background = DIRS.iter().position(|&d| d == offset).unwrap_or(0);
        current = next;
        if current == start {
            continue;
        }
        contour.push((current.0 as i32, current.1 as i32));
    }
    contour
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binary_from(rows: &[&str]) -> BinaryImage {
        let width = rows[0].len();
        let data = rows.iter().flat_map(|row| row.bytes().map(|b| u8::from(b == b'#'))).collect();
        BinaryImage::new(width, rows.len(), data).unwrap()
    }

    #[test]
    fn square_ring_traces_outer_boundary_only() {
        let binary = binary_from(&["........", ".######.", ".#....#.", ".#....#.", ".#....#.", ".######.", "........"]);
        let contours = outer_contours(&binary, &ComponentFilter::default());
        assert_eq!(contours.len(), 1);
        let contour = &contours[0];
        assert_eq!(contour[0], (1, 1));
        // 6x5 ring boundary: 2*(6+5)-4 = 18 pixels.
        assert_eq!(contour.len(), 18);
        assert!(contour.contains(&(6, 5)));
        assert!(!contour.contains(&(3, 3)));
    }

    #[test]
    fn separate_components_get_separate_contours() {
        let binary = binary_from(&["##...##", "##...##", "......."]);
        assert_eq!(outer_contours(&binary, &ComponentFilter::default()).len(), 2);
    }

    #[test]
    fn single_pixel_is_a_one_point_contour() {
        let binary = binary_from(&["...", ".#.", "..."]);
        assert_eq!(outer_contours(&binary, &ComponentFilter::default()), vec![vec![(1, 1)]]);
    }

    #[test]
    fn filter_skips_small_components() {
        let binary = binary_from(&["#......", ".......", "..####.", "..####."]);
        let filter = ComponentFilter { min_box_perimeter: 8, max_box_perimeter: usize::MAX };
        assert_eq!(outer_contours(&binary, &filter).len(), 1);
    }
}
