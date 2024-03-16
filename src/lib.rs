//! Ants library.
//!

mod config;
pub use config::CliArgs;

#[cfg(feature = "example")]
pub mod example;

mod worker;
pub use worker::*;

mod errors;
pub use errors::AntsError;
