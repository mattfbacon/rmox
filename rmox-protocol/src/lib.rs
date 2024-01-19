use std::num::NonZeroU32;

use serde::{Deserialize, Serialize};

pub mod io;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Id(NonZeroU32);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SurfaceId(pub Id);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskId(pub Id);

impl Id {
	pub const START: Self = Self(match NonZeroU32::new(1) {
		Some(v) => v,
		None => panic!(),
	});

	pub fn step(self) -> Self {
		let next = self.0.get().wrapping_add(1);
		NonZeroU32::new(next).map_or(Self::START, Self)
	}
}

pub mod client_to_server;
pub mod server_to_client;

pub mod server {
	pub use crate::{client_to_server as recv, server_to_client as send};
}

pub mod client {
	pub use crate::{client_to_server as send, server_to_client as recv};
}
