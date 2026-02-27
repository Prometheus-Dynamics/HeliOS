use libcamera_sys::*;

/// Represents `libcamera::Point`
#[derive(Debug, Clone, Copy)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl From<libcamera_point_t> for Point {
    fn from(p: libcamera_point_t) -> Self {
        Self { x: p.x, y: p.y }
    }
}

/// Represents `libcamera::Size`
#[derive(Debug, Clone, Copy)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

impl Size {
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    pub fn align_down_to(self, h_alignment: u32, v_alignment: u32) -> Self {
        if h_alignment == 0 || v_alignment == 0 {
            return self;
        }
        Self {
            width: self.width / h_alignment * h_alignment,
            height: self.height / v_alignment * v_alignment,
        }
    }

    pub fn align_up_to(self, h_alignment: u32, v_alignment: u32) -> Self {
        if h_alignment == 0 || v_alignment == 0 {
            return self;
        }
        Self {
            width: self.width.div_ceil(h_alignment) * h_alignment,
            height: self.height.div_ceil(v_alignment) * v_alignment,
        }
    }

    pub fn bound_to(self, bound: Size) -> Self {
        Self {
            width: self.width.min(bound.width),
            height: self.height.min(bound.height),
        }
    }

    pub fn expand_to(self, expand: Size) -> Self {
        Self {
            width: self.width.max(expand.width),
            height: self.height.max(expand.height),
        }
    }

    pub fn grow_by(self, margins: Size) -> Self {
        Self {
            width: self.width.saturating_add(margins.width),
            height: self.height.saturating_add(margins.height),
        }
    }

    pub fn shrink_by(self, margins: Size) -> Self {
        Self {
            width: self.width.saturating_sub(margins.width),
            height: self.height.saturating_sub(margins.height),
        }
    }

    /// Bound this size down to match the aspect ratio of `ratio`.
    pub fn bounded_to_aspect_ratio(self, ratio: Size) -> Self {
        if ratio.width == 0 || ratio.height == 0 {
            return self;
        }

        let ratio1 = self.width as u64 * ratio.height as u64;
        let ratio2 = ratio.width as u64 * self.height as u64;

        if ratio1 > ratio2 {
            Self {
                width: (ratio2 / ratio.height as u64) as u32,
                height: self.height,
            }
        } else {
            Self {
                width: self.width,
                height: (ratio1 / ratio.width as u64) as u32,
            }
        }
    }

    /// Expand this size up to match the aspect ratio of `ratio`.
    pub fn expanded_to_aspect_ratio(self, ratio: Size) -> Self {
        if ratio.width == 0 || ratio.height == 0 {
            return self;
        }

        let ratio1 = self.width as u64 * ratio.height as u64;
        let ratio2 = ratio.width as u64 * self.height as u64;

        if ratio1 < ratio2 {
            Self {
                width: (ratio2 / ratio.height as u64) as u32,
                height: self.height,
            }
        } else {
            Self {
                width: self.width,
                height: (ratio1 / ratio.width as u64) as u32,
            }
        }
    }

    /// Center a rectangle of this size at the given point.
    pub fn centered_to(self, center: Point) -> Rectangle {
        let x = center.x - (self.width as i32 / 2);
        let y = center.y - (self.height as i32 / 2);
        Rectangle {
            x,
            y,
            width: self.width,
            height: self.height,
        }
    }
}

impl From<libcamera_size_t> for Size {
    fn from(s: libcamera_size_t) -> Self {
        Self {
            width: s.width,
            height: s.height,
        }
    }
}

impl From<Size> for libcamera_size_t {
    fn from(s: Size) -> Self {
        Self {
            width: s.width,
            height: s.height,
        }
    }
}

/// Represents `libcamera::SizeRange`
#[derive(Debug, Clone, Copy)]
pub struct SizeRange {
    pub min: Size,
    pub max: Size,
    pub h_step: u32,
    pub v_step: u32,
}

impl SizeRange {
    pub fn contains(&self, size: Size) -> bool {
        if size.width < self.min.width
            || size.width > self.max.width
            || size.height < self.min.height
            || size.height > self.max.height
        {
            return false;
        }

        if self.h_step != 0 {
            let delta_w = size.width - self.min.width;
            if !delta_w.is_multiple_of(self.h_step) {
                return false;
            }
        }
        if self.v_step != 0 {
            let delta_h = size.height - self.min.height;
            if !delta_h.is_multiple_of(self.v_step) {
                return false;
            }
        }

        true
    }
}

impl From<libcamera_size_range_t> for SizeRange {
    fn from(r: libcamera_size_range_t) -> Self {
        Self {
            min: r.min.into(),
            max: r.max.into(),
            h_step: r.hStep,
            v_step: r.vStep,
        }
    }
}

impl From<SizeRange> for libcamera_size_range_t {
    fn from(r: SizeRange) -> Self {
        Self {
            min: r.min.into(),
            max: r.max.into(),
            hStep: r.h_step,
            vStep: r.v_step,
        }
    }
}

