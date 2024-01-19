use serde::{Deserialize, Serialize};

use crate::types::{vec2, Rectangle, Rotation, Vec2};

crate::macros::enum_all! {
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Side {
	Top,
	Right,
	Bottom,
	Left,
}
}

impl Side {
	#[inline]
	#[must_use]
	pub fn rotate(self, rotation: Rotation) -> Self {
		Side::ALL[(self as usize + rotation as usize) % Side::ALL.len()]
	}

	pub fn take(self, amount: i32, from: &mut Rectangle) -> Rectangle {
		match self {
			Self::Top => {
				// let ret = Rectangle::new(from.top_left, Size::new(from.size.width, amount));
				let ret = from.with_height(amount);
				*from.height_mut() -= amount;
				from.origin.y += amount;
				ret
			}
			Self::Right => {
				let x = from.origin.x + from.size.x - amount;
				let ret = from.with_x(x).with_width(amount);
				*from.width_mut() -= amount;
				ret
			}
			Self::Bottom => {
				let y = from.origin.y + from.size.y - amount;
				let ret = from.with_y(y).with_height(amount);
				*from.height_mut() -= amount;
				ret
			}
			Self::Left => {
				let ret = from.with_width(amount);
				*from.width_mut() -= amount;
				from.origin.x += amount;
				ret
			}
		}
	}

	#[inline]
	#[must_use]
	pub fn vec_toward(self) -> Vec2 {
		match self {
			Self::Top => vec2(0, -1),
			Self::Right => vec2(1, 0),
			Self::Bottom => vec2(0, 1),
			Self::Left => vec2(-1, 0),
		}
	}
}
