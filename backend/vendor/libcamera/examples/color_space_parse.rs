//! Parse and adjust color spaces and pixel formats from strings.
use std::str::FromStr;

use libcamera::{color_space::ColorSpace, pixel_format::PixelFormat};

fn main() {
    let cs = ColorSpace::rec709();
    let cs_str = cs.to_repr();
    let pf_str = "YUYV";

    let mut cs = ColorSpace::from_string(&cs_str).expect("parse color space");
    let pf = PixelFormat::from_str(pf_str).expect("parse pixel format");

    println!("Parsed color space: {}", cs);
    println!("Parsed pixel format: {:?}", pf);

    let adjusted = cs.adjust_for_format(pf);
    println!(
        "{} color space for {}: {}",
        if adjusted { "Adjusted" } else { "Already compatible" },
        pf_str,
        cs
    );
}
