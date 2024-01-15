use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::{Dimensions, OriginDimensions, Point, Size};
use embedded_graphics::mono_font::{ascii as fonts, MonoTextStyle};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::primitives::{
	Line, Polyline, Primitive as _, PrimitiveStyleBuilder, Rectangle as BadRect, StrokeAlignment,
};
use embedded_graphics::text::{Baseline, Text, TextStyle};
use embedded_graphics::{Drawable as _, Pixel};
use rmox_common::{
	mut_draw_target, rect, EinkUpdate, EinkUpdateExt as _, Pos2, Rectangle, Rotation, Side,
	UpdateDepth, UpdateStyle,
};
use rmox_fb::Framebuffer;
use rmox_input::{Input, Key, KeyEventKind, Modifier, Modifiers};
use rmox_protocol::server::recv::Command;
use rmox_protocol::server::send::Event;
use rmox_protocol::{SurfaceDescription, SurfaceInit};
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use tokio::{pin, select};
use tokio_stream::StreamExt as _;
use tracing_subscriber::filter::LevelFilter;

type Id = u32;
type SurfaceId = Id;
type TaskId = Id;

#[derive(Debug, Clone, Copy)]
struct Surface {
	id: SurfaceId,
	description: SurfaceDescription,
	task: TaskId,
}

#[derive(Debug)]
struct Task {
	id: TaskId,
	handle: JoinHandle<()>,
	channel: mpsc::Sender<Event>,
}

#[derive(Debug)]
struct ShellLayer {
	anchor: Side,
	size: i32,
	surface: SurfaceId,
}

#[derive(Debug)]
struct Shell {
	layers: Vec<ShellLayer>,
	// TODO: Tree of containers.
	root: Vec<SurfaceId>,
}

#[derive(Debug)]
struct Manager {
	global_rotation: Rotation,
	inset: i32,

	id_counter: Id,
	surfaces: Vec<Surface>,
	tasks: Vec<Task>,

	shell: Shell,
}

impl Manager {
	fn new(global_rotation: Rotation, inset: i32) -> Self {
		Self {
			global_rotation,
			inset,

			id_counter: 1,
			surfaces: Vec::new(),
			tasks: Vec::new(),

			shell: Shell {
				layers: Vec::new(),
				root: Vec::new(),
			},
		}
	}

	fn next_id(&mut self) -> Id {
		let ret = self.id_counter;
		self.id_counter = self.id_counter.wrapping_add(1);
		ret
	}

	async fn reassign_areas(&mut self) {
		let mut rect = Rectangle::new(Pos2::ZERO, Framebuffer::SIZE).inset(self.inset);

		for layer in &self.shell.layers {
			let surface = self
				.surfaces
				.iter_mut()
				.find(|surface| surface.id == layer.surface)
				.unwrap();
			let old = surface.description.base_rect;
			surface.description.base_rect = layer.anchor.take(layer.size, &mut rect);
			if old != surface.description.base_rect {
				let task = self
					.tasks
					.iter_mut()
					.find(|task| task.id == surface.task)
					.unwrap();
				// TODO: Should check this. We should handle dead tasks in general (by removing their surfaces).
				_ = task.channel.send(Event::Surface(surface.description)).await;
			}
		}

		if self.shell.root.is_empty() {
			return;
		}

		// For now this is hard-coded as an N-way horizontal split.
		let num_roots = self.shell.root.len();
		let root_width =
			self.global_rotation.transform_size(Framebuffer::SIZE).x / i32::try_from(num_roots).unwrap();
		for (i, &root) in self.shell.root.iter().enumerate() {
			let surface = self
				.surfaces
				.iter_mut()
				.find(|surface| surface.id == root)
				.unwrap();
			let this_rect = if i + 1 == num_roots {
				rect
			} else {
				Side::Left
					.rotate(self.global_rotation)
					.take(root_width, &mut rect)
			};

			let old = surface.description.base_rect;
			surface.description.base_rect = this_rect;
			if old != surface.description.base_rect {
				let task = self
					.tasks
					.iter_mut()
					.find(|task| task.id == surface.task)
					.unwrap();
				// TODO: Should check this. We should handle dead tasks in general (by removing their surfaces).
				_ = task.channel.send(Event::Surface(surface.description)).await;
			}
		}
	}

