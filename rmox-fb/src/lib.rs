//! Currently only supporting rM2 because this is the device that I have.
#![deny(
	absolute_paths_not_starting_with_crate,
	keyword_idents,
	macro_use_extern_crate,
	meta_variable_misuse,
	missing_abi,
	missing_copy_implementations,
	non_ascii_idents,
	nonstandard_style,
	noop_method_call,
	pointer_structural_match,
	rust_2018_idioms,
	unused_qualifications
)]
#![warn(clippy::pedantic)]
// Unsafe code is allowed in this crate due to the low-level interfacing.

use embedded_graphics_core::draw_target::DrawTarget;
use embedded_graphics_core::geometry::{Dimensions, OriginDimensions, Size};
use embedded_graphics_core::pixelcolor::raw::{RawData, RawU16};
use embedded_graphics_core::pixelcolor::Rgb565;
use embedded_graphics_core::primitives::{PointsIter as _, Rectangle as BadRect};
use rmox_common::eink_update::{EinkUpdate, UpdateDepth, UpdateStyle};
use rmox_common::mut_draw_target;
use rmox_common::types::{Pos2, Rectangle, Vec2};

use crate::channel::Channel;
use crate::mapping::Mapping;

mod channel;
mod mapping;
pub mod util;

#[derive(Debug)]
pub struct Framebuffer {
	mapping: Mapping,
	channel: Channel,
}

impl Framebuffer {
	pub const WIDTH: i32 = rmox_common::fb::WIDTH;
	pub const HEIGHT: i32 = rmox_common::fb::HEIGHT;
	pub const SIZE: Vec2 = Vec2 {
		x: Self::WIDTH,
		y: Self::HEIGHT,
	};
	pub const RECT: Rectangle = Rectangle {
		origin: Pos2::ZERO,
		size: Self::SIZE,
	};

	/// # Errors
	///
	/// - Opening the framebuffer
	/// - Mapping the framebuffer
	/// - Getting the rm2fb IPC channel
	#[inline]
	pub fn open() -> std::io::Result<Self> {
		tracing::debug!("open framebuffer");

		Ok(Self {
			mapping: Mapping::open()?,
			channel: Channel::open()?,
		})
	}

	#[inline]
	#[must_use]
	pub fn pixels_mut(&mut self) -> &mut [u16] {
		self.mapping.pixels_mut()
	}
}

impl OriginDimensions for Framebuffer {
	#[inline]
	#[must_use]
	fn size(&self) -> Size {
		Self::SIZE.try_into().unwrap()
	}
}

impl DrawTarget for Framebuffer {
	type Color = Rgb565;
	type Error = std::convert::Infallible;

	fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
	where
		I: IntoIterator<Item = embedded_graphics_core::Pixel<Rgb565>>,
	{
		let bounds = self.bounding_box();
		let pixels = pixels.into_iter().filter(|pixel| bounds.contains(pixel.0));
		for pixel in pixels {
			self.mapping.set_pixel(pixel.0.into(), pixel.1);
		}
		Ok(())
	}

	fn fill_contiguous<I>(&mut self, area: &BadRect, colors: I) -> Result<(), Self::Error>
	where
		I: IntoIterator<Item = Self::Color>,
	{
		let bounds = self.bounding_box();
		let intersection = bounds.intersection(area);

		let pixels = area
			.points()
			.zip(colors)
			.map(|(pos, color)| embedded_graphics_core::Pixel(pos, color));

		// Only filter if part of `area` is out-of-bounds.
		if &intersection == area {
			for pixel in pixels {
				self.mapping.set_pixel(pixel.0.into(), pixel.1);
			}
		} else {
			let pixels = pixels.into_iter().filter(|pixel| bounds.contains(pixel.0));
			for pixel in pixels {
				self.mapping.set_pixel(pixel.0.into(), pixel.1);
			}
		}

		Ok(())
	}

	fn fill_solid(&mut self, area: &BadRect, color: Self::Color) -> Result<(), Self::Error> {
		let area = area.intersection(&self.bounding_box());
		let color = RawU16::from(color).into_inner();
		let pixels = self.mapping.pixels_mut();
		for y in area.rows() {
			let y_index = Mapping::point_to_index(Pos2 { x: 0, y });
			let x_range = area.columns();
			let x_range = usize::try_from(x_range.start).unwrap()..usize::try_from(x_range.end).unwrap();
			pixels[y_index..][x_range].fill(color);
		}
		Ok(())
	}

	fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
		self
			.mapping
			.pixels_mut()
			.fill(RawU16::from(color).into_inner());
		Ok(())
	}
}

impl EinkUpdate for Framebuffer {
	#[inline]
	fn update(
		&self,
		area: &Rectangle,
		style: UpdateStyle,
		depth: UpdateDepth,
	) -> std::io::Result<()> {
		self.channel._update(area, style, depth)
	}
}

mut_draw_target!(Framebuffer);
