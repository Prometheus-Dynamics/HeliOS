//! Detect markers in binary PGM files and print them as JSON lines.
//! `cargo run --release -p helios-vision --example detect_pgm -- 4x4_50 a.pgm b.pgm`

use helios_vision::aruco::{self, DecodeConfig, DetectorConfig, Dictionary};

mod common;
use common::read_pgm;

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
