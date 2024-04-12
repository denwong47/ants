//! An enum wrapper to provide a uniform type for all messages.

use super::Message;

use std::ops::Deref;
use std::sync::Arc;
use tokio::sync::Notify;

use super::{traits, MessageKey};
use crate::{worker::proto::*, AntsError};

use super::super::PostBox;

/// A wrapper for all message types.
#[derive(Debug, PartialEq)]
pub enum MessageType {
    /// A ping message, which contains no reply.
    Ping(Message<PingReply>),

    /// A Reserve message, which contains the reservation result.
    Reserve(Message<ReserveReply>),

    /// A Release message, which contains the release result.
    Release(Message<ReleaseReply>),

    /// A work message, which contains the work acknowledgement.
    Work(Message<WorkReply>),

    /// A delivery message, which contains the work result.
    ///
    /// This is a special message that is triggered by the remote worker, hence
    /// it is stored as the request, not the reply.
    Deliver(Message<DeliverRequest>),
}

impl std::cmp::Eq for MessageType {}

impl std::hash::Hash for MessageType {
    /// Hash the message, including its type and the message body.
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        macro_rules! expand_variants {
            ($($variant:ident),*) => {
                match self {
                    $(Self::$variant(msg) => {
                        stringify!($variant).hash(state);
                        msg.hash(state)
                    },)*
                }
            }
        }

        expand_variants!(Ping, Reserve, Release, Work, Deliver);
    }
}

macro_rules! expand_variants {
    ($($variant:ident),*) => {
        impl MessageType {
            /// Get the name of the kind of message.
            pub const fn kind(&self) -> &'static str {
                match self {
                    $(Self::$variant(_) => stringify!($variant),)*
                }
            }

            /// Get the key of the message.
            pub fn key(&self) -> MessageKey {
                match self {
                    $(Self::$variant(msg) => (self.kind(), msg.identifier()),)*
                }
            }

            /// Get the internal [`Notify`] instance for the message.
            pub fn notify(&self) -> Arc<Notify> {
                match self {
                    $(Self::$variant(msg) => msg.notify(),)*
                }
            }

            /// Wait for the message to be populated, with a defined timeout.
            ///
            /// Contrary to the [`Message::wait_for`] method that this wraps, this
            /// method returns [`()`] instead of the message body.
            pub async fn wait_for(&self, timeout: tokio::time::Duration) -> Result<(), AntsError> {
                match self {
                    $(Self::$variant(msg) => msg.wait_for(timeout).await.map(|_| ()),)*
                }
            }

            /// Get the timestamp of the message, if it has been set.
            pub fn timestamp(&self) -> Option<tokio::time::Instant> {
                match self {
                    $(Self::$variant(msg) => msg.timestamp(),)*
                }
            }

            /// Get the age of the message, if it has been set.
            pub fn age(&self) -> Option<tokio::time::Duration> {
                match self {
                    $(Self::$variant(msg) => msg.age(),)*
                }
            }
        }

    }
}

expand_variants!(Ping, Reserve, Release, Work, Deliver);

macro_rules! expand_from_types {
    (
        $($variant:ident($mapped_type:ident)),*$(,)?
    ) => {
        $(
            impl From<Message<$mapped_type>> for MessageType {
                fn from(msg: Message<$mapped_type>) -> Self {
                    Self::$variant(msg)
                }
            }
        )*

        $(
            impl traits::HandleMessageBody<$mapped_type> for MessageType {
                /// Get the body of the message.
                fn get_body(&self) -> Result<&$mapped_type, AntsError> {
                    match self {
                        Self::$variant(msg) => msg.get_body(),
                        _ => Err(AntsError::IncompatibleRecipientType(
                            self.kind().to_owned(),
                            stringify!($mapped_type).to_owned(),
                        )),
                    }
                }

                /// Set the body of the message.
                async fn set_body(&self, body: $mapped_type) -> Result<(), AntsError> {
                    match self {
                        Self::$variant(msg) => msg.set_body(body).await,
                        _ => Err(AntsError::IncompatibleMessage(
                            stringify!($mapped_type).to_owned(),
                            std::any::type_name_of_val(&body).to_owned(),
                        )),
                    }
                }
            }

            /// Implement the same for [`PostBox`].
            impl traits::SetMessageBodyByKey<$mapped_type> for PostBox {
                /// Set the body of the message.
                async fn set_body(&self, key: &MessageKey, body: $mapped_type) -> Result<(), AntsError> {
                    let message = self.get(key).await?;

                    // Call the inner implementation to do the actual setting.
                    traits::HandleMessageBody::<$mapped_type>::set_body(message.deref(), body).await
                }
            }

            impl TryFrom<MessageType> for Message<$mapped_type> {
                type Error = AntsError;

                /// Attempt to convert the message type into the inner message.
                fn try_from(msg: MessageType) -> Result<Self, Self::Error> {
                    match msg {
                        MessageType::$variant(msg) => Ok(msg),
                        _ => Err(AntsError::IncompatibleMessage(
                            std::any::type_name_of_val(&msg).to_owned(),
                            stringify!($variant).to_owned(),
                        )),
                    }
                }
            }
        )*
    }
}

expand_from_types! {
    Ping(PingReply),
    Reserve(ReserveReply),
    Release(ReleaseReply),
    Work(WorkReply),
    Deliver(DeliverRequest),
}
