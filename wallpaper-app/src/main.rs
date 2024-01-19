use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::Dimensions;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::primitives::PointsIter as _;
use rmox_common::eink_update::{EinkUpdateExt as _, UpdateStyle};
use rmox_fb::Framebuffer;
use rmox_protocol::client::recv::Event;
use rmox_protocol::server::recv::{Command, SurfaceInit};
use tokio::pin;
use tokio_stream::StreamExt as _;
use tracing_subscriber::filter::LevelFilter;

#[tokio::main(flavor = "current_thread")]
async fn main() {
	eprintln!("starting");

	tracing_subscriber::fmt::fmt()
		.with_max_level(LevelFilter::INFO)
		.init();

	let socket_path = std::env::var_os("RMOX_SOCKET").expect("missing RMOX_SOCKET env var");
	let socket = tokio::net::UnixStream::connect(&socket_path)
		.await
		.unwrap_or_else(|error| panic!("connecting to {socket_path:?} (RMOX_SOCKET): {error}"));
	let socket = rmox_protocol::io::Stream::new(socket);
	pin!(socket);

	socket
		.write(&Command::CreateSurface(SurfaceInit::Wallpaper))
		.await
		.unwrap();

	let mut fb = Framebuffer::open().unwrap();

	while let Some(res) = socket.next().await {
		let event = res.unwrap();
		let desc = match event {
			Event::Surface { id: _, description } => description,
			Event::SurfaceQuit(_id) => break,
			Event::Input { .. } => continue,
		};

		if !desc.visible {
			continue;
		}

		let mut fb = desc.transform(&mut fb);

		fb.clear(Rgb565::new(31, 63, 31)).unwrap();
		fb.draw_iter(
			fb.bounding_box()
				.points()
				.filter(|point| (point.x / 2) % 3 == 0 && (point.y / 2) % 3 == 0)
				.map(|point| embedded_graphics::Pixel(point, Rgb565::new(0, 0, 0))),
		)
		.unwrap();

		fb.update_partial(&fb.bounding_box().into(), UpdateStyle::Monochrome)
			.unwrap();
	}
}
