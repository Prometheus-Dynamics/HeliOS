use super::render::{paste_warped, render_marker};
use super::*;
use crate::geometry::distance;

/// Deterministic xorshift noise in `-amplitude..=amplitude`.
fn add_noise(image: &mut GrayImage, amplitude: i32, seed: u32) {
    let mut state = seed.max(1);
    for value in image.data_mut() {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        let delta = (state % (2 * amplitude as u32 + 1)) as i32 - amplitude;
        *value = (*value as i32 + delta).clamp(0, 255) as u8;
    }
}

fn scene(width: usize, height: usize, placements: &[(&Dictionary, usize, [Point; 4])]) -> GrayImage {
    let mut canvas = GrayImage::filled(width, height, 150);
    for &(dict, id, corners) in placements {
        let marker = render_marker(dict, id, 16, 1).unwrap();
        assert!(paste_warped(&mut canvas, &marker, corners));
    }
    canvas
}

/// The marker's own corners inside a quiet-zone placement: the quiet zone is
/// one cell of `n + 2` cells per side.
fn marker_corners(dict: &Dictionary, placement: [Point; 4]) -> [Point; 4] {
    let n = dict.total_width() as f32 + 2.0;
    let side = n;
    let h = crate::geometry::Homography::from_points([[0.0, 0.0], [side, 0.0], [side, side], [0.0, side]], placement).unwrap();
    [h.map([1.0, 1.0]), h.map([n - 1.0, 1.0]), h.map([n - 1.0, n - 1.0]), h.map([1.0, n - 1.0])]
}

fn config(dict: &'static Dictionary) -> DetectorConfig {
    DetectorConfig { decode: DecodeConfig { dictionary: dict, ..DecodeConfig::default() }, ..DetectorConfig::default() }
}

fn assert_corners_near(found: &[Point; 4], expected: &[Point; 4], tolerance: f32) {
    for i in 0..4 {
        assert!(distance(found[i], expected[i]) < tolerance, "corner {i}: found {:?}, expected {:?}", found[i], expected[i]);
    }
}

#[test]
fn rendered_marker_round_trips_through_cells() {
    for id in [0, 7, 49] {
        let cells = render::marker_cells(&DICT_4X4_50, id).unwrap();
        // Border is black.
        assert!(!cells[0] && !cells[5] && !cells[30] && !cells[35]);
    }
    assert!(render::marker_cells(&DICT_4X4_50, 50).is_none());
}

#[test]
fn detects_axis_aligned_marker_with_top_left_first() {
    let placement = [[100.0, 80.0], [260.0, 80.0], [260.0, 240.0], [100.0, 240.0]];
    let image = scene(480, 360, &[(&DICT_4X4_50, 7, placement)]);
    let markers = detect(&image, &config(&DICT_4X4_50));
    assert_eq!(markers.len(), 1, "{markers:?}");
    assert_eq!(markers[0].id, 7);
    assert_eq!(markers[0].hamming, 0);
    assert_corners_near(&markers[0].corners, &marker_corners(&DICT_4X4_50, placement), 2.5);
}

#[test]
fn rotation_is_resolved_for_every_orientation() {
    let base = [[120.0, 90.0], [280.0, 90.0], [280.0, 250.0], [120.0, 250.0]];
    for turn in 0..4 {
        // Rotating the placement order rotates the printed marker in the image.
        let placement = [base[turn], base[(turn + 1) % 4], base[(turn + 2) % 4], base[(turn + 3) % 4]];
        let image = scene(480, 360, &[(&DICT_4X4_50, 23, placement)]);
        let markers = detect(&image, &config(&DICT_4X4_50));
        assert_eq!(markers.len(), 1, "turn {turn}: {markers:?}");
        assert_eq!(markers[0].id, 23, "turn {turn}");
        assert_corners_near(&markers[0].corners, &marker_corners(&DICT_4X4_50, placement), 2.5);
    }
}

#[test]
fn perspective_and_noise_are_tolerated() {
    let placement = [[150.0, 70.0], [330.0, 110.0], [310.0, 280.0], [130.0, 250.0]];
    let mut image = scene(480, 360, &[(&DICT_4X4_50, 42, placement)]);
    add_noise(&mut image, 25, 7);
    let markers = detect(&image, &config(&DICT_4X4_50));
    assert_eq!(markers.len(), 1, "{markers:?}");
    assert_eq!(markers[0].id, 42);
    assert_corners_near(&markers[0].corners, &marker_corners(&DICT_4X4_50, placement), 3.0);
}

#[test]
fn several_markers_in_one_frame() {
    let placements = [
        (&DICT_4X4_50, 3usize, [[40.0, 40.0], [160.0, 40.0], [160.0, 160.0], [40.0, 160.0]]),
        (&DICT_4X4_50, 11, [[240.0, 60.0], [380.0, 80.0], [360.0, 210.0], [230.0, 190.0]]),
        (&DICT_4X4_50, 49, [[80.0, 220.0], [190.0, 230.0], [180.0, 340.0], [70.0, 330.0]]),
    ];
    let image = scene(480, 360, &placements);
    let mut ids: Vec<u32> = detect(&image, &config(&DICT_4X4_50)).iter().map(|m| m.id).collect();
    ids.sort();
    assert_eq!(ids, vec![3, 11, 49]);
}

#[test]
fn apriltag_36h11_markers_decode() {
    for id in [0usize, 1, 300, 586] {
        let placement = [[100.0, 60.0], [300.0, 70.0], [290.0, 270.0], [90.0, 260.0]];
        let image = scene(420, 340, &[(&TAG_36H11, id, placement)]);
        let markers = detect(&image, &config(&TAG_36H11));
        assert_eq!(markers.len(), 1, "id {id}: {markers:?}");
        assert_eq!(markers[0].id, id as u32);
        assert_eq!(markers[0].dictionary, "36h11");
    }
}

#[test]
fn small_marker_is_found() {
    // About 6 px per cell at 4x4 (6 cells incl. border).
    let placement = [[200.0, 150.0], [248.0, 150.0], [248.0, 198.0], [200.0, 198.0]];
    let image = scene(480, 360, &[(&DICT_4X4_50, 5, placement)]);
    let markers = detect(&image, &config(&DICT_4X4_50));
    assert_eq!(markers.iter().map(|m| m.id).collect::<Vec<_>>(), vec![5]);
}

#[test]
fn no_false_positives_on_noise_or_blank_frames() {
    let blank = GrayImage::filled(320, 240, 128);
    assert!(detect(&blank, &config(&DICT_4X4_50)).is_empty());

    let mut noise = GrayImage::filled(320, 240, 128);
    add_noise(&mut noise, 120, 99);
    assert!(detect(&noise, &config(&DICT_4X4_50)).is_empty());
    assert!(detect(&noise, &config(&TAG_36H11)).is_empty());
}

#[test]
fn plain_black_square_is_not_a_marker() {
    let mut image = GrayImage::filled(320, 240, 200);
    for y in 60..160 {
        for x in 100..200 {
            image.set(x, y, 20);
        }
    }
    assert!(detect(&image, &config(&DICT_4X4_50)).is_empty());
}

#[test]
fn dictionary_lookup_by_name() {
    assert_eq!(Dictionary::by_name("DICT_4X4_50").map(|d| d.name), Some("4x4_50"));
    assert_eq!(Dictionary::by_name("tag36h11").map(|d| d.name), Some("36h11"));
    assert!(Dictionary::by_name("5x5_100").is_none());
}
