use super::*;

#[node(
    id = "decode_grid",
    inputs(
        "image",
        port(name = "contour", source = "Contour", ty = crate::daedalus_types::contour()),
        port(name = "dictionary", default = "apriltag_16h5")
    ),
    outputs(port(name = "grid", source = "BinaryImage", ty = crate::daedalus_types::binary_image()))
)]
fn cv_decode_grid(image: &GrayImage, contour: &Vec<Point>, dictionary: ArucoDictionaryKind) -> Result<BinaryImage, NodeError> {
    let mut out = BinaryImage::new(0, 0);
    if contour.len() < 3 {
        return Ok(out);
    }

    let Some(family) = dictionary.as_apriltag_family() else {
        return Err(NodeError::InvalidInput("decode_grid requires an apriltag dictionary".into()));
    };
    let coords: Vec<(f32, f32)> = contour.iter().map(|p| (p.x as f32, p.y as f32)).collect();
    let poly = Polygon::new(LineString::from(coords.clone()), vec![]);
    if let Some(rect) = poly.minimum_rotated_rect() {
        let rect_points: Vec<Point> = rect.exterior().points().map(|c| Point { x: c.x() as f64, y: c.y() as f64 }).collect();

        if rect_points.len() == 4 {
            let tag_family = family.into_family();
            let side = tag_family.total_width() as u32;
            let from = [
                (rect_points[0].x as f32, rect_points[0].y as f32),
                (rect_points[1].x as f32, rect_points[1].y as f32),
                (rect_points[2].x as f32, rect_points[2].y as f32),
                (rect_points[3].x as f32, rect_points[3].y as f32),
            ];
            let to = [(0.0, 0.0), (side as f32, 0.0), (side as f32, side as f32), (0.0, side as f32)];
            if let Some(proj) = Projection::from_control_points(from, to) {
                let mut warped = GrayImage::new(side, side);
                warp_into(image, &proj, Interpolation::Bilinear, Luma([0u8]), &mut warped);
                let grid_vec = decode_marker_grid(&warped, &tag_family).unwrap_or_default();
                let mut img = GrayImage::new(side, side);
                for (y, row) in grid_vec.iter().enumerate() {
                    for (x, &bit) in row.iter().enumerate() {
                        let v = if bit == 0 { 0 } else { 255 };
                        img.put_pixel(x as u32, y as u32, Luma([v]));
                    }
                }
                out = BinaryImage::from_gray(&img);
            }
        }
    }

    Ok(out)
}
