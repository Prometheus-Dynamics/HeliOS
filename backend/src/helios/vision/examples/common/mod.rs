//! Helpers shared by the examples.

use helios_vision::image::GrayImage;

/// Read a binary (P5) PGM file.
pub fn read_pgm(path: &str) -> GrayImage {
    let bytes = std::fs::read(path).expect("read pgm");
    let mut fields = Vec::new();
    let mut i = 0;
    while fields.len() < 4 {
        while bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if bytes[i] == b'#' {
            while bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        let start = i;
        while !bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        fields.push(String::from_utf8_lossy(&bytes[start..i]).to_string());
    }
    assert_eq!(fields[0], "P5", "only binary PGM is supported");
    let (width, height): (usize, usize) = (fields[1].parse().unwrap(), fields[2].parse().unwrap());
    GrayImage::new(width, height, bytes[i + 1..i + 1 + width * height].to_vec()).expect("pgm size")
}
