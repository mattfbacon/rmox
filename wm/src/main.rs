use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;
use rmox_common::eink_update::{EinkUpdateExt as _, UpdateStyle};
use rmox_common::types::{Pos2, Rectangle, Rotation, Side};
use rmox_fb::Framebuffer;
use rmox_input::Input;
use rmox_protocol::server::recv::{Command, SurfaceInit};
use rmox_protocol::server::send::{Event, InputEvent, SurfaceDescription};
use rmox_protocol::server_to_client::{StylusEvent, StylusPhase, TouchEvent, TouchPhase};
use rmox_protocol::{Id, SurfaceId, TaskId};
use tokio::sync::mpsc;
use tokio::{pin, select};
use tokio_stream::StreamExt as _;
use tracing_subscriber::filter::LevelFilter;

#[derive(Debug, Clone, Copy)]
struct Surface {
	description: SurfaceDescription,
	task: TaskId,
}

#[derive(Debug)]
struct Task {
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
	surfaces: HashMap<SurfaceId, Surface>,
	tasks: HashMap<TaskId, Task>,

	keyboard_focused_surface: Option<SurfaceId>,

	shell: Shell,

	input: Input,
}

enum ManagerCommand {
	CreateSurface { task: TaskId, options: SurfaceInit },
	RemoveTask { task: TaskId },
}

#[derive(Clone)]
struct ManagerHandle {
	channel: mpsc::Sender<ManagerCommand>,
}

// TODO: Avoid unwraps when getting tasks and surfaces by IDs.
impl Manager {
	fn new(global_rotation: Rotation, inset: i32) -> std::io::Result<Self> {
		Ok(Self {
			global_rotation,
			inset,

			id_counter: 1,
			surfaces: HashMap::new(),
			tasks: HashMap::new(),

			keyboard_focused_surface: None,

			shell: Shell {
				layers: Vec::new(),
				root: Vec::new(),
			},

			input: Input::open()?,
		})
	}

	fn next_id(&mut self) -> Id {
		let ret = self.id_counter;
		self.id_counter = self.id_counter.wrapping_add(1);
		ret
	}

	fn prune_shell(&mut self) {
		self
			.shell
			.layers
			.retain(|layer| self.surfaces.contains_key(&layer.surface));
		self
			.shell
			.root
			.retain(|surface| self.surfaces.contains_key(surface));
	}

	fn remove_task_(&mut self, id: TaskId) {
		self.tasks.remove(&id);
		self.surfaces.retain(|_, surface| surface.task != id);

		self.prune_shell();

		if self
			.keyboard_focused_surface
			.map_or(false, |id| !self.surfaces.contains_key(&id))
		{
			self.keyboard_focused_surface = self.shell.root.last().copied();
		}
	}

	/// Since the removal of the task's surfaces may affect layout, this calls `reassign_areas`.
	async fn remove_task(&mut self, id: TaskId) {
		self.remove_task_(id);
		self.reassign_areas().await;
	}

	// TODO: Avoid the duplication of the interior of the `for` loops.
	async fn reassign_areas(&mut self) {
		'outer: loop {
			let mut rect = Rectangle::new(Pos2::ZERO, Framebuffer::SIZE).inset(self.inset);

			for layer in &self.shell.layers {
				let surface_id = layer.surface;
				let surface = self.surfaces.get_mut(&surface_id).unwrap();
				let old = surface.description.base_rect;
				surface.description.base_rect = layer.anchor.take(layer.size, &mut rect);
				if old != surface.description.base_rect {
					let task_id = surface.task;
					let task = self.tasks.get_mut(&task_id).unwrap();
					let event = Event::Surface {
						id: surface_id,
						description: surface.description,
					};
					if task.channel.send(event).await.is_err() {
						self.remove_task_(task_id);
						// We need to restart the assignment because previously processed surfaces may have also been owned by this task and thus removed.
						// The check for a differing `base_rect` should avoid repetition of surface assignments to clients.
						// Nonetheless it is somewhat suboptimal if the `send` fails later in this process,
						// because removal of other earlier surfaces might cause some tasks to be assigned different surfaces in quick succession.
						continue 'outer;
					}
				}
			}

			if self.shell.root.is_empty() {
				return;
			}

			// For now this is hard-coded as an N-way horizontal split.
			let num_roots = self.shell.root.len();
			let root_width = self
				.global_rotation
				.transform_size(Framebuffer::SIZE)
				.x
				.abs() / i32::try_from(num_roots).unwrap();
			for (i, &root) in self.shell.root.iter().enumerate() {
				let surface_id = root;
				let surface = self.surfaces.get_mut(&surface_id).unwrap();
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
					let task_id = surface.task;
					let task = self.tasks.get_mut(&task_id).unwrap();
					let event = Event::Surface {
						id: surface_id,
						description: surface.description,
					};
					if task.channel.send(event).await.is_err() {
						self.remove_task_(task_id);
						continue 'outer;
					}
				}
			}

