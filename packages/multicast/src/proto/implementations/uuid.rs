//! Methods to retrieve UUIDs from a multicast message.
//!

use super::super::MulticastMessage;
use std::io;
use uuid::Uuid;

use crate::build_error;

impl MulticastMessage {
    /// Retrieve the UUID from the message.
    pub fn uuid(&self) -> io::Result<Uuid> {
        Uuid::parse_str(&self.uuid)
            .map_err(|exc| build_error(&format!("Failed to parse {} as UUID: {}", &self.uuid, exc)))
    }
}
