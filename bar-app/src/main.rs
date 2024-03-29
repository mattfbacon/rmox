use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::{Dimensions, Point};
use embedded_graphics::mono_font::{ascii as fonts, MonoTextStyle};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::text::{Baseline, Text};
use embedded_graphics::Drawable as _;
use rmox_common::eink_update::{EinkUpdateExt as _, UpdateStyle};
use rmox_common::types::Side;
use rmox_fb::util::Scaled;
use rmox_fb::Framebuffer;
use rmox_protocol::client::recv::{Event, SurfaceEvent};
use rmox_protocol::client::send::{Command, SurfaceInit};
use tokio::{pin, select};
use tokio_stream::StreamExt as _;

struct Battery {
	percentage: u32,
	charging: bool,
}

fn get_battery() -> Battery {
	let percentage = std::fs::read_to_string("/sys/class/power_supply/max77818_battery/capacity")
		.unwrap()
		.trim()
		.parse()
		.unwrap();
	let charging = std::fs::read_to_string("/sys/class/power_supply/max77818_battery/status")
		.unwrap()
		.trim()
		!= "Discharging";
	Battery {
		percentage,
		charging,
	}
}

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
		.write(&Command::CreateSurface(SurfaceInit::Layer {
			anchor: Side::Top,
			size: 48,
		}))
		.await
		.unwrap();

	let mut fb = Framebuffer::open().unwrap();

	let mut desc = None;

	let mut time_interval = tokio::time::interval(std::time::Duration::from_secs(1));
	time_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

	let mut time = time::OffsetDateTime::now_utc();
	let mut battery = get_battery();

	loop {
		select! {
			res = socket.next() => {
				let Some(res) = res else { break; };
				let event = res.unwrap();
				match dbg!(event) {
					Event::Surface { id: _, event } => match event {
						SurfaceEvent::Description(new_desc) => {
							desc = Some(new_desc);
						}
						SurfaceEvent::Quit => break,
						SurfaceEvent::Input(..) => continue,
					}
				}
			}
			_ = time_interval.tick() => {
				time = time::OffsetDateTime::now_utc();
				battery = get_battery();
			}
		}

		let Some(desc) = desc else {
			continue;
		};
		if !desc.visible {
			continue;
		}

		let mut fb = desc.transform(&mut fb);
		let bounds = fb.bounding_box();
		fb.fill_solid(&bounds, Rgb565::new(0, 0, 0)).unwrap();
		Text::with_baseline(
			&format!(
				"{:04}-{:02}-{:02} {:02}:{:02}:{:02} | {:>3.0}%{}",
				time.year(),
				time.month() as u8,
				time.day(),
				time.hour(),
				time.minute(),
				time.second(),
				battery.percentage,
				if battery.charging { "^" } else { "v" },
			),
			Point::new(bounds.top_left.x + 8, bounds.center().y) / 2,
			MonoTextStyle::new(&fonts::FONT_7X14, Rgb565::new(31, 63, 31)),
			Baseline::Middle,
		)
		.draw(&mut Scaled::<_, 2>(&mut fb))
		.unwrap();
		fb.update_partial(&fb.bounding_box().into(), UpdateStyle::Monochrome)
			.unwrap();
	}
}
