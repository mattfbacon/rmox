mod pos2;
mod rectangle;
mod rotation;
mod side;
mod vec2;

pub use self::pos2::{pos2, Pos2};
pub use self::rectangle::{rect, Rectangle};
pub use self::rotation::Rotation;
pub use self::side::Side;
pub use self::vec2::{vec2, Vec2};

#[derive(Debug)]
pub struct ComponentOutOfRange;
