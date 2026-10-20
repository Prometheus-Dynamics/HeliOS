//! `helios-recorder`: recordings offline. `export --eidos <recording.mcap> <outdir>` writes Eidos's
//! replay input (see `helios_recorder::export`).

use std::{path::Path, process::ExitCode};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [command, flag, recording, outdir] if command == "export" && flag == "--eidos" => match helios_recorder::export::export_eidos(Path::new(recording), Path::new(outdir)) {
            Ok(summary) => {
                println!(
                    "exported {} frames ({}x{}, camera {}) to {}, {} and {}",
                    summary.frames,
                    summary.width,
                    summary.height,
                    summary.camera_status,
                    summary.raw.display(),
                    summary.replay.display(),
                    summary.frames_file.display()
                );
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("helios-recorder: {error}");
                ExitCode::FAILURE
            }
        },
        _ => {
            eprintln!("usage: helios-recorder export --eidos <recording.mcap> <outdir>");
            ExitCode::from(2)
        }
    }
}
