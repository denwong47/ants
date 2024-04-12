//! Common traits for [`Message`]s.
//!

use crate::{worker::proto::*, AntsError};

use super::{Message, MessageId, MessageKey, MessageType};

/// A marker trait for any types that can be used as a message body.
pub trait MessageBodyTypeMarker: std::fmt::Debug {
    /// Create a new message with the given key.
    fn create_message(key: MessageId) -> MessageType;
}

macro_rules! expand_message_types {
    (
        $($variant:ident($mapped_type:ident)),*$(,)?
    ) => {
        $(
            impl MessageBodyTypeMarker for $mapped_type {
                fn create_message(identifier: MessageId) -> MessageType {
                    MessageType::$variant(Message::<$mapped_type>::new(identifier))
                }
            }
        )*
    }
}

expand_message_types!(
    Ping(PingReply),
    Reserve(ReserveReply),
    Release(ReleaseReply),
    Work(WorkReply),
    Deliver(DeliverRequest),
);

/// A trait allowing generic setting of the message body.
#[allow(async_fn_in_trait)]
pub trait HandleMessageBody<T>
where
    T: MessageBodyTypeMarker,
{
    /// Get the body of the message.
    fn get_body(&self) -> Result<&T, AntsError>;

    /// Set the body of the message.
    async fn set_body(&self, body: T) -> Result<(), AntsError>;
}

/// A trait allowing generic setting of the message body.
#[allow(async_fn_in_trait)]
pub trait SetMessageBodyByKey<T>
where
    T: MessageBodyTypeMarker,
{
    /// Set the body of the message.
    async fn set_body(&self, key: &MessageKey, body: T) -> Result<(), AntsError>;
}
