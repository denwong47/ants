//! Configurations for the chat server.
//!
//! These are typically the default values; they can be overridden by command line arguments.
//!

use std::net::Ipv4Addr;

/// The multicast address used in this chat server.
pub const MULTICAST_HOST: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);

/// The multicast port used in this chat server.
pub const MULTICAST_PORT: u16 = 65432;

/// The default packet size used in this chat server.
pub const PACKET_SIZE: usize = 4096;

/// The default name used for users.
pub const DEFAULT_NAME: &str = "Anonymous";

/// The default colour used for users.
pub const DEFAULT_COLOUR: u8 = 198;

/// The colour used for system messages.
pub const SYSTEM_COLOUR: u8 = 21;

/// The colour used for self messages.
pub const SELF_COLOUR: u8 = 243;
