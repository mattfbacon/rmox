#[doc(hidden)]
pub mod __macro_private {
	pub use embedded_graphics_core;
}

#[macro_export]
macro_rules! enum_all {
	(
		$(#[$meta:meta])*
		$vis:vis enum $name:ident {
			$($(#[$variant_meta:meta])* $variant:ident,)*
		}
	) => {
		$(#[$meta])*
		$vis enum $name {
			$($(#[$variant_meta])* $variant,)*
		}

		impl $name {
			pub const ALL: &'static [Self] = &[$(Self::$variant,)*];
		}
	};
}
pub(crate) use enum_all;

#[macro_export]
macro_rules! mut_draw_target {
	($ty:ty $(: [$($generics:tt)*])?) => {
		impl$(<$($generics)*>)? $crate::__macro_private::embedded_graphics_core::geometry::OriginDimensions for &mut $ty {
			#[inline]
			fn size(&self) -> $crate::__macro_private::embedded_graphics_core::geometry::Size {
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
