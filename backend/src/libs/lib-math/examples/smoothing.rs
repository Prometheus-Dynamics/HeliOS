use lib_math::{Filter, LowPassConfig, LowPassFilter};

fn main() {
    let mut filter = LowPassFilter::<1>::from_config(LowPassConfig { alpha: 0.25 }).expect("valid config");
    for sample in [0.0_f32, 10.0, 20.0, 30.0] {
        let output = filter.update([sample])[0];
        println!("input={sample:.1}, filtered={output:.2}");
    }
}
