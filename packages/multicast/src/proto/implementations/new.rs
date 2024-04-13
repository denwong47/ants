//! Constructors for `MulticastMessage` and related types.

use super::super::{multicast_message, MulticastMessage};
use super::hash::multicast_message_components_checksum;
use uuid::Uuid;

impl MulticastMessage {
    /// Create a new `MulticastMessage` with the given string body.
    ///
    /// The timestamp will be set to the current time.
    pub fn new(
        uuid: &Uuid,
        kind: multicast_message::Kind,
        body: impl ToString,
        ttl: Option<u32>,
    ) -> Self {
        let timestamp = prost_types::Timestamp::from(std::time::SystemTime::now());
        let ttl = ttl.unwrap_or(0);
        let checksum = multicast_message_components_checksum(
            &uuid.to_string(),
            &(kind as i32),
            Some(&timestamp),
            &body.to_string(),
            &ttl,
        );

        MulticastMessage {
            uuid: uuid.to_string(),
            kind: kind.into(),
            timestamp: Some(timestamp),
            body: body.to_string(),
            ttl: ttl,
            checksum: checksum,
        }
    }
}