/// Represents `libcamera::Rectangle`
#[derive(Debug, Clone, Copy)]
pub struct Rectangle {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rectangle {
    pub fn size(&self) -> Size {
        Size {
            width: self.width,
            height: self.height,
        }
    }

    /// Center point of the rectangle.
    pub fn center(&self) -> Point {
        Point {
            x: self.x.saturating_add((self.width / 2) as i32),
            y: self.y.saturating_add((self.height / 2) as i32),
        }
    }

    /// Top-left corner of the rectangle.
    pub fn top_left(&self) -> Point {
        Point { x: self.x, y: self.y }
    }

    /// Intersection of this rectangle with another.
    pub fn bounded_to(self, bound: Rectangle) -> Rectangle {
        let top_left_x = i64::from(self.x).max(i64::from(bound.x));
        let top_left_y = i64::from(self.y).max(i64::from(bound.y));
        let bottom_right_x =
            (i64::from(self.x) + i64::from(self.width)).min(i64::from(bound.x) + i64::from(bound.width));
        let bottom_right_y =
            (i64::from(self.y) + i64::from(self.height)).min(i64::from(bound.y) + i64::from(bound.height));

        let new_width = if bottom_right_x > top_left_x {
            (bottom_right_x - top_left_x) as u32
        } else {
            0
        };
        let new_height = if bottom_right_y > top_left_y {
            (bottom_right_y - top_left_y) as u32
        } else {
            0
        };

        Rectangle {
            x: top_left_x as i32,
            y: top_left_y as i32,
            width: new_width,
            height: new_height,
        }
    }

    /// Translate the rectangle so it remains enclosed within the given boundary.
    pub fn enclosed_in(self, boundary: Rectangle) -> Rectangle {
        let mut result = self.bounded_to(Rectangle {
            x: self.x,
            y: self.y,
            width: boundary.width,
            height: boundary.height,
        });

        let clamp = |val: i64, min: i64, max: i64| -> i64 {
            if val < min {
                min
            } else if val > max {
                max
            } else {
                val
            }
        };

        let max_x = i64::from(boundary.x) + i64::from(boundary.width) - i64::from(result.width);
        let max_y = i64::from(boundary.y) + i64::from(boundary.height) - i64::from(result.height);

        result.x = clamp(i64::from(result.x), i64::from(boundary.x), max_x) as i32;
        result.y = clamp(i64::from(result.y), i64::from(boundary.y), max_y) as i32;
        result
    }

    /// Return a translated rectangle.
    pub fn translated_by(self, delta: Point) -> Rectangle {
        Rectangle {
            x: self.x.saturating_add(delta.x),
            y: self.y.saturating_add(delta.y),
            width: self.width,
            height: self.height,
        }
    }

    /// Return a rectangle scaled by rational factors.
    pub fn scaled_by(self, numerator: Size, denominator: Size) -> Rectangle {
        if denominator.width == 0 || denominator.height == 0 {
            return self;
        }
        Rectangle {
            x: (i64::from(self.x) * i64::from(numerator.width) / i64::from(denominator.width)) as i32,
            y: (i64::from(self.y) * i64::from(numerator.height) / i64::from(denominator.height)) as i32,
            width: (u64::from(self.width) * u64::from(numerator.width) / u64::from(denominator.width)) as u32,
            height: (u64::from(self.height) * u64::from(numerator.height) / u64::from(denominator.height)) as u32,
        }
    }
}

impl core::ops::Mul<f32> for Size {
    type Output = Size;

    fn mul(self, rhs: f32) -> Self::Output {
        Size {
            width: (self.width as f32 * rhs) as u32,
            height: (self.height as f32 * rhs) as u32,
        }
    }
}

impl core::ops::Div<f32> for Size {
    type Output = Size;

    fn div(self, rhs: f32) -> Self::Output {
        Size {
            width: (self.width as f32 / rhs) as u32,
            height: (self.height as f32 / rhs) as u32,
        }
    }
}

impl core::ops::MulAssign<f32> for Size {
    fn mul_assign(&mut self, rhs: f32) {
        *self = *self * rhs;
    }
}

impl core::ops::DivAssign<f32> for Size {
    fn div_assign(&mut self, rhs: f32) {
        *self = *self / rhs;
    }
}

impl From<libcamera_rectangle_t> for Rectangle {
    fn from(r: libcamera_rectangle_t) -> Self {
        Self {
            x: r.x,
            y: r.y,
            width: r.width,
            height: r.height,
        }
    }
}

impl From<Rectangle> for libcamera_rectangle_t {
    fn from(r: Rectangle) -> Self {
        Self {
            x: r.x,
            y: r.y,
            width: r.width,
            height: r.height,
        }
    }
}

impl Size {
    /// Return a size with width and height swapped.
    pub fn transposed(self) -> Self {
        Size {
            width: self.height,
            height: self.width,
        }
    }
}
