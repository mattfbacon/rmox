#[cfg(feature = "input-impl")]
use evdev::KeyCode;
use rmox_common::types::{pos2, Pos2};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy)]
pub struct Event {
	pub phase: Phase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
	Hover,
	Touch,
	Change,
	Lift,
	Leave,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tool {
	Pen,
	Rubber,
}

#[cfg(feature = "input-impl")]
impl Tool {
	#[must_use]
	fn from_evdev(key: KeyCode) -> Option<Self> {
		Some(match key {
			KeyCode::BTN_TOOL_PEN => Self::Pen,
			KeyCode::BTN_TOOL_RUBBER => Self::Rubber,
			_ => return None,
		})
	}
}

#[allow(clippy::module_name_repetitions)] // `State` is already taken.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct StylusState {
	tool: Tool,
	touching: bool,
	x: u16,
	y: u16,
	pressure: u16,
	distance: u8,
	tilt_x: i16,
	tilt_y: i16,
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)] // Our numbers are all in a reasonable range.
impl StylusState {
	#[inline]
	#[must_use]
	pub fn tool(self) -> Tool {
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
		f32::from(self.y) * (rmox_common::fb::WIDTH as f32 / 15725.0)
	}

	#[inline]
	#[must_use]
	pub fn y(self) -> f32 {
		rmox_common::fb::HEIGHT as f32 - f32::from(self.x) * (rmox_common::fb::HEIGHT as f32 / 20967.0)
	}

	#[inline]
	#[must_use]
	pub fn position(self) -> Pos2 {
		pos2(self.x() as i32, self.y() as i32)
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
	pub fn tilt(self) -> Pos2 {
		pos2(self.tilt_x.into(), self.tilt_y.into())
	}
}

#[cfg(feature = "input-impl")]
pub(crate) type State = Option<StylusState>;

#[cfg(feature = "input-impl")]
pub(crate) fn handle_events(
	events: impl IntoIterator<Item = evdev::InputEvent>,
	input: &mut crate::InputState,
) {
	use evdev::{AbsoluteAxisCode as A, EventSummary as S};
	#[derive(Debug, Clone, Copy)]
	enum InternalEvent {
		Tool(Option<Tool>),
		Touch(bool),
		PositionX(u16),
		PositionY(u16),
		Pressure(u16),
		Distance(u8),
		TiltX(i16),
		TiltY(i16),
	}
	use InternalEvent as E;

	let state = &mut input.stylus;

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
				if let Some(tool) = Tool::from_evdev(key) {
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

	macro_rules! state {
		() => {{
			let Some(state) = state else {
				continue;
			};
			state
		}};
	}

	let prev_touching = state.map(|state| state.touching);

	for event in events {
		match event {
			E::Tool(v) => {
				*state = v.map(|tool| StylusState {
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
	let phase = match (prev_touching, state.map(|state| state.touching)) {
		(None, None) => return,
		(None, Some(true)) => Phase::Touch,
		(None, Some(false)) => Phase::Hover,
		(Some(true), None) => Phase::Lift,
		(Some(false), None) => Phase::Leave,
		(Some(false), Some(false)) | (Some(true), Some(true)) => Phase::Change,
		(Some(true), Some(false)) => Phase::Lift,
		(Some(false), Some(true)) => Phase::Touch,
	};
	input.enqueue(crate::Event::Stylus(Event { phase }));
}

#[cfg(feature = "input-impl")]
impl crate::Input {
	#[inline]
	#[must_use]
	pub fn stylus_state(&self) -> State {
		self.state.stylus
	}
}
