//! `helios-sim-scene`: builds the glTF scene a simulated camera renders (Styx's `scene=gltf`)
//! from a field layout, a camera path and a lens. Dev only: tools/sim uses it, no board image does.
//!
//! Writes the `.glb` to `--out` and prints the lens keys of Styx's spec on stdout, e.g.
//! `lens=fisheye,lens_width=1280,...` (`tools/sim/up.sh` joins them into `STYX_SIM_CAMERAS`).

mod geometry;
mod glb;
mod scene;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use helios_field::{FRC_TAG_SIDE_M, FieldLayout};

#[derive(Parser)]
#[command(name = "helios-sim-scene", about = "Build a glTF scene of AprilTags for Styx's simulated camera")]
struct Args {
    /// The field layout: a WPILib AprilTag field JSON, or a Limelight .fmap.
    #[arg(long)]
    layout: PathBuf,
    /// The camera: `{"pose": {x, y, z, roll, pitch, yaw}}` or `{"trajectory": [{"t": ..., ...}, ...]}`
    /// (WPILib: X forward, Y left, Z up; metres and degrees; the camera looks along its +X).
    #[arg(long)]
    camera: PathBuf,
    /// HeliOS's calibration JSON: the lens and the image size.
    #[arg(long)]
    calibration: PathBuf,
    /// The `.glb` to write.
    #[arg(long)]
    out: PathBuf,
    /// The black square's side for a WPILib layout (metres; a .fmap has its own sizes).
    #[arg(long, default_value_t = FRC_TAG_SIDE_M)]
    tag_size: f64,
}

fn main() -> Result<()> {
    let args = Args::parse();
    ensure_positive(args.tag_size)?;
    let layout_text = std::fs::read_to_string(&args.layout).with_context(|| format!("reading {}", args.layout.display()))?;
    let (layout, _format): (FieldLayout, _) =
        FieldLayout::parse(&layout_text, args.tag_size).with_context(|| format!("reading the layout {}", args.layout.display()))?;
    let camera = scene::CameraPlan::read(&args.camera)?;
    let calibration_text = std::fs::read_to_string(&args.calibration).with_context(|| format!("reading {}", args.calibration.display()))?;
    let calibration: scene::Calibration =
        serde_json::from_str(&calibration_text).with_context(|| format!("reading the calibration {}", args.calibration.display()))?;

    let glb = scene::build(&layout, &camera, &calibration)?;
    std::fs::write(&args.out, &glb).with_context(|| format!("writing {}", args.out.display()))?;
    eprintln!("[sim-scene] {} tags, {} bytes -> {}", layout.tags.len(), glb.len(), args.out.display());
    println!("{}", calibration.lens_spec()?);
    Ok(())
}

fn ensure_positive(tag_size: f64) -> Result<()> {
    anyhow::ensure!(tag_size.is_finite() && tag_size > 0.0, "--tag-size must be a positive length");
    Ok(())
}
