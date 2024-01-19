//! Implements a simple message protocol where messages are a little-endian u32 of the payload length followed by a CBOR payload.

use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{ready, Context, Poll};

use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt as _, ReadBuf};

async fn write<T: AsyncWrite + Unpin, Item: Serialize + ?Sized>(
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

enum ReadState {
	Start,
	Size(u32),
}

pin_project_lite::pin_project! {
pub struct Stream<T, ReadItem, WriteItem: ?Sized> {
	#[pin]
	inner: T,
	buf: Vec<u8>,
	read_state: ReadState,
	_items: PhantomData<(ReadItem, WriteItem)>,
}
}

impl<T, ReadItem, WriteItem> Stream<T, ReadItem, WriteItem> {
	pub fn new(inner: T) -> Self {
		Self {
			inner,
			buf: Vec::with_capacity(4),
			read_state: ReadState::Start,
			_items: PhantomData,
		}
	}
}

struct LenGuard<'a> {
	buf: &'a mut Vec<u8>,
	prev_len: usize,
}

impl<'a> LenGuard<'a> {
	fn new(buf: &'a mut Vec<u8>, len: usize) -> Self {
		let prev_len = buf.len();
		debug_assert!(prev_len < len);
		buf.resize(len, 0);
		Self { buf, prev_len }
	}

	fn finish(mut self, len: usize) {
		self.prev_len = len;
		drop(self);
	}
}

impl Drop for LenGuard<'_> {
	fn drop(&mut self) {
		self.buf.truncate(self.prev_len);
	}
}

impl<T, ReadItem, WriteItem> tokio_stream::Stream for Stream<T, ReadItem, WriteItem>
where
	T: AsyncRead + Unpin,
	ReadItem: DeserializeOwned,
	WriteItem: ?Sized,
{
	type Item = Result<ReadItem, ciborium::de::Error<std::io::Error>>;

	fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
		let mut this = self.project();
		loop {
			match *this.read_state {
				ReadState::Start => {
					let buf = LenGuard::new(this.buf, 4);
					let mut read_buf = ReadBuf::new(buf.buf);
					read_buf.set_filled(buf.prev_len);
					if let Err(error) = ready!(this.inner.as_mut().poll_read(cx, &mut read_buf)) {
						return Poll::Ready(if error.kind() == std::io::ErrorKind::ConnectionReset {
							None
						} else {
							Some(Err(error.into()))
						});
					}
					let read = read_buf.filled();
					if read.len() == buf.prev_len {
						return Poll::Ready(None);
					}
					if read.len() >= 4 {
						assert_eq!(read.len(), 4);
						let message_size = u32::from_le_bytes(read[..4].try_into().unwrap());
						*this.read_state = ReadState::Size(message_size);
						buf.finish(0);
					} else {
						let len = read.len();
						buf.finish(len);
					}
				}
				ReadState::Size(message_size) => {
					let message_size = message_size.try_into().unwrap();
					let buf = LenGuard::new(this.buf, message_size);
					let mut read_buf = tokio::io::ReadBuf::new(buf.buf);
					read_buf.set_filled(buf.prev_len);
					if let Err(error) = ready!(this.inner.as_mut().poll_read(cx, &mut read_buf)) {
						return Poll::Ready(Some(Err(error.into())));
					}
					let read = read_buf.filled();
					if read.len() == buf.prev_len {
						let error = std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "early eof");
						return Poll::Ready(Some(Err(error.into())));
					}
					if read.len() >= message_size {
						assert_eq!(read.len(), message_size);
						*this.read_state = ReadState::Start;
						let res = ciborium::from_reader(read);
						buf.finish(0);
						return Poll::Ready(Some(res));
					} else {
						let len = read.len();
						buf.finish(len);
					}
				}
			}
		}
	}
}

impl<T, ReadItem, WriteItem> Stream<T, ReadItem, WriteItem>
where
	T: AsyncWrite + Unpin,
	WriteItem: Serialize + ?Sized,
{
	#[inline]
	pub async fn write(&mut self, message: &WriteItem) -> std::io::Result<()> {
		write(&mut self.inner, message).await
	}
}