			break;
		}
	}

	async fn spawn_task(
		&mut self,
		mut client: tokio::net::UnixStream,
		handle: ManagerHandle,
	) -> (TaskId, mpsc::Sender<Event>) {
		let (event_send, mut event_recv) = mpsc::channel(2);
		let task_id = self.next_id();
		tokio::spawn(async move {
			let (client_r, mut client_w) = client.split();
			let commands = rmox_protocol::io::read_stream::<_, Command>(client_r);
			pin!(commands);
			loop {
				select! {
					Some(event) = event_recv.recv() => {
						tracing::debug!(?task_id, ?event, "received event for client");
						let res = rmox_protocol::io::write(&mut client_w, &event).await;
						if let Err(error) = res {
							tracing::warn!(?task_id, ?error, "error writing to client");
							break;
						}
					}
					res = commands.next() => {
						match res {
							Some(Ok(command)) => {
								tracing::debug!(?task_id, ?command, "received command from client");
								match command {
									Command::CreateSurface(options) => {
										handle.create_surface(task_id, options).await;
									}
								}
							}
							None => break,
							Some(Err(error)) => {
								tracing::warn!(?task_id, ?error, "error reading from client");
								break;
							}
						}
					}
				}
			}
			tracing::debug!(?task_id, "client loop ended");
			handle.remove_task(task_id).await;
		});

		self.tasks.insert(
			task_id,
			Task {
				channel: event_send.clone(),
			},
		);

		(task_id, event_send)
	}

	async fn create_surface(&mut self, task: TaskId, options: SurfaceInit) {
		let surface_id = self.next_id();
		let surface = Surface {
			description: SurfaceDescription {
				// Will be set by `reassign_areas`.
				base_rect: Rectangle::ZERO,
				rotation: self.global_rotation,
				scale: 1,
			},
			task,
		};
		self.surfaces.insert(surface_id, surface);

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
				// As a rule, we consider normal surfaces to be keyboard-focusable and layer surfaces to not be.
				// We may change this if necessary, e.g., for dmenu-type things.
				self.keyboard_focused_surface = Some(surface_id);
			}
		}

		self.reassign_areas().await;
	}

	async fn handle_input(&mut self, event: rmox_input::Event) {
		let surface_id = match &event {
			rmox_input::Event::Key(_) | rmox_input::Event::Text(_) | rmox_input::Event::Button(_) => {
				let Some(surface_id) = self.keyboard_focused_surface else {
					return;
				};
				surface_id
			}
			rmox_input::Event::Touch(_) | rmox_input::Event::Stylus(_) => {
				// TODO: Find the surface based on the location of the event.
				// Also, we will need to send a leave event to one surface and an enter event to another in some cases.
				tracing::warn!(?event, "touch/stylus event not yet implemented");
				return;
			}
			rmox_input::Event::DevicePresence(_) => return,
		};
		let event = match event {
			rmox_input::Event::Key(v) => InputEvent::Key(v),
			rmox_input::Event::Text(v) => InputEvent::Text(v),
			rmox_input::Event::Button(v) => InputEvent::Button(v),
			rmox_input::Event::Touch(event) => InputEvent::Touch(TouchEvent {
				id: event.touch_id,
				phase: match event.phase {
					rmox_input::touch::Phase::Start => {
						TouchPhase::Start(self.input.touch_state(event.touch_id).unwrap())
					}
					rmox_input::touch::Phase::Change => {
						TouchPhase::Change(self.input.touch_state(event.touch_id).unwrap())
					}
					rmox_input::touch::Phase::End => TouchPhase::End,
				},
			}),
			rmox_input::Event::Stylus(event) => InputEvent::Stylus(StylusEvent {
				phase: match event.phase {
					rmox_input::stylus::Phase::Hover => {
						StylusPhase::Hover(self.input.stylus_state().unwrap())
					}
					rmox_input::stylus::Phase::Touch => {
						StylusPhase::Touch(self.input.stylus_state().unwrap())
					}
					rmox_input::stylus::Phase::Change => {
						StylusPhase::Change(self.input.stylus_state().unwrap())
					}
					rmox_input::stylus::Phase::Lift => StylusPhase::Lift(self.input.stylus_state().unwrap()),
					rmox_input::stylus::Phase::Leave => StylusPhase::Leave,
				},
			}),
			rmox_input::Event::DevicePresence(_) => return,
		};
		let surface = self.surfaces.get(&surface_id).unwrap();
		let task_id = surface.task;
		let task = self.tasks.get(&task_id).unwrap();
		let event = Event::Input {
			surface: surface_id,
			event,
		};
		if task.channel.send(event).await.is_err() {
			self.remove_task(task_id).await;
		}
	}
}

