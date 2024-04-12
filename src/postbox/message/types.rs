//! Common types for [`Message`]s.

use crate::token::MessageToken;

use crate::worker::token::ReservationToken;

/// The unique identifier for a message. This is used for the [`fxhash::HashSet`] to
/// deduplicate messages.
pub type MessageId = (ReservationToken, MessageToken);

/// The body of a message, containing a timestamped payload.
#[derive(Debug)]
pub struct MessageBody<T: std::fmt::Debug> {
    pub payload: T,
    pub timestamp: tokio::time::Instant,
}

impl<T: std::fmt::Debug> MessageBody<T> {
    /// Create a new message body with the given payload.
    pub fn new(payload: T) -> Self {
        Self {
            payload,
            timestamp: tokio::time::Instant::now(),
        }
    }
}
