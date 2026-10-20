//! 8-bit grey frames for Eidos. A raw recording's frames go through Styx's software ISP, its luma
//! output: each pixel is the mosaic's `(R + 2G + B) / 4` (a 3x3 binomial filter over the Bayer
//! cells), with no demosaic and no colour correction, which is what the replay's detectors want.
//! A luma recording's frames are already grey and pass through.

use styx::{
    core::prelude::FrameLease,
    prelude::{FourCc, MediaFormat},
};
use styx_softisp::{IspParams, OutputBuffers, RawFormat, Scale, SoftIsp};

use crate::error::RecorderError;

pub enum Converter {
    /// Raw Bayer frames of `width` x `height` (16-bit samples), through the software ISP.
    Raw { isp: Box<SoftIsp>, width: usize, height: usize },
    /// Grey frames of `width` x `height` (8-bit samples), as they are.
    Grey { width: usize, height: usize },
}

impl Converter {
    pub fn new(format: MediaFormat) -> Result<Self, RecorderError> {
        let (width, height) = (format.resolution.width.get() as usize, format.resolution.height.get() as usize);
        if format.code == FourCc::GREY {
            return Ok(Self::Grey { width, height });
        }
        let raw = RawFormat::from_fourcc(format.code, width as u32, height as u32).ok_or_else(|| RecorderError::Mcap(format!("no grey conversion for {}", format.code)))?;
        let isp = SoftIsp::new(raw, IspParams::default()).map_err(|error| RecorderError::Mcap(error.to_string()))?;
        Ok(Self::Raw { isp: Box::new(isp), width, height })
    }

    /// One frame as `width * height` grey bytes.
    pub fn convert(&mut self, frame: &FrameLease) -> Result<Vec<u8>, RecorderError> {
        let bytes = frame.to_visible_vec().map_err(|error| RecorderError::Mcap(error.to_string()))?;
        match self {
            Self::Grey { width, height } => {
                if bytes.len() != *width * *height {
                    return Err(RecorderError::Mcap(format!("a grey frame has {} bytes, not {}x{}", bytes.len(), width, height)));
                }
                Ok(bytes)
            }
            Self::Raw { isp, width, height } => {
                let mut grey = vec![0u8; *width * *height];
                isp.process(&bytes, *width * 2, Scale::Full, OutputBuffers::Luma { data: &mut grey, stride: *width }).map_err(|error| RecorderError::Mcap(error.to_string()))?;
                Ok(grey)
            }
        }
    }
}
