//! Linear algebra helpers that back Helios pose estimation and sensor fusion.

use thiserror::Error;

pub mod matrix;
pub mod quaternion;
pub mod vector;

pub use matrix::{Mat3, Mat4};
pub use quaternion::Quaternion;
pub use vector::{Vec3, Vec4};

#[derive(Debug, Clone, Copy, PartialEq, Error)]
pub enum LinalgError {
    #[error("matrix is not invertible")]
    SingularMatrix,
    #[error("cannot normalize a zero-magnitude value")]
    ZeroMagnitude,
}
