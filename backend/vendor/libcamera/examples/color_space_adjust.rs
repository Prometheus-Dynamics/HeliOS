//! Show cloning and adjusting color spaces without mutating the original.
use libcamera::{color_space::ColorSpace, pixel_format::PixelFormat};

fn main() {
    let src = ColorSpace::rec709();
    let pf = PixelFormat::parse("YUYV").expect("parse");
    println!("source color space: {}", src);
    let (adjusted, changed) = {
        let mut clone = src;
        let changed = clone.adjust_for_format(pf);
        (clone, changed)
    };
    println!(
        "{} for {pf:?}: {}",
        if changed { "adjusted" } else { "already compatible" },
        adjusted
    );
}
