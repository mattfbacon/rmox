use std::time::Duration;

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::{Dimensions, OriginDimensions, Point, Size};
use embedded_graphics::mono_font::{ascii as fonts, MonoTextStyle};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::primitives::{Primitive as _, PrimitiveStyleBuilder, Rectangle};
use embedded_graphics::text::{Text, TextStyle};
use embedded_graphics::{Drawable as _, Pixel};
use rmox_fb::{
	mut_draw_target, EinkUpdate, EinkUpdateExt as _, Framebuffer, UpdateDepth, UpdateStyle,
};
use rmox_input::{Event, Input, Modifier, Modifiers};
use tokio::select;
use tokio_stream::StreamExt as _;
use tracing_subscriber::filter::LevelFilter;

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

fn upper_if(lower: char, cond: bool) -> char {
	if cond {
		lower.to_ascii_uppercase()
	} else {
		lower
	}
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
	tracing_subscriber::fmt::fmt()
		.with_max_level(LevelFilter::INFO)
		.init();

	let mut input = Input::open().unwrap();
	let fb = Framebuffer::open().expect("open framebuffer");

	let mut fb = Rotate90(fb);

	let bg = Rgb565::new(31, 63, 31);
	let fg = Rgb565::new(0, 0, 0);

	fb.clear(bg).unwrap();
	fb.update_all(UpdateStyle::Init).unwrap();
	std::thread::sleep(Duration::from_millis(1000));

	let draw_bar =
		|time: &time::OffsetDateTime, modifiers: Modifiers, fb: &mut Rotate90<Framebuffer>| {
			let height = 64;
			let bar = Rectangle::new(
				Point::zero(),
				Size::new(fb.bounding_box().size.width, height),
			)
			.into_styled(PrimitiveStyleBuilder::new().fill_color(fg).build());

			bar.draw(fb).unwrap();

			let text = format!(
				"{:04}-{:02}-{:02} {:02}:{:02} | {}{}{}{}{}{}{}",
				time.year(),
				time.month() as u8,
				time.day(),
				time.hour(),
				time.minute(),
				upper_if('c', modifiers.contains(Modifier::Ctrl)),
				upper_if('a', modifiers.contains(Modifier::Alt)),
				upper_if('o', modifiers.contains(Modifier::Opt)),
				upper_if('o', modifiers.contains(Modifier::AltOpt)),
				upper_if('s', modifiers.contains(Modifier::LeftShift)),
				upper_if('s', modifiers.contains(Modifier::RightShift)),
				upper_if('c', modifiers.contains(Modifier::CapsLock)),
			);

			Text::with_text_style(
				&text,
				Point::new(4, 4),
				MonoTextStyle::new(&fonts::FONT_7X14, bg),
				TextStyle::with_baseline(embedded_graphics::text::Baseline::Top),
			)
			.draw(&mut Scaled::<_, 3>(&mut *fb))
			.unwrap();

			fb.update_partial(&bar.bounding_box(), UpdateStyle::Monochrome)
				.unwrap();
		};

	let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
	interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
	let mut last_time = time::OffsetDateTime::now_utc();
	let mut last_modifiers = input.modifiers();

	loop {
		select! {
			_ = interval.tick() => {
				last_time = time::OffsetDateTime::now_utc();
			}
			Some(event) = input.next() => {
				if let Event::Key {  .. } = event {
					let modifiers = input.modifiers();
					if modifiers == last_modifiers {
						continue;
					}
					last_modifiers = modifiers;
				}
			}
		}
		draw_bar(&last_time, last_modifiers, &mut fb);
	}
}
