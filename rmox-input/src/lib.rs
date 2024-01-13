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

use std::collections::VecDeque;
use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll};

use embedded_graphics_core::geometry::Point;
use evdev::{AbsoluteAxisCode, Device, EventStream, EventSummary, EventType, KeyCode};
use futures_core::Stream;

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
pub struct TouchId(pub(crate) u8);

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
	Touch {
		id: TouchId,
		phase: TouchPhase,
	},
	Stylus(StylusPhase),
	DevicePresence(SupportedDeviceType),
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

struct Devices {
	devices: [Option<EventStream>; SupportedDeviceType::ALL.len()],
	last_polled_device: u8,
	inotify: inotify::EventStream<[u8; 256]>,
}

struct InputState {
	out_queue: VecDeque<Event>,

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

pub struct Input {
	devices: Devices,
	state: InputState,
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

fn handle_keyboard(events: impl IntoIterator<Item = evdev::InputEvent>, input: &mut InputState) {
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
		input.process_key(key, event);
	}
}

fn handle_touchscreen(events: impl IntoIterator<Item = evdev::InputEvent>, input: &mut InputState) {
	use evdev::AbsoluteAxisCode as A;
	use InternalTouchscreenEvent as E;
	let events = events.into_iter().filter_map(|event| {
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
	});
	input.process_touchscreen(events);
}

fn handle_stylus(events: impl IntoIterator<Item = evdev::InputEvent>, input: &mut InputState) {
	use evdev::{AbsoluteAxisCode as A, EventSummary as S};
	use InternalStylusEvent as E;
	let events = events.into_iter().filter_map(|event| {
		Some(match event.destructure() {
			S::AbsoluteAxis(_, axis, value) => match axis {
				A::ABS_X => E::PositionX(value.try_into().unwrap()),
				A::ABS_Y => E::PositionY(value.try_into().unwrap()),
				A::ABS_PRESSURE => E::Pressure(value.try_into().unwrap()),
				A::ABS_DISTANCE => E::Distance(value.try_into().unwrap()),
				A::ABS_TILT_X => E::TiltX(value.try_into().unwrap()),
				A::ABS_TILT_Y => E::TiltY(value.try_into().unwrap()),
				_ => return None,
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
	});
	input.process_stylus(events);
}

const INPUT_DIR: &str = "/dev/input";

impl Input {
	pub fn open() -> std::io::Result<Self> {
		let inotify = inotify::Inotify::init()?;
		inotify
			.watches()
			.add(INPUT_DIR, inotify::WatchMask::CREATE)?;
		let inotify = inotify.into_event_stream([0u8; 256])?;

		let mut ret = Self {
			devices: Devices {
				devices: std::array::from_fn(|_| None),
				last_polled_device: 0,
				inotify,
			},

			state: InputState {
				out_queue: VecDeque::with_capacity(1),

				keyboard_layout: Box::new(DefaultLayout),
				modifiers: Modifiers::none(),
				held_keys: [None; Scancode::ALL.len()],

				touch_states: TouchStates::default(),

				stylus_state: None,
			},
		};

		ret.devices.enumerate()?;

		Ok(ret)
	}
}

impl Devices {
	fn enumerate(&mut self) -> std::io::Result<()> {
		tracing::debug!("Input::new, performing initial enumeration");

		for (path, device) in evdev::enumerate() {
			self.autodetect_device(&path, device)?;
		}

		Ok(())
	}

	fn autodetect_device(
		&mut self,
		path: &Path,
		device: Device,
	) -> std::io::Result<Option<SupportedDeviceType>> {
		let type_ = detect_device_type(&device);
		let name = device.name().unwrap();

		tracing::debug!(?path, ?name, ?type_, "auto-detecting device");

		let Some(type_) = type_ else {
			return Ok(None);
		};

		let slot = &mut self.devices[type_ as usize];
		if let Some(old) = &slot {
			tracing::warn!(old=?old.device().name().unwrap(), new=?name, ?type_, "duplicate device for type. ignoring new device.");
			return Ok(None);
		}

		tracing::debug!(device=?name, ?type_, "device connected");
		*slot = Some(device.into_event_stream()?);
		Ok(Some(type_))
	}
}

impl InputState {
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

	fn process_touchscreen(&mut self, events: impl Iterator<Item = InternalTouchscreenEvent>) {
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

		for event in events {
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

	fn process_stylus(&mut self, events: impl Iterator<Item = InternalStylusEvent>) {
		macro_rules! state {
			() => {{
				let Some(state) = &mut self.stylus_state else {
					continue;
				};
				state
			}};
		}

		let prev_touching = self.stylus_state.map(|state| state.touching);

		for event in events {
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
}

impl Input {
	#[inline]
	#[must_use]
	pub fn modifiers(&self) -> Modifiers {
		self.state.modifiers
	}

	#[inline]
	#[must_use]
	pub fn touch_state(&self, id: TouchId) -> Option<TouchState> {
		// We assert that any `TouchId` will fit within the bounds of our states array,
		// because its inner field is private and only we construct it.
		*self
			.state
			.touch_states
			.get(id.0)
			.unwrap_or_else(|| unreachable!("invalid {id:?} out of bounds of touch_states"))
	}

	#[inline]
	#[must_use]
	pub fn stylus_state(&self) -> Option<StylusState> {
		self.state.stylus_state
	}

	#[inline]
	#[must_use]
	pub fn device_present(&self, device: SupportedDeviceType) -> bool {
		self.devices.devices[device as usize].is_some()
	}
}

impl Stream for Input {
	type Item = std::io::Result<Event>;

	#[inline]
	fn poll_next(
		mut self: Pin<&mut Self>,
		cx: &mut Context<'_>,
	) -> Poll<Option<std::io::Result<Event>>> {
		let this = &mut *self;
		let devices = &mut this.devices;
		let state = &mut this.state;

		if let Some(prev_event) = state.out_queue.pop_front() {
			return Poll::Ready(Some(Ok(prev_event)));
		}

		if let Poll::Ready(Some(event)) = Pin::new(&mut devices.inotify).poll_next(cx) {
			match (|| {
				let event = event?;

				let Some(name) = event.name else {
					return Ok(None);
				};

				let path = Path::new(INPUT_DIR).join(name);

				tracing::debug!(?path, "new input device");
				let device = Device::open(&path)?;
				devices.autodetect_device(&path, device)
			})() {
				Ok(Some(connected_type)) => {
					return Poll::Ready(Some(Ok(Event::DevicePresence(connected_type))));
				}
				Ok(None) => {}
				Err(error) => return Poll::Ready(Some(Err(error))),
			}
		}

		'each: for _ in 0..devices.devices.len() {
			let i = usize::from(devices.last_polled_device);
			devices.last_polled_device = devices.last_polled_device.wrapping_add(1)
				% u8::try_from(SupportedDeviceType::ALL.len()).unwrap();

			let type_ = SupportedDeviceType::ALL[i];
			let mut slot = &mut devices.devices[i];
			if let Some(device) = &mut slot {
				let handler = match type_ {
					SupportedDeviceType::Keyboard | SupportedDeviceType::Buttons => handle_keyboard,
					SupportedDeviceType::Touchscreen => handle_touchscreen,
					SupportedDeviceType::Stylus => handle_stylus,
				};

				let events = device.poll_event(cx);
				let events = events.map(|res| {
					res.map(|events| {
						handler(events, state);
					})
				});
				match events {
					Poll::Ready(res) => {
						match res {
							Ok(()) => {
								break 'each;
							}
							Err(error) => match error.raw_os_error() {
								// `errno` for "No such device". The device was disconnected.
								Some(19) => {
									*slot = None;
									state.enqueue(Event::DevicePresence(type_));
									continue;
								}
								_ => {
									return Poll::Ready(Some(Err(error)));
								}
							},
						}
					}
					Poll::Pending => continue,
				}
			}
		}

		if let Some(prev_event) = state.out_queue.pop_front() {
			Poll::Ready(Some(Ok(prev_event)))
		} else {
			Poll::Pending
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
			held_keys: self.state.held_keys.iter().enumerate(),
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
