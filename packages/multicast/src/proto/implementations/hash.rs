//! Implementation of hashing for the protobuf structs.
//!
use super::super::MulticastMessage;
use super::build_error;
use std::{
    hash::{Hash, Hasher},
    io,
};

/// Hash the components of a `MulticastMessage` using the given hasher.
pub fn hash_multicast_message_components(
    uuid: &str,
    kind: &i32,
    timestamp: Option<&prost_types::Timestamp>,
    body: &str,
    ttl: &u32,
    hasher: &mut impl std::hash::Hasher,
) {
    uuid.hash(hasher);
    kind.hash(hasher);
    timestamp.hash(hasher);
    body.hash(hasher);
    ttl.hash(hasher);
}

/// Generate a checksum for a `MulticastMessage`.
pub fn multicast_message_components_checksum(
    uuid: &str,
    kind: &i32,
    timestamp: Option<&prost_types::Timestamp>,
    body: &str,
    ttl: &u32,
) -> u64 {
    let mut hasher = std::hash::DefaultHasher::new();
    hash_multicast_message_components(uuid, kind, timestamp, body, ttl, &mut hasher);
    hasher.finish()
}

impl Hash for MulticastMessage {
    /// Hash the `MulticastMessage` using the given hasher.
    fn hash<H: Hasher>(&self, state: &mut H) {
        hash_multicast_message_components(
            &self.uuid,
            &self.kind,
            self.timestamp.as_ref(),
            &self.body,
            &self.ttl,
            state,
        );
    }
}

/// Generate a checksum for a `MulticastMessage`.
impl MulticastMessage {
    /// Generate a checksum for the message.
    pub fn generate_checksum(&self) -> u64 {
        multicast_message_components_checksum(
            &self.uuid,
            &self.kind,
            self.timestamp.as_ref(),
            &self.body,
            &self.ttl,
        )
    }

    /// Regenerate the checksum for the message.
    pub fn regenerate_checksum(mut self) -> Self {
        self.checksum = self.generate_checksum();

        self
    }

    /// Validate the checksum of the message.
    pub fn validate_checksum(&self) -> io::Result<()> {
        if self.checksum == self.generate_checksum() {
            Ok(())
        } else {
            Err(build_error("Checksum validation for message failed."))
        }
    }
}

#[cfg(test)]
mod test {
    use super::super::super::multicast_message;
    use super::*;

    /// Test the hashing of a `MulticastMessage`.
    ///
    /// This test ensures that the hash of a `MulticastMessage` is the same as the checksum.
    #[test]
    fn multicast_message_hash() {
        let message1 = MulticastMessage::new(
            &uuid::Uuid::new_v4(),
            multicast_message::Kind::Origin,
            "Hello, world!",
            None,
        );
        let message2 = MulticastMessage::new(
            &uuid::Uuid::new_v4(), // Different UUID
            multicast_message::Kind::Origin,
            "Hello, world!",
            None,
        );

        let mut hasher = std::hash::DefaultHasher::new();
        message1.hash(&mut hasher);
        let hash1 = hasher.finish();

        let mut hasher = std::hash::DefaultHasher::new();
        message2.hash(&mut hasher);
        let hash2 = hasher.finish();

        assert_eq!(hash1, message1.checksum);
        assert_eq!(hash2, message2.checksum);

        assert_ne!(hash1, hash2);
    }
}
