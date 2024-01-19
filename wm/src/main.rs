use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;
use rmox_common::eink_update::{EinkUpdateExt as _, UpdateStyle};
use rmox_common::types::{Pos2, Rectangle, Rotation, Side};
use rmox_fb::Framebuffer;
use rmox_input::keyboard::Key;
use rmox_input::Input;
use rmox_protocol::server::recv::{Command, SurfaceInit};
use rmox_protocol::server::send::{Event, InputEvent, SurfaceDescription, SurfaceEvent};
use rmox_protocol::server_to_client::{StylusEvent, StylusPhase, TouchEvent, TouchPhase};
use rmox_protocol::{Id, SurfaceId, TaskId};
use tokio::sync::mpsc;
use tokio::{pin, select};
use tokio_stream::StreamExt as _;

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
enum ContainerKind {
	Horizontal,
	Vertical,
}

#[derive(Debug)]
struct Container {
	kind: ContainerKind,
	children: Vec<ShellNode>,
}

impl Container {
	fn retain(&mut self, f: &mut impl FnMut(SurfaceId) -> bool) -> bool {
		self.children.retain_mut(|child| child.retain(f));
		!self.children.is_empty()
	}

	fn fix_path(&self, path: &mut Vec<u8>, i: usize) {
		let container_index = if let Some(&index) = path.get(i) {
			index
		} else {
			// Arbitrary choice.
			let index = 0;
			path.push(index);
			index
		};
		let child = if let Some(child) = self.children.get(usize::from(container_index)) {
			child
		} else {
			path[i] = (self.children.len() - 1).try_into().unwrap();
			// We assert that containers have at least one item.
			self.children.last().unwrap()
		};
		child.fix_path(path, i + 1);
	}

	fn get_path(&self, path: &[u8]) -> Option<&ShellNode> {
		let [index, rest @ ..] = path else {
			// Path is not deep enough.
			return None;
		};

		match &self.children[usize::from(*index)] {
			ShellNode::Container(container) => container.get_path(rest),
			// If `None`, path is too deep.
			node @ ShellNode::Surface(_) => rest.is_empty().then_some(node),
		}
	}

	fn get_container_mut(&mut self, path: &[u8]) -> Option<&mut Self> {
		let [index, rest @ ..] = path else {
			return Some(self);
		};

		match &mut self.children[usize::from(*index)] {
			ShellNode::Container(container) => container.get_container_mut(rest),
			// Path is too deep.
			ShellNode::Surface(_) => None,
		}
	}
}

#[derive(Debug)]
enum ShellNode {
	Container(Container),
	Surface(SurfaceId),
}

impl ShellNode {
	/// The returned `bool` indicates if this node itself should be retained.
	fn retain(&mut self, f: &mut impl FnMut(SurfaceId) -> bool) -> bool {
		match self {
			Self::Container(container) => container.retain(f),
			Self::Surface(id) => f(*id),
		}
	}

	fn fix_path(&self, path: &mut Vec<u8>, i: usize) {
		match self {
			Self::Surface(_) => {
				path.truncate(i + 1);
			}
			Self::Container(container) => container.fix_path(path, i),
		}
	}
}

#[derive(Debug)]
struct Shell {
	layers: Vec<ShellLayer>,
	root: Option<Container>,
	wallpaper: Option<SurfaceId>,
}

impl Shell {
	fn retain(&mut self, mut f: impl FnMut(SurfaceId) -> bool) {
		self.layers.retain(|layer| f(layer.surface));
		if let Some(root) = &mut self.root {
			if !root.retain(&mut f) {
				self.root = None;
			}
		}
		self.wallpaper = self.wallpaper.filter(|id| f(*id));
	}

	fn get_path(&self, path: &[u8]) -> Option<&ShellNode> {
		let root = self.root.as_ref()?;
		root.get_path(path)
	}

