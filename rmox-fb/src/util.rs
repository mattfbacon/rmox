use embedded_graphics_core::draw_target::DrawTarget;
use embedded_graphics_core::geometry::{OriginDimensions, Size};
use embedded_graphics_core::primitives::Rectangle as BadRect;
use embedded_graphics_core::Pixel;
use rmox_common::{mut_draw_target, EinkUpdate, Rectangle, UpdateDepth, UpdateStyle};

pub struct Scaled<T, const N: usize>(pub T);

impl<T: OriginDimensions, const N: usize> OriginDimensions for Scaled<T, N> {
	fn size(&self) -> Size {
		self.0.size() / N.try_into().unwrap()
	}
}

impl<T: DrawTarget + OriginDimensions, const N: usize> DrawTarget for Scaled<T, N> {
	type Color = T::Color;

	type Error = T::Error;

	fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
	where
		I: IntoIterator<Item = Pixel<Self::Color>>,
	{
		self.0.draw_iter(pixels.into_iter().flat_map(|pixel| {
			let mut rect = Rectangle::single(pixel.0.into());
			rect *= N.try_into().unwrap();
			rect.points().map(move |point| Pixel(point.into(), pixel.1))
		}))
	}

	fn fill_solid(&mut self, area: &BadRect, color: Self::Color) -> Result<(), Self::Error> {
		let area = Rectangle::from(*area) * N.try_into().unwrap();
		self.0.fill_solid(&area.into(), color)
	}

	fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
		self.0.clear(color)
	}
}

mut_draw_target!(Scaled<T, N>: [T: DrawTarget + OriginDimensions, const N: usize]);

impl<T: EinkUpdate, const N: usize> EinkUpdate for Scaled<T, N> {
	fn update(
		&self,
		area: &Rectangle,
		style: UpdateStyle,
		depth: UpdateDepth,
	) -> std::io::Result<()> {
		let area = (*area) * N.try_into().unwrap();
		self.0.update(&area, style, depth)
	}
}
