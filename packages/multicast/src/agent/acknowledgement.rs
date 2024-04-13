//! A separate module for the multicast agent's acknowledgement system.
//!

use std::{io, net::SocketAddr, sync::Mutex};

use uuid::Uuid;

use super::MulticastAgent;
use crate::proto::{multicast_message, MulticastMessage};

use crate::logger;

macro_rules! ignore_poison {
    ($uuid:expr) => {
        |poison| {
            logger::warn!(
                "Poisoned lock ignored for {uuid}, \
                but we may have lost some acknowledgements.",
                uuid = $uuid
            );
            poison.into_inner()
        }
    };
}

impl<T> MulticastAgent<T>
where
    T: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + 'static,
{
    /// Ensures that an acknowledgement is stored for the given UUID.
    ///
    /// This method will insert the UUID into the agent's internal map of acknowledgements.
    /// If the UUID is already in the map, and a sender is provided, then the sender will
    /// be added to the list of acknowledgements for that UUID.
    ///
    /// If the UUID is already in the map, and no sender is provided, then the method will
    /// do nothing.
    pub async fn ensures_acknowledgement_for(&self, uuid: &Uuid, sender: Option<SocketAddr>) {
        // If the UUID is already in the map, and we don't have a sender to insert,
        // then we can skip the write lock. This prevents write locking the mutex
        // unnecessarily.
        if sender.is_none() && self.acknowledgements.read().await.contains_key(uuid) {
            return;
        }

        // The above check is not atomic, so we still need to be careful about
        // the entry now suddenly existing in between the check and the write.
        self.acknowledgements
            .write()
            .await
            .entry(*uuid)
            .and_modify(|acknowledgements| {
                if let Some(socket_addr) = sender {
                    let mut inner = acknowledgements.lock().unwrap_or_else(
                        // If the lock is poisoned, we clear the poison and print a warning.
                        |poison| {
                            logger::warn!(
                                "Poisoned lock cleared for {uuid}, \
                                but we have may have lost some acknowledgements."
                            );
                            acknowledgements.clear_poison();
                            poison.into_inner()
                        },
                    );

                    inner.push((socket_addr, tokio::time::Instant::now()));
                }
            })
            .or_insert(
                // If the entry does not exist, we create a new one.
                // Since we are in a write lock, we can be sure that the entry
                // will not be simultaneously created by another thread.
                if let Some(socket_addr) = sender {
                    Mutex::new(vec![(socket_addr, tokio::time::Instant::now())])
                } else {
                    // If we don't have a sender, we can just create an empty list.
                    Default::default()
                },
            );
    }

    /// Acknowledge a message by sending an acknowledgement message back to the sender.
    ///
    /// This method will create a new [`MulticastMessage`] with the same UUID as the
    /// original message, but with a different kind. The new message will be sent back
    /// to the sender of the original message.
    ///
    /// The original message will be stored in the agent's internal cache, and the
    /// acknowledgement message will be sent to the sender. The sender will then be
    /// able to verify the receipt of the original message by checking the UUID of the
    /// acknowledgement message.
    pub async fn acknowledge(&self, uuid: &uuid::Uuid, sender: &std::net::SocketAddr) {
        logger::info!(
            "Acknowledging message with UUID {uuid} from {sender}.",
            uuid = uuid,
            sender = sender
        );
        self.ensures_acknowledgement_for(uuid, Some(*sender)).await;
    }

    /// Adds an acknowledgement requirement for a message with the given UUID.
    ///
    /// A [`MulticastAgent`] would discard any acknowledgement messages that it does
    /// not expect. This method allows the agent to start expecting an acknowledgement
    /// message with the given UUID.
    pub async fn expects_acknowledgement_for(&self, uuid: &Uuid) {
        self.ensures_acknowledgement_for(uuid, None).await;
    }

    /// Checks if the agent is expecting an acknowledgement for the given UUID.
    pub async fn is_expecting_acknowledgement_for(&self, uuid: &Uuid) -> bool {
        self.acknowledgements.read().await.contains_key(uuid)
    }

    /// Removes the acknowledgement requirement for the given UUID.
    pub async fn remove_acknowledgements_for(
        &self,
        uuid: &Uuid,
    ) -> Option<Vec<(SocketAddr, tokio::time::Instant)>> {
        self.acknowledgements
            .write()
            .await
            .remove(uuid)
            .map(|mutex| mutex.into_inner().unwrap_or_else(ignore_poison!(uuid)))
    }

    /// Get the list of acknowledgements, then perform the provided predicate on the
    /// [`Vec`] of acknowledgements.
    pub async fn get_acknowledgements_and_then<R>(
        &self,
        uuid: &Uuid,
        predicate: impl FnOnce(&Vec<(SocketAddr, tokio::time::Instant)>) -> R,
    ) -> Option<R> {
        self.acknowledgements
            .read()
            .await
            .get(uuid)
            .map(|mutex| predicate(&mutex.lock().unwrap_or_else(ignore_poison!(uuid))))
    }

    /// Get the list of acknowledgements for the given UUID.
    ///
    /// This method will clone the list of acknowledgements for the given UUID, and return
    /// a new [`Vec`] containing the cloned acknowledgements.
    ///
    /// If possible, use [`Self::get_acknowledgements_and_then`] instead, as it allows
    /// for in-place processing of the acknowledgements.
    pub async fn get_acknowledgements_for(
        &self,
        uuid: &Uuid,
    ) -> Option<Vec<(SocketAddr, tokio::time::Instant)>> {
        self.acknowledgements
            .read()
            .await
            .get(uuid)
            .map(|mutex| mutex.lock().unwrap_or_else(ignore_poison!(uuid)).clone())
    }

    /// Count the number of acknowledgements for the given UUID.
    pub async fn count_acknowledgements_for(&self, uuid: &Uuid) -> Option<usize> {
        self.get_acknowledgements_and_then(uuid, |acknowledgements| acknowledgements.len())
            .await
    }

    /// Check if the given sender has acknowledged the message with the given UUID.
    pub async fn has_acknowledged(&self, uuid: &Uuid, sender: &SocketAddr) -> Option<bool> {
        self.get_acknowledgements_and_then(uuid, |acknowledgements| {
            acknowledgements
                .iter()
                .any(|(socket_addr, _)| socket_addr == sender)
        })
        .await
    }

    /// Check if all the expected acknowledgements have been received.
    ///
    /// This method will return a list of the expected acknowledgements that have not
    /// been received. An empty [`Vec`] will be returned if all acknowledgements have
    /// been received; otherwise, [`None`] will be returned if the UUID is not in the
    /// agent's internal map of acknowledgements.
    pub async fn all_acknowledged<'s>(
        &self,
        uuid: &Uuid,
        expected: impl Iterator<Item = &'s SocketAddr>,
    ) -> Option<Vec<&'s SocketAddr>> {
        self.get_acknowledgements_and_then(uuid, |acknowledgements| {
            expected
                .filter(|candidate| {
                    !acknowledgements
                        .iter()
                        .any(|(socket_addr, _)| socket_addr == *candidate)
                })
                .collect()
        })
        .await
    }

    /// Broadcast an acknowledgement message to any sender that is expecting it.
    pub async fn send_acknowledgement(&self, uuid: &Uuid) -> io::Result<usize> {
        let message = MulticastMessage::new(
            uuid,
            multicast_message::Kind::Acknowledge,
            "".to_string(),
            Some(0),
        );
        logger::debug!("Sending acknowledgement for {uuid}...", uuid = uuid);
        self.send_message(uuid, message, false).await
    }

    /// Get a [`Notify`] whenever an acknowledgement is received by this agent.
    ///
    /// This [`Notify`] does not store permits. If the [`Notify`] is notified while
    /// no one is waiting, the next call to [`Notify::notified`] will block until
    /// the next notification.
    pub async fn wait_for_acknowledgement(&self) {
        self._acknowledgement_flag.notified().await;
    }
}
