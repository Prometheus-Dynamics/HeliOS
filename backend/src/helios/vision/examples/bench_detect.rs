//! Time the ArUco stages on a synthetic 1280x800 frame.
//! `cargo run --release -p helios-vision --example bench_detect`

use std::time::Instant;

use helios_vision::aruco::render::{paste_warped, render_marker};
use helios_vision::aruco::{self, DICT_4X4_50, DecodeConfig};
use helios_vision::image::GrayImage;
use helios_vision::quads::{QuadConfig, find_quads, scale_quads};
use helios_vision::threshold::{ThresholdConfig, adaptive_threshold};

fn main() {
    let mut scene = GrayImage::filled(1280, 800, 130);
    for (i, &(x, y)) in [(150.0f32, 120.0f32), (600.0, 300.0), (950.0, 500.0)].iter().enumerate() {
        let marker = render_marker(&DICT_4X4_50, i * 7, 20, 1).unwrap();
        paste_warped(&mut scene, &marker, [[x, y], [x + 160.0, y + 10.0], [x + 150.0, y + 170.0], [x - 5.0, y + 160.0]]);
    }
    // Texture so the threshold and contour stages see a realistic amount of edges.
    let mut state = 12345u32;
    for value in scene.data_mut() {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        *value = (*value as i32 + (state % 31) as i32 - 15).clamp(0, 255) as u8;
    }
    let decode = DecodeConfig { dictionary: &DICT_4X4_50, ..DecodeConfig::default() };
    let runs = 50;
    for decimate in [1u32, 2] {
        let (mut t_th, mut t_q, mut t_d, mut found) = (0.0, 0.0, 0.0, 0);
        for _ in 0..runs {
            let a = Instant::now();
            let small = if decimate == 2 { Some(scene.downscale2()) } else { None };
            let binary = adaptive_threshold(small.as_ref().unwrap_or(&scene), &ThresholdConfig::default());
            let b = Instant::now();
            let quads = scale_quads(&find_quads(&binary, &QuadConfig::default()), decimate as f32);
            let c = Instant::now();
            found = aruco::decode_quads(scene.view(), &quads, &decode).len();
            let d = Instant::now();
            t_th += (b - a).as_secs_f64();
            t_q += (c - b).as_secs_f64();
            t_d += (d - c).as_secs_f64();
        }
        let ms = |t: f64| t * 1e3 / runs as f64;
        println!("1280x800 decimate {decimate}: threshold {:.2} ms, find_quads {:.2} ms, decode {:.2} ms, total {:.2} ms, markers {found}", ms(t_th), ms(t_q), ms(t_d), ms(t_th + t_q + t_d));
    }
}
