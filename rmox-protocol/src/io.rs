//! Implements a simple message protocol where messages are a little-endian u32 of the payload length followed by a CBOR payload.

use futures_util::FutureExt as _;
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};
use tokio_stream::Stream;

pub async fn read_<T: AsyncRead + Unpin, Item: DeserializeOwned>(
	mut reader: T,
) -> (Option<Result<Item, ciborium::de::Error<std::io::Error>>>, T) {
	let mut buf = [0u8; 4];
	if let Err(error) = reader.read_exact(&mut buf).await {
		let ret = if error.kind() == std::io::ErrorKind::UnexpectedEof {
			None
		} else {
			Some(Err(error.into()))
		};
		return (ret, reader);
	}
	let size: usize = u32::from_le_bytes(buf).try_into().unwrap();

	let mut buf = vec![0u8; size];
	if let Err(error) = reader.read_exact(&mut buf).await {
		return (Some(Err(error.into())), reader);
	}
	(Some(ciborium::from_reader(&*buf)), reader)
}

/// Returns `None` if the reader has reached EOF.
pub async fn read<T: AsyncRead + Unpin, Item: DeserializeOwned>(
	reader: T,
) -> Option<Result<Item, ciborium::de::Error<std::io::Error>>> {
	read_(reader).await.0
}

/// This is useful because [`read`] is not cancel-safe while this, being a `Stream`, inherently is.
///
/// So if you want to use [`read`] in a `select!` arm, use this function instead.
/// It's essentially a more convenient form of creating the future, pinning it,
/// polling it through `&mut` in the `select!` arm,
/// and replacing the future with a new instance in that arm.
#[inline]
pub fn read_stream<'a, T: AsyncRead + Unpin + 'a, Item: DeserializeOwned + 'a>(
	reader: T,
) -> impl Stream<Item = Result<Item, ciborium::de::Error<std::io::Error>>> + 'a {
	futures_util::stream::unfold(reader, move |reader| {
		read_(reader).map(|(opt, reader)| Some((opt?, reader)))
	})
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
