//! Configure logging to stdout without creating a CameraManager.
use libcamera::logging::{configure_stdout, LoggingLevel};

fn main() {
    // Direct libcamera logs to stdout and set the Camera category to INFO.
    configure_stdout("Camera", LoggingLevel::Info, false).expect("configure stdout logging");
    println!("Logging configured for category \"Camera\" to stdout at INFO level.");
}
