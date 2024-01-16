pub mod io;

pub mod client_to_server;
pub mod server_to_client;

pub mod server {
	pub use crate::{client_to_server as recv, server_to_client as send};
}

pub mod client {
	pub use crate::{client_to_server as send, server_to_client as recv};
}
