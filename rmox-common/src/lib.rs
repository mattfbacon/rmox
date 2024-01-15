use std::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Range, Sub, SubAssign};

use embedded_graphics_core::geometry::Dimensions;
use serde::{Deserialize, Serialize};

pub const FB_WIDTH: i32 = 1404;
pub const FB_HEIGHT: i32 = 1872;

/// How the E-Ink driver will refresh the pixels.
#[derive(Debug, Clone, Copy)]
pub enum UpdateStyle {
	/// A very fast method with minimal ghosting, but only works for black and white.
	Monochrome,
	/// A relatively fast method with some ghosting. Works for all colors.
	Rgb,
	/// A slow method with no ghosting. Works for all colors.
	Init,
}

/// How much the E-Ink driver will try to remove ghosting.
#[derive(Debug, Clone, Copy)]
pub enum UpdateDepth {
	/// A normal and relatively fast update.
	Partial,
	/// A longer and more thorough update. Will flash between black and white.
	Full,
}

pub trait EinkUpdate {
	/// Update `rect` using the specified `style` and `depth`.
	///
	/// The `style` determines how the E-Ink driver refreshes the pixels.
	/// See the [`UpdateStyle`] docs for more info.
	///
	/// The `depth` determines how hard the driver tries to remove ghosting.
	/// See the [`UpdateDepth`] docs for more info.
	///
	/// # Errors
	///
	/// Writing to the rm2fb IPC channel.
	fn update(&self, rect: &Rectangle, style: UpdateStyle, depth: UpdateDepth)
		-> std::io::Result<()>;
}

impl<T: EinkUpdate + ?Sized> EinkUpdate for &T {
	#[inline]
	fn update(
		&self,
		area: &Rectangle,
		style: UpdateStyle,
		depth: UpdateDepth,
	) -> std::io::Result<()> {
		<T as EinkUpdate>::update(self, area, style, depth)
	}
}

impl<T: EinkUpdate + ?Sized> EinkUpdate for &mut T {
	#[inline]
	fn update(
		&self,
		area: &Rectangle,
		style: UpdateStyle,
		depth: UpdateDepth,
	) -> std::io::Result<()> {
		<T as EinkUpdate>::update(self, area, style, depth)
	}
}

pub trait EinkUpdateExt: EinkUpdate {
	/// [`EinkUpdate::update`] with [`UpdateDepth::Full`].
	///
	/// # Errors
	///
	/// Same as [`EinkUpdate::update`].
	#[inline]
	fn update_full(&self, area: &Rectangle, style: UpdateStyle) -> std::io::Result<()> {
		self.update(area, style, UpdateDepth::Full)
	}

	/// [`EinkUpdate::update`] with [`UpdateDepth::Partial`].
	///
	/// # Errors
	///
	/// Same as [`EinkUpdate::update`].
	#[inline]
	fn update_partial(&self, area: &Rectangle, style: UpdateStyle) -> std::io::Result<()> {
		self.update(area, style, UpdateDepth::Partial)
	}

	/// [`EinkUpdate::update`] with the full bounding box of the framebuffer and [`UpdateDepth::Full`].
	///
	/// # Errors
	///
	/// Same as [`EinkUpdate::update`].
	#[inline]
	fn update_all(&self, style: UpdateStyle) -> std::io::Result<()>
	where
		Self: Dimensions,
	{
		self.update(&self.bounding_box().into(), style, UpdateDepth::Full)
	}
}

impl<T: EinkUpdate + ?Sized> EinkUpdateExt for T {}

#[doc(hidden)]
pub mod __macro_private {
	pub use embedded_graphics_core;
}

