#![deny(
	absolute_paths_not_starting_with_crate,
	keyword_idents,
	macro_use_extern_crate,
	meta_variable_misuse,
	missing_abi,
	missing_copy_implementations,
	non_ascii_idents,
	nonstandard_style,
	noop_method_call,
	pointer_structural_match,
	rust_2018_idioms,
	unused_qualifications
)]
#![warn(clippy::pedantic)]
#![forbid(unsafe_code)]
// TODO: Clean up panics and unwraps.

use std::collections::VecDeque;
use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll};

use embedded_graphics_core::geometry::Point;
use evdev::{AbsoluteAxisCode, Device, EventSummary, EventType, FetchEventsSynced, KeyCode};
use futures_core::{ready, Stream};
use tokio::sync::mpsc;

pub use crate::key::{Key, Scancode};
use crate::layout::DefaultLayout;
pub use crate::layout::{KeyboardLayout, Resolved};
pub use crate::modifiers::{Modifier, Modifiers};

mod key;
mod layout;
mod modifiers;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyEventKind {
	Release,
	Press,
	Repeat,
}

impl KeyEventKind {
	/// Does this event represent a logical activation of a key?
	#[inline]
	#[must_use]
	pub const fn press(self) -> bool {
		match self {
			KeyEventKind::Release => false,
			KeyEventKind::Press | KeyEventKind::Repeat => true,
		}
	}

	/// Does this event represent a logical deactivation of a key?
	#[inline]
	#[must_use]
	pub const fn release(self) -> bool {
		!self.press()
	}

	/// Does this event represent a repetition in software rather than a physical user input?
	#[inline]
	#[must_use]
	pub const fn repeat(self) -> bool {
		match self {
			KeyEventKind::Repeat => true,
			KeyEventKind::Release | KeyEventKind::Press => false,
		}
	}
}

#[derive(Debug, Clone, Copy)]
pub enum Button {
	Power,
}

// Internal invariant: `self.0` is a valid index into `Input::touch_states`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TouchId(u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TouchPhase {
	Start,
	Change,
	End,
}

#[derive(Debug, Clone, Copy)]
pub enum StylusPhase {
	Hover,
	Touch,
	Change,
	Lift,
	Leave,
}

#[derive(Debug)]
pub enum Event {
	Key {
		scancode: Scancode,
		key: Option<Key>,
		event: KeyEventKind,
		modifiers: Modifiers,
	},
	Text(Box<str>),
	Button {
		button: Button,
		pressed: bool,
	},
	DevicePresence {
		device: SupportedDeviceType,
		present: bool,
	},
	Touch {
		id: TouchId,
		phase: TouchPhase,
	},
	Stylus(StylusPhase),
}

