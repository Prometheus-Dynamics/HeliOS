use lib_cv::modules::aruco::tag::dictionary::{Dict4x4_50, Dictionary, mm_to_px, px_to_mm};
use lib_cv::modules::aruco::tag::{ArucoTagDecoding, dictionary::family::Family16H5};

#[test]
fn marker_4x4_basic() {
    let side = 60;
    let border = 1;
    let img = Dict4x4_50::draw_marker(0, side, border).expect("marker");
    assert_eq!(img.width(), side);
    assert_eq!(img.height(), side);
    let cell = side / (4 + 2 * border as u32);
    // Check border pixel
    assert_eq!(img.get_pixel(0, 0)[0], 0);

    // Validate bits for id 0 (code [181, 50]) at cell centers
    let code = [181u8, 50u8];
    for y in 0..4 {
        for x in 0..4 {
            let idx = y * 4 + x;
            let byte = if idx < 8 { code[0] } else { code[1] };
            let shift = 7 - (idx % 8);
            let bit = (byte >> shift) & 1;
            let expected = if bit == 1 { 255 } else { 0 };
            let px_x = (x as u32 + border as u32) * cell + cell / 2;
            let px_y = (y as u32 + border as u32) * cell + cell / 2;
            assert_eq!(img.get_pixel(px_x, px_y)[0], expected);
        }
    }
}

#[test]
fn marker_4x4_invalid() {
    assert!(Dict4x4_50::draw_marker(50, 60, 1).is_none());
    assert!(Dict4x4_50::draw_marker(0, 60, 0).is_none());
    assert!(Dict4x4_50::draw_marker(0, 5, 1).is_none());
}

#[test]
fn charuco_board_basic() {
    // marker length must be a multiple of 6 so generated markers fit exactly
    let board = Dict4x4_50::draw_charuco_board(2, 2, 20, 12);
    assert_eq!(board.width(), 40);
    assert_eq!(board.height(), 40);
    // Board should contain both black and white pixels
    let mut has_black = false;
    let mut has_white = false;
    for p in board.pixels() {
        if p[0] == 0 {
            has_black = true;
        }
        if p[0] == 255 {
            has_white = true;
        }
    }
    assert!(has_black && has_white);
}

#[test]
fn mm_px_conversion() {
    let dpi = 300.0;
    let mm = 50.0;
    let px = mm_to_px(mm, dpi).expect("mm to px");
    let mm_back = px_to_mm(px, dpi).unwrap();
    assert!((mm_back - mm).abs() < 0.1);
    assert!(mm_to_px(mm, 0.0).is_none());
    assert!(px_to_mm(px, 0.0).is_none());
}

#[test]
fn marker_set_generation() {
    let set = Dict4x4_50::generate_marker_set(60, 1);
    assert_eq!(set.len(), Dict4x4_50::marker_count());
    let (id0, img0) = &set[0];
    assert_eq!(*id0, 0);
    let img_single = Dict4x4_50::draw_marker(0, 60, 1).unwrap();
    assert_eq!(img0.dimensions(), img_single.dimensions());
    assert_eq!(img0.as_raw(), img_single.as_raw());
}

#[test]
fn marker_set_mm_generation() {
    let dpi = 300.0;
    let set = Dict4x4_50::generate_marker_set_mm(50.0, dpi, 1).expect("gen");
    assert_eq!(set.len(), Dict4x4_50::marker_count());
}

#[test]
fn aruco_tag_marker_basic() {
    let fam = Family16H5;
    let img = fam.draw_marker(0, 60).expect("tag");
    assert_eq!(img.width(), 60);
    assert_eq!(img.height(), 60);
    let mut has_black = false;
    let mut has_white = false;
    for p in img.pixels() {
        if p[0] == 0 {
            has_black = true;
        }
        if p[0] == 255 {
            has_white = true;
        }
    }
    assert!(has_black && has_white);
}

#[test]
fn aruco_tag_charuco_board() {
    let fam = Family16H5;
    let board = fam.draw_charuco_board(2, 2, 20, 12);
    assert_eq!(board.width(), 40);
    assert_eq!(board.height(), 40);
}
