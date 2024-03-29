#[cfg(feature = "input-impl")]
use evdev::EventSummary;
use rmox_common::types::{pos2, Pos2};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy)]
pub struct Event {
	pub touch_id: Id,
	pub phase: Phase,
}

// Internal invariant: `self.0` is a valid index into `Input::touch_states`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Id(pub(crate) u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Phase {
	Start,
	Change,
	End,
}

#[allow(clippy::module_name_repetitions)] // `State` is already taken.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
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
	pub fn position(&self) -> Pos2 {
		// The Y is mirrored relative to the framebuffer.
		let y = rmox_common::fb::HEIGHT - i32::from(self.y);
		pos2(self.x.into(), y)
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

#[cfg(feature = "input-impl")]
#[derive(Debug)]
pub(crate) struct State {
	/// Invariant: `states.get(slot).is_some()`.
	slot: u8,
	states: [Option<TouchState>; 32],
}

#[cfg(feature = "input-impl")]
#[allow(clippy::derivable_impls)] // Clarity.
impl Default for State {
	fn default() -> Self {
		Self {
			slot: 0,
			states: [None; 32],
		}
	}
}

#[cfg(feature = "input-impl")]
impl State {
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

#[cfg(feature = "input-impl")]
pub(crate) fn handle_events(
	events: impl IntoIterator<Item = evdev::InputEvent>,
	input: &mut crate::InputState,
) {
	use evdev::AbsoluteAxisCode as A;
	#[derive(Debug, Clone, Copy)]
	enum InternalEvent {
		Slot(u8),
		TouchEnd,
		PositionX(u16),
		PositionY(u16),
		Pressure(u8),
		TouchMajor(u8),
		TouchMinor(u8),
		Orientation(i8),
	}
	use InternalEvent as E;

	let state = &mut input.touch;

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

	let mut changes = [None; 32];

	macro_rules! state {
		() => {{
			let Some((slot, state)) = state.current() else {
				continue;
			};

			let change = &mut changes[usize::from(slot)];
			if state.is_none() {
				*change = Some(Phase::Start);
			} else if *change != Some(Phase::Start) {
				// This also correctly handles a Start following an End, combining them into a Change.
				*change = Some(Phase::Change);
			}

			state.get_or_insert_with(TouchState::default)
		}};
	}

	for event in events {
		match event {
			E::Slot(v) => {
				state.set_slot(v);
			}
			E::TouchEnd => {
				let Some((slot, state)) = state.current() else {
					continue;
				};
				*state = None;

				let change = &mut changes[slot];
				*change = if *change == Some(Phase::Start) {
					None
				} else {
					Some(Phase::End)
				};
			}
			E::PositionX(v) => state!().x = v,
			E::PositionY(v) => state!().y = v,
			E::Pressure(v) => state!().pressure = v,
			E::TouchMajor(v) => state!().touch_major = v,
			E::TouchMinor(v) => state!().touch_minor = v,
			E::Orientation(v) => state!().orientation = v,
		}
	}

	for (slot, phase) in changes.into_iter().enumerate() {
		if let Some(phase) = phase {
			let event = crate::Event::Touch(Event {
				// We are using the slot as the ID because, AFAICT, it satisfies the criteria:
				// it doesn't change for the duration of the contact.
				touch_id: Id(slot.try_into().unwrap()),
				phase,
			});
			input.enqueue(event);
		}
	}
}

#[cfg(feature = "input-impl")]
impl crate::Input {
	#[inline]
	#[must_use]
	pub fn touch_state(&self, id: Id) -> Option<TouchState> {
		// We assert that any `TouchId` will fit within the bounds of our states array,
		// because its inner field is private and only we construct it.
		*self
			.state
			.touch
			.get(id.0)
			.unwrap_or_else(|| unreachable!("invalid {id:?} out of bounds of touch_states"))
	}
}