#[macro_export]
macro_rules! mut_draw_target {
	($ty:ty $(: [$($generics:tt)*])?) => {
		impl$(<$($generics)*>)? $crate::__macro_private::embedded_graphics_core::geometry::OriginDimensions for &mut $ty {
			#[inline]
			fn size(&self) -> Size {
				<$ty as $crate::__macro_private::embedded_graphics_core::geometry::OriginDimensions>::size(*self)
			}
		}

		impl$(<$($generics)*>)? $crate::__macro_private::embedded_graphics_core::draw_target::DrawTarget for &mut $ty {
			type Color = <$ty as $crate::__macro_private::embedded_graphics_core::draw_target::DrawTarget>::Color;

			type Error = <$ty as $crate::__macro_private::embedded_graphics_core::draw_target::DrawTarget>::Error;

			fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
			where
				I: IntoIterator<Item = $crate::__macro_private::embedded_graphics_core::Pixel<Self::Color>>,
			{
				<$ty as $crate::__macro_private::embedded_graphics_core::draw_target::DrawTarget>::draw_iter(*self, pixels)
			}

			fn fill_contiguous<I>(&mut self, area: &$crate::__macro_private::embedded_graphics_core::primitives::Rectangle, colors: I) -> Result<(), Self::Error>
			where
				I: IntoIterator<Item = Self::Color>,
			{
				<$ty as $crate::__macro_private::embedded_graphics_core::draw_target::DrawTarget>::fill_contiguous(*self, area, colors)
			}

			fn fill_solid(&mut self, area: &$crate::__macro_private::embedded_graphics_core::primitives::Rectangle, color: Self::Color) -> Result<(), Self::Error> {
				<$ty as $crate::__macro_private::embedded_graphics_core::draw_target::DrawTarget>::fill_solid(*self, area, color)
			}

			fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
				<$ty as $crate::__macro_private::embedded_graphics_core::draw_target::DrawTarget>::clear(*self, color)
			}
		}
	};
}

macro_rules! enum_all {
	(
		$(#[$meta:meta])*
		$vis:vis enum $name:ident {
			$($variant:ident,)*
		}
	) => {
		$(#[$meta])*
		$vis enum $name {
			$($variant,)*
		}

		impl $name {
			const ALL: &'static [Self] = &[$(Self::$variant,)*];
		}
	};
}

enum_all! {
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Rotation {
	None,
	Rotate90,
	Rotate180,
	Rotate270,
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

	pub fn transform_rect(self, rect: &mut Rectangle, container: &Vec2) {
		rect.origin = self.transform_point(rect.origin, *container);
		rect.size = self
			.transform_point(rect.size.to_pos(), vec2(0, 0))
			.to_vec();
		rect.normalize();
	}

	#[inline]
	#[must_use]
	pub fn transform_size(self, size: Vec2) -> Vec2 {
		match self {
			Self::None | Self::Rotate180 => size,
			Self::Rotate90 | Self::Rotate270 => size.swap(),
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

impl From<embedded_graphics_core::geometry::Point> for Pos2 {
	fn from(lib: embedded_graphics_core::geometry::Point) -> Self {
		Self { x: lib.x, y: lib.y }
	}
}

#[derive(Debug)]
pub struct ComponentOutOfRange;

impl From<Pos2> for embedded_graphics_core::geometry::Point {
	fn from(our: Pos2) -> Self {
		Self { x: our.x, y: our.y }
	}
}

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
	pub fn normalize(&mut self) {
		if self.size.x < 0 {
			self.size.x = -self.size.x;
			self.origin.x -= self.size.x;
		}

		if self.size.y < 0 {
			self.size.y = -self.size.y;
			self.origin.y -= self.size.y;
		}
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

	pub fn inset(&self, inset: i32) -> Self {
		let inset = Vec2::splat(inset);
		Self {
			origin: self.origin + inset,
			size: self.size - inset * 2,
		}
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
	pub fn points(&self) -> impl Iterator<Item = Pos2> + Clone {
		let x_range = self.x_range();
		self.y_range().flat_map(move |y| {
			let x_range = x_range.clone();
			x_range.map(move |x| Pos2 { x, y })
		})
	}
}

impl Mul<i32> for Rectangle {
	type Output = Self;

	fn mul(self, scale: i32) -> Self {
		Self {
			origin: self.origin * scale,
			size: self.size * scale,
		}
	}
}

impl MulAssign<i32> for Rectangle {
	fn mul_assign(&mut self, scale: i32) {
		self.origin *= scale;
		self.size *= scale;
	}
}

impl From<embedded_graphics_core::primitives::Rectangle> for Rectangle {
	fn from(lib: embedded_graphics_core::primitives::Rectangle) -> Self {
		Self {
			origin: lib.top_left.into(),
			size: lib.size.into(),
		}
	}
}

impl From<Rectangle> for embedded_graphics_core::primitives::Rectangle {
	fn from(mut our: Rectangle) -> Self {
		our.normalize();
		Self {
			top_left: our.origin.into(),
			// `normalize` makes the size positive so this can never fail.
			size: our.size.try_into().unwrap(),
		}
	}
}
