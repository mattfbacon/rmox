use std::fmt::Write;
use std::time::{Duration, Instant};

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::{Dimensions, OriginDimensions, Point, Size};
use embedded_graphics::mono_font::{ascii as fonts, MonoFont, MonoTextStyle, MonoTextStyleBuilder};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::primitives::{
	Circle, Line, Primitive as _, PrimitiveStyleBuilder, Rectangle, StrokeAlignment,
};
use embedded_graphics::text::renderer::TextRenderer as _;
use embedded_graphics::text::{Baseline, Text};
use embedded_graphics::{Drawable as _, Pixel};
use rmox_fb::{
	mut_draw_target, EinkUpdate, EinkUpdateExt as _, Framebuffer, UpdateDepth, UpdateStyle,
};
use rmox_input::Event;
use tracing_subscriber::filter::LevelFilter;

struct Traced<T>(T);

impl<T: OriginDimensions> OriginDimensions for Traced<T> {
	fn size(&self) -> Size {
		self.0.size()
	}
}

impl<T: DrawTarget + OriginDimensions> DrawTarget for Traced<T>
where
	T::Color: std::fmt::Debug,
{
	type Color = T::Color;

	type Error = T::Error;

	fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
	where
		I: IntoIterator<Item = embedded_graphics::Pixel<Self::Color>>,
	{
		eprintln!("draw_iter");
		self.0.draw_iter(pixels.into_iter().map(|pixel| {
			eprintln!(" > {pixel:?}");
			pixel
		}))
	}

	fn fill_solid(
		&mut self,
		area: &embedded_graphics::primitives::Rectangle,
		color: Self::Color,
	) -> Result<(), Self::Error> {
		eprintln!("fill_solid(area={area:?}, color={color:?})");
		self.0.fill_solid(area, color)
	}

	fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
		eprintln!("clear(color={color:?})");
		self.0.clear(color)
	}
}

struct Scaled<T, const N: usize>(T);

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
		I: IntoIterator<Item = embedded_graphics::Pixel<Self::Color>>,
	{
		self.0.draw_iter(
			pixels
				.into_iter()
				.flat_map(|pixel| {
					std::array::from_fn::<_, N, _>(|i| {
						let y = pixel.0.y * N as i32 + i32::try_from(i).unwrap();
						Pixel(Point::new(pixel.0.x, y), pixel.1)
					})
				})
				.flat_map(|pixel| {
					std::array::from_fn::<_, N, _>(|i| {
						let x = pixel.0.x * N as i32 + i32::try_from(i).unwrap();
						Pixel(Point::new(x, pixel.0.y), pixel.1)
					})
				}),
		)
	}

	fn fill_solid(
		&mut self,
		area: &embedded_graphics::primitives::Rectangle,
		color: Self::Color,
	) -> Result<(), Self::Error> {
		let rect = Rectangle {
			top_left: area.top_left * N.try_into().unwrap(),
			size: area.size * N.try_into().unwrap(),
		};
		self.0.fill_solid(&rect, color)
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
		let area = Rectangle {
			top_left: area.top_left * N.try_into().unwrap(),
			size: area.size * N.try_into().unwrap(),
		};
		self.0.update(&area, style, depth)
	}
}

struct Rotate90<T>(T);

impl<T: OriginDimensions> OriginDimensions for Rotate90<T> {
	fn size(&self) -> Size {
		let size = self.0.size();
		Size {
			width: size.height,
			height: size.width,
		}
	}
}

fn rotate90(container: Size, point: Point) -> Point {
	Point {
		x: i32::try_from(container.height).unwrap() - point.y,
		y: point.x,
	}
}

fn rotate90_rect(container: Size, rect: &Rectangle) -> Rectangle {
	let top_left = Point {
		x: i32::try_from(container.height).unwrap()
			- rect.top_left.y
			- i32::try_from(rect.size.height).unwrap(),
		y: rect.top_left.x,
	};
	Rectangle {
		top_left,
		size: Size {
			width: rect.size.height,
			height: rect.size.width,
		},
	}
}

impl<T: DrawTarget + OriginDimensions> DrawTarget for Rotate90<T> {
	type Color = T::Color;

	type Error = T::Error;

	fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
	where
		I: IntoIterator<Item = embedded_graphics::Pixel<Self::Color>>,
	{
		let size = self.size();
		self.0.draw_iter(
			pixels
				.into_iter()
				.map(|pixel| Pixel(rotate90(size, pixel.0), pixel.1)),
		)
	}

	fn fill_solid(
		&mut self,
		area: &embedded_graphics::primitives::Rectangle,
		color: Self::Color,
	) -> Result<(), Self::Error> {
		let area = rotate90_rect(self.size(), area);
		self.0.fill_solid(&area, color)
	}

	fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
		self.0.clear(color)
	}
}

impl<T: EinkUpdate + OriginDimensions> EinkUpdate for Rotate90<T> {
	fn update(
		&self,
		area: &Rectangle,
		style: UpdateStyle,
		depth: UpdateDepth,
	) -> std::io::Result<()> {
		let area = rotate90_rect(self.size(), area);
		self.0.update(&area, style, depth)
	}
}

mut_draw_target!(Rotate90<T>: [T: DrawTarget + OriginDimensions]);

struct TextArea {
	dirty_region: Rectangle,
	current_pos: Point,
	font: &'static MonoFont<'static>,
	fg: Rgb565,
	bg: Rgb565,
}

fn rect_union(a: &Rectangle, b: &Rectangle) -> Rectangle {
	let top_left = {
		let a = a.top_left;
		let b = b.top_left;
		Point {
			x: std::cmp::min(a.x, b.x),
			y: std::cmp::min(a.y, b.y),
		}
	};
	let bottom_right = {
		let a = a.top_left + a.size;
		let b = b.top_left + b.size;
		Point {
			x: std::cmp::max(a.x, b.x),
			y: std::cmp::max(a.y, b.y),
		}
	};
	let size = Size {
		width: (bottom_right.x - top_left.x).try_into().unwrap(),
		height: (bottom_right.y - top_left.y).try_into().unwrap(),
	};
	Rectangle { top_left, size }
}

impl TextArea {
	fn new(font: &'static MonoFont<'static>, fg: Rgb565, bg: Rgb565, origin: Point) -> Self {
		Self {
			dirty_region: Rectangle {
				top_left: origin,
				size: Size::zero(),
			},
			current_pos: origin,
			font,
			fg,
			bg,
		}
	}

	fn text_style(&self) -> MonoTextStyle<Rgb565> {
		MonoTextStyleBuilder::new()
			.font(self.font)
			.text_color(self.fg)
			.background_color(self.bg)
			.build()
	}

	fn write_<T: DrawTarget<Color = Rgb565>>(
		&mut self,
		text: &str,
		draw: &mut T,
	) -> Result<(), <T as DrawTarget>::Error> {
		let text = Text::with_baseline(text, self.current_pos, self.text_style(), Baseline::Top);
		let next_pos = text.draw(draw)?;
		let text_rect = text.bounding_box();
		self.current_pos.x = next_pos.x;
		// `y` is left unchanged because we stay on the same line.

		let old_x = self.dirty_region.top_left.x;
		self.dirty_region = rect_union(&self.dirty_region, &text_rect);
		assert_eq!(old_x, self.dirty_region.top_left.x);

		Ok(())
	}

	fn write_carriage_return(&mut self) {
		self.current_pos.x = self.dirty_region.top_left.x;
	}

	fn write_newline(&mut self) {
		self.write_carriage_return();
		self.current_pos.y += i32::try_from(self.text_style().line_height()).unwrap();
	}

	fn write<T: DrawTarget<Color = Rgb565>>(
		&mut self,
		text: &str,
		draw: &mut T,
	) -> Result<(), <T as DrawTarget>::Error> {
		for run in text.split_inclusive(&['\n', '\r']) {
			// TODO: implement line wrapping.
			let newline = run.ends_with('\n');
			let carriage_return = run.ends_with('\r');
			let run = run.trim_end_matches(&['\n', '\r']);
			if !run.is_empty() {
				self.write_(run, draw)?;
			}
			if newline {
				self.write_newline();
			} else if carriage_return {
				self.write_carriage_return();
			}
		}
		Ok(())
	}

	fn flush_updates(&mut self, style: UpdateStyle, target: impl EinkUpdate) -> std::io::Result<()> {
		target.update_partial(&self.dirty_region, style)?;
		self.dirty_region.top_left.y += i32::try_from(self.dirty_region.size.height).unwrap();
		self.dirty_region.size = Size::zero();
		Ok(())
	}

	fn writer<'a, T>(&'a mut self, draw: &'a mut T) -> Writer<'a, T> {
		Writer { area: self, draw }
	}
}

struct Writer<'a, T> {
	area: &'a mut TextArea,
	draw: &'a mut T,
}

impl<T: DrawTarget<Color = Rgb565>> Write for Writer<'_, T> {
	fn write_str(&mut self, s: &str) -> std::fmt::Result {
		self.area.write(s, self.draw).map_err(|_| std::fmt::Error)
	}
}

