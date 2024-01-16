use embedded_graphics_core::pixelcolor::raw::{RawData, RawU16};
use embedded_graphics_core::pixelcolor::Rgb565;
use memmap2::MmapMut;
use rmox_common::types::Pos2;

use crate::Framebuffer;

#[derive(Debug)]
pub struct Mapping {
	mapping: MmapMut,
}

impl Mapping {
	const PATH: &'static str = "/dev/shm/swtfb.01";

	/// Does not bounds-check the point.
	#[must_use]
	pub fn point_to_index(point: Pos2) -> usize {
		usize::try_from(point.y).unwrap() * usize::try_from(Framebuffer::WIDTH).unwrap()
			+ usize::try_from(point.x).unwrap()
	}

	pub fn open() -> std::io::Result<Self> {
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
	pub fn set_pixel(&mut self, point: Pos2, color: Rgb565) {
		self.pixels_mut()[Self::point_to_index(point)] = RawU16::from(color).into_inner();
	}
}
