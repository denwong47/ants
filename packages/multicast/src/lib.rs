//! Crate to handle multicast communication.

pub mod delivery;
pub mod socket;

/// The protocol buffer generated code.
pub mod proto;

/// The main multicast struct that handles sending and receiving multicast messages.
mod agent;
pub use agent::*;

use std::io;
/// Builds a [`io::Error`] with the given message.
pub fn build_error(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
pub mod _tests;

/// Re-export the [`logger`] module.
pub use logger;
