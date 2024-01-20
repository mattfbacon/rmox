use std::sync::Arc;
use std::time::Duration;

use alacritty_terminal::grid::Indexed;
use alacritty_terminal::term::cell::Cell;
use alacritty_terminal::term::{RenderableCursor, TermDamage};
use alacritty_terminal::Term;
use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::Dimensions;
use embedded_graphics::mono_font::{ascii as fonts, MonoTextStyle};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::text::{Baseline, Text};
use embedded_graphics::Drawable as _;
use rmox_common::eink_update::{EinkUpdateExt as _, UpdateStyle};
use rmox_common::types::{vec2, Rectangle, Vec2};
use rmox_fb::util::Scaled;
use rmox_fb::Framebuffer;
use rmox_input::keyboard::Key;
use rmox_protocol::client::recv::{
	Event, InputEvent, SurfaceDescription, SurfaceEvent, Transformed,
};
use rmox_protocol::client::send::{Command, SurfaceInit};
use tokio::time::Instant;
use tokio::{pin, select};
use tokio_stream::StreamExt as _;

#[derive(Debug, Clone, Copy)]
struct TermDimensions {
	scrollback_lines: usize,
	screen_lines: usize,
	columns: usize,
}

const SCROLLBACK: usize = 10_000;

impl TermDimensions {
	pub fn from_vec2(size: Vec2) -> Self {
		Self {
			scrollback_lines: SCROLLBACK,
			screen_lines: size.y.try_into().unwrap(),
			columns: size.x.try_into().unwrap(),
		}
	}
}

impl alacritty_terminal::grid::Dimensions for TermDimensions {
	fn total_lines(&self) -> usize {
		self.scrollback_lines + self.screen_lines
	}

	fn screen_lines(&self) -> usize {
		self.screen_lines
	}

	fn columns(&self) -> usize {
		self.columns
	}

	fn history_size(&self) -> usize {
		self.scrollback_lines
	}
}

