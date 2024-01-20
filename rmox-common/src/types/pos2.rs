use std::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Sub, SubAssign};

use serde::{Deserialize, Serialize};

use crate::types::{Rectangle, Side, Vec2};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Pos2 {
	pub x: i32,
	pub y: i32,
}

#[inline]
#[must_use]
pub const fn pos2(x: i32, y: i32) -> Pos2 {
	Pos2 { x, y }
}

impl Pos2 {
	pub const ZERO: Self = Self { x: 0, y: 0 };

	#[inline]
	#[must_use]
	pub const fn splat(v: i32) -> Self {
		Self { x: v, y: v }
	}

	#[inline]
	#[must_use]
	pub const fn to_vec(self) -> Vec2 {
		Vec2 {
			x: self.x,
			y: self.y,
		}
	}

	#[inline]
	#[must_use]
	pub fn min_components(self, other: Self) -> Self {
		Self {
			x: std::cmp::min(self.x, other.x),
			y: std::cmp::min(self.y, other.y),
		}
	}

	#[inline]
	#[must_use]
	pub fn max_components(self, other: Self) -> Self {
		Self {
			x: std::cmp::max(self.x, other.x),
			y: std::cmp::max(self.y, other.y),
		}
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

	#[inline]
	#[must_use]
	pub fn offset(self, toward: Side, offset: i32) -> Self {
		self + toward.vec_toward() * offset
	}

	#[inline]
	#[must_use]
	pub fn wrap_within(self, container: &Rectangle) -> Self {
		let mut offset = self - container.origin;
		offset.x = offset.x.rem_euclid(container.size.x);
		offset.y = offset.y.rem_euclid(container.size.y);
		container.origin + offset
	}
}

impl Add<Vec2> for Pos2 {
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

impl AddAssign<Vec2> for Pos2 {
	#[inline]
	fn add_assign(&mut self, offset: Vec2) {
		self.x += offset.x;
		self.y += offset.y;
	}
}

impl Sub<Vec2> for Pos2 {
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

impl SubAssign<Vec2> for Pos2 {
	#[inline]
	fn sub_assign(&mut self, offset: Vec2) {
		self.x -= offset.x;
		self.y -= offset.y;
	}
}

impl Sub<Pos2> for Pos2 {
	type Output = Vec2;

	#[inline]
	#[must_use]
	fn sub(self, from: Pos2) -> Vec2 {
		Vec2 {
			x: self.x - from.x,
			y: self.y - from.y,
		}
	}
}

impl Mul<i32> for Pos2 {
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

impl MulAssign<i32> for Pos2 {
	#[inline]
	fn mul_assign(&mut self, scale: i32) {
		self.x *= scale;
		self.y *= scale;
	}
}

impl Div<i32> for Pos2 {
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

impl DivAssign<i32> for Pos2 {
	#[inline]
	fn div_assign(&mut self, scale: i32) {
		self.x /= scale;
		self.y /= scale;
	}
}

impl From<embedded_graphics_core::geometry::Point> for Pos2 {
	fn from(lib: embedded_graphics_core::geometry::Point) -> Self {
		Self { x: lib.x, y: lib.y }
	}
}

impl From<Pos2> for embedded_graphics_core::geometry::Point {
	fn from(our: Pos2) -> Self {
		Self { x: our.x, y: our.y }
	}
}
