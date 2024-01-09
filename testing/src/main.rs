use std::time::Duration;

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::Dimensions;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::primitives::{Circle, Primitive as _, PrimitiveStyleBuilder};
use embedded_graphics::Drawable as _;
use rmox_fb::{EinkUpdateExt as _, Framebuffer, UpdateStyle};
use rmox_input::Event;
use tracing_subscriber::filter::LevelFilter;

fn main() {
	eprintln!("starting");

	tracing_subscriber::fmt::fmt()
		.with_max_level(LevelFilter::INFO)
		.init();

	let mut input = rmox_input::Input::open().unwrap();

	let mut fb = Framebuffer::open().expect("open framebuffer");

	let bg = Rgb565::new(31, 63, 31);
	let fg = Rgb565::new(0, 0, 0);

	fb.clear(bg).unwrap();
	fb.update_all(UpdateStyle::Init).unwrap();
	std::thread::sleep(Duration::from_secs(1));

	loop {
		let event = input.next_event();
		tracing::info!("event: {event:?}");
		if let Event::Stylus(_) = event {
			if let Some(state) = input.stylus_state() {
				let erase = match state.tool() {
					rmox_input::StylusTool::Pen => false,
					rmox_input::StylusTool::Rubber => true,
				};
				let hovering = !state.touching();
				let point = state.position();
				let thickness = if erase {
					50
				} else if hovering {
					((255 - state.distance()) / 5).saturating_sub(30).into()
				} else {
					u32::from(state.pressure()) / 100 + 5
				};
				let circle = Circle::with_center(point, thickness);
				let color = if erase { bg } else { fg };
				circle
					.into_styled(PrimitiveStyleBuilder::new().fill_color(color).build())
					.draw(&mut fb)
					.unwrap();
				fb.update_partial(&circle.bounding_box(), UpdateStyle::Monochrome)
					.unwrap();
			}
		}
	}
}
