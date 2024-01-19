use std::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Sub, SubAssign};

use serde::{Deserialize, Serialize};

use crate::types::{ComponentOutOfRange, Pos2};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Vec2 {
	pub x: i32,
	pub y: i32,
}

impl Vec2 {
	pub const ZERO: Self = Self { x: 0, y: 0 };

	#[inline]
	#[must_use]
	pub const fn splat(v: i32) -> Self {
		Self { x: v, y: v }
	}

	#[inline]
	#[must_use]
	pub fn with_x(self, x: i32) -> Self {
		Self { x, ..self }
	}

	#[inline]
	#[must_use]
	pub fn with_y(self, y: i32) -> Self {
		Self { y, ..self }
	}
}

#[inline]
#[must_use]
pub const fn vec2(x: i32, y: i32) -> Vec2 {
	Vec2 { x, y }
}

impl Add<Vec2> for Vec2 {
	type Output = Self;

	#[inline]
	#[must_use]
	fn add(self, offset: Vec2) -> Self {
		Self {
			x: self.x + offset.x,
			y: self.y + offset.y,
		}
	}
}

impl AddAssign<Vec2> for Vec2 {
	#[inline]
	fn add_assign(&mut self, offset: Vec2) {
		self.x += offset.x;
		self.y += offset.y;
	}
}

impl Sub<Vec2> for Vec2 {
	type Output = Self;

	#[inline]
	#[must_use]
	fn sub(self, offset: Vec2) -> Self {
		Self {
			x: self.x - offset.x,
			y: self.y - offset.y,
		}
	}
}

impl SubAssign<Vec2> for Vec2 {
	#[inline]
	fn sub_assign(&mut self, offset: Vec2) {
		self.x -= offset.x;
		self.y -= offset.y;
	}
}

impl Mul<i32> for Vec2 {
	type Output = Self;

	#[inline]
	#[must_use]
	fn mul(self, scale: i32) -> Self {
		Self {
			x: self.x * scale,
			y: self.y * scale,
		}
	}
}

impl MulAssign<i32> for Vec2 {
	#[inline]
	fn mul_assign(&mut self, scale: i32) {
		self.x *= scale;
		self.y *= scale;
	}
}

impl Div<i32> for Vec2 {
	type Output = Self;

	#[inline]
	#[must_use]
	fn div(self, scale: i32) -> Self {
		Self {
			x: self.x / scale,
			y: self.y / scale,
		}
	}
}

impl DivAssign<i32> for Vec2 {
	#[inline]
	fn div_assign(&mut self, scale: i32) {
		self.x /= scale;
		self.y /= scale;
	}
}

impl Neg for Vec2 {
	type Output = Self;

	#[inline]
	#[must_use]
	fn neg(self) -> Self {
		Self {
			x: -self.x,
			y: -self.y,
		}
	}
}

impl Vec2 {
	#[inline]
	#[must_use]
	pub const fn to_pos(self) -> Pos2 {
		Pos2 {
			x: self.x,
			y: self.y,
		}
	}

	#[inline]
	#[must_use]
	pub const fn swap(self) -> Self {
		Self {
			x: self.y,
			y: self.x,
		}
	}

	#[inline]
	#[must_use]
	pub const fn is_empty(self) -> bool {
		self.x == 0 || self.y == 0
	}

	#[inline]
	#[must_use]
	pub fn abs(self) -> Self {
		Self {
			x: self.x.abs(),
			y: self.y.abs(),
		}
	}
}

impl From<embedded_graphics_core::geometry::Size> for Vec2 {
	fn from(lib: embedded_graphics_core::geometry::Size) -> Self {
		// For simplicity we assume that the size will never be so large that this conversion fails.
		Self {
			x: lib.width.try_into().unwrap(),
			y: lib.height.try_into().unwrap(),
		}
	}
}

impl TryFrom<Vec2> for embedded_graphics_core::geometry::Size {
	type Error = ComponentOutOfRange;

	fn try_from(our: Vec2) -> Result<Self, Self::Error> {
		Ok(Self {
			width: our.x.try_into().map_err(|_| ComponentOutOfRange)?,
			height: our.y.try_into().map_err(|_| ComponentOutOfRange)?,
		})
	}
}
