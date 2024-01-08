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
use std::sync::mpsc::{Receiver, SyncSender};

use embedded_graphics_core::geometry::Point;
use evdev::{AbsoluteAxisType, Device, EventType, FetchEventsSynced, InputEventKind};

pub use crate::key::{Key, Scancode};
use crate::layout::{DefaultLayout, KeyboardLayout, Resolved};
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
	KeyboardPresence {
		connected: bool,
	},
	Touch {
		id: TouchId,
		phase: TouchPhase,
	},
	// A temporary monkey patch.
	Unknown(Box<dyn std::fmt::Debug + Send>),
}

#[derive(Debug, Clone, Copy)]
enum InternalTouchscreenEvent {
	Slot(u8),
	TouchEnd,
	PositionX(u16),
	PositionY(u16),
	Pressure(u8),
	Distance(u8),
	TouchMajor(u8),
	TouchMinor(u8),
	Orientation(i8),
}

#[derive(Debug)]
enum InternalEvent {
	Key {
		scancode: Scancode,
		event: KeyEventKind,
	},
	Touchscreen(Box<[InternalTouchscreenEvent]>),
	// A temporary monkey patch.
	Unknown(Box<dyn std::fmt::Debug + Send>),
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TouchState {
	x: u16,
	y: u16,
	pressure: u8,
	distance: u8,
	touch_major: u8,
	touch_minor: u8,
	orientation: i8,
}

impl TouchState {
	#[inline]
	#[must_use]
	pub fn position(&self) -> Point {
		// The Y is mirrored relative to the framebuffer.
		let y = i32::try_from(rmox_common::FB_HEIGHT).unwrap() - i32::from(self.y);
		Point::new(self.x.into(), y)
	}

	#[inline]
	#[must_use]
	pub fn pressure(&self) -> u8 {
		self.pressure
	}

	#[inline]
	#[must_use]
	pub fn distance(&self) -> u8 {
		self.distance
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
		// TODO: Handle invalid slots.
		assert!(self.states.get(usize::from(new)).is_some());
		self.slot = new;
	}

	fn get(&self, slot: u8) -> Option<Option<TouchState>> {
		self.states.get(usize::from(slot)).copied()
	}
}

#[derive(Debug)]
pub struct Input {
	events_recv: Receiver<InternalEvent>,
	out_queue: VecDeque<Event>,

	keyboard_layout: Box<dyn KeyboardLayout>,
	modifiers: Modifiers,
	/// This is a map from `Scancode` to `Option<Key>`.
	/// Each entry is `Some` iff the key with the given `Scancode` is currently pressed.
	/// The value indicates which `Key` was reported by the keyboard layout for that `Scancode` when it was pressed (which could depend on modifiers at that time).
	/// The value here will be reported as `key` for the release event, to ensure that applications see the same `Key` for the press and release regardless of modifier state when the key is released.
	held_keys: [Option<Key>; Scancode::ALL.len()],

