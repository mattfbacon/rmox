use enumset::{EnumSet, EnumSetType};
use serde::{Deserialize, Serialize};

#[derive(Debug, EnumSetType)]
#[enumset(no_ops)]
#[repr(u8)]
pub enum Modifier {
	Ctrl,
	Alt,
	Opt,
	AltOpt,
	LeftShift,
	RightShift,
	CapsLock,
}

impl Modifier {
	/// Does pressing this key toggle the modifier as opposed to momentarily activating it?
	#[must_use]
	pub(crate) fn is_toggle(self) -> bool {
		match self {
			Modifier::Ctrl
			| Modifier::Alt
			| Modifier::Opt
			| Modifier::AltOpt
			| Modifier::LeftShift
			| Modifier::RightShift => false,
			Modifier::CapsLock => true,
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Modifiers(EnumSet<Modifier>);

impl Modifiers {
	#[inline]
	#[must_use]
	pub const fn none() -> Self {
		Self(EnumSet::EMPTY)
	}

	#[inline]
	#[must_use]
	pub fn just(this: Modifier) -> Self {
		Self(EnumSet::only(this))
	}

	#[inline]
	#[must_use]
	pub fn contains(self, modifier: impl Into<Modifiers>) -> bool {
		self.0.is_superset(modifier.into().0)
	}

	/// Checks if a whole set of modifiers is present and consumes them from `self` if they are.
	#[inline]
	#[must_use = "Do not use this function to remove modifiers if you don't care whether they were present."]
	pub fn consume(&mut self, set: Self) -> bool {
		if self.0.is_superset(set.0) {
			self.0 = self.0.difference(set.0);
			true
		} else {
			false
		}
	}

	/// If `alphabet` is `true`, Caps Lock will be considered as a Shift key.
	#[inline]
	#[must_use]
	pub fn shift(self, alphabet: bool) -> bool {
		let shift = self.contains(Modifier::LeftShift) || self.contains(Modifier::RightShift);
		let caps = alphabet && self.contains(Modifier::CapsLock);
		shift ^ caps
	}

	#[inline]
	#[must_use]
	pub fn ctrl(self) -> bool {
		self.contains(Modifier::Ctrl)
	}

	#[inline]
	#[must_use]
	pub fn opt(self) -> bool {
		self.contains(Modifier::Opt)
	}

	#[inline]
	#[must_use]
	pub fn alt(self) -> bool {
		self.contains(Modifier::Alt)
	}

	#[inline]
	#[must_use]
	pub fn alt_opt(self) -> bool {
		self.contains(Modifier::AltOpt)
	}
}

impl From<Modifier> for Modifiers {
	fn from(value: Modifier) -> Self {
		Self(value.into())
	}
}

impl std::ops::Add<Self> for Modifiers {
	type Output = Self;

	fn add(self, rhs: Modifiers) -> Self::Output {
		Self(self.0.union(rhs.0))
	}
}

impl std::ops::Sub<Self> for Modifiers {
	type Output = Self;

	fn sub(self, rhs: Modifiers) -> Self::Output {
		Self(self.0.difference(rhs.0))
	}
}

impl std::ops::BitXor<Self> for Modifiers {
	type Output = Self;

	fn bitxor(self, rhs: Modifiers) -> Self::Output {
		Self(self.0 ^ rhs.0)
	}
}

macro_rules! modifiers_ops {
	($trait:ident, $method:ident, $assign_trait:ident, $assign_method:ident) => {
		impl std::ops::$trait<Modifier> for Modifiers {
			type Output = Self;

			fn $method(self, rhs: Modifier) -> Self::Output {
				<Self as std::ops::$trait<Self>>::$method(self, rhs.into())
			}
		}

		impl<T> std::ops::$assign_trait<T> for Modifiers
		where
			Self: std::ops::$trait<T, Output = Self>,
		{
			fn $assign_method(&mut self, rhs: T) {
				*self = <Self as std::ops::$trait<T>>::$method(*self, rhs);
			}
		}
	};
}

modifiers_ops!(Add, add, AddAssign, add_assign);
modifiers_ops!(Sub, sub, SubAssign, sub_assign);
modifiers_ops!(BitXor, bitxor, BitXorAssign, bitxor_assign);
