use serde::{Deserialize, Serialize};

macro_rules! scancode_and_key {
	(physical: [$($physical_name:tt = $physical_evdev:tt,)*] virtual: [$($virtual_name:tt,)*]) => {
		/// Scancodes refer to physical locations of keys on the Type Folio.
		/// As such, they are named by their physical label.
		/// Keys with multiple labels use their base mapping, i.e., without any modifiers.
		#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
		#[repr(u8)]
		pub enum Scancode {
			$($physical_name,)*
		}

		impl Scancode {
			#[cfg(feature = "input-impl")]
			#[must_use]
			pub(crate) fn from_evdev(raw: evdev::KeyCode) -> Option<Self> {
				Some(match raw {
					$(evdev::KeyCode::$physical_evdev => Self::$physical_name,)*
					_ => return None,
				})
			}

			/// As stated in the docs for [`Key`], every physical scancode corresponds to a virtual key in its base state.
			/// This function performs that base state mapping.
			#[inline]
			#[must_use]
			pub fn to_key_base(self) -> Key {
				match self {
					$(Self::$physical_name => Key::$physical_name,)*
				}
			}
		}

		impl Scancode {
			pub const ALL: &'static [Self] = &[$(Self::$physical_name,)*];
		}

		/// Keys are virtual and refer to concepts that users expect when pressing certain keys.
		/// Every physical key corresponds to one virtual key in its base state, but some physical keys may also have additional virtual keys that can be accessed with modifiers.
		///
		/// To convert from scancodes to keys, you must go through the keyboard layout.
		#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
		#[repr(u8)]
		pub enum Key {
			$($physical_name,)*
			$($virtual_name,)*
		}

		impl Key {
			pub const ALL: &'static [Self] = &[$(Self::$physical_name,)* $(Self::$virtual_name,)*];
		}
	};
}

scancode_and_key! {
	physical: [
		// Keyboard.
		Num1 = KEY_1,
		Num2 = KEY_2,
		Num3 = KEY_3,
		Num4 = KEY_4,
		Num5 = KEY_5,
		Num6 = KEY_6,
		Num7 = KEY_7,
		Num8 = KEY_8,
		Num9 = KEY_9,
		Num0 = KEY_0,
		Hyphen = KEY_EQUAL,
		Backspace = KEY_BACKSPACE,

		Tab = KEY_TAB,
		Q = KEY_Q,
		W = KEY_W,
		E = KEY_E,
		R = KEY_R,
		T = KEY_T,
		Y = KEY_Y,
		U = KEY_U,
		I = KEY_I,
		O = KEY_O,
		P = KEY_P,
		Grave = KEY_GRAVE,
		Tilde = KEY_BACKSLASH,

		CapsLock = KEY_CAPSLOCK,
		A = KEY_A,
		S = KEY_S,
		D = KEY_D,
		F = KEY_F,
		G = KEY_G,
		H = KEY_H,
		J = KEY_J,
		K = KEY_K,
		L = KEY_L,
		Semicolon = KEY_SEMICOLON,
		Apostrophe = KEY_APOSTROPHE,
		Enter = KEY_ENTER,

		LeftShift = KEY_LEFTSHIFT,
		Z = KEY_Z,
		X = KEY_X,
		C = KEY_C,
		V = KEY_V,
		B = KEY_B,
		N = KEY_N,
		M = KEY_M,
		Comma = KEY_COMMA,
		Period = KEY_DOT,
		Slash = KEY_SLASH,
		RightShift = KEY_RIGHTSHIFT,
		Ctrl = KEY_LEFTCTRL,
		Opt = KEY_END,
		Alt = KEY_LEFTALT,
		Space = KEY_SPACE,
		AltOpt = KEY_RIGHTALT,
		ArrowLeft = KEY_LEFT,
		ArrowUp = KEY_UP,
		ArrowDown = KEY_DOWN,
		ArrowRight = KEY_RIGHT,

		// Buttons.
		Power = KEY_POWER,
	]
	virtual: [
		Escape,
		Insert,
		Delete,
		PageUp,
		PageDown,
		Home,
		End,
	]
}
