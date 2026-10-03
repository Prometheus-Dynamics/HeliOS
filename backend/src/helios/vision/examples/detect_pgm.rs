//! Detect markers in binary PGM files and print them as JSON lines.
//! `cargo run --release -p helios-vision --example detect_pgm -- 4x4_50 a.pgm b.pgm`

use helios_vision::aruco::{self, DecodeConfig, DetectorConfig, Dictionary};
use helios_vision::image::GrayImage;

fn read_pgm(path: &str) -> GrayImage {
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

fn main() {
    let mut args = std::env::args().skip(1);
    let dictionary = Dictionary::by_name(&args.next().expect("dictionary")).expect("known dictionary");
    let config = DetectorConfig { decode: DecodeConfig { dictionary, ..DecodeConfig::default() }, ..DetectorConfig::default() };
    for path in args {
        let gray = read_pgm(&path);
        let markers = aruco::detect(&gray, &config);
        let list: Vec<String> = markers.iter().map(|m| format!("[{},{}]", m.id, m.corners.iter().map(|c| format!("[{:.2},{:.2}]", c[0], c[1])).collect::<Vec<_>>().join(","))).collect();
        println!("{{\"file\":\"{path}\",\"markers\":[{}]}}", list.join(","));
    }
}
