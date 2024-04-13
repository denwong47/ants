//! Conversion of generic types supporting [`serde::Serialize`] and
//! [`serde::Deserialize`] to and from bytes.
//!

use std::io;

use std::any::type_name;

use super::super::{multicast_message, MulticastMessage};
use super::build_error;

impl MulticastMessage {
    /// Creates a [`MulticastMessage`] from by serializing the given value.
    pub fn from_serializable<T: serde::Serialize>(
        uuid: &uuid::Uuid,
        kind: multicast_message::Kind,
        value: &T,
        ttl: Option<u32>,
    ) -> Result<Self, io::Error> {
        let json = serde_json::to_string(value)
            .map_err(|exc| build_error(&format!("Failed to serialize value: {}", exc)))?;

        Ok(MulticastMessage::new(uuid, kind, json, ttl))
    }

    /// Deserializes the body of the message into the given type.
    pub fn to_deserializable<T: serde::de::DeserializeOwned>(&self) -> Result<T, io::Error> {
        self.validate_checksum()?;

        serde_json::from_str(&self.body).map_err(|exc| {
            build_error(&format!(
                "Failed to deserialize value into {}: {}",
                type_name::<T>(),
                exc
            ))
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use crate::_tests::{TestStruct, SECRET};

    #[test]
    fn simple_serde() {
        let uuid = uuid::Uuid::new_v4();
        let message = MulticastMessage::from_serializable(
            &uuid,
            multicast_message::Kind::Origin,
            &SECRET,
            None,
        )
        .unwrap();

        let deserialized: TestStruct = message.to_deserializable().unwrap();

        assert_eq!(deserialized, SECRET);
    }

    #[test]
    fn bad_checksum() {
        let uuid = uuid::Uuid::new_v4();
        let mut message = MulticastMessage::from_serializable(
            &uuid,
            multicast_message::Kind::Origin,
            &SECRET,
            None,
        )
        .unwrap();

        message.checksum = message.checksum.wrapping_add(1);

        assert!(message.to_deserializable::<TestStruct>().is_err());
    }

    #[test]
    fn bad_receiving_type() {
        let uuid = uuid::Uuid::new_v4();
        let message = MulticastMessage::from_serializable(
            &uuid,
            multicast_message::Kind::Origin,
            &SECRET,
            None,
        )
        .unwrap();

        assert!(message.to_deserializable::<u64>().is_err());
    }
}
