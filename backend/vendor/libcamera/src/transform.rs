use libcamera_sys::*;

use crate::camera::Orientation;

/// 2D plane transform matching libcamera::Transform.
#[derive(Clone, Copy, Debug)]
pub struct Transform(pub libcamera_transform_t);

impl Transform {
    pub fn hflip() -> Self {
        Transform(unsafe { libcamera_transform_hflip() })
    }
    pub fn vflip() -> Self {
        Transform(unsafe { libcamera_transform_vflip() })
    }
    pub fn transpose() -> Self {
        Transform(unsafe { libcamera_transform_transpose() })
    }
}

impl Transform {
    pub fn identity() -> Self {
        Transform(unsafe { libcamera_transform_identity() })
    }

    /// Construct from rotation degrees, optionally applying hflip.
    pub fn from_rotation(angle: i32, hflip: bool) -> Option<Self> {
        let mut success = false;
        let t = unsafe { libcamera_transform_from_rotation(angle, hflip, &mut success) };
        if success {
            Some(Transform(t))
        } else {
            None
        }
    }

    pub fn inverse(self) -> Self {
        Transform(unsafe { libcamera_transform_inverse(self.0) })
    }

    pub fn combine(self, other: Transform) -> Self {
        Transform(unsafe { libcamera_transform_combine(self.0, other.0) })
    }

    pub fn to_string_repr(self) -> String {
        unsafe {
            let ptr = libcamera_transform_to_string(self.0);
            if ptr.is_null() {
                return String::new();
            }
            let s = std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned();
            libc::free(ptr.cast());
            s
        }
    }
}

impl std::fmt::Display for Transform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_string_repr())
    }
}

impl Transform {
    /// Compute the transform between two orientations (equivalent to libcamera Orientation division).
    pub fn between_orientations(from: Orientation, to: Orientation) -> Self {
        Transform(unsafe { libcamera_transform_between_orientations(from.into(), to.into()) })
    }
}

pub fn apply_transform_to_orientation(orientation: Orientation, transform: Transform) -> Orientation {
    unsafe {
        libcamera_transform_apply_orientation(orientation.into(), transform.0)
            .try_into()
            .unwrap()
    }
}

/// Convert a rotation angle to an EXIF orientation using libcamera's helper.
pub fn orientation_from_rotation(angle: i32) -> Option<Orientation> {
    let mut success = false;
    let ori = unsafe { libcamera_orientation_from_rotation(angle, &mut success) };
    if success {
        ori.try_into().ok()
    } else {
        None
    }
}

impl core::ops::BitOr for Transform {
    type Output = Transform;
    fn bitor(self, rhs: Self) -> Self::Output {
        Transform(unsafe { libcamera_transform_or(self.0, rhs.0) })
    }
}

impl core::ops::BitAnd for Transform {
    type Output = Transform;
    fn bitand(self, rhs: Self) -> Self::Output {
        Transform(unsafe { libcamera_transform_and(self.0, rhs.0) })
    }
}

impl core::ops::BitXor for Transform {
    type Output = Transform;
    fn bitxor(self, rhs: Self) -> Self::Output {
        Transform(unsafe { libcamera_transform_xor(self.0, rhs.0) })
    }
}

impl core::ops::Not for Transform {
    type Output = Transform;
    fn not(self) -> Self::Output {
        Transform(unsafe { libcamera_transform_not(self.0) })
    }
}
