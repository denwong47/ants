//! Ants library.
//!
//! This library provides a simple way to create a worker pool to handle tasks.
//! It assumes that the tasks are CPU bound and that the workers are stateless.
//!
//! Built on top of the [`tokio`] runtime and [`tonic`] for
//! communication among the workers, this library does not require a leader
//! or a coordinator to manage the workers. Each worker is capable of accepting
//! tasks and processing them independently, or forwarding them to another
//! worker should it be busy.
//!
//! It is anticipated that each worker will host their own Axum server to
//! receive tasks and return the results, but this library does not enforce
//! that. It is up to the user to decide how to take tasks and do work, while
//! the library provides the means to relocate tasks among workers.
//!
//! It is not required for any data transferred to be in JSON, but it must be
//! serializable and deserializable into a string. Technically, base64 encoded
//! binary data can be used.
//!
//! See `bin/serve.rs` for an example of how to use this library to host a
//! server to listen and do work in ~100 lines of code.

pub mod config;
pub use config::CliArgs;

#[cfg(feature = "example")]
pub mod example;

pub mod postbox;

pub mod token;

pub mod nodes;

mod worker;
pub use worker::*;

mod errors;
pub use errors::AntsError;
