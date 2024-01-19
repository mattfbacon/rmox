use serde::{Deserialize, Serialize};

use crate::types::{vec2, Pos2, Rectangle, Vec2};

crate::macros::enum_all! {
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Rotation {
	None,
	/// Rotate clockwise by 90 degrees.
	Rotate90,
	/// Rotate by 180 degrees.
	Rotate180,
	/// Rotate counterclockwise by 90 degrees.
	Rotate270,
}
}

impl Rotation {
	#[must_use]
	pub fn transform_point(self, point: Pos2, container: Vec2) -> Pos2 {
		match self {
			Self::None => point,
			Self::Rotate90 => Pos2 {
				x: container.x - point.y,
				y: point.x,
			},
			Self::Rotate180 => container.to_pos() - point.to_vec(),
			Self::Rotate270 => Pos2 {
				x: point.y,
				y: container.y - point.x,
			},
		}
	}

	pub fn transform_rect(self, mut rect: Rectangle, container: &Vec2) -> Rectangle {
		rect.origin = self.transform_point(rect.origin, *container);
		rect.size = self
			.transform_point(rect.size.to_pos(), vec2(0, 0))
			.to_vec();
		rect.normalize()
	}

	#[inline]
	#[must_use]
	pub fn transform_size(self, size: Vec2) -> Vec2 {
		match self {
			Self::None => size,
			Self::Rotate90 => vec2(size.y, -size.x),
			Self::Rotate180 => -size,
			Self::Rotate270 => vec2(-size.y, size.x),
		}
	}

	#[inline]
	#[must_use]
	pub fn inverse(self) -> Self {
		match self {
			Self::None => Self::Rotate180,
			Self::Rotate90 => Self::Rotate270,
			Self::Rotate180 => Self::None,
			Self::Rotate270 => Self::Rotate90,
		}
	}
}

#[test]
fn test_transform_point() {
	use crate::types::{pos2, vec2};

	let container = vec2(3, 3);
	let origin = pos2(1, 2);
	assert_eq!(Rotation::None.transform_point(origin, container), origin);
	assert_eq!(
		Rotation::Rotate90.transform_point(origin, container),
		pos2(1, 1)
	);
	assert_eq!(
		Rotation::Rotate180.transform_point(origin, container),
		pos2(2, 1),
	);
	assert_eq!(
		Rotation::Rotate270.transform_point(origin, container),
		pos2(2, 2),
	);
	assert_eq!(
		Rotation::Rotate270.transform_point(pos2(0, 0), container),
		pos2(0, 3),
	);
}

#[test]
fn test_transform_rect() {
	use crate::types::{rect, vec2};

	let r = rect(0, 0, 1, 2);
	assert_eq!(
		Rotation::Rotate270.transform_rect(r, &vec2(3, 3)),
		rect(0, 2, 2, 1),
	);
}
