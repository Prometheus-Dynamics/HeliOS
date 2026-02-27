//! Simple 2D point type used by computer-vision nodes.
//!
//! ```
//! use lib_cv::Point;
//!
//! let p: Point = (1.2, 3.4).into();
//! let geo: geo::Point<f64> = p.into();
//! assert_eq!(geo.x(), 1.2);
//! ```

use geo::Point as GeoPoint;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[cfg_attr(feature = "engine", derive(daedalus_macros::DaedalusTypeExpr, daedalus_macros::DaedalusToValue))]
#[cfg_attr(feature = "engine", daedalus(type_key = "cv:point2d"))]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl From<(f64, f64)> for Point {
    fn from(value: (f64, f64)) -> Self {
        Self { x: value.0, y: value.1 }
    }
}

impl From<[f64; 2]> for Point {
    fn from(value: [f64; 2]) -> Self {
        Self { x: value[0], y: value[1] }
    }
}

impl From<GeoPoint<f64>> for Point {
    fn from(value: GeoPoint<f64>) -> Self {
        Self { x: value.x(), y: value.y() }
    }
}

impl From<Point> for GeoPoint<f64> {
    fn from(value: Point) -> Self {
        GeoPoint::new(value.x, value.y)
    }
}
