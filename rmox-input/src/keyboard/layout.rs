use crate::{Key, Modifier, Modifiers, Scancode};

#[derive(Debug)]
pub enum Resolved {
	Text(Box<str>),
	Modifier(Modifier),
	NoneOfThese,
}

pub trait KeyboardLayout: std::fmt::Debug + Send {
	/// `modifiers` is provided mutably so that any modifiers that act as accessors for alternate keys can be consumed.
	fn scancode_to_key(&self, scancode: Scancode, modifiers: &mut Modifiers) -> Option<Key>;
	/// In this case `modifiers` cannot be modified because the `Key` has already been resolved and nothing at this point would justify hiding a modifier from the client.
	fn resolve(&self, key: Key, modifiers: Modifiers) -> Resolved;
}

#[derive(Debug)]
pub(crate) struct DefaultLayout;

impl KeyboardLayout for DefaultLayout {
	/// `modifiers` is provided mutably so that any modifiers that act as accessors for alternate keys can be consumed.
	fn scancode_to_key(&self, scancode: Scancode, modifiers: &mut Modifiers) -> Option<Key> {
		// We are using AltOpt as the accessor for alternative keys.
		'alt: {
			if modifiers.contains(Modifier::AltOpt) {
				let key = match scancode {
					Scancode::Tab => Key::Escape,
					Scancode::ArrowLeft => Key::Home,
					Scancode::ArrowRight => Key::End,
					Scancode::ArrowUp => Key::PageUp,
					Scancode::ArrowDown => Key::PageDown,
					Scancode::Backspace => Key::Delete,
					Scancode::Enter => Key::Insert,
					_ => break 'alt,
				};
				*modifiers -= Modifier::Opt;
				return Some(key);
			}
		}
		Some(scancode.to_key_base())
	}

	/// In this case `modifiers` cannot be modified because the `Key` has already been resolved and nothing at this point would justify hiding a modifier from the client.
	fn resolve(&self, key: Key, modifiers: Modifiers) -> Resolved {
		fn basic_key(layers: [u8; 4], alpha: bool, modifiers: Modifiers) -> Resolved {
			let ch = if modifiers.opt() {
				if modifiers.shift(false) {
					layers[3]
				} else {
					layers[2]
				}
			} else if modifiers.shift(alpha) {
				layers[1]
			} else {
				layers[0]
			};
			let mut buf = [0u8; 4];
			let str = &*char::from(ch).encode_utf8(&mut buf);
			Resolved::Text(str.into())
		}

		fn special_key(layers: [u8; 4], modifiers: Modifiers) -> Resolved {
			basic_key(layers, false, modifiers)
		}

		fn nonalpha_key(layers: [u8; 2], modifiers: Modifiers) -> Resolved {
			basic_key(
				[layers[0], layers[1], layers[0], layers[1]],
				false,
				modifiers,
			)
		}

		fn alpha_key(lower: u8, modifiers: Modifiers) -> Resolved {
			let upper = lower.to_ascii_uppercase();
			basic_key([lower, upper, lower, upper], true, modifiers)
		}

		#[allow(clippy::match_same_arms)] // One arm per key.
		match key {
			Key::Num1 => special_key(*b"1!`~", modifiers),
			Key::Num2 => nonalpha_key(*b"2@", modifiers),
			Key::Num3 => nonalpha_key(*b"3#", modifiers),
			Key::Num4 => nonalpha_key(*b"4$", modifiers),
			Key::Num5 => nonalpha_key(*b"5%", modifiers),
			Key::Num6 => nonalpha_key(*b"6^", modifiers),
			Key::Num7 => nonalpha_key(*b"7&", modifiers),
			Key::Num8 => nonalpha_key(*b"8*", modifiers),
			Key::Num9 => nonalpha_key(*b"9(", modifiers),
			Key::Num0 => nonalpha_key(*b"0)", modifiers),
			Key::Hyphen => special_key(*b"[{]}", modifiers),
			Key::Backspace => Resolved::NoneOfThese,

			Key::Tab => Resolved::Text("\t".into()),
			Key::Q => nonalpha_key(*br#"'""#, modifiers),
			Key::W => nonalpha_key(*b",<", modifiers),
			Key::E => nonalpha_key(*b".>", modifiers),
			Key::R => alpha_key(b'p', modifiers),
			Key::T => alpha_key(b'y', modifiers),
			Key::Y => alpha_key(b'f', modifiers),
			Key::U => alpha_key(b'g', modifiers),
			Key::I => alpha_key(b'c', modifiers),
			Key::O => alpha_key(b'r', modifiers),
			Key::P => alpha_key(b'l', modifiers),
			Key::Grave => nonalpha_key(*b"/?", modifiers),
			Key::Tilde => special_key(*br"=+\|", modifiers),

			Key::CapsLock => Resolved::Modifier(Modifier::CapsLock),
			Key::A => alpha_key(b'a', modifiers),
			Key::S => alpha_key(b'o', modifiers),
			Key::D => alpha_key(b'e', modifiers),
			Key::F => alpha_key(b'u', modifiers),
			Key::G => alpha_key(b'i', modifiers),
			Key::H => alpha_key(b'd', modifiers),
			Key::J => alpha_key(b'h', modifiers),
			Key::K => alpha_key(b't', modifiers),
			Key::L => alpha_key(b'n', modifiers),
			Key::Semicolon => alpha_key(b's', modifiers),
			Key::Apostrophe => nonalpha_key(*b"-_", modifiers),
			// egui's convention is that Enter does not have any associated text.
			// However, we're trying this instead.
			Key::Enter => Resolved::Text("\n".into()),

			Key::LeftShift => Resolved::Modifier(Modifier::LeftShift),
			Key::Z => nonalpha_key(*b";:", modifiers),
			Key::X => alpha_key(b'q', modifiers),
			Key::C => alpha_key(b'j', modifiers),
			Key::V => alpha_key(b'k', modifiers),
			Key::B => alpha_key(b'x', modifiers),
			Key::N => alpha_key(b'b', modifiers),
			Key::M => alpha_key(b'm', modifiers),
			Key::Comma => alpha_key(b'w', modifiers),
			Key::Period => alpha_key(b'v', modifiers),
			Key::Slash => alpha_key(b'z', modifiers),
			Key::RightShift => Resolved::Modifier(Modifier::RightShift),

			Key::Ctrl => Resolved::Modifier(Modifier::Ctrl),
			Key::Opt => Resolved::Modifier(Modifier::Opt),
			Key::Alt => Resolved::Modifier(Modifier::Alt),
			Key::Space => Resolved::Text(" ".into()),
			Key::AltOpt => Resolved::Modifier(Modifier::AltOpt),
			Key::ArrowLeft => Resolved::NoneOfThese,
			Key::ArrowUp => Resolved::NoneOfThese,
			Key::ArrowDown => Resolved::NoneOfThese,
			Key::ArrowRight => Resolved::NoneOfThese,
			Key::Escape => Resolved::NoneOfThese,
			Key::Insert => Resolved::NoneOfThese,
			Key::Delete => Resolved::NoneOfThese,
			Key::PageUp => Resolved::NoneOfThese,
			Key::PageDown => Resolved::NoneOfThese,
			Key::Home => Resolved::NoneOfThese,
			Key::End => Resolved::NoneOfThese,

			Key::Power => Resolved::NoneOfThese,
		}
	}
}
