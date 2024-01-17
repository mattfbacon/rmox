use std::ops::Range;

use serde::{Deserialize, Serialize};

use crate::types::{Pos2, Vec2};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Rectangle {
	pub origin: Pos2,
	pub size: Vec2,
}

#[inline]
#[must_use]
pub fn rect(x: i32, y: i32, width: i32, height: i32) -> Rectangle {
	Rectangle {
		origin: Pos2 { x, y },
		size: Vec2 {
			x: width,
			y: height,
		},
	}
}

impl Rectangle {
	pub const ZERO: Self = Self {
		origin: Pos2::ZERO,
		size: Vec2::ZERO,
	};

	#[inline]
	#[must_use]
	pub fn new(origin: Pos2, size: Vec2) -> Self {
		Self { origin, size }
	}

	#[inline]
	#[must_use]
	pub fn single(origin: Pos2) -> Self {
		Self {
			origin,
			size: Vec2::splat(1),
		}
	}

	#[inline]
	#[must_use]
	pub fn with_x(mut self, x: i32) -> Self {
		self.origin.x = x;
		self
	}

	#[inline]
	#[must_use]
	pub fn x(&self) -> i32 {
		self.origin.x
	}

	#[inline]
	#[must_use]
	pub fn x_mut(&mut self) -> &mut i32 {
		&mut self.origin.x
	}

	#[inline]
	#[must_use]
	pub fn with_y(mut self, y: i32) -> Self {
		self.origin.y = y;
		self
	}

	#[inline]
	#[must_use]
	pub fn y(&self) -> i32 {
		self.origin.y
	}

	#[inline]
	#[must_use]
	pub fn y_mut(&mut self) -> &mut i32 {
		&mut self.origin.y
	}

	#[inline]
	#[must_use]
	pub fn with_size(mut self, size: Vec2) -> Self {
		self.size = size;
		self
	}

	#[inline]
	#[must_use]
	pub fn with_width(mut self, width: i32) -> Self {
		self.size.x = width;
		self
	}

	#[inline]
	#[must_use]
	pub fn width(&self) -> i32 {
		self.size.x
	}

	#[inline]
	#[must_use]
	pub fn width_mut(&mut self) -> &mut i32 {
		&mut self.size.x
	}

	#[inline]
	#[must_use]
	pub fn with_height(mut self, height: i32) -> Self {
		self.size.y = height;
		self
	}

	#[inline]
	#[must_use]
	pub fn height(&self) -> i32 {
		self.size.y
	}

	#[inline]
	#[must_use]
	pub fn height_mut(&mut self) -> &mut i32 {
		&mut self.size.y
	}

	/// Make the origin be the top-left corner.
	#[inline]
	pub fn normalize(mut self) -> Self {
		if self.size.x < 0 {
			self.size.x = -self.size.x;
			self.origin.x -= self.size.x;
		}

		if self.size.y < 0 {
			self.size.y = -self.size.y;
			self.origin.y -= self.size.y;
		}

		self
	}

	#[inline]
	#[must_use]
	pub fn end(&self) -> Pos2 {
		self.origin + self.size
	}

	#[inline]
	#[must_use]
	pub fn top_left(&self) -> Pos2 {
		let end = self.end();
		Pos2::min_components(self.origin, end)
	}

	#[inline]
	#[must_use]
	pub fn bottom_right(&self) -> Pos2 {
		let end = self.end();
		Pos2::max_components(self.origin, end)
	}

	#[inline]
	#[must_use]
	pub fn intersection(&self, other: &Self) -> Self {
		let top_left = Pos2::max_components(self.top_left(), other.top_left());
		let bottom_right = Pos2::max_components(self.bottom_right(), other.bottom_right());
		Self::from_corners(top_left, bottom_right)
	}

	#[inline]
	#[must_use]
	pub fn from_corners(origin: Pos2, end: Pos2) -> Self {
		Self {
			origin,
			size: end - origin,
		}
	}

	#[inline]
	#[must_use]
	pub fn is_empty(&self) -> bool {
		self.size.is_empty()
	}

	#[inline]
	#[must_use]
	pub fn inset(&self, inset: i32) -> Self {
		let inset = Vec2::splat(inset);
		Self {
			origin: self.origin + inset,
			size: self.size - inset * 2,
		}
	}

	#[inline]
	pub fn scale_all(mut self, factor: i32) -> Self {
		self.origin *= factor;
		self.size *= factor;
		self
	}

	#[inline]
	#[must_use]
	pub fn contains(&self, point: Pos2) -> bool {
		self.x_range().contains(&point.x) && self.y_range().contains(&point.y)
	}
}

fn order_range(range: Range<i32>) -> Range<i32> {
	if range.end < range.start {
		range.end..range.start
	} else {
		range
	}
}

impl Rectangle {
	#[inline]
	#[must_use]
	pub fn x_range(&self) -> Range<i32> {
		order_range(self.origin.x..(self.origin.x + self.size.x))
	}

	#[inline]
	#[must_use]
	pub fn y_range(&self) -> Range<i32> {
		order_range(self.origin.y..(self.origin.y + self.size.y))
	}

	// TODO: A better implementation of this with a proper size hint, nameable type, and whatnot.
	#[inline]
	pub fn points(&self) -> impl Iterator<Item = Pos2> + Clone {
		let x_range = self.x_range();
		self.y_range().flat_map(move |y| {
			let x_range = x_range.clone();
			x_range.map(move |x| Pos2 { x, y })
		})
	}
}

impl From<embedded_graphics_core::primitives::Rectangle> for Rectangle {
	#[inline]
	#[must_use]
	fn from(lib: embedded_graphics_core::primitives::Rectangle) -> Self {
		Self {
			origin: lib.top_left.into(),
			size: lib.size.into(),
		}
	}
}

impl From<Rectangle> for embedded_graphics_core::primitives::Rectangle {
	#[inline]
	#[must_use]
	fn from(our: Rectangle) -> Self {
		let our = our.normalize();
		Self {
			top_left: our.origin.into(),
			// `normalize` makes the size positive so this can never fail.
			size: our.size.try_into().unwrap(),
		}
	}
}
