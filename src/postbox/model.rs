//! The postbox module that handles the receipt of foreign messages and their subsequent
//! delivery locally.
//!
//!
use crate::AntsError;

use super::message::{
    traits::MessageBodyTypeMarker, MessageId, MessageKey, MessageMap, MessageType,
};
use std::sync::Arc;

use tokio::sync::RwLock;

/// The postbox that receives messages and delivers them to the recipient.
pub struct PostBox {
    messages: RwLock<MessageMap>,
}

/// NOTE Part of implementation is in `wrapper.rs`, allowing setting the message
/// body by key.
impl Default for PostBox {
    fn default() -> Self {
        Self::new()
    }
}

impl PostBox {
    /// Create a new postbox.
    pub fn new() -> Self {
        Self {
            messages: RwLock::new(MessageMap::default()),
        }
    }

    /// Create a new postbox, and return the atomic reference to it.
    pub fn new_arc() -> Arc<Self> {
        Arc::new(Self::new())
    }

    /// Create a new message of the given type.
    pub async fn create_message<T>(&self, identifier: MessageId) -> Arc<MessageType>
    where
        T: MessageBodyTypeMarker,
    {
        let message = T::create_message(identifier);
        logger::trace!("Creating a message for {:?}...", message.key());

        #[cfg(debug_assertions)]
        let key = message.key();

        let for_return = self.insert(message).await;

        #[cfg(debug_assertions)]
        assert!(self.get(&key).await.is_ok());

        for_return
    }

    /// Add a [`MessageType`] to the postbox, and return the atomic reference to it.
    pub async fn insert(&self, message: MessageType) -> Arc<MessageType> {
        let mut messages = self.messages.write().await;
        let message_ref = Arc::new(message);
        if let Some(message_existing) = messages.insert(message_ref.key(), message_ref.clone()) {
            logger::warn!(
                "Message with key {:?} already exists in the postbox.",
                message_existing.key()
            );
            message_existing
        } else {
            message_ref
        }
    }

    /// Get the atomic reference to a [`MessageType`] from the postbox by its key.
    ///
    /// If the message is not found, this will return [`None`].
    pub async fn get(&self, key: &MessageKey) -> Result<Arc<MessageType>, AntsError> {
        let messages = self.messages.read().await;
        messages
            .get(key)
            .cloned()
            .ok_or_else(|| AntsError::MessageNotFound(format!("{:?}", key)))
    }

    /// Remove a [`MessageType`] from the postbox by its key.
    pub async fn remove(&self, key: &MessageKey) -> Option<Arc<MessageType>> {
        let mut messages = self.messages.write().await;
        logger::trace!("Removing message with key {:?}...", key);
        messages.remove(key)
    }

    /// Get the number of messages in the postbox.
    pub async fn len(&self) -> usize {
        let messages = self.messages.read().await;
        messages.len()
    }

    /// Check if the postbox is empty.
    pub async fn is_empty(&self) -> bool {
        self.len().await == 0
    }

    /// Remove all messages that have been delivered older than the given duration.
    pub async fn clean_old_messages(&self, duration: tokio::time::Duration) {
        let mut messages = self.messages.write().await;
        messages.retain(|_, message| {
            message
                .age()
                .map(|age| age < duration)
                .unwrap_or_else(|| true)
        });
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        postbox::{
            traits::{HandleMessageBody, SetMessageBodyByKey},
            Message,
        },
        token::generate_token as generate_message_token,
        worker::{proto::*, token::generate_token as generate_reservation_token},
    };

    #[tokio::test]
    async fn simple() {
        let postbox = PostBox::new_arc();

        let token = generate_reservation_token();
        let task_id = generate_message_token();

        let identifier = (token, task_id);

        let message = postbox.create_message::<DeliverRequest>(identifier).await;
        let key = message.key();

        let body = "Test Body";

        let remote_task = {
            let remote_postbox = postbox.clone();

            let expected = DeliverRequest {
                token,
                task_id,
                success: true,
                error: String::default(),
                body: body.to_owned(),
                worker: "Test Worker".to_owned(),
            };

            let mut bad_result = expected.clone();
            "Bad Body".clone_into(&mut bad_result.body);

            // This is to simulate the remote worker sending a message to the postbox.
            async move {
                logger::debug!("Sleeping for a bit.");
                tokio::time::sleep(tokio::time::Duration::from_secs_f32(0.5)).await;

                logger::debug!("Setting the message body.");
                remote_postbox
                    .set_body(&key, expected)
                    .await
                    .expect("Failed to set the message body.");

                logger::debug!("Setting the message body again, expecting a failure.");
                let result = remote_postbox.set_body(&key, bad_result).await;
                assert!(result.is_err());
            }
        };
        let local_task = async move {
            logger::debug!("Waiting for the message.");
            message
                .wait_for(tokio::time::Duration::from_secs(1))
                .await
                .unwrap();
            logger::debug!("Message received.")
        };

        tokio::join!(remote_task, local_task);

        let message = Arc::try_unwrap(
            postbox
                .remove(&key)
                .await
                .expect("Failed to remove the message."),
        )
        .expect("Failed to unwrap the Arc: the message is still in use elsewhere.");

        let inner: Message<DeliverRequest> = message
            .try_into()
            .expect("Failed to convert the message into the inner type.");

        let delivered = inner.get_body().expect("Failed to get the message body.");

        assert_eq!(&delivered.body, body);
    }
}
