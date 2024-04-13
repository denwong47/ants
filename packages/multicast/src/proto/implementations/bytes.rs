//! Deals with the serialization and deserialization of the multicast messages into bytes.

use prost::Message;

use super::{super::MulticastMessage, build_error};

use std::io;

impl MulticastMessage {
    /// Serialize the message into bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.encode_to_vec()
    }

    /// Deserialize the message from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, io::Error> {
        MulticastMessage::decode(bytes)
            .map_err(|e| build_error(&format!("Failed to decode message: {}", e)))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::proto::multicast_message;

    #[test]
    fn simple_serde() {
        let uuid = uuid::Uuid::new_v4();
        let message = MulticastMessage::new(
            &uuid,
            multicast_message::Kind::Origin,
            "Hello, world!",
            None,
        );

        let bytes = message.to_bytes();
        let new_message = MulticastMessage::from_bytes(&bytes).unwrap();

        assert_eq!(message, new_message);
    }
}