impl ManagerHandle {
	async fn create_surface(&self, task: TaskId, options: SurfaceInit) {
		let command = ManagerCommand::CreateSurface { task, options };
		self.channel.send(command).await.unwrap();
	}

	async fn remove_task(&self, task: TaskId) {
		let command = ManagerCommand::RemoveTask { task };
		self.channel.send(command).await.unwrap();
	}
}

/*
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
*/

/// Run the window manager.
#[derive(argh::FromArgs, Debug)]
struct Args {
	/// the path of the control socket, which will be bound to and exposed for clients
	#[argh(option)]
	control_socket: PathBuf,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
	tracing_subscriber::fmt::fmt()
		.with_max_level(LevelFilter::INFO)
		.init();

	tracing::info!("starting");

	let args: Args = argh::from_env();

	tracing::info!("RMOX_SOCKET={}", args.control_socket.display());
	_ = std::fs::remove_file(&args.control_socket);
	let control_socket = tokio::net::UnixListener::bind(&args.control_socket)
		.unwrap_or_else(|error| panic!("opening socket at {:?}: {error}", args.control_socket));

	let mut fb = Framebuffer::open().expect("open framebuffer");

	fb.clear(Rgb565::new(31, 63, 31)).unwrap();
	fb.update_all(UpdateStyle::Init).unwrap();
	std::thread::sleep(Duration::from_millis(500));
	tracing::info!("cleared");

	let mut manager = Manager::new(Rotation::Rotate90, 4).unwrap();

	let (command_send, mut command_recv) = mpsc::channel(2);

	let handle = ManagerHandle {
		channel: command_send,
	};

	loop {
		select! {
			res = control_socket.accept() => {
				let client = match res {
					Ok((client, _)) => client,
					Err(error) => {
						tracing::error!(?error, "control socket error");
						break;
					}
				};
				tracing::debug!("new control socket connection");
				manager.spawn_task(client, handle.clone()).await;
			}
			Some(command) = command_recv.recv() => {
				match command {
					ManagerCommand::CreateSurface { task, options } => {
						manager.create_surface(task, options).await;
					}
					ManagerCommand::RemoveTask { task } => {
						manager.remove_task(task).await;
					}
				}
			}
			Some(event) = manager.input.next() => {
				let event = match event {
					Ok(event) => event,
					Err(error) => {
						tracing::error!(?error, "input event recv error");
						break;
					}
				};
				tracing::trace!(?event, "input event through WM");
				/*
				// TODO: This kind of thing should be handled by a dedicated daemon and some kind of hotkey reservation protocol.
				match &event {
					rmox_input::Event::Key(rmox_input::keyboard::KeyEvent {
						scancode: _,
						key: Some(key),
						event: KeyEventKind::Press,
						modifiers,
					}) => match key {
						Key::Enter if modifiers.opt() => {
							manager.open(test_task, SurfaceInit::Normal).await;
						}
						_ => {}
					},
					_ => {}
				}
				*/
				manager.handle_input(event).await;
			}
		}
	}
}
