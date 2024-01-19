use rmox_common::types::Side;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum SurfaceInit {
	Layer { anchor: Side, size: i32 },
	Normal,
	Wallpaper,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Command {
	CreateSurface(SurfaceInit),
}
