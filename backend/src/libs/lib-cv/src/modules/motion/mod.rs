use image::{DynamicImage, GrayImage, Rgba};
use rayon::prelude::*;

/// Compute the absolute difference between two frames as a grayscale image.
pub fn frame_difference(prev: &DynamicImage, current: &DynamicImage) -> GrayImage {
    match (prev, current) {
        (DynamicImage::ImageLuma8(p), DynamicImage::ImageLuma8(c)) => {
            assert_eq!(p.dimensions(), c.dimensions());
            let (w, h) = p.dimensions();
            let mut out = GrayImage::new(w, h);
            let obuf = out.as_flat_samples_mut().samples;
            let pbuf = p.as_flat_samples().samples;
            let cbuf = c.as_flat_samples().samples;
            obuf.par_iter_mut().zip(pbuf.par_iter()).zip(cbuf.par_iter()).for_each(|((o, &a), &b)| *o = a.abs_diff(b));
            out
        }
        _ => {
            let p = prev.to_luma8();
            let c = current.to_luma8();
            assert_eq!(p.dimensions(), c.dimensions());
            let (w, h) = p.dimensions();
            let mut out = GrayImage::new(w, h);
            let obuf = out.as_flat_samples_mut().samples;
            let pbuf = p.as_flat_samples().samples;
            let cbuf = c.as_flat_samples().samples;
            obuf.par_iter_mut().zip(pbuf.par_iter()).zip(cbuf.par_iter()).for_each(|((o, &a), &b)| *o = a.abs_diff(b));
            out
        }
    }
}

/// Overlay a mask onto a frame using the provided color.
pub fn highlight_motion(frame: &DynamicImage, mask: &GrayImage, color: Rgba<u8>) -> DynamicImage {
    if mask.as_flat_samples().samples.iter().all(|&v| v == 0) {
        return frame.clone();
    }
    let mut rgba = match frame {
        DynamicImage::ImageRgba8(buf) => buf.clone(),
        other => other.to_rgba8(),
    };
    let fb = rgba.as_flat_samples_mut().samples;
    let mb = mask.as_flat_samples().samples;
    let alpha = color[3] as f32 / 255.0;
    let r = color[0] as f32;
    let g = color[1] as f32;
    let b = color[2] as f32;
    fb.par_chunks_mut(4).zip(mb.par_iter()).for_each(|(px, &m)| {
        if m != 0 {
            px[0] = ((px[0] as f32 * (1.0 - alpha) + r * alpha).round()) as u8;
            px[1] = ((px[1] as f32 * (1.0 - alpha) + g * alpha).round()) as u8;
            px[2] = ((px[2] as f32 * (1.0 - alpha) + b * alpha).round()) as u8;
        }
    });
    DynamicImage::ImageRgba8(rgba)
}

#[cfg(feature = "engine")]
pub mod nodes {
    use daedalus::declare_plugin;
    use daedalus::macros::node;
    use daedalus::runtime::NodeError;
    use image::{DynamicImage, GenericImageView, GrayImage, Rgba};

    use crate::modules::motion::{frame_difference, highlight_motion};

    #[node(id = "frame_diff", inputs("prev", "current"), outputs("mask"))]
    fn cv_frame_diff(prev: &DynamicImage, current: &DynamicImage) -> Result<GrayImage, NodeError> {
        if prev.dimensions() != current.dimensions() {
            return Err(NodeError::InvalidInput("frame_diff: mismatched dimensions".into()));
        }
        Ok(frame_difference(prev, current))
    }

    #[node(id = "highlight_motion", inputs("frame", "mask"), outputs("frame"))]
    fn cv_highlight_motion(frame: &DynamicImage, mask: &GrayImage) -> Result<DynamicImage, NodeError> {
        if frame.dimensions() != mask.dimensions() {
            return Err(NodeError::InvalidInput("highlight_motion: mismatched dimensions".into()));
        }
        Ok(highlight_motion(frame, mask, Rgba([255, 0, 0, 128])))
    }

    declare_plugin!(CvMotionPlugin, "motion", [cv_frame_diff, cv_highlight_motion]);
}
