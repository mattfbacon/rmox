use serde::{Deserialize, Serialize};

use crate::types::{Rectangle, Rotation};

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
}
