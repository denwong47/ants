//! An agent for handling all the multicast traffic bound to a specific address.

mod model;
pub use model::*;

mod acknowledgement;

mod delivery;

mod system;
pub use system::*;
