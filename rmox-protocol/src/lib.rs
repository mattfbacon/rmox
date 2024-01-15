use embedded_graphics_core::draw_target::DrawTarget;
use embedded_graphics_core::geometry::{OriginDimensions, Size};
use embedded_graphics_core::primitives::Rectangle as BadRect;
use embedded_graphics_core::Pixel;
use futures_util::FutureExt as _;
use rmox_common::{
	mut_draw_target, EinkUpdate, Pos2, Rectangle, Rotation, Side, UpdateDepth, UpdateStyle, Vec2,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};
use tokio_stream::Stream;

/*
#[inline]
#[must_use]
pub fn encode<T: Serialize + ?Sized>(message: &T) -> Vec<u8> {
	let mut ret = Vec::new();
	ciborium::into_writer(&message, &mut ret).unwrap();
	ret
}

#[inline]
#[must_use]
pub fn decode<T: DeserializeOwned>(raw: &[u8]) -> Result<T, ciborium::de::Error<std::io::Error>> {
	ciborium::from_reader(raw)
}
*/

pub async fn read_<T: AsyncRead + Unpin, Item: DeserializeOwned>(
	mut reader: T,
) -> (Result<Item, ciborium::de::Error<std::io::Error>>, T) {
	let mut buf = [0u8; 4];
	if let Err(error) = reader.read_exact(&mut buf).await {
		return (Err(error.into()), reader);
	}
	let size: usize = u32::from_le_bytes(buf).try_into().unwrap();

	let mut buf = vec![0u8; size];
	if let Err(error) = reader.read_exact(&mut buf).await {
		return (Err(error.into()), reader);
	}
	(ciborium::from_reader(&*buf), reader)
}

pub async fn read<T: AsyncRead + Unpin, Item: DeserializeOwned>(
	reader: T,
) -> Result<Item, ciborium::de::Error<std::io::Error>> {
	read_(reader).await.0
}

#[inline]
pub fn read_stream<'a, T: AsyncRead + Unpin + 'a, Item: DeserializeOwned + 'a>(
	reader: T,
) -> impl Stream<Item = Result<Item, ciborium::de::Error<std::io::Error>>> + 'a {
	futures_util::stream::unfold(reader, move |reader| read_(reader).map(Some))
}

pub async fn write<T: AsyncWrite + Unpin, Item: Serialize + ?Sized>(
	mut writer: T,
	message: &Item,
) -> std::io::Result<()> {
	let mut ret = vec![0u8; 4];
	ciborium::into_writer(message, &mut ret).unwrap();

	let size: u32 = (ret.len() - 4).try_into().unwrap();
	ret[0..4].copy_from_slice(&size.to_le_bytes());

	writer.write_all(&ret).await?;
	Ok(())
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SurfaceDescription {
	pub base_rect: Rectangle,
	pub rotation: Rotation,
	pub scale: u8,
}

impl SurfaceDescription {
	#[inline]
	#[must_use]
	pub fn transform_point(&self, mut point: Pos2) -> Pos2 {
		point *= self.scale.into();
		point = self.rotation.transform_point(point, self.base_rect.size);
		point += self.base_rect.origin.to_vec();
		point
	}

	#[inline]
	pub fn transform_rect(&self, rect: &mut Rectangle) {
		rect.origin *= self.scale.into();
		rect.size *= self.scale.into();

		self.rotation.transform_rect(rect, &self.base_rect.size);

		rect.origin += self.base_rect.origin.to_vec();
	}

	#[inline]
	#[must_use]
	pub fn size(&self) -> Vec2 {
		let mut size = self.base_rect.size;
		size = self.rotation.inverse().transform_size(size);
		size /= self.scale.into();
		size
	}

	#[inline]
	#[must_use]
	pub fn transform<'a, T: ?Sized>(&'a self, base: &'a mut T) -> Transformed<'a, T> {
		Transformed {
			base,
			description: self,
		}
	}
}

#[test]
fn test_transform_point() {
	use rmox_common::{pos2, rect};

	let desc = SurfaceDescription {
		base_rect: rect(200, 200, 500, 800),
		rotation: Rotation::Rotate270,
		scale: 2,
	};
	assert_eq!(desc.transform_point(pos2(0, 0)), pos2(200, 1000),);
	assert_eq!(desc.transform_point(pos2(10, 0)), pos2(200, 980),);
	assert_eq!(desc.transform_point(pos2(10, 20)), pos2(240, 980),);
}

#[test]
fn test_transform_rect() {
	use rmox_common::{rect, vec2};

	let mut r = rect(0, 0, 100, 200);
	Rotation::Rotate270.transform_rect(&mut r, &vec2(300, 300));
	assert_eq!(r, rect(0, 200, 200, 100));
}

pub struct Transformed<'a, T: ?Sized> {
	base: &'a mut T,
	description: &'a SurfaceDescription,
}

impl<T: OriginDimensions> OriginDimensions for Transformed<'_, T> {
	fn size(&self) -> Size {
		self.description.size().try_into().unwrap()
	}
}

impl<T: OriginDimensions + DrawTarget> DrawTarget for Transformed<'_, T> {
	type Color = <T as DrawTarget>::Color;

	type Error = <T as DrawTarget>::Error;

	fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
	where
		I: IntoIterator<Item = Pixel<Self::Color>>,
	{
		let map = |pixel: Pixel<_>| {
			let point = self.description.transform_point(pixel.0.into()).into();
			Pixel(point, pixel.1)
		};
		self.base.draw_iter(pixels.into_iter().map(map))
	}

	fn fill_solid(&mut self, area: &BadRect, color: Self::Color) -> Result<(), Self::Error> {
		let mut area: Rectangle = (*area).into();
		self.description.transform_rect(&mut area);
		self.base.fill_solid(&area.into(), color)
	}

	fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
		let rect = self.description.base_rect.into();
		self.base.fill_solid(&rect, color)
	}
}

impl<T: EinkUpdate> EinkUpdate for Transformed<'_, T> {
	fn update(
		&self,
		area: &Rectangle,
		style: UpdateStyle,
		depth: UpdateDepth,
	) -> std::io::Result<()> {
		let mut area = *area;
		self.description.transform_rect(&mut area);
		self.base.update(&area, style, depth)
	}
}

mut_draw_target!(Transformed<'a, T>: ['a, T: OriginDimensions + DrawTarget]);

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum SurfaceInit {
	Layer { anchor: Side, size: i32 },
	Normal,
}

mod internal {
	pub mod server_to_client {
		use serde::{Deserialize, Serialize};

		use crate::SurfaceDescription;

		#[derive(Debug, Serialize, Deserialize)]
		pub enum Event {
			Surface(SurfaceDescription),
			Quit,
		}
	}

	pub mod client_to_server {
		use serde::{Deserialize, Serialize};

		use crate::SurfaceInit;

		#[derive(Debug, Serialize, Deserialize)]
		pub enum Command {
			CreateSurface(SurfaceInit),
		}
	}
}

pub mod server {
	pub use crate::internal::{client_to_server as recv, server_to_client as send};
}

pub mod client {
	pub use crate::internal::{client_to_server as send, server_to_client as recv};
}
