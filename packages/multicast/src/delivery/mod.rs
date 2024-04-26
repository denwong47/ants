//! Helper functions to generate and parse multicast messages.
//!

use std::{io, net::SocketAddr, time::SystemTime};
use uuid::Uuid;

use crate::{build_error, proto::MulticastMessage};

/// A received [`MulticastMessage`] with its sender and the time it was received.
///
/// This is an intermediate type used to store the message and its metadata before
/// it is processed by the multicast client such as deduplication and deserialization.
pub type MulticastReceipt = (MulticastMessage, SocketAddr, tokio::time::Instant);

/// A delivered [`MulticastMessage`] that is awaiting processing by downstream
/// processes.
///
/// A [`MulticastMessage`] could be duplicated or echoed back to the sender by some
/// other network devices. It can also contain system messages that is meant to be
/// consumed locally by the current crate, rather than the downstream application.
///
/// By the time a [`MulticastDelivery`] is created, the downstream application should
/// be able to assume that the message is unique, correct and deserialized into the
/// intended type.

#[derive(Debug, Clone, PartialEq)]
pub struct MulticastDelivery<T: serde::de::DeserializeOwned> {
    /// The unique identifier of the message.
    pub uuid: Uuid,

    /// The sender from whom the message was received. In the case of a echo message,
    /// this will be the source of the echo, not the original sender. If the original
    /// sender is required, it should be stored in the message body as a "reply-to"
    /// field.
    pub sender: SocketAddr,

    /// The timestamp of the message when it was first created.
    ///
    /// This is the timestamp of the message when it was first created by the original
    /// sender, not the time when it was received by this client. Since this timestamp
    /// relies on the system clock of the sender, which has no guarantee of synchronicity
    /// between different devices, it should be treated as a reference point rather than
    /// an exact time.
    pub timestamp: Option<std::time::SystemTime>,

    /// The [timestamp] when the message was received by this client.
    ///
    /// [timestamp]: tokio::time::Instant
    pub received: tokio::time::Instant,

    /// The deserialized body of the message.
    pub body: T,
}

impl<T: serde::de::DeserializeOwned> TryFrom<MulticastReceipt> for MulticastDelivery<T> {
    type Error = io::Error;

    /// Try to convert a [`MulticastReceipt`] into a [`MulticastDelivery`].
    ///
    /// This method will attempt to deserialize the message body into the intended type,
    /// along with other metadata such as the sender, timestamp and received time.
    fn try_from((message, sender, received): MulticastReceipt) -> Result<Self, Self::Error> {
        let body: T = message.to_deserializable()?;

        Ok(MulticastDelivery {
            uuid: Uuid::parse_str(&message.uuid)
                .map_err(|err| build_error(&format!("Failed to parse UUID: {}", err)))?,
            sender,
            timestamp: message
                .timestamp
                .ok_or_else(|| build_error("Message timestamp is missing."))
                .and_then(|timestamp| {
                    SystemTime::try_from(timestamp).map_err(|err| {
                        build_error(&format!("Failed to convert timestamp: {}", err))
                    })
                })
                .ok(),
            received,
            body,
        })
    }
}