	async fn spawn_task<Fut: std::future::Future<Output = ()> + Send + 'static>(
		&mut self,
		f: impl FnOnce(TaskId, mpsc::Receiver<Event>) -> Fut,
	) -> (TaskId, mpsc::Sender<Event>) {
		let (chan_send, chan_recv) = mpsc::channel(2);
		let task_id = self.next_id();
		let handle = tokio::spawn(f(task_id, chan_recv));

		self.tasks.push(Task {
			id: task_id,
			handle,
			channel: chan_send.clone(),
		});

		(task_id, chan_send)
	}

	async fn create_surface(&mut self, task: TaskId, options: SurfaceInit) {
		let surface_id = self.next_id();
		let surface = Surface {
			id: surface_id,
			description: SurfaceDescription {
				// Will be set by `reassign_areas`.
				base_rect: Rectangle::ZERO,
				rotation: self.global_rotation,
				scale: 1,
			},
			task,
		};
		self.surfaces.push(surface);

		match options {
			SurfaceInit::Layer { anchor, size } => {
				let anchor = anchor.rotate(self.global_rotation);
				self.shell.layers.push(ShellLayer {
					anchor,
					size,
					surface: surface_id,
				});
			}
			SurfaceInit::Normal => {
				self.shell.root.push(surface_id);
			}
		}

		self.reassign_areas().await;
	}

	async fn open<Fut: std::future::Future<Output = ()> + Send + 'static>(
		&mut self,
		f: impl FnOnce(mpsc::Receiver<Event>) -> Fut,
		options: SurfaceInit,
	) {
		let (task_id, _) = self.spawn_task(|_, events| f(events)).await;
		self.create_surface(task_id, options).await;
	}
}

async fn test_task(mut channel: mpsc::Receiver<Event>) {
	let mut fb = Framebuffer::open().unwrap();

	while let Some(event) = channel.recv().await {
		match event {
			Event::Surface(desc) => {
				let mut fb = desc.transform(&mut fb);
				let rect = fb.bounding_box().into_styled(
					PrimitiveStyleBuilder::new()
						.fill_color(Rgb565::new(31, 63, 31))
						.stroke_color(Rgb565::new(0, 0, 0))
						.stroke_width(4)
						.stroke_alignment(StrokeAlignment::Inside)
						.build(),
				);
				rect.draw(&mut fb).unwrap();
				let line_style = PrimitiveStyleBuilder::new()
					.stroke_color(Rgb565::new(0, 0, 0))
					.stroke_width(2)
					.stroke_alignment(StrokeAlignment::Center)
					.reset_fill_color()
					.build();
				Polyline::new(&[Point::new(20, 20), Point::new(40, 30), Point::new(20, 40)])
					.into_styled(line_style)
					.draw(&mut fb)
					.unwrap();
				fb.update_partial(&rect.bounding_box().into(), UpdateStyle::Monochrome)
					.unwrap();
			}
			Event::Quit => break,
		}
	}
}

/// Run the window manager.
#[derive(argh::FromArgs, Debug)]
struct Args {
	/// the path of the control socket, which will be bound to and exposed for clients
	#[argh(option)]
	control_socket: PathBuf,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
	eprintln!("starting");

	let args: Args = argh::from_env();

	eprintln!("RMOX_SOCKET={}", args.control_socket.display());
	_ = std::fs::remove_file(&args.control_socket);
	let control_socket = tokio::net::UnixListener::bind(&args.control_socket)
		.unwrap_or_else(|error| panic!("opening socket at {:?}: {error}", args.control_socket));

	tracing_subscriber::fmt::fmt()
		.with_max_level(LevelFilter::INFO)
		.init();

	let mut input = Input::open().unwrap();

	let mut fb = Framebuffer::open().expect("open framebuffer");

	fb.clear(Rgb565::new(31, 63, 31)).unwrap();
	fb.update_all(UpdateStyle::Init).unwrap();
	std::thread::sleep(Duration::from_millis(500));
	eprintln!("cleared");

	let manager = Manager::new(Rotation::Rotate90, 4);
	// TODO: Is the `Arc` and `Mutex` necessary? Maybe there's a better way? Actor pattern?
	let manager = Arc::new(Mutex::new(manager));

	// TODO: Terminate the whole WM gracefully if anything fails, instead of panicking.
	loop {
		select! {
			res = control_socket.accept() => {
				eprintln!("new control socket connection {res:?}");
				let (mut client, _) = res.unwrap();
				manager.lock().await.spawn_task({
					// TODO: Remove the client (its task and its surfaces) if anything fails, instead of panicking.
					// Remember to treat EOFs on the socket as Ok.
					let manager = Arc::clone(&manager);
					move |task_id, mut event_recv| async move {
					let (client_r, mut client_w) = client.split();
						let commands = rmox_protocol::read_stream::<_, Command>(client_r);
						pin!(commands);
						loop {
							select! {
								Some(event) = event_recv.recv() => {
									eprintln!("received event {event:?} for task id {task_id}");
									rmox_protocol::write(&mut client_w, &event).await.unwrap();
								}
								Some(res) = commands.next() => {
									let command = res.unwrap();
									eprintln!("received command {command:?} from task id {task_id}");
									match command {
										Command::CreateSurface(options) => {
											manager.lock().await.create_surface(task_id, options).await;
										}
									}
								}
							}
						}
				}}).await;
			}
			Some(event) = input.next() => {
				let event = event.unwrap();
				// TODO: This kind of thing should be handled by a dedicated daemon and some kind of hotkey reservation protocol.
				match &event {
					rmox_input::Event::Key {
						scancode: _,
						key: Some(key),
						event: KeyEventKind::Press,
						modifiers,
					} => match key {
						Key::Enter if modifiers.opt() => {
							manager.lock().await.open(test_task, SurfaceInit::Normal).await;
						}
						_ => {}
					},
					_ => {}
				}
			}
		}
	}
}
