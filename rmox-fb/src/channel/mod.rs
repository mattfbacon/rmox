use rmox_common::eink_update::{UpdateDepth, UpdateStyle};
use rmox_common::types::Rectangle;

use self::xsi_queue::XsiQueue;
use crate::Framebuffer;

mod xsi_queue;

#[derive(Debug)]
pub struct Channel {
	queue: XsiQueue,
}

impl Channel {
	const QUEUE_KEY: libc::key_t = 0x2257c;

	pub fn open() -> std::io::Result<Self> {
		tracing::debug!("open channel");

		Ok(Self {
			queue: XsiQueue::open(Self::QUEUE_KEY)?,
		})
	}

	pub fn _update(
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

		let rect = rect.normalize().intersection(&Framebuffer::RECT);
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
