use serde::{Deserialize, Serialize};
#[cfg(feature = "schema")]
use utoipa::ToSchema;

use lib_cv::Point;

#[cfg_attr(feature = "schema", derive(ToSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArucoDetection {
    pub id: u32,
    pub rotation: u8,
    pub border_width: u8,
    pub data_width: u8,
    pub corners: Vec<Point>,
}

#[cfg_attr(feature = "schema", derive(ToSchema))]
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct NormalizedBoundingBox {
    pub ymin: f32,
    pub xmin: f32,
    pub ymax: f32,
    pub xmax: f32,
}

impl NormalizedBoundingBox {
    #[must_use]
    pub fn clamp(self) -> Self {
        let (ymin, ymax) = if self.ymin <= self.ymax { (self.ymin, self.ymax) } else { (self.ymax, self.ymin) };
        let (xmin, xmax) = if self.xmin <= self.xmax { (self.xmin, self.xmax) } else { (self.xmax, self.xmin) };
        Self { ymin: ymin.clamp(0.0, 1.0), xmin: xmin.clamp(0.0, 1.0), ymax: ymax.clamp(0.0, 1.0), xmax: xmax.clamp(0.0, 1.0) }
    }
}

#[cfg_attr(feature = "schema", derive(ToSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisionDetection2D {
    pub bbox: NormalizedBoundingBox,
    pub score: f32,
    pub label: Option<String>,
    pub class_id: Option<u32>,
}
