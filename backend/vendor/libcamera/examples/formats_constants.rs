//! Show generated libcamera pixel-format layout info for NV12 (if available).
use libcamera::{geometry::Size, pixel_format::PixelFormat};

fn main() {
    let fmt = PixelFormat::parse("NV12").unwrap_or_else(|| {
        eprintln!("NV12 not available in this libcamera build; pick another format");
        std::process::exit(0);
    });
    println!("Using NV12 => {fmt:?}");

    let size = Size {
        width: 640,
        height: 480,
    };
    if let Some(info) = fmt.info() {
        println!(
            "name={} bits_per_pixel={} planes={} pixels_per_group={}",
            info.name,
            info.bits_per_pixel,
            info.planes.len(),
            info.pixels_per_group
        );
        println!(
            "frame size for {}x{} (align=0): {} bytes",
            size.width,
            size.height,
            fmt.frame_size(size, 0)
        );
    } else {
        eprintln!("PixelFormatInfo unavailable; ensure libcamera headers are discoverable.");
    }
}
