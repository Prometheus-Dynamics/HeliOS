use image::DynamicImage;

#[derive(Debug, Clone, Copy)]
pub enum Rotation {
    Deg0,
    Deg90,
    Deg180,
    Deg270,
}

pub fn rotate_fast(image: DynamicImage, rotation: Rotation) -> DynamicImage {
    match rotation {
        Rotation::Deg0 => image,
        Rotation::Deg90 => image.rotate90(),
        Rotation::Deg180 => image.rotate180(),
        Rotation::Deg270 => image.rotate270(),
    }
}
