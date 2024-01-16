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

use evdev::{AbsoluteAxisCode, Device, EventStream, EventType, KeyCode};
use futures_core::Stream;

use crate::keyboard::key::{Key, Scancode};
use crate::keyboard::modifiers::{Modifier, Modifiers};
use crate::keyboard::{ButtonEvent, KeyEvent};
use crate::stylus::StylusEvent;
use crate::touch::TouchEvent;

pub mod keyboard;
pub mod stylus;
pub mod touch;

#[derive(Debug)]
pub enum Event {
	Key(KeyEvent),
	Text(Box<str>),
	Button(ButtonEvent),
	Touch(TouchEvent),
	Stylus(StylusEvent),
	DevicePresence(SupportedDeviceType),
}

struct Devices {
	devices: [Option<EventStream>; SupportedDeviceType::ALL.len()],
	last_polled_device: u8,
	inotify: inotify::EventStream<[u8; 256]>,
}

struct InputState {
	out_queue: VecDeque<Event>,

	keyboard: crate::keyboard::State,
	touch: crate::touch::State,
	stylus: crate::stylus::State,
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

				keyboard: crate::keyboard::State::default(),
				touch: crate::touch::State::default(),
				stylus: crate::stylus::State::default(),
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
	fn enqueue(&mut self, event: Event) {
		self.out_queue.push_back(event);
	}
}

impl Input {
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
					SupportedDeviceType::Keyboard | SupportedDeviceType::Buttons => {
						crate::keyboard::handle_events
					}
					SupportedDeviceType::Touchscreen => crate::touch::handle_events,
					SupportedDeviceType::Stylus => crate::stylus::handle_events,
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