	touch_states: TouchStates,
}

#[derive(Debug, Clone, Copy)]
pub enum SupportedDeviceType {
	Stylus,
	Buttons,
	Touchscreen,
	Keyboard,
}

fn detect_device_type(device: &Device) -> Option<SupportedDeviceType> {
	// Based on https://github.com/Eeems-Org/oxide/blob/1c997c4e9470feec08e4748942f17e517c5efa49/shared/liboxide/liboxide.cpp#L138-L170.
	if device
		.supported_absolute_axes()
		.is_some_and(|axes| axes.contains(AbsoluteAxisType::ABS_MT_SLOT))
	{
		return Some(SupportedDeviceType::Touchscreen);
	}

	if let Some(keys) = device.supported_keys() {
		let ty = if keys.contains(evdev::Key::BTN_STYLUS)
			&& device.supported_events().contains(EventType::ABSOLUTE)
		{
			SupportedDeviceType::Stylus
		} else if keys.contains(evdev::Key::KEY_POWER) {
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
	events_send: &SyncSender<InternalEvent>,
) -> Result<(), ()> {
	for event in events {
		let InputEventKind::Key(key) = event.kind() else {
			continue;
		};
		let Some(key) = Scancode::from_evdev(key) else {
			continue;
		};
		let event = match event.value() {
			0 => KeyEventKind::Release,
			1 => KeyEventKind::Press,
			2 => KeyEventKind::Repeat,
			_ => continue,
		};
		let event = InternalEvent::Key {
			scancode: key,
			event,
		};
		events_send.send(event).map_err(|_| ())?;
	}
	Ok(())
}

fn handle_touchscreen(
	events: FetchEventsSynced<'_>,
	events_send: &SyncSender<InternalEvent>,
) -> Result<(), ()> {
	use evdev::AbsoluteAxisType as A;
	use InternalTouchscreenEvent as E;
	let events = events
		.filter_map(|event| {
			let evdev::InputEventKind::AbsAxis(axis) = event.kind() else {
				return None;
			};
			let value = event.value();
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
				A::ABS_MT_DISTANCE => {
					if value != 0 {
						eprintln!("distance = {value}");
					}
					E::Distance(value.try_into().unwrap())
				}
				A::ABS_MT_TOUCH_MAJOR => E::TouchMajor(value.try_into().unwrap()),
				A::ABS_MT_TOUCH_MINOR => E::TouchMinor(value.try_into().unwrap()),
				A::ABS_MT_ORIENTATION => E::Orientation(value.try_into().unwrap()),
				_ => return None,
			};
			Some(event)
		})
		.collect();
	let event = InternalEvent::Touchscreen(events);
	events_send.send(event).map_err(|_| ())
}

fn handle_stylus(
	events: FetchEventsSynced<'_>,
	events_send: &SyncSender<InternalEvent>,
) -> Result<(), ()> {
	handle_todo(events, events_send, SupportedDeviceType::Stylus)
}

fn handle_todo(
	events: FetchEventsSynced<'_>,
	events_send: &SyncSender<InternalEvent>,
	type_: SupportedDeviceType,
) -> Result<(), ()> {
	let events = events
		.filter(|event| event.event_type() != evdev::EventType::SYNCHRONIZATION)
		.map(|event| (type_, event.kind(), event.value()))
		.collect::<Vec<_>>();
	let event = InternalEvent::Unknown(Box::new(events));
	events_send.send(event).map_err(|_| ())
}

fn handle_device(
	type_: SupportedDeviceType,
	device: &mut Device,
	events_send: &SyncSender<InternalEvent>,
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
		if handler(events, events_send).is_err() {
			return;
		}
	}
	tracing::debug!(device=?device.name().unwrap(), ?type_, "device disconnected");
}

impl Input {
	pub fn new() -> std::io::Result<Self> {
		tracing::debug!("Input::new, discovering devices");

		let (events_send, events_recv) = std::sync::mpsc::sync_channel(4);

		for (path, mut device) in evdev::enumerate() {
			let type_ = detect_device_type(&device);
			let name = device.name().expect("device is missing name");
			tracing::debug!("device path={path:?} name={name:?} type={type_:?}");
			if let Some(type_) = type_ {
				let events_send = events_send.clone();
				// TODO: Consider using something like `select` instead of threads.
				std::thread::spawn(move || handle_device(type_, &mut device, &events_send));
			}
		}

		// TODO: Use the udev monitor to dynamically create input handler threads when new devices are created.

		Ok(Self {
			events_recv,
			out_queue: VecDeque::with_capacity(1),

			keyboard_layout: Box::new(DefaultLayout),
			modifiers: Modifiers::none(),
			held_keys: [None; Scancode::ALL.len()],

			touch_states: TouchStates::default(),
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
			match event {
				InternalTouchscreenEvent::Slot(v) => {
					self.touch_states.set_slot(v);
				}
				InternalTouchscreenEvent::TouchEnd => {
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
				InternalTouchscreenEvent::PositionX(v) => touch_state!().x = v,
				InternalTouchscreenEvent::PositionY(v) => touch_state!().y = v,
				InternalTouchscreenEvent::Pressure(v) => touch_state!().pressure = v,
				InternalTouchscreenEvent::Distance(v) => touch_state!().distance = v,
				InternalTouchscreenEvent::TouchMajor(v) => touch_state!().touch_major = v,
				InternalTouchscreenEvent::TouchMinor(v) => touch_state!().touch_minor = v,
				InternalTouchscreenEvent::Orientation(v) => touch_state!().orientation = v,
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

	fn process_event(&mut self, event: InternalEvent) {
		match event {
			InternalEvent::Key { scancode, event } => self.process_key(scancode, event),
			InternalEvent::Touchscreen(events) => self.process_touchscreen(&events),
			InternalEvent::Unknown(data) => self.enqueue(Event::Unknown(data)),
		}
	}

	#[inline]
	#[must_use]
	pub fn next_event(&mut self) -> Event {
		loop {
			if let Some(prev_event) = self.out_queue.pop_front() {
				return prev_event;
			}

			let raw = self
				.events_recv
				.recv()
				.expect("all event handler threads crashed");
			self.process_event(raw);
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
		self
			.touch_states
			.get(id.0)
			.expect("invalid TouchId out of bounds of touch_states")
	}
}

impl Iterator for Input {
	type Item = Event;

	#[inline]
	fn next(&mut self) -> Option<Event> {
		Some(self.next_event())
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