#[derive(Debug, Clone, Copy)]
enum InternalTouchscreenEvent {
	Slot(u8),
	TouchEnd,
	PositionX(u16),
	PositionY(u16),
	Pressure(u8),
	TouchMajor(u8),
	TouchMinor(u8),
	Orientation(i8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StylusTool {
	Pen,
	Rubber,
}

impl StylusTool {
	#[must_use]
	fn from_evdev(key: KeyCode) -> Option<Self> {
		Some(match key {
			KeyCode::BTN_TOOL_PEN => Self::Pen,
			KeyCode::BTN_TOOL_RUBBER => Self::Rubber,
			_ => return None,
		})
	}
}

#[derive(Debug, Clone, Copy)]
enum InternalStylusEvent {
	Tool(Option<StylusTool>),
	Touch(bool),
	PositionX(u16),
	PositionY(u16),
	Pressure(u16),
	Distance(u8),
	TiltX(i16),
	TiltY(i16),
}

#[derive(Debug)]
enum InternalEvent {
	Key {
		scancode: Scancode,
		event: KeyEventKind,
	},
	Touchscreen(Box<[InternalTouchscreenEvent]>),
	Stylus(Box<[InternalStylusEvent]>),
	DevicePresence {
		device: SupportedDeviceType,
		present: bool,
	},
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TouchState {
	x: u16,
	y: u16,
	pressure: u8,
	touch_major: u8,
	touch_minor: u8,
	orientation: i8,
}

impl TouchState {
	#[inline]
	#[must_use]
	pub fn position(&self) -> Point {
		let height = i32::try_from(rmox_common::FB_HEIGHT).unwrap_or_else(|_| unreachable!());
		// The Y is mirrored relative to the framebuffer.
		let y = height - i32::from(self.y);
		Point::new(self.x.into(), y)
	}

	#[inline]
	#[must_use]
	pub fn pressure(&self) -> u8 {
		self.pressure
	}

	#[inline]
	#[must_use]
	pub fn touch_major(&self) -> u8 {
		self.touch_major
	}

	#[inline]
	#[must_use]
	pub fn touch_minor(&self) -> u8 {
		self.touch_minor
	}

	#[inline]
	#[must_use]
	pub fn orientation(&self) -> i8 {
		self.orientation
	}
}

#[derive(Debug)]
struct TouchStates {
	/// Invariant: `states.get(slot).is_some()`.
	slot: u8,
	states: [Option<TouchState>; 32],
}

#[allow(clippy::derivable_impls)] // Clarity.
impl Default for TouchStates {
	fn default() -> Self {
		Self {
			slot: 0,
			states: [None; 32],
		}
	}
}

impl TouchStates {
	fn current(&mut self) -> Option<(usize, &mut Option<TouchState>)> {
		let index = usize::from(self.slot);
		Some((index, self.states.get_mut(index)?))
	}

	fn set_slot(&mut self, new: u8) {
		self.slot = new;
	}

	fn get(&self, slot: u8) -> Option<&Option<TouchState>> {
		self.states.get(usize::from(slot))
	}
}

#[derive(Debug, Clone, Copy)]
pub struct StylusState {
	tool: StylusTool,
	touching: bool,
	x: u16,
	y: u16,
	pressure: u16,
	distance: u8,
	tilt_x: i16,
	tilt_y: i16,
}

impl StylusState {
	#[inline]
	#[must_use]
	pub fn tool(self) -> StylusTool {
		self.tool
	}

	#[inline]
	#[must_use]
	pub fn touching(self) -> bool {
		self.touching
	}

	#[inline]
	#[must_use]
	pub fn x(self) -> f32 {
		f32::from(self.y) * (rmox_common::FB_WIDTH as f32 / 15725.0)
	}

	#[inline]
	#[must_use]
	pub fn y(self) -> f32 {
		rmox_common::FB_HEIGHT as f32 - f32::from(self.x) * (rmox_common::FB_HEIGHT as f32 / 20967.0)
	}

	#[inline]
	#[must_use]
	pub fn position(self) -> Point {
		Point::new(self.x() as i32, self.y() as i32)
	}

	#[inline]
	#[must_use]
	pub fn pressure(self) -> u16 {
		self.pressure
	}

	#[inline]
	#[must_use]
	pub fn distance(self) -> u8 {
		self.distance
	}

	#[inline]
	#[must_use]
	pub fn tilt(self) -> Point {
		Point::new(self.tilt_x.into(), self.tilt_y.into())
	}
}

#[derive(Debug)]
pub struct Input {
	events_recv: mpsc::Receiver<InternalEvent>,
	out_queue: VecDeque<Event>,

	device_presence: [bool; SupportedDeviceType::ALL.len()],

	keyboard_layout: Box<dyn KeyboardLayout>,
	modifiers: Modifiers,
	/// This is a map from `Scancode` to `Option<Key>`.
	/// Each entry is `Some` iff the key with the given `Scancode` is currently pressed.
	/// The value indicates which `Key` was reported by the keyboard layout for that `Scancode` when it was pressed (which could depend on modifiers at that time).
	/// The value here will be reported as `key` for the release event, to ensure that applications see the same `Key` for the press and release regardless of modifier state when the key is released.
	held_keys: [Option<Key>; Scancode::ALL.len()],

	touch_states: TouchStates,

	stylus_state: Option<StylusState>,
}

macro_rules! device_types {
	($($variant:ident,)*) => {
		#[derive(Debug, Clone, Copy, PartialEq, Eq)]
		#[non_exhaustive]
		pub enum SupportedDeviceType {
			$($variant,)*
		}

		impl SupportedDeviceType {
			pub const ALL: &'static [Self] = &[$(Self::$variant,)*];
		}
	};
}

device_types! {
	Stylus,
	Buttons,
	Touchscreen,
	Keyboard,
}

fn detect_device_type(device: &Device) -> Option<SupportedDeviceType> {
	// Based on https://github.com/Eeems-Org/oxide/blob/1c997c4e9470feec08e4748942f17e517c5efa49/shared/liboxide/liboxide.cpp#L138-L170.
	if device
		.supported_absolute_axes()
		.is_some_and(|axes| axes.contains(AbsoluteAxisCode::ABS_MT_SLOT))
	{
		return Some(SupportedDeviceType::Touchscreen);
	}

	if let Some(keys) = device.supported_keys() {
		let ty = if keys.contains(KeyCode::BTN_STYLUS)
			&& device.supported_events().contains(EventType::ABSOLUTE)
		{
			SupportedDeviceType::Stylus
		} else if keys.contains(KeyCode::KEY_POWER) {
			SupportedDeviceType::Buttons
		} else {
			SupportedDeviceType::Keyboard
		};
		return Some(ty);
	}

	None
}

fn handle_keyboard(
	events: FetchEventsSynced<'_>,
	events_send: &mpsc::Sender<InternalEvent>,
) -> Result<(), ()> {
	for event in events {
		let EventSummary::Key(_, key, value) = event.destructure() else {
			continue;
		};
		let Some(key) = Scancode::from_evdev(key) else {
			continue;
		};
		let event = match value {
			0 => KeyEventKind::Release,
			1 => KeyEventKind::Press,
			2 => KeyEventKind::Repeat,
			_ => continue,
		};
		let event = InternalEvent::Key {
			scancode: key,
			event,
		};
		events_send.try_send(event).map_err(|_| ())?;
	}
	Ok(())
}

fn handle_touchscreen(
	events: FetchEventsSynced<'_>,
	events_send: &mpsc::Sender<InternalEvent>,
) -> Result<(), ()> {
	use evdev::AbsoluteAxisCode as A;
	use InternalTouchscreenEvent as E;
	let events: Box<[E]> = events
		.filter_map(|event| {
			let EventSummary::AbsoluteAxis(_, axis, value) = event.destructure() else {
				return None;
			};
			let event = match axis {
				A::ABS_MT_SLOT => E::Slot(value.try_into().unwrap()),
				A::ABS_MT_TRACKING_ID => {
					if value == -1 {
						E::TouchEnd
					} else {
						return None;
					}
				}
				A::ABS_MT_POSITION_X => E::PositionX(value.try_into().unwrap()),
				A::ABS_MT_POSITION_Y => E::PositionY(value.try_into().unwrap()),
				A::ABS_MT_PRESSURE => E::Pressure(value.try_into().unwrap()),
				A::ABS_MT_TOUCH_MAJOR => E::TouchMajor(value.try_into().unwrap()),
				A::ABS_MT_TOUCH_MINOR => E::TouchMinor(value.try_into().unwrap()),
				A::ABS_MT_ORIENTATION => E::Orientation(value.try_into().unwrap()),
				// Although the touchscreen does report `ABS_MT_DISTANCE`, it seems to always be zero, so we ignore it.
				_ => return None,
			};
			Some(event)
		})
		.collect();
	if events.is_empty() {
		return Ok(());
	}
	let event = InternalEvent::Touchscreen(events);
	events_send.try_send(event).map_err(|_| ())
}

fn handle_stylus(
	events: FetchEventsSynced<'_>,
	events_send: &mpsc::Sender<InternalEvent>,
) -> Result<(), ()> {
	use evdev::{AbsoluteAxisCode as A, EventSummary as S};
	use InternalStylusEvent as E;
	let events: Box<[E]> = events
		.filter_map(|event| {
			Some(match event.destructure() {
				S::AbsoluteAxis(_, axis, value) => match axis {
					A::ABS_X => E::PositionX(value.try_into().unwrap()),
					A::ABS_Y => E::PositionY(value.try_into().unwrap()),
					A::ABS_PRESSURE => E::Pressure(value.try_into().unwrap()),
					A::ABS_DISTANCE => E::Distance(value.try_into().unwrap()),
					A::ABS_TILT_X => E::TiltX(value.try_into().unwrap()),
					A::ABS_TILT_Y => E::TiltY(value.try_into().unwrap()),
					_ => {
						tracing::warn!(?axis, "unhandled abs axis");
						return None;
					}
				},
				S::Key(_, key, value) => {
					let press = value == 1;
					if let Some(tool) = StylusTool::from_evdev(key) {
						E::Tool(press.then_some(tool))
					} else if key == KeyCode::BTN_TOUCH {
						E::Touch(press)
					} else {
						return None;
					}
				}
				_ => return None,
			})
		})
		.collect();
	if events.is_empty() {
		return Ok(());
	}
	let event = InternalEvent::Stylus(events);
	events_send.try_send(event).map_err(|_| ())
}

fn handle_device(
	type_: SupportedDeviceType,
	device: &mut Device,
	events_send: &mpsc::Sender<InternalEvent>,
) {
	let handler = match type_ {
		SupportedDeviceType::Keyboard | SupportedDeviceType::Buttons => handle_keyboard,
		SupportedDeviceType::Touchscreen => handle_touchscreen,
		SupportedDeviceType::Stylus => handle_stylus,
	};

	loop {
		let events = match device.fetch_events() {
			Ok(events) => events,
			Err(error) => match error.raw_os_error() {
				// `errno` for "No such device". The device was disconnected.
				Some(19) => break,
				_ => panic!("{error}"),
			},
		};
		let res = handler(events, events_send);
		if res.is_err() {
			return;
		}
	}

	tracing::debug!(device=?device.name().unwrap(), ?type_, "device disconnected");
	_ = events_send.try_send(InternalEvent::DevicePresence {
		device: type_,
		present: false,
	});
}

fn autodetect_device(
	path: &Path,
	mut device: Device,
	events_send: &mpsc::Sender<InternalEvent>,
) -> Result<(), ()> {
	let type_ = detect_device_type(&device);
	let name = device.name().unwrap();

	tracing::debug!(?path, ?name, ?type_, "auto-detecting device");
	if let Some(type_) = type_ {
		tracing::debug!(device=?name, ?type_, "device connected");
		let event = InternalEvent::DevicePresence {
			device: type_,
			present: true,
		};
		events_send.try_send(event).map_err(|_| ())?;

		let events_send = events_send.clone();
		// TODO: Consider using something like `select` instead of threads.
		std::thread::spawn(move || handle_device(type_, &mut device, &events_send));
	}

	Ok(())
}

impl Input {
	pub fn open() -> std::io::Result<Self> {
		tracing::debug!("Input::new, performing initial enumeration");

		let (events_send, events_recv) = mpsc::channel(8);

		for (path, device) in evdev::enumerate() {
			_ = autodetect_device(&path, device, &events_send);
		}

		let input_dir = Path::new("/dev/input");
		let mut inotify = inotify::Inotify::init()?;
		inotify.watches().add(
			input_dir,
			inotify::WatchMask::CREATE | inotify::WatchMask::MOVED_TO,
		)?;
		std::thread::spawn(move || {
			let mut buf = [0u8; 1024];
			loop {
				let events = inotify.read_events_blocking(&mut buf).unwrap();
				for event in events {
					let Some(name) = event.name else {
						continue;
					};
					let path = input_dir.join(name);
					tracing::debug!(?path, "new input device");
					let device = Device::open(&path).unwrap();
					if autodetect_device(&path, device, &events_send).is_err() {
						break;
					}
				}
			}
		});

		Ok(Self {
			events_recv,
			out_queue: VecDeque::with_capacity(1),

			device_presence: [false; SupportedDeviceType::ALL.len()],

			keyboard_layout: Box::new(DefaultLayout),
			modifiers: Modifiers::none(),
			held_keys: [None; Scancode::ALL.len()],

			touch_states: TouchStates::default(),

			stylus_state: None,
		})
	}

	fn update_modifier(&mut self, modifier: Modifier, event: KeyEventKind) {
		if modifier.is_toggle() {
			if event == KeyEventKind::Press {
				self.modifiers ^= modifier;
			}
		} else {
			match event {
				KeyEventKind::Release => {
					self.modifiers -= modifier;
				}
				KeyEventKind::Press => {
					self.modifiers += modifier;
				}
				KeyEventKind::Repeat => {}
			}
		}
	}

	fn enqueue(&mut self, event: Event) {
		self.out_queue.push_back(event);
	}

	fn process_key(&mut self, scancode: Scancode, kind: KeyEventKind) {
		let mut these_modifiers = self.modifiers;
		// `scancode_to_key` may consume some of `these_modifiers` if they are interpreted as part of a compound keystroke to select an alternative `Key` for the given `Scancode`.
		let key = if kind.release() {
			// This could return `None` if the program was started while a physical key was held.
			// In that case, `key` should be `None` because we don't know what modifiers were active when the physical key was pressed.
			self.held_keys[scancode as usize].take()
		} else {
			let key = self
				.keyboard_layout
				.scancode_to_key(scancode, &mut these_modifiers);
			// This could override an existing entry if modifiers change while the physical key is held and auto-repeating.
			// This is the intended behavior.
			self.held_keys[scancode as usize] = key;
			key
		};

		let key_event = Event::Key {
			scancode,
			key,
			event: kind,
			modifiers: these_modifiers,
		};
		self.enqueue(key_event);

		if let Some(key) = key {
			match self.keyboard_layout.resolve(key, these_modifiers) {
				Resolved::Modifier(modifier) => {
					self.update_modifier(modifier, kind);
				}
				Resolved::Text(text) => {
					if kind.press() {
						self.out_queue.push_back(Event::Text(text));
					}
				}
				Resolved::NoneOfThese => {}
			}
		}
	}

	fn process_touchscreen(&mut self, events: &[InternalTouchscreenEvent]) {
		let mut changes = [None; 32];

		macro_rules! touch_state {
			() => {{
				let Some((slot, state)) = self.touch_states.current() else {
					continue;
				};

				let change = &mut changes[usize::from(slot)];
				if state.is_none() {
					*change = Some(TouchPhase::Start);
				} else if *change != Some(TouchPhase::Start) {
					// This also correctly handles a Start following an End, combining them into a Change.
					*change = Some(TouchPhase::Change);
				}

				state.get_or_insert_with(TouchState::default)
			}};
		}

		for &event in events {
			use InternalTouchscreenEvent as E;

			match event {
				E::Slot(v) => {
					self.touch_states.set_slot(v);
				}
				E::TouchEnd => {
					let Some((slot, state)) = self.touch_states.current() else {
						continue;
					};
					*state = None;

					let change = &mut changes[slot];
					*change = if *change == Some(TouchPhase::Start) {
						None
					} else {
						Some(TouchPhase::End)
					};
				}
				E::PositionX(v) => touch_state!().x = v,
				E::PositionY(v) => touch_state!().y = v,
				E::Pressure(v) => touch_state!().pressure = v,
				E::TouchMajor(v) => touch_state!().touch_major = v,
				E::TouchMinor(v) => touch_state!().touch_minor = v,
				E::Orientation(v) => touch_state!().orientation = v,
			}
		}

		for (slot, phase) in changes.into_iter().enumerate() {
			if let Some(phase) = phase {
				let event = Event::Touch {
					// We are using the slot as the ID because, AFAICT, it satisfies the criteria:
					// it doesn't change for the duration of the contact.
					id: TouchId(slot.try_into().unwrap()),
					phase,
				};
				self.enqueue(event);
			}
		}
	}

	fn process_stylus(&mut self, events: &[InternalStylusEvent]) {
		macro_rules! state {
			() => {{
				let Some(state) = &mut self.stylus_state else {
					continue;
				};
				state
			}};
		}

		let prev_touching = self.stylus_state.map(|state| state.touching);

		for &event in events {
			use InternalStylusEvent as E;

			match event {
				E::Tool(v) => {
					self.stylus_state = v.map(|tool| StylusState {
						tool,
						touching: false,
						x: 0,
						y: 0,
						pressure: 0,
						distance: 0,
						tilt_x: 0,
						tilt_y: 0,
					});
				}
				E::Touch(v) => state!().touching = v,
				E::PositionX(v) => state!().x = v,
				E::PositionY(v) => state!().y = v,
				E::Pressure(v) => state!().pressure = v,
				E::Distance(v) => state!().distance = v,
				E::TiltX(v) => state!().tilt_x = v,
				E::TiltY(v) => state!().tilt_y = v,
			}
		}

		#[allow(clippy::match_same_arms)] // Clarity.
		let phase = match (prev_touching, self.stylus_state.map(|state| state.touching)) {
			(None, None) => return,
			(None, Some(true)) => StylusPhase::Touch,
			(None, Some(false)) => StylusPhase::Hover,
			(Some(true), None) => StylusPhase::Lift,
			(Some(false), None) => StylusPhase::Leave,
			(Some(false), Some(false)) | (Some(true), Some(true)) => StylusPhase::Change,
			(Some(true), Some(false)) => StylusPhase::Lift,
			(Some(false), Some(true)) => StylusPhase::Touch,
		};
		self.enqueue(Event::Stylus(phase));
	}

	fn process_presence(&mut self, device: SupportedDeviceType, present: bool) {
		self.device_presence[device as usize] = present;
		self.enqueue(Event::DevicePresence { device, present });
	}

	fn process_event(&mut self, event: InternalEvent) {
		match event {
			InternalEvent::Key { scancode, event } => self.process_key(scancode, event),
			InternalEvent::Touchscreen(events) => self.process_touchscreen(&events),
			InternalEvent::Stylus(events) => self.process_stylus(&events),
			InternalEvent::DevicePresence { device, present } => self.process_presence(device, present),
		}
	}

	#[inline]
	#[must_use]
	pub fn modifiers(&self) -> Modifiers {
		self.modifiers
	}

	#[inline]
	#[must_use]
	pub fn touch_state(&self, id: TouchId) -> Option<TouchState> {
		// We assert that any `TouchId` will fit within the bounds of our states array,
		// because its inner field is private and only we construct it.
		*self
			.touch_states
			.get(id.0)
			.unwrap_or_else(|| unreachable!("invalid {id:?} out of bounds of touch_states"))
	}

	#[inline]
	#[must_use]
	pub fn stylus_state(&self) -> Option<StylusState> {
		self.stylus_state
	}

	#[inline]
	#[must_use]
	pub fn device_present(&self, device: SupportedDeviceType) -> bool {
		self.device_presence[device as usize]
	}
}

impl Stream for Input {
	type Item = Event;

	#[inline]
	fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Event>> {
		loop {
			if let Some(prev_event) = self.out_queue.pop_front() {
				return Poll::Ready(Some(prev_event));
			}

			let raw = ready!(self.events_recv.poll_recv(cx)).expect("all event threads crashed");
			self.process_event(raw);
		}
	}
}

#[derive(Debug, Clone)]
pub struct PressedKeys<'a> {
	held_keys: std::iter::Enumerate<std::slice::Iter<'a, Option<Key>>>,
}

impl Input {
	#[inline]
	#[must_use]
	pub fn pressed_keys(&self) -> PressedKeys<'_> {
		PressedKeys {
			held_keys: self.held_keys.iter().enumerate(),
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PressedKey {
	pub scancode: Scancode,
	pub key: Key,
}

impl Iterator for PressedKeys<'_> {
	type Item = PressedKey;

	#[inline]
	fn next(&mut self) -> Option<Self::Item> {
		self.held_keys.find_map(|(i, &key)| {
			Some(PressedKey {
				scancode: Scancode::ALL[i],
				key: key?,
			})
		})
	}
}
