//! Pixel format constants generated from the installed `libcamera/formats.h`.
//!
//! The names and values in this module mirror `libcamera::formats` for the
//! libcamera version detected at build time. Use these to avoid hand-rolling
//! fourcc/modifier pairs:
//!
//! ```text
//! // Example:
//! // let fmt = PixelFormat::parse("NV12").unwrap();
//! // let info = fmt.info().unwrap();
//! // assert_eq!(fmt.to_string(), info.name);
//! ```
include!(concat!(env!("OUT_DIR"), "/formats.rs"));
