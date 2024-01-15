//! Currently only supporting rM2 because this is the device that I have.
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
// Unsafe code is allowed in this crate due to the low-level interfacing.

use embedded_graphics_core::draw_target::DrawTarget;
use embedded_graphics_core::geometry::{Dimensions, OriginDimensions, Size};
use embedded_graphics_core::pixelcolor::raw::{RawData, RawU16};
use embedded_graphics_core::pixelcolor::Rgb565;
use embedded_graphics_core::primitives::{PointsIter as _, Rectangle as BadRect};
use memmap2::MmapMut;
use rmox_common::{mut_draw_target, EinkUpdate, Pos2, Rectangle, UpdateDepth, UpdateStyle, Vec2};

pub mod util;

/// A safe wrapper for an XSI message queue.
///
/// Currently only supports sending because that is what we need here.
struct XsiQueue {
	handle: libc::c_int,
}

impl XsiQueue {
	/// Open the existing queue with the specified `id`.
	///
	/// The queue will not be created if it does not exist.
	fn open(id: libc::key_t) -> std::io::Result<Self> {
		// Flags (the second parameter) only apply if we are creating the message queue.
		// Since we are not, we just leave it as 0.
		// SAFETY: I contacted Dennis Ritchie in a seance and he told me it's thread safe.
		let handle = unsafe { libc::msgget(id, 0) };
		if handle == -1 {
			return Err(std::io::Error::last_os_error());
		}
		Ok(Self { handle })
	}

	/// Send a message with the given type and data.
	///
	/// The data is (currently) limited to 512 bytes because the message data is stored on the stack.
	///
	/// The `IPC_NOWAIT` flag is not set, so sends will block if the queue is full.
	/// This mirrors the behavior of Rust's standard channels.
	/// Since there is no way to wait for space in the queue with `poll`/`select`-type interfaces,
	/// the best way to implement a non-blocking send (or an asynchronous send, which would be based on such)
	/// is to spawn a thread and call this method.
	fn send(&self, message_type: i32, data: &[u8]) -> std::io::Result<()> {
		#[derive(bytemuck::Pod, bytemuck::Zeroable, Clone, Copy)]
		#[repr(C)]
		struct RawMessage {
			type_: libc::c_long,
			data: [u8; 512],
		}

		let mut raw = RawMessage {
			type_: message_type.into(),
			data: [0u8; 512],
		};
		raw
			.data
			.get_mut(..data.len())
			.expect("data is too large")
			.copy_from_slice(data);
		let raw = bytemuck::bytes_of(&raw);
		// SAFETY: The message struct is `repr(C)` and has fields `long` and `char[]`.
		// The length passed matches the length of `data`,
		// which is guaranteed to stay within the bounds of `raw`
		// because it is checked when we copy the data in.
		//
		// As for thread safety, who knows!
		let ret = unsafe { libc::msgsnd(self.handle, raw.as_ptr().cast(), data.len(), 0) };
		if ret == -1 {
			return Err(std::io::Error::last_os_error());
		}
		Ok(())
	}
}

struct Channel {
	queue: XsiQueue,
}

impl Channel {
	const QUEUE_KEY: libc::key_t = 0x2257c;

	#[inline]
	fn open() -> std::io::Result<Self> {
		tracing::debug!("open channel");

		Ok(Self {
			queue: XsiQueue::open(Self::QUEUE_KEY)?,
		})
	}

	fn _update(
		&self,
		rect: &Rectangle,
		style: UpdateStyle,
		depth: UpdateDepth,
	) -> std::io::Result<()> {
		#[derive(bytemuck::Pod, bytemuck::Zeroable, Clone, Copy)]
		#[repr(C)]
		struct Raw {
			top: u32,
			left: u32,
			width: u32,
			height: u32,
			waveform_mode: u32,
			update_mode: u32,
			update_marker: u32,
			temp: i32,
			flags: u32,
			dither_mode: i32,
			quant_bit: i32,
			_unused: [u32; 7],
		}

		tracing::debug!(?rect, ?style, ?depth, "channel update");

		let rect = rect.intersection(&Framebuffer::RECT);
		if rect.is_empty() {
			return Ok(());
		}

		let raw = Raw {
			top: rect.origin.y.try_into().unwrap(),
			left: rect.origin.x.try_into().unwrap(),
			width: rect.size.x.try_into().unwrap(),
			height: rect.size.y.try_into().unwrap(),
			waveform_mode: match style {
				// Init.
				UpdateStyle::Init => 0x0,
				// Gc16-fast.
				UpdateStyle::Rgb => 0x3,
				// Direct update.
				UpdateStyle::Monochrome => 0x1,
			},
			update_mode: match depth {
				// Full update.
				UpdateDepth::Full => 1,
				// Partial update.
				UpdateDepth::Partial => 0,
			},
			// Unused since we don't wait for updates (yet).
			update_marker: 1,
			// "Remarkable draw" mode.
			temp: 0x0018,
			flags: 0,
			// Tell EPDC to use dithering passthrough.
			dither_mode: 0,
			// No idea what this does.
			quant_bit: 0,
			_unused: [0; 7],
		};
		// Update message type.
		self.queue.send(2, bytemuck::bytes_of(&raw))
	}
}