	fn fix_path(&mut self, path: &mut Option<Vec<u8>>) {
		if let Some(root) = &mut self.root {
			if let Some(path) = path {
				root.fix_path(path, 0);
			}
		} else {
			*path = None;
		}
	}
}

#[derive(Debug)]
struct ManagerConfig {
	global_rotation: Rotation,
	inset: i32,
}

#[derive(Debug)]
struct ManagerState {
	config: ManagerConfig,

	id_counter: Id,

	surfaces: HashMap<SurfaceId, Surface>,
	tasks: HashMap<TaskId, Task>,
	// The `Vec` represents a path into `shell.root` where each item is an index into the children of a container, e.g., `Some(vec![1])` is the second child of the root container.
	keyboard_focused_container: Option<Vec<u8>>,
}

impl ManagerState {
	fn next_id(&mut self) -> Id {
		let ret = self.id_counter;
		self.id_counter = self.id_counter.step();
		ret
	}

	fn reassign_container(
		&mut self,
		container: &Container,
		rect: &Rectangle,
		dirty_surfaces: &mut Vec<SurfaceId>,
	) {
		tracing::trace!(?rect, "reassignment - reassign container");
		let child_side = match container.kind {
			ContainerKind::Horizontal => Side::Left,
			ContainerKind::Vertical => Side::Top,
		}
		.rotate(self.config.global_rotation);
		let child_size = match child_side {
			Side::Top | Side::Bottom => rect.size.y,
			Side::Left | Side::Right => rect.size.x,
		} / i32::try_from(container.children.len()).unwrap();
		let mut rect = *rect;
		for child in &container.children[..container.children.len() - 1] {
			let child_rect = child_side.take(child_size, &mut rect);
			self.reassign_node(child, &child_rect, dirty_surfaces);
		}
		self.reassign_node(container.children.last().unwrap(), &rect, dirty_surfaces);
	}

	fn reassign_node(
		&mut self,
		node: &ShellNode,
		rect: &Rectangle,
		dirty_surfaces: &mut Vec<SurfaceId>,
	) {
		tracing::trace!(?rect, "reassignment - reassign node");
		match node {
			ShellNode::Container(container) => self.reassign_container(container, rect, dirty_surfaces),
			ShellNode::Surface(id) => {
				let surface = self.surfaces.get_mut(id).unwrap();
				if surface.description.base_rect != *rect {
					dirty_surfaces.push(*id);
				}
				surface.description.base_rect = *rect;
			}
		}
	}
}

#[derive(Debug)]
struct Manager {
	state: ManagerState,
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
	fn new(config: ManagerConfig) -> std::io::Result<Self> {
		Ok(Self {
			state: ManagerState {
				config,

				id_counter: Id::START,

				surfaces: HashMap::new(),
				tasks: HashMap::new(),
				keyboard_focused_container: None,
			},
			shell: Shell {
				layers: Vec::new(),
				root: None,
				wallpaper: None,
			},
			input: Input::open()?,
		})
	}

	fn prune_shell(&mut self) {
		tracing::trace!(?self.shell, ?self.state.keyboard_focused_container, "prune shell - before");
		self
			.shell
			.retain(|surface| self.state.surfaces.contains_key(&surface));
		self
			.shell
			.fix_path(&mut self.state.keyboard_focused_container);
		tracing::trace!(?self.shell, ?self.state.keyboard_focused_container, "prune shell - after");
	}

	fn remove_task_(&mut self, id: TaskId) {
		let Some(_) = self.state.tasks.remove(&id) else {
			return;
		};
		self.state.surfaces.retain(|_, surface| surface.task != id);
		self.prune_shell();
	}

	async fn remove_surface_(&mut self, id: SurfaceId) -> Result<(), ()> {
		let Some(surface) = self.state.surfaces.remove(&id) else {
			return Err(());
		};
		let task_id = surface.task;
		let task = self.state.tasks.get(&task_id).unwrap();
		if task
			.channel
			.send(Event::Surface {
				id,
				event: SurfaceEvent::Quit,
			})
			.await
			.is_err()
		{
			self.remove_task(task_id).await;
			return Err(());
		}

		Ok(())
	}