struct LogListener(&'static str);

impl alacritty_terminal::event::EventListener for LogListener {
	fn send_event(&self, event: alacritty_terminal::event::Event) {
		eprintln!("term event (tag={:?}) {event:?}", self.0);
	}
}

struct ChannelListener(tokio::sync::mpsc::Sender<alacritty_terminal::event::Event>);

impl alacritty_terminal::event::EventListener for ChannelListener {
	fn send_event(&self, event: alacritty_terminal::event::Event) {
		self.0.blocking_send(event).unwrap();
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
		.write(&Command::CreateSurface(SurfaceInit::Normal))
		.await
		.unwrap();

	let mut fb = Framebuffer::open().unwrap();

	let fg = Rgb565::new(0, 0, 0);
	let bg = Rgb565::new(31, 63, 31);

	alacritty_terminal::tty::setup_env();

	let font = &fonts::FONT_6X10;
	let cell_size =
		(Vec2::from(font.character_size) + vec2(font.character_spacing.try_into().unwrap(), 0)) * 2;
	let config = alacritty_terminal::term::Config::default();
	let desc_to_dimensions = |desc: &SurfaceDescription| {
		let dimensions = desc.size().max_components(Vec2::ZERO) / cell_size;
		TermDimensions::from_vec2(dimensions)
	};
	let window_size = |dimensions: &TermDimensions| alacritty_terminal::event::WindowSize {
		num_lines: dimensions.screen_lines.try_into().unwrap(),
		num_cols: dimensions.columns.try_into().unwrap(),
		cell_width: cell_size.x.try_into().unwrap(),
		cell_height: cell_size.y.try_into().unwrap(),
	};
	// Dummy.
	let mut dimensions = TermDimensions::from_vec2(Vec2::splat(1));
	let (pty_event_send, mut pty_event_recv) = tokio::sync::mpsc::channel(8);
	let terminal = Term::new(config, &dimensions, ChannelListener(pty_event_send.clone()));
	let terminal = Arc::new(alacritty_terminal::sync::FairMutex::new(terminal));
	let pty_config = alacritty_terminal::tty::Options::default();
	let pty = alacritty_terminal::tty::new(&pty_config, window_size(&dimensions), 0).unwrap();
	let pty_loop = alacritty_terminal::event_loop::EventLoop::new(
		Arc::clone(&terminal),
		ChannelListener(pty_event_send),
		pty,
		pty_config.hold,
		false,
	);
	let pty_channel = pty_loop.channel();
	_ = pty_loop.spawn();

	let mut desc = None;
	let mut old_cursor = None;

	// Intentionally create an elapsed sleep.
	let pty_debounce = tokio::time::sleep_until(Instant::now() - Duration::from_secs(1));
	pin!(pty_debounce);
	loop {
		let mut full_update = false;
		select! {
			res = socket.next() => {
				let Some(res) = res else { break; };
				let event: Event = res.unwrap();
				match event {
					Event::Surface { id: _, event } => match event {
						SurfaceEvent::Description(new_desc) => {
							desc = Some(new_desc);
							dimensions = desc_to_dimensions(&new_desc);
							terminal.lock().resize(dimensions);
							pty_channel.send(alacritty_terminal::event_loop::Msg::Resize(window_size(&dimensions))).unwrap();
							full_update = true;
						}
						SurfaceEvent::Quit => break,
						SurfaceEvent::Input(input) => match input {
							InputEvent::Key(event) => {
								if !event.event.press() {
									continue;
								}
								let Some(key) = event.key else { continue; };
								// TODO: Ctrl-C (C-a is \x01, C-b is \x02, etc). Blocked by the `Key` refactor.
								let raw = match key {
									Key::Backspace => b"\x7f".as_slice(),
									Key::ArrowLeft => b"\x1b[D".as_slice(),
									Key::ArrowRight => b"\x1b[C".as_slice(),
									Key::ArrowUp => b"\x1b[A".as_slice(),
									Key::ArrowDown => b"\x1b[B".as_slice(),
									Key::Home => b"\x1b[H".as_slice(),
									Key::End => b"\x1b[F".as_slice(),
									Key::PageUp => b"\x1b5~".as_slice(),
									Key::PageDown => b"\x1b6~".as_slice(),
									Key::Insert => b"\x1b2~".as_slice(),
									Key::Delete => b"\x1b3~".as_slice(),
									Key::Escape => b"\x1b".as_slice(),
									_ => continue,
								};
								pty_channel.send(alacritty_terminal::event_loop::Msg::Input(raw.into())).unwrap();
								continue;
							},
							InputEvent::Text(text) => pty_channel.send(alacritty_terminal::event_loop::Msg::Input(String::from(text).into_bytes().into())).unwrap(),
							_ => continue,
						},
					},
				}
			}
			Some(event) = pty_event_recv.recv() => {
				use alacritty_terminal::event::Event as E;
				match event {
					// TODO: Anything else we need to do here?
					E::MouseCursorDirty => {}
					// TODO: Title support in the WM.
					E::Title(..) | E::ResetTitle => continue,
					// TODO: Clipboard support in the WM.
					E::ClipboardStore(..) | E::ClipboardLoad(..) => continue,
					// TODO: Change if/when implementing colors.
					E::ColorRequest(_index, format) => {
						let color = format(alacritty_terminal::vte::ansi::Rgb { r: 0, g: 0, b: 0 });
						pty_channel.send(alacritty_terminal::event_loop::Msg::Input(color.into_bytes().into())).unwrap();
					}
					E::PtyWrite(text) => pty_channel.send(alacritty_terminal::event_loop::Msg::Input(text.into_bytes().into())).unwrap(),
					E::TextAreaSizeRequest(format) => pty_channel.send(alacritty_terminal::event_loop::Msg::Input(format(window_size(&dimensions)).into_bytes().into())).unwrap(),
					// Not implemented.
					E::CursorBlinkingChange | E::Bell => continue,
					E::Wakeup => {},
					E::Exit => break,
				}
				// TODO: Is it necessary to debounce?
				pty_debounce.as_mut().reset(Instant::now() + Duration::from_millis(5));
				continue;
			}
			_ = &mut pty_debounce, if !pty_debounce.is_elapsed() => { /* fall through to drawing code */ }
		}

		let Some(desc) = desc else {
			continue;
		};

		let mut fb = desc.transform(&mut fb);

		let point_to_pos = |point: alacritty_terminal::index::Point| {
			let point = vec2(point.column.0.try_into().unwrap(), point.line.0);
			(point * cell_size).to_pos()
		};
		let draw_cell = |fb: &mut Transformed<Framebuffer>, cell: Indexed<&Cell>| {
			let pos = point_to_pos(cell.point);

			let mut str_buf = [0u8; 4];
			let str = cell.c.encode_utf8(&mut str_buf);

			// TODO: Use the actual color from the cell.
			let cell_fg = fg;
			let cell_bg = bg;

			// Setting the background in the `MonoTextStyle` isn't enough to clear the cell because of extra line spacing.
			fb.fill_solid(&Rectangle::new(pos, cell_size).into(), cell_bg)
				.unwrap();
			Text::with_baseline(
				str,
				(pos / 2).into(),
				MonoTextStyle::new(font, cell_fg),
				Baseline::Top,
			)
			.draw(&mut Scaled::<_, 2>(fb))
			.unwrap();

			Rectangle::new(pos, cell_size)
		};
		let cursor_rect = |cursor: &RenderableCursor| {
			let cursor_pos = point_to_pos(cursor.point);
			Rectangle::new(cursor_pos, cell_size)
		};
		let draw_cursor = |fb: &mut Transformed<Framebuffer>, cursor: &RenderableCursor| {
			let cursor_rect = cursor_rect(cursor);
			// Just using the underline cursor for now since it refreshes better.
			/*
			match cursor.shape {
				alacritty_terminal::vte::ansi::CursorShape::Block => {
					fb.fill_solid(&cursor_rect.into(), fg).unwrap();
				}
				alacritty_terminal::vte::ansi::CursorShape::Underline => {
					let y = cursor_rect.end().y - 2;
					fb.fill_solid(&cursor_rect.with_y(y).with_height(2).into(), fg)
						.unwrap();
				}
				alacritty_terminal::vte::ansi::CursorShape::Beam => {
					fb.fill_solid(&cursor_rect.with_x(2).into(), fg).unwrap();
				}
				alacritty_terminal::vte::ansi::CursorShape::HollowBlock => {
					fb.fill_solid(&cursor_rect.into(), fg).unwrap();
					fb.fill_solid(&cursor_rect.inset(2).into(), bg).unwrap();
				}
				alacritty_terminal::vte::ansi::CursorShape::Hidden => {
					fb.fill_solid(&cursor_rect.into(), bg).unwrap();
				}
			}
			*/
			let y = cursor_rect.end().y - 2;
			let cursor_rect = cursor_rect.with_y(y).with_height(2);
			fb.fill_solid(&cursor_rect.into(), fg).unwrap();
			cursor_rect
		};
		let get_cursor = |terminal: &Term<_>| alacritty_terminal::term::RenderableCursor {
			shape: terminal.cursor_style().shape,
			point: terminal.grid().cursor.point,
		};

		let mut terminal = terminal.lock();
		let damage = terminal.damage();
		let partial_damage = match damage {
			// Since `TermDamageIterator` mutably borrows `terminal`, I don't think we can avoid this `collect`.
			TermDamage::Partial(damage) if !full_update => Some(damage.collect::<Vec<_>>()),
			_ => None,
		};
		if let Some(partial_damage) = partial_damage {
			let grid = terminal.grid();
			for bounds in partial_damage {
				let line = alacritty_terminal::index::Line(bounds.line.try_into().unwrap());
				let row = &grid[line];
				if !bounds.is_damaged() {
					continue;
				}
				for column in bounds.left..bounds.right {
					let column = alacritty_terminal::index::Column(column);
					let point = alacritty_terminal::index::Point { line, column };
					let cell = &row[column];
					draw_cell(&mut fb, alacritty_terminal::grid::Indexed { point, cell });
				}
				let rect = Rectangle::from_corners(
					point_to_pos(alacritty_terminal::index::Point {
						line,
						column: bounds.left.into(),
					}),
					point_to_pos(alacritty_terminal::index::Point {
						line: line + 1,
						column: bounds.right.into(),
					}),
				);
				fb.update_partial(&rect, UpdateStyle::Monochrome).unwrap();
			}
			let new_cursor = get_cursor(&terminal);
			let old_cursor = old_cursor.replace(new_cursor);
			if old_cursor != Some(new_cursor) {
				if let Some(old_cursor) = old_cursor {
					let point = old_cursor.point;
					let cell_rect = draw_cell(
						&mut fb,
						alacritty_terminal::grid::Indexed {
							point,
							cell: &terminal.grid()[point],
						},
					);
					fb.update_partial(&cell_rect, UpdateStyle::Monochrome)
						.unwrap();
				}

				let cursor_rect = draw_cursor(&mut fb, &new_cursor);
				fb.update_partial(&cursor_rect, UpdateStyle::Monochrome)
					.unwrap();
			}
		} else {
			fb.clear(bg).unwrap();
			let content = terminal.renderable_content();
			for cell in content.display_iter {
				draw_cell(&mut fb, cell);
			}

			_ = draw_cursor(&mut fb, &content.cursor);
			old_cursor = Some(content.cursor);

			fb.update_partial(&fb.bounding_box().into(), UpdateStyle::Monochrome)
				.unwrap();
		}
		terminal.reset_damage();
		drop(terminal);
	}
}
