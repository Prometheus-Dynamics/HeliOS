use std::io::Cursor;

use anyhow::{Context, Result, anyhow};
use image::{GenericImageView, ImageFormat};

use crate::api_tools_protocol::ToolCropRect;

pub(super) fn edit_image_bytes(bytes: &[u8], content_type: &str, rotate_degrees: Option<i32>, crop: Option<ToolCropRect>) -> Result<(Vec<u8>, u32, u32)> {
    let mut image = image::load_from_memory(bytes).context("failed to decode image")?;
    if let Some(crop) = crop {
        let (w, h) = image.dimensions();
        if crop.width == 0 || crop.height == 0 || crop.x >= w || crop.y >= h {
            return Err(anyhow!("invalid crop rectangle"));
        }
        let crop_w = crop.width.min(w - crop.x);
        let crop_h = crop.height.min(h - crop.y);
        image = image.crop_imm(crop.x, crop.y, crop_w, crop_h);
    }

    let rotation = rotate_degrees.unwrap_or(0).rem_euclid(360);
    if rotation != 0 {
        image = match rotation {
            90 => image.rotate90(),
            180 => image.rotate180(),
            270 => image.rotate270(),
            _ => return Err(anyhow!("rotation must be 0/90/180/270")),
        };
    }

    let (width, height) = image.dimensions();
    let mut out = Vec::new();
    let format = if content_type == "image/png" { ImageFormat::Png } else { ImageFormat::Jpeg };
    image.write_to(&mut Cursor::new(&mut out), format).context("failed to encode image")?;
    Ok((out, width, height))
}
