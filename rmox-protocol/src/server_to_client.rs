use embedded_graphics_core::draw_target::DrawTarget;
use embedded_graphics_core::geometry::{OriginDimensions, Size};
use embedded_graphics_core::primitives::Rectangle as BadRect;
use embedded_graphics_core::Pixel;
use rmox_common::eink_update::{EinkUpdate, UpdateDepth, UpdateStyle};
use rmox_common::mut_draw_target;
use rmox_common::types::{Pos2, Rectangle, Rotation, Vec2};
use serde::{Deserialize, Serialize};

use crate::SurfaceId;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SurfaceDescription {
	pub base_rect: Rectangle,
	pub rotation: Rotation,
	pub scale: u8,
	pub visible: bool,
}

impl SurfaceDescription {
	#[inline]
	#[must_use]
	pub fn transform_point(&self, mut point: Pos2) -> Pos2 {
		point *= self.scale.into();
		point = self.rotation.transform_point(point, self.base_rect.size);
		point += self.base_rect.origin.to_vec();
		point
	}

	#[inline]
	pub fn transform_rect(&self, mut rect: Rectangle) -> Rectangle {
		rect.origin *= self.scale.into();
		rect.size *= self.scale.into();

		rect = self.rotation.transform_rect(rect, &self.base_rect.size);

		rect.origin += self.base_rect.origin.to_vec();

		rect
	}

	#[inline]
	#[must_use]
	pub fn size(&self) -> Vec2 {
		let mut size = self.base_rect.size;
		size = self.rotation.inverse().transform_size(size).abs();
		size /= self.scale.into();
		size
	}

	#[inline]
	#[must_use]
	pub fn transform<'a, T: ?Sized>(&'a self, base: &'a mut T) -> Transformed<'a, T> {
		Transformed {
			base,
			description: self,
		}
	}
}

#[test]
fn test_transform_point() {
	use rmox_common::types::{pos2, rect};

	let desc = SurfaceDescription {
		base_rect: rect(200, 200, 500, 800),
		rotation: Rotation::Rotate270,
		scale: 2,
		visible: true,
	};
	assert_eq!(desc.transform_point(pos2(0, 0)), pos2(200, 1000));
	assert_eq!(desc.transform_point(pos2(10, 0)), pos2(200, 980));
	assert_eq!(desc.transform_point(pos2(10, 20)), pos2(240, 980));
}

#[test]
fn test_transform_rect() {
	use rmox_common::types::rect;

	let origin = rect(100, 200, 300, 400);
	let desc = SurfaceDescription {
		base_rect: rect(200, 200, 1500, 1800),
		rotation: Rotation::Rotate90,
		scale: 2,
		visible: true,
	};
	assert_eq!(desc.transform_rect(origin), rect(500, 400, 800, 600));
}

pub struct Transformed<'a, T: ?Sized> {
	base: &'a mut T,
	description: &'a SurfaceDescription,
}

impl<T: OriginDimensions> OriginDimensions for Transformed<'_, T> {
	fn size(&self) -> Size {
		self.description.size().try_into().unwrap()
	}
}

impl<T: OriginDimensions + DrawTarget> DrawTarget for Transformed<'_, T> {
	type Color = <T as DrawTarget>::Color;

	type Error = <T as DrawTarget>::Error;

	fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
	where
		I: IntoIterator<Item = Pixel<Self::Color>>,
	{
		if !self.description.visible {
			return Ok(());
		}

		let map = |pixel: Pixel<_>| {
			let point = self.description.transform_point(pixel.0.into());
			if self.description.base_rect.contains(point) {
				Some(Pixel(point.into(), pixel.1))
			} else {
				None
			}
		};
		self.base.draw_iter(pixels.into_iter().flat_map(map))
	}

	fn fill_solid(&mut self, area: &BadRect, color: Self::Color) -> Result<(), Self::Error> {
		if !self.description.visible {
			return Ok(());
		}

		let area = self
			.description
			.transform_rect((*area).into())
			.intersection(&self.description.base_rect);
		self.base.fill_solid(&area.into(), color)
	}

	fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
		if !self.description.visible {
			return Ok(());
		}

		let rect = self.description.base_rect.into();
		self.base.fill_solid(&rect, color)
	}
}

impl<T: EinkUpdate> EinkUpdate for Transformed<'_, T> {
	fn update(
		&self,
		area: &Rectangle,
		style: UpdateStyle,
		depth: UpdateDepth,
	) -> std::io::Result<()> {
		let area = self.description.transform_rect(*area);
		self.base.update(&area, style, depth)
	}
}

mut_draw_target!(Transformed<'a, T>: ['a, T: OriginDimensions + DrawTarget]);

#[derive(Debug, Serialize, Deserialize)]
pub enum TouchPhase {
	Start(rmox_input::touch::TouchState),
	Change(rmox_input::touch::TouchState),
	End,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TouchEvent {
	pub id: rmox_input::touch::Id,
	pub phase: TouchPhase,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum StylusPhase {
	Hover(rmox_input::stylus::StylusState),
	Touch(rmox_input::stylus::StylusState),
	Change(rmox_input::stylus::StylusState),
	Lift(rmox_input::stylus::StylusState),
	Leave,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StylusEvent {
	pub phase: StylusPhase,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum InputEvent {
	// TODO: Keyboard focus enter and leave events, similar to wayland.
	Key(rmox_input::keyboard::KeyEvent),
	Text(Box<str>),
	Touch(TouchEvent),
	Stylus(StylusEvent),
	Button(rmox_input::keyboard::ButtonEvent),
	// TODO: Does this make sense, and if so, how would we decide which tasks to send it to?
	// DevicePresence(SupportedDeviceType),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Event {
	Surface {
		id: SurfaceId,
		description: SurfaceDescription,
	},
	SurfaceQuit(SurfaceId),
	Input {
		surface: SurfaceId,
		event: InputEvent,
	},
}
