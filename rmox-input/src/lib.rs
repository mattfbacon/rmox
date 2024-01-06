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

use std::sync::mpsc::{Receiver, SyncSender};

use enumset::{EnumSet, EnumSetType};
use evdev::{AbsoluteAxisType, Device, EventType, InputEventKind};

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
	// A temporary monkey patch.
	Unknown(Box<dyn std::fmt::Debug + Send>),
}

enum InternalEvent {
	Key {
		scancode: Scancode,
		event: KeyEventKind,
	},
	// A temporary monkey patch.
	Unknown(Box<dyn std::fmt::Debug + Send>),
}

pub struct Input {
	events: Receiver<InternalEvent>,
	extra_event: Option<Event>,

	keyboard_layout: Box<dyn KeyboardLayout>,
	modifiers: Modifiers,
	/// This is a map from `Scancode` to `Option<Key>`.
	/// Each entry is `Some` iff the key with the given `Scancode` is currently pressed.
	/// The value indicates which `Key` was reported by the keyboard layout for that `Scancode` when it was pressed (which could depend on modifiers at that time).
	/// The value here will be reported as `key` for the release event, to ensure that applications see the same `Key` for the press and release regardless of modifier state when the key is released.
	///
	/// The array is boxed for size reasons.
	held_keys: Box<[Option<Key>; Scancode::ALL.len()]>,
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

// TODO: Unify this duplicated code.
fn handle_keyboard(device: &mut Device, events_send: &SyncSender<InternalEvent>) {
	loop {
		let events = match device.fetch_events() {
			Ok(events) => events,
			Err(error) => match error.raw_os_error() {
				// `errno` for "No such device". The device was disconnected.
				Some(19) => break,
				_ => panic!("{error}"),
			},
		};
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
			if events_send.send(event).is_err() {
				break;
			}
		}
	}
}

fn handle_touchscreen(device: &mut Device, events_send: &SyncSender<InternalEvent>) {
	/*
	loop {
		let events = match device.fetch_events() {
			Ok(events) => events,
			Err(error) => match error.raw_os_error() {
				// `errno` for "No such device". The device was disconnected.
				Some(19) => break,
				_ => panic!("{error}"),
			},
		};
		let mut slot = 0;
		let mut tracking_id = None;
		for event in events {
			let InputEventKind::AbsAxis(axis) = event.kind() else {
				continue;
			};
			match axis {
				AbsoluteAxisType::ABS_MT_SLOT => {}
				AbsoluteAxisType::ABS_MT_TRACKING_ID => {
					tracking_id = Some(tracking_id);
				}
			}
		}
	}
	*/
	handle_todo(device, events_send, SupportedDeviceType::Touchscreen);
}

fn handle_stylus(device: &mut Device, events_send: &SyncSender<InternalEvent>) {
	handle_todo(device, events_send, SupportedDeviceType::Stylus);
}

fn handle_todo(
	device: &mut Device,
	events_send: &SyncSender<InternalEvent>,
	type_: SupportedDeviceType,
) {
	loop {
		let events = match device.fetch_events() {
			Ok(events) => events,
			Err(error) => match error.raw_os_error() {
				// `errno` for "No such device". The device was disconnected.
				Some(19) => break,
				_ => panic!("{error}"),
			},
		};
		let events = events
			.filter(|event| event.event_type() != evdev::EventType::SYNCHRONIZATION)
			.map(|event| (type_, event.kind(), event.value()))
			.collect::<Vec<_>>();
		let event = InternalEvent::Unknown(Box::new(events));
		if events_send.send(event).is_err() {
			break;
		}
	}
}

fn handle_device(
	type_: SupportedDeviceType,
	device: &mut Device,
	events_send: &SyncSender<InternalEvent>,
) {
	match type_ {
		SupportedDeviceType::Keyboard | SupportedDeviceType::Buttons => {
			handle_keyboard(device, events_send);
		}
		SupportedDeviceType::Touchscreen => handle_touchscreen(device, events_send),
		SupportedDeviceType::Stylus => handle_stylus(device, events_send),
	}
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
			events: events_recv,
			extra_event: None,
			keyboard_layout: Box::new(DefaultLayout),
			modifiers: Modifiers::none(),
			held_keys: Box::new([None; Scancode::ALL.len()]),
		})
	}

	fn set_extra_event(&mut self, event: Event) {
		debug_assert!(self.extra_event.is_none());
		self.extra_event = Some(event);
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

	fn process_key(&mut self, scancode: Scancode, event: KeyEventKind) -> Event {
		let mut these_modifiers = self.modifiers;
		// `scancode_to_key` may consume some of `these_modifiers` if they are interpreted as part of a compound keystroke to select an alternative `Key` for the given `Scancode`.
		let key = if event.release() {
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
		if let Some(key) = key {
			match self.keyboard_layout.resolve(key, these_modifiers) {
				Resolved::Modifier(modifier) => {
					self.update_modifier(modifier, event);
				}
				Resolved::Text(text) => {
					if event.press() {
						let text_event = Event::Text(text);
						self.set_extra_event(text_event);
					}
				}
				Resolved::NoneOfThese => {}
			}
		}

		Event::Key {
			scancode,
			key,
			event,
			modifiers: these_modifiers,
		}
	}

	fn next_event_(&mut self, event: InternalEvent) -> Event {
		match event {
			InternalEvent::Key { scancode, event } => self.process_key(scancode, event),
			InternalEvent::Unknown(data) => Event::Unknown(data),
		}
	}

	#[inline]
	#[must_use]
	pub fn next_event(&mut self) -> Event {
		if let Some(prev_event) = self.extra_event.take() {
			return prev_event;
		}

		let raw = self
			.events
			.recv()
			.expect("all event handler threads crashed");
		self.next_event_(raw)
	}

	#[inline]
	#[must_use]
	pub fn modifiers(&self) -> Modifiers {
		self.modifiers
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
