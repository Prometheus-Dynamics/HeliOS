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
pub fn outer_contours(binary: &BinaryImage, filter: &ComponentFilter) -> Vec<Vec<(i32, i32)>> {
    let (width, height) = (binary.width(), binary.height());
    let labels = label_components(binary);
    let count = labels.count;
    if count == 0 {
        return Vec::new();
    }

    // Bounding box and first (top-most, then left-most) pixel per component.
    let mut boxes = vec![(usize::MAX, usize::MAX, 0usize, 0usize); count];
    let mut starts = vec![None; count];
    for y in 0..height {
        for x in 0..width {
            let label = labels.map[y * width + x];
            if label == 0 {
                continue;
            }
            let index = (label - 1) as usize;
            let b = &mut boxes[index];
            b.0 = b.0.min(x);
            b.1 = b.1.min(y);
            b.2 = b.2.max(x);
            b.3 = b.3.max(y);
            if starts[index].is_none() {
                starts[index] = Some((x, y));
            }
        }
    }

    let mut contours = Vec::new();
    for (index, start) in starts.iter().enumerate() {
        let Some((sx, sy)) = *start else { continue };
        let (x0, y0, x1, y1) = boxes[index];
        let box_perimeter = 2 * ((x1 - x0 + 1) + (y1 - y0 + 1));
        if box_perimeter < filter.min_box_perimeter || box_perimeter > filter.max_box_perimeter {
            continue;
        }
        contours.push(trace(&labels, width, height, index as u32 + 1, sx as isize, sy as isize));
    }
    contours
}

struct Labels {
    map: Vec<u32>,
    count: usize,
}

/// 8-connected component labelling with union-find. Labels are 1-based and
/// dense; 0 is background.
fn label_components(binary: &BinaryImage) -> Labels {
    let (width, height) = (binary.width(), binary.height());
    let mut provisional = vec![0u32; width * height];
    let mut parent: Vec<u32> = vec![0];

    fn find(parent: &mut [u32], mut a: u32) -> u32 {
        while parent[a as usize] != a {
            parent[a as usize] = parent[parent[a as usize] as usize];
            a = parent[a as usize];
        }
        a
    }

    for y in 0..height {
        for x in 0..width {
            if binary.data()[y * width + x] == 0 {
                continue;
            }
            // Already-visited neighbours: W, NW, N, NE.
            let mut neighbours = [0u32; 4];
            let mut n = 0;
            for (dx, dy) in [(-1isize, 0isize), (-1, -1), (0, -1), (1, -1)] {
                let (nx, ny) = (x as isize + dx, y as isize + dy);
                if nx >= 0 && ny >= 0 && (nx as usize) < width {
                    let label = provisional[ny as usize * width + nx as usize];
                    if label != 0 {
                        neighbours[n] = label;
                        n += 1;
                    }
                }
            }
            let label = if n == 0 {
                let next = parent.len() as u32;
                parent.push(next);
                next
            } else {
                let mut root = find(&mut parent, neighbours[0]);
                for &other in &neighbours[1..n] {
                    let other_root = find(&mut parent, other);
                    if other_root != root {
                        let (lo, hi) = (root.min(other_root), root.max(other_root));
                        parent[hi as usize] = lo;
                        root = lo;
                    }
                }
                root
            };
            provisional[y * width + x] = label;
        }
    }

    let mut dense = vec![0u32; parent.len()];
    let mut count = 0u32;
    for label in 1..parent.len() as u32 {
        let root = find(&mut parent, label);
        if dense[root as usize] == 0 {
            count += 1;
            dense[root as usize] = count;
        }
        dense[label as usize] = dense[root as usize];
    }
    for value in provisional.iter_mut() {
        *value = dense[*value as usize];
    }
    Labels { map: provisional, count: count as usize }
}

/// Moore-neighbour tracing of one component's outer boundary, starting at its
/// top-most, left-most pixel (whose west neighbour is background).
fn trace(labels: &Labels, width: usize, height: usize, label: u32, sx: isize, sy: isize) -> Vec<(i32, i32)> {
    let is_member = |x: isize, y: isize| x >= 0 && y >= 0 && (x as usize) < width && (y as usize) < height && labels.map[y as usize * width + x as usize] == label;

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
