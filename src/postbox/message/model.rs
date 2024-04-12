use std::sync::{Arc, OnceLock};
use tokio::sync::Notify;

use crate::errors::AntsError;

use super::{traits, MessageBody, MessageId};

/// A message that can be awaited for.
#[derive(Debug)]
pub struct Message<T: traits::MessageBodyTypeMarker> {
    identifier: MessageId,
    notify: Arc<Notify>,
    body: OnceLock<MessageBody<T>>,
}

impl<T: traits::MessageBodyTypeMarker> std::hash::Hash for Message<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.identifier.hash(state);
    }
}

impl<T: traits::MessageBodyTypeMarker> std::cmp::PartialEq for Message<T> {
    /// Compare two messages by their identifier, and that their notifier is the
    /// same instance.
    fn eq(&self, other: &Self) -> bool {
        self.identifier == other.identifier && Arc::ptr_eq(&self.notify, &other.notify)
    }
}

impl<T: traits::MessageBodyTypeMarker> Message<T> {
    /// Create a new message with the given identifier.
    pub fn new(identifier: MessageId) -> Self {
        Self {
            identifier,
            notify: Arc::new(Notify::new()),
            body: OnceLock::new(),
        }
    }

    /// Get the identifier of the message.
    pub fn identifier(&self) -> MessageId {
        self.identifier
    }

    /// Get the underlying notifier for this message.
    pub fn notify(&self) -> Arc<Notify> {
        self.notify.clone()
    }

    /// Get the timestamp of the message body, if it has been set.
    pub fn timestamp(&self) -> Option<tokio::time::Instant> {
        self.body.get().map(|body| body.timestamp)
    }

    /// Get the age of the message body, if it has been set.
    pub fn age(&self) -> Option<tokio::time::Duration> {
        self.timestamp().map(|ts| ts.elapsed())
    }

    /// Wait for the message to be resolved with a timeout.
    pub async fn wait_for(&self, timeout: tokio::time::Duration) -> Result<&T, AntsError> {
        tokio::select! {
            _ = self.notify.notified() => {
                self.body.get().map(|b| &b.payload).ok_or(
                    AntsError::MessageNotSet(format!("{:?}", self.identifier))
                )
            }
            _ = tokio::time::sleep(timeout) => {
                Err(AntsError::TaskExecutionError(
                    format!("Timed out waiting for message {0:?}.", self.identifier)
                ))
            }
        }
    }
}

impl<T> traits::HandleMessageBody<T> for Message<T>
where
    T: traits::MessageBodyTypeMarker,
{
    /// Get the body of the message.
    fn get_body(&self) -> Result<&T, AntsError> {
        self.body
            .get()
            .map(|b| &b.payload)
            .ok_or(AntsError::MessageNotSet(format!("{:?}", self.identifier)))
    }

    /// Set the body of the message.
    async fn set_body(&self, body: T) -> Result<(), AntsError> {
        self.body
            .set(MessageBody::new(body))
            .map_err(|_| AntsError::MessageAlreadySet(format!("{:?}", self.identifier)))?;
        self.notify.notify_waiters();

        Ok(())
    }
}
