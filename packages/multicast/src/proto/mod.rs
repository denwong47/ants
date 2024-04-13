//! protobuf structs and their implementations.
//!
//! The [`MulticastMessage`] struct is the main struct used in this crate. It supports
//! serialization and deserialization of Generic types to and from bytes, hashing,
//! and checksum generation.
//!
//! This allows a complex type to be serialized into a [`MulticastMessage`] and then
//! broadcasted to other nodes listening on the same multicast address.

include!(concat!(env!("OUT_DIR"), "/multicast.proto.rs"));

mod implementations;