fn main() {
	tracing_subscriber::fmt::fmt()
		.with_max_level(LevelFilter::INFO)
		.init();

	/*
	let fb = Framebuffer::open().expect("open framebuffer");
	let mut fb = Rotate90(fb);

	let bg = Rgb565::new(31, 63, 31);
	let fg = Rgb565::new(0, 0, 0);

	fb.clear(bg).unwrap();
	fb.bounding_box()
		.into_styled(
			PrimitiveStyleBuilder::new()
				.stroke_color(fg)
				.stroke_alignment(StrokeAlignment::Inside)
				.stroke_width(20)
				.build(),
		)
		.draw(&mut fb)
		.unwrap();

	let analog_center = Point::new(400, 400);
	let analog_circle = Circle::with_center(analog_center, 410).into_styled(
		PrimitiveStyleBuilder::new()
			.stroke_color(fg)
			.stroke_width(5)
			.fill_color(bg)
			.build(),
	);
	analog_circle.draw(&mut fb).unwrap();
	fn hand(
		fb: &mut Rotate90<Framebuffer>,
		thickness: u32,
		ratio: f32,
		radius: f32,
		center: Point,
		fg: Rgb565,
	) {
		let input = ratio * std::f32::consts::TAU;
		let x = (input.cos() * radius).round() as i32;
		let y = (input.sin() * radius).round() as i32;
		Line::with_delta(center, Point::new(y, -x))
			.into_styled(
				PrimitiveStyleBuilder::new()
					.stroke_color(fg)
					.stroke_width(thickness)
					.build(),
			)
			.draw(fb)
			.unwrap();
	}

	let text_pos = Point::new(20, 22);
	let text_style = MonoTextStyle::new(&fonts::FONT_7X14, fg);
	let text = Text::new("Hello Remarkable! ", text_pos, text_style);
	text.draw(&mut Scaled::<_, 4>(&mut fb)).unwrap();
	let bounds = text.bounding_box();
	fb.update_all(UpdateStyle::Init).unwrap();
	std::thread::sleep(Duration::from_secs(1));

	let time_x = bounds.top_left.x + i32::try_from(bounds.size.width).unwrap();

	let mut now = Instant::now();
	loop {
		let time = time::OffsetDateTime::now_utc();
		let text = format!(
			"{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
			time.year(),
			time.month() as u8,
			time.day(),
			time.hour(),
			time.minute(),
			time.second(),
		);
		let text = Text::new(
			text.as_str(),
			Point {
				x: time_x,
				..text_pos
			},
			text_style,
		);
		text.draw(&mut Scaled::<_, 4>(&mut fb)).unwrap();
		let bounds = text.bounding_box();
		Scaled::<_, 4>(&mut fb)
			.update_partial(&bounds, UpdateStyle::Monochrome)
			.unwrap();

		Circle::with_center(analog_center, 20)
			.into_styled(PrimitiveStyleBuilder::new().fill_color(fg).build())
			.draw(&mut fb)
			.unwrap();
		let second = time.second() as f32 / 60.0;
		hand(&mut fb, 2, second, 190.0, analog_center, fg);
		let minute = (time.minute() as f32 + second) / 60.0;
		hand(&mut fb, 7, minute, 180.0, analog_center, fg);
		let hour = (time.hour() as f32 + minute) / 24.0;
		hand(&mut fb, 20, hour, 150.0, analog_center, fg);

		fb.update_partial(&analog_circle.bounding_box(), UpdateStyle::Monochrome)
			.unwrap();
		now += Duration::from_secs(1);
		std::thread::sleep(now - Instant::now());
		Scaled::<_, 4>(&mut fb).fill_solid(&bounds, bg).unwrap();
		analog_circle.draw(&mut fb).unwrap();
	}
	*/
	let mut input = rmox_input::Input::new().unwrap();

	let fb = Framebuffer::open().expect("open framebuffer");
	let mut fb = Scaled::<_, 5>(Rotate90(fb));

	let bg = Rgb565::new(31, 63, 31);
	let fg = Rgb565::new(0, 0, 0);

	fb.clear(bg).unwrap();
	fb.update_all(UpdateStyle::Init).unwrap();
	std::thread::sleep(Duration::from_secs(1));

	let mut text_area = TextArea::new(&fonts::FONT_7X14, fg, bg, Point::new(4, 4));
	writeln!(text_area.writer(&mut fb), "ready").unwrap();
	text_area
		.flush_updates(UpdateStyle::Monochrome, &fb)
		.unwrap();

	loop {
		let event = input.next_event();
		tracing::info!("event: {event:?}");

		if let Event::Text(text) = event {
			text_area.write(&text, &mut fb).unwrap();
			text_area
				.flush_updates(UpdateStyle::Monochrome, &fb)
				.unwrap();
		}
	}
}
