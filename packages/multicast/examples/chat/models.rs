//! Models for use in the chat example.
//!

use std::{
    io,
    net::{IpAddr, Ipv4Addr, SocketAddr},
};

use clap::Parser;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::config;

use multicast::{self, build_error};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ChatDelivery {
    Message {
        identity: Uuid,
        message: String,
    },
    Joined {
        identity: Uuid,
        name: String,
        colour: u8,
    },
    Left {
        identity: Uuid,
    },
}

#[derive(Parser, Debug)]
#[command(version, about)]
pub struct ChatOptions {
    /// The multicast address to connect to.
    #[arg(long, default_value_t = config::MULTICAST_HOST)]
    pub host: Ipv4Addr,

    /// The multicast port to connect to.
    #[arg(short, long, default_value_t = config::MULTICAST_PORT)]
    pub port: u16,

    /// The packet size to use for sending and receiving messages.
    ///
    /// This governs the maximum size of a message that can be sent or received.
    #[arg(long, default_value_t = config::PACKET_SIZE)]
    pub packet_size: usize,

    /// The name to use in the chat.
    #[arg(short, long, default_value = config::DEFAULT_NAME)]
    pub name: String,

    /// The colour to use in the chat.
    #[arg(short, long, default_value_t = config::DEFAULT_COLOUR)]
    pub colour: u8,
}

impl ChatOptions {
    /// Returns the multicast address to connect to.
    pub fn multicast_addr(&self) -> io::Result<SocketAddr> {
        let ip = IpAddr::V4(self.host);

        if ip.is_multicast() {
            Ok(SocketAddr::new(ip, self.port))
        } else {
            Err(build_error(&format!(
                "The given IP address ({ip}) is not a multicast address."
            )))
        }
    }
}
