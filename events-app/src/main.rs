use std::fmt::Write as _;

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::{Dimensions, Point};
use embedded_graphics::mono_font::{ascii as fonts, MonoTextStyle};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::text::{Baseline, Text};
use embedded_graphics::Drawable as _;
use rmox_common::eink_update::{EinkUpdateExt as _, UpdateStyle};
use rmox_common::types::Rectangle;
use rmox_fb::util::Scaled;
use rmox_fb::Framebuffer;
use rmox_protocol::client::recv::{Event, SurfaceEvent};
use rmox_protocol::server::recv::{Command, SurfaceInit};
use tokio::pin;
use tokio_stream::StreamExt as _;

#[tokio::main(flavor = "current_thread")]
async fn main() {
	eprintln!("starting");

	tracing_subscriber::fmt::init();

	let socket_path = std::env::var_os("RMOX_SOCKET").expect("missing RMOX_SOCKET env var");
	let socket = tokio::net::UnixStream::connect(&socket_path)
		.await
		.unwrap_or_else(|error| panic!("connecting to {socket_path:?} (RMOX_SOCKET): {error}"));
	let socket = rmox_protocol::io::Stream::new(socket);
	pin!(socket);

	socket
		.write(&Command::CreateSurface(SurfaceInit::Normal))
		.await
		.unwrap();

	let mut fb = Framebuffer::open().unwrap();

	let mut desc = None;

	let mut input_buf = "ready\n".to_owned();
	let mut y = 8;

	loop {
		let mut just_last_line = true;
		let Some(res) = socket.next().await else {
			break;
		};
		let event = res.unwrap();
		match event {
			Event::Surface { id: _, event } => match event {
				SurfaceEvent::Description(new_desc) => {
					desc = Some(new_desc);
					just_last_line = false;
				}
				SurfaceEvent::Quit => break,
				SurfaceEvent::Input(input) => {
					writeln!(input_buf, "{input:?}").unwrap();
				}
			},
		}

		let Some(desc) = desc else {
			continue;
		};
		if !desc.visible {
			continue;
		}

		let mut fb = desc.transform(&mut fb);

		let text_style = MonoTextStyle::new(&fonts::FONT_6X10, Rgb565::new(0, 0, 0));
		if just_last_line {
			let text = Text::with_baseline(
				input_buf.lines().last().unwrap(),
				Point::new(4, y / 2),
				text_style,
				Baseline::Top,
			);
			text.draw(&mut Scaled::<_, 2>(&mut fb)).unwrap();
			let mut bounds: Rectangle = text.bounding_box().into();
			bounds = bounds.scale_all(2);
			y = bounds.end().y;
			fb.update_partial(&bounds, UpdateStyle::Monochrome).unwrap();
		} else {
			fb.clear(Rgb565::new(31, 63, 31)).unwrap();
			let text = Text::with_baseline(&input_buf, Point::new(4, 4), text_style, Baseline::Top);
			text.draw(&mut Scaled::<_, 2>(&mut fb)).unwrap();
			y = Rectangle::from(text.bounding_box()).end().y * 2;
			fb.update_partial(&fb.bounding_box().into(), UpdateStyle::Monochrome)
				.unwrap();
		}
	}
}
