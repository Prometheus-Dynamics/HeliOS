//! Demonstrates round-tripping PixelFormat fourcc/modifier pairs.
use libcamera::pixel_format::PixelFormat;

fn main() {
    let fmt = PixelFormat::parse("XRGB8888").expect("parse");
    let (fourcc, modifier) = fmt.to_raw();
    let rebuilt = PixelFormat::from_raw_parts(fourcc, modifier);
    println!("original={fmt:?} fourcc=0x{fourcc:08x} modifier=0x{modifier:016x}");
    println!("rebuilt equals original? {}", rebuilt == fmt);
}
