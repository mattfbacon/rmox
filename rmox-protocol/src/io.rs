//! Implements a simple message protocol where messages are a little-endian u32 of the payload length followed by a CBOR payload.

use futures_util::FutureExt as _;
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};
use tokio_stream::Stream;

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

/// This is useful because [`read`] is not cancel-safe while this `Stream` implementation necessarily is.
/// So if you want to use [`read`] in a `select!` arm, use this function instead.
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
