use embedded_graphics_core::geometry::Dimensions;

use crate::types::Rectangle;

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
