//! Additional implementations for the protobuf generated structs.

mod bytes;
mod conversions;
mod echo;
mod hash;
mod new;
mod uuid;

// Re-export the implementations for the protobuf structs.
use crate::build_error;
