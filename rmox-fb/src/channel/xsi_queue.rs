/// A safe wrapper for an XSI message queue.
///
/// Currently only supports sending because that is what we need here.
#[derive(Debug)]
pub struct XsiQueue {
	handle: libc::c_int,
}

impl XsiQueue {
	/// Open the existing queue with the specified `id`.
	///
	/// The queue will not be created if it does not exist.
	pub fn open(id: libc::key_t) -> std::io::Result<Self> {
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
	pub fn send(&self, message_type: i32, data: &[u8]) -> std::io::Result<()> {
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
