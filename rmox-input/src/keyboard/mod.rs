#[cfg(feature = "input-impl")]
use evdev::EventSummary;
use serde::{Deserialize, Serialize};

pub use self::key::{Key, Scancode};
#[cfg(feature = "input-impl")]
use self::layout::{DefaultLayout, Layout, Resolved};
pub use self::modifiers::{Modifier, Modifiers};
#[cfg(feature = "input-impl")]
use crate::Event;

pub mod key;
pub mod layout;
pub mod modifiers;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct KeyEvent {
	pub scancode: Scancode,
	pub key: Option<Key>,
	pub event: KeyEventKind,
	pub modifiers: Modifiers,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ButtonEvent {
	pub button: Button,
	pub pressed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Button {
	Power,
}

#[cfg(feature = "input-impl")]
#[derive(Debug)]
pub(crate) struct State {
	keyboard_layout: Box<dyn Layout>,
	modifiers: Modifiers,
	/// This is a map from `Scancode` to `Option<Key>`.
	/// Each entry is `Some` iff the key with the given `Scancode` is currently pressed.
	/// The value indicates which `Key` was reported by the keyboard layout for that `Scancode` when it was pressed (which could depend on modifiers at that time).
	/// The value here will be reported as `key` for the release event, to ensure that applications see the same `Key` for the press and release regardless of modifier state when the key is released.
	held_keys: [Option<Key>; Scancode::ALL.len()],
}

#[cfg(feature = "input-impl")]
impl Default for State {
	fn default() -> Self {
		Self {
			keyboard_layout: Box::new(DefaultLayout),
			modifiers: Modifiers::none(),
			held_keys: [None; Scancode::ALL.len()],
		}
	}
}

#[cfg(feature = "input-impl")]
impl State {
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

	fn process_key(
		&mut self,
		scancode: Scancode,
		kind: KeyEventKind,
		mut enqueue: impl FnMut(Event),
	) {
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

		let key_event = KeyEvent {
			scancode,
			key,
			event: kind,
			modifiers: these_modifiers,
		};
		enqueue(Event::Key(key_event));

		if let Some(key) = key {
			match self.keyboard_layout.resolve(key, these_modifiers) {
				Resolved::Modifier(modifier) => {
					self.update_modifier(modifier, kind);
				}
				Resolved::Text(text) => {
					if kind.press() {
						enqueue(Event::Text(text));
					}
				}
				Resolved::NoneOfThese => {}
			}
		}
	}
}

#[cfg(feature = "input-impl")]
pub(crate) fn handle_events(
	events: impl IntoIterator<Item = evdev::InputEvent>,
	state: &mut crate::InputState,
) {
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
		state
			.keyboard
			.process_key(key, event, |event| state.out_queue.push_back(event));
	}
}

#[cfg(feature = "input-impl")]
impl crate::Input {
	#[inline]
	#[must_use]
	pub fn modifiers(&self) -> Modifiers {
		self.state.keyboard.modifiers
	}
}

#[cfg(feature = "input-impl")]
#[derive(Debug, Clone)]
pub struct PressedKeys<'a> {
	held_keys: std::iter::Enumerate<std::slice::Iter<'a, Option<Key>>>,
}

#[cfg(feature = "input-impl")]
impl crate::Input {
	#[inline]
	#[must_use]
	pub fn pressed_keys(&self) -> PressedKeys<'_> {
		PressedKeys {
			held_keys: self.state.keyboard.held_keys.iter().enumerate(),
		}
	}
}

#[cfg(feature = "input-impl")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PressedKey {
	pub scancode: Scancode,
	pub key: Key,
}

#[cfg(feature = "input-impl")]
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