	async fn remove_surface(&mut self, id: SurfaceId) -> Result<(), ()> {
		self.remove_surface_(id).await?;
		self.prune_shell();
		self.reassign_areas().await;
		Ok(())
	}

	/// Since the removal of the task's surfaces may affect layout, this calls `reassign_areas`.
	async fn remove_task(&mut self, id: TaskId) {
		self.remove_task_(id);
		self.reassign_areas().await;
	}

	async fn reassign_areas(&mut self) {
		tracing::trace!("reassign areas");
		let mut dirty_surfaces = Vec::new();
		'outer: loop {
			dirty_surfaces.clear();

			let mut rect = Rectangle::new(Pos2::ZERO, Framebuffer::SIZE).inset(self.state.config.inset);

			for layer in &self.shell.layers {
				tracing::trace!(?layer, "reassignment - processing layer");
				let surface_id = layer.surface;
				let new_rect = layer.anchor.take(layer.size, &mut rect);
				let surface = self.state.surfaces.get_mut(&surface_id).unwrap();
				if new_rect != surface.description.base_rect {
					dirty_surfaces.push(surface_id);
				}
				surface.description.base_rect = new_rect;
			}

			if let Some(wallpaper) = self.shell.wallpaper {
				let surface = self.state.surfaces.get_mut(&wallpaper).unwrap();
				let old = surface.description;
				surface.description.base_rect = rect;
				surface.description.visible = self.shell.root.is_none();
				if old != surface.description {
					dirty_surfaces.push(wallpaper);
				}
			}

			if let Some(root) = &self.shell.root {
				self
					.state
					.reassign_container(root, &rect, &mut dirty_surfaces);
			};

			tracing::trace!(num_dirty=?dirty_surfaces.len(), "processing dirty surfaces");
			for &surface_id in &dirty_surfaces {
				let surface = self.state.surfaces.get(&surface_id).unwrap();
				tracing::trace!(?surface_id, ?surface, "processing dirty surface");
				let task_id = surface.task;
				let task = self.state.tasks.get(&task_id).unwrap();
				let event = Event::Surface {
					id: surface_id,
					event: SurfaceEvent::Description(surface.description),
				};
				if task.channel.send(event).await.is_err() {
					self.remove_task_(task_id);
					// We need to restart the assignment because previously processed surfaces may have also been owned by this task and thus removed.
					// The check for a differing `base_rect` should avoid repetition of surface assignments to clients.
					// Nonetheless it is somewhat suboptimal if the `send` fails later in this process,
					// because removal of other earlier surfaces might cause some tasks to be assigned different surfaces in quick succession.
					tracing::trace!("task had to be removed while reassigning, repeating reassignment");
					continue 'outer;
				}
			}
			break;
		}
	}

	async fn spawn_task(
		&mut self,
		client: tokio::net::UnixStream,
		handle: ManagerHandle,
	) -> (TaskId, mpsc::Sender<Event>) {
		let (event_send, mut event_recv) = mpsc::channel(2);
		let task_id = TaskId(self.state.next_id());
		tokio::spawn(async move {
			let client = rmox_protocol::io::Stream::new(client);
			pin!(client);
			loop {
				select! {
					Some(event) = event_recv.recv() => {
						tracing::debug!(?task_id, ?event, "received event for client");
						let res = client.write(&event).await;
						if let Err(error) = res {
							tracing::warn!(?task_id, ?error, "error writing to client");
							break;
						}
					}
					res = client.next() => {
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

		self.state.tasks.insert(
			task_id,
			Task {
				channel: event_send.clone(),
			},
		);

		(task_id, event_send)
	}

	async fn create_surface(&mut self, task: TaskId, options: SurfaceInit) {
		tracing::trace!(?task, ?options, "create surface");
		let surface_id = SurfaceId(self.state.next_id());
		let surface = Surface {
			description: SurfaceDescription {
				// Will be set by `reassign_areas`.
				base_rect: Rectangle::ZERO,
				rotation: self.state.config.global_rotation,
				scale: 1,
				visible: true,
			},
			task,
		};
		self.state.surfaces.insert(surface_id, surface);

		match options {
			SurfaceInit::Layer { anchor, size } => {
				let anchor = anchor.rotate(self.state.config.global_rotation);
				self.shell.layers.push(ShellLayer {
					anchor,
					size,
					surface: surface_id,
				});
			}
			SurfaceInit::Normal => {
				// As a rule, we consider normal surfaces to be keyboard-focusable and any others to not be.
				// We may change this if necessary, e.g., for dmenu-type things.

				if let Some(root) = &mut self.shell.root {
					let path = self.state.keyboard_focused_container.as_mut().unwrap();
					// Get the container of the currently focused node by removing the last path segment.
					let container = root.get_container_mut(&path[..path.len() - 1]).unwrap();
					container.children.push(ShellNode::Surface(surface_id));
					*path.last_mut().unwrap() = (container.children.len() - 1).try_into().unwrap();
				} else {
					self.shell.root = Some(Container {
						kind: ContainerKind::Horizontal,
						children: vec![ShellNode::Surface(surface_id)],
					});
					self.state.keyboard_focused_container = Some(vec![0]);
				}
			}
			SurfaceInit::Wallpaper => {
				let old = self.shell.wallpaper.replace(surface_id);
				if let Some(old) = old {
					_ = self.remove_surface(old).await;
				}
			}
		}

		self.reassign_areas().await;
	}

	// TODO: If a parent container of a surface is focused,
	// there may be some situations where we want to force the focus to one of the child surfaces,
	// e.g., if the user types on the keyboard.
	// Not sure if jumping to a child surface is better or worse than ignoring the keyboard input entirely.
	fn focused_surface(&self) -> Option<SurfaceId> {
		if let Some(ShellNode::Surface(surface_id)) = self
			.state
			.keyboard_focused_container
			.as_deref()
			.and_then(|path| self.shell.get_path(path))
		{
			Some(*surface_id)
		} else {
			None
		}
	}

	async fn handle_input(&mut self, event: rmox_input::Event) {
		// TODO: This kind of thing should be handled by a dedicated daemon and some kind of hotkey reservation protocol.
		if let rmox_input::Event::Key(event) = &event {
			if event.event.press() {
				if let Some(surface_id) = self.focused_surface() {
					if let Some(key) = event.key {
						match key {
							Key::X if event.modifiers.opt() && event.modifiers.shift(false) => {
								tracing::trace!(?surface_id, "M-S-x, removing surface");
								_ = self.remove_surface(surface_id).await;
								return;
							}
							// TODO: Bindings for changing container kind, selecting parent container, and changing focus.
							_ => {}
						}
					}
				}
			}
		}

		let surface_id = match &event {
			rmox_input::Event::Key(_) | rmox_input::Event::Text(_) | rmox_input::Event::Button(_) => {
				let Some(surface_id) = self.focused_surface() else {
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
		let surface = self.state.surfaces.get(&surface_id).unwrap();
		let task_id = surface.task;
		let task = self.state.tasks.get(&task_id).unwrap();
		let event = Event::Surface {
			id: surface_id,
			event: SurfaceEvent::Input(event),
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

/// Run the window manager.
#[derive(argh::FromArgs, Debug)]
struct Args {
	/// the path of the control socket, which will be bound to and exposed for clients
	#[argh(option)]
	control_socket: PathBuf,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
	tracing_subscriber::fmt::init();

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

	let config = ManagerConfig {
		global_rotation: Rotation::Rotate90,
		inset: 4,
	};
	let mut manager = Manager::new(config).unwrap();

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
				manager.handle_input(event).await;
			}
		}
	}
}