struct FramebufferMapping {
	mapping: MmapMut,
}

impl FramebufferMapping {
	const PATH: &'static str = "/dev/shm/swtfb.01";

	/// Does not bounds-check the point.
	#[must_use]
	fn point_to_index(point: Pos2) -> usize {
		usize::try_from(point.y).unwrap() * usize::try_from(Framebuffer::WIDTH).unwrap()
			+ usize::try_from(point.x).unwrap()
	}

	fn open() -> std::io::Result<Self> {
		tracing::debug!("open framebuffer mapping");

		let size_bytes = u64::try_from(Framebuffer::WIDTH * Framebuffer::HEIGHT)
			.unwrap_or_else(|_| unreachable!())
			* u64::try_from(std::mem::size_of::<Rgb565>()).unwrap_or_else(|_| unreachable!());

		let file = std::fs::OpenOptions::new()
			.read(true)
			.write(true)
			.open(Self::PATH)?;
		file.set_len(size_bytes)?;
		// SAFETY: Yeah, the buffer is shared and can change underneath us.
		// But in practice we are using it as a write-only bitbucket so it's not really an issue.
		// And it _probably_ won't change while we're accessing it.
		let mapping = unsafe { MmapMut::map_mut(&file) }?;
		Ok(Self { mapping })
	}

	#[must_use]
	pub fn pixels_mut(&mut self) -> &mut [u16] {
		bytemuck::cast_slice_mut(&mut self.mapping)
	}

	/// Does not bounds-check the point.
	fn set_pixel(&mut self, point: Pos2, color: Rgb565) {
		self.pixels_mut()[Self::point_to_index(point)] = RawU16::from(color).into_inner();
	}
}

pub struct Framebuffer {
	mapping: FramebufferMapping,
	channel: Channel,
}

impl Framebuffer {
	pub const WIDTH: i32 = rmox_common::FB_WIDTH;
	pub const HEIGHT: i32 = rmox_common::FB_HEIGHT;
	pub const SIZE: Vec2 = Vec2 {
		x: Self::WIDTH,
		y: Self::HEIGHT,
	};
	pub const RECT: Rectangle = Rectangle {
		origin: Pos2 { x: 0, y: 0 },
		size: Self::SIZE,
	};

	/// # Errors
	///
	/// - Opening the framebuffer
	/// - Mapping the framebuffer
	/// - Getting the rm2fb IPC channel
	#[inline]
	pub fn open() -> std::io::Result<Self> {
		tracing::debug!("open framebuffer");

		Ok(Self {
			mapping: FramebufferMapping::open()?,
			channel: Channel::open()?,
		})
	}

	#[inline]
	#[must_use]
	pub fn pixels_mut(&mut self) -> &mut [u16] {
		self.mapping.pixels_mut()
	}
}

impl OriginDimensions for Framebuffer {
	#[inline]
	fn size(&self) -> Size {
		Self::SIZE.try_into().unwrap()
	}
}

impl DrawTarget for Framebuffer {
	type Color = Rgb565;
	type Error = std::convert::Infallible;

	fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
	where
		I: IntoIterator<Item = embedded_graphics_core::Pixel<Rgb565>>,
	{
		let bounds = self.bounding_box();
		let pixels = pixels.into_iter().filter(|pixel| bounds.contains(pixel.0));
		for pixel in pixels {
			self.mapping.set_pixel(pixel.0.into(), pixel.1);
		}
		Ok(())
	}

	fn fill_contiguous<I>(&mut self, area: &BadRect, colors: I) -> Result<(), Self::Error>
	where
		I: IntoIterator<Item = Self::Color>,
	{
		let bounds = self.bounding_box();
		let intersection = bounds.intersection(area);

		let pixels = area
			.points()
			.zip(colors)
			.map(|(pos, color)| embedded_graphics_core::Pixel(pos, color));

		// Only filter if part of `area` is out-of-bounds.
		if &intersection == area {
			for pixel in pixels {
				self.mapping.set_pixel(pixel.0.into(), pixel.1);
			}
		} else {
			let pixels = pixels.into_iter().filter(|pixel| bounds.contains(pixel.0));
			for pixel in pixels {
				self.mapping.set_pixel(pixel.0.into(), pixel.1);
			}
		}

		Ok(())
	}

	fn fill_solid(&mut self, area: &BadRect, color: Self::Color) -> Result<(), Self::Error> {
		let area = area.intersection(&self.bounding_box());
		let color = RawU16::from(color).into_inner();
		let pixels = self.mapping.pixels_mut();
		for y in area.rows() {
			let y_index = FramebufferMapping::point_to_index(Pos2 { x: 0, y });
			let x_range = area.columns();
			let x_range = usize::try_from(x_range.start).unwrap()..usize::try_from(x_range.end).unwrap();
			pixels[y_index..][x_range].fill(color);
		}
		Ok(())
	}

	fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
		self
			.mapping
			.pixels_mut()
			.fill(RawU16::from(color).into_inner());
		Ok(())
	}
}

impl EinkUpdate for Framebuffer {
	fn update(
		&self,
		area: &Rectangle,
		style: UpdateStyle,
		depth: UpdateDepth,
	) -> std::io::Result<()> {
		self.channel._update(area, style, depth)
	}
}

mut_draw_target!(Framebuffer);
