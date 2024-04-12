//! The type alias for a [`fxhash::HashSet`] of [`MessageType`]s.
//!
//!
use super::{MessageId, MessageType};
use fxhash::FxHashMap;
use std::sync::Arc;

/// The unique key for a message. This is used for the [`fxhash::FxHashMap`] to deduplicate messages.
pub type MessageKey = (&'static str, MessageId);

/// The type alias for a [`fxhash::FxHashMap`] of [`MessageType`]s.
pub type MessageMap = FxHashMap<MessageKey, Arc<MessageType>>;
