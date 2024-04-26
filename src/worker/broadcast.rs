//! Broadcast message type.
//!

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::sync::Arc;

use crate::AntsError;

use super::Worker;

/// Broadcast message type.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum WorkerBroadcastMessage {
    Heartbeat { host: String, port: u16 },
    Leave { host: String, port: u16 },
}

impl<T, R, F, FO, E> Worker<T, R, F, FO, E>
where
    T: DeserializeOwned + Serialize + std::marker::Sync + std::marker::Send + 'static,
    R: DeserializeOwned + Serialize + std::marker::Sync + std::marker::Send + 'static,
    FO: std::future::Future<Output = Result<R, E>>
        + std::marker::Sync
        + std::marker::Send
        + 'static,
    F: Fn(T) -> FO + 'static,
    E: std::fmt::Display + std::fmt::Debug + 'static,
    Self: std::marker::Sync + std::marker::Send,
{
    /// Broadcast a message to all nodes.a
    pub async fn broadcast(&self, message: WorkerBroadcastMessage) -> Result<(), AntsError> {
        self.broadcaster
            .send(message)
            .await
            .map_err(|err| {
                AntsError::MulticastSendError(format!("Failed to broadcast message: {:?}", err))
            })
            .map(|_| ())
    }

    /// Announce the worker to the multicast group.
    pub async fn announce_join(&self) -> Result<(), AntsError> {
        self.broadcast(WorkerBroadcastMessage::Heartbeat {
            host: self.host(),
            port: self.port(),
        })
        .await
    }

    /// Announce the worker leaving the multicast group.
    ///
    /// # Note
    ///
    /// This method is provided for completeness, but the destructor [`Self::drop`]
    /// does not actaully use it due to lifetime constraints.
    pub async fn announce_leave(&self) -> Result<(), AntsError> {
        self.broadcast(WorkerBroadcastMessage::Leave {
            host: self.host(),
            port: self.port(),
        })
        .await
    }

    /// Process a broadcast message.
    pub async fn process_broadcast(&self, message: WorkerBroadcastMessage) {
        match message {
            WorkerBroadcastMessage::Heartbeat { host, port } => {
                // If we indeed have added the node, then the new node also
                // needs to know about us.
                if self.add_node((host, port)).await {
                    self.announce_join().await.unwrap();
                }
            }
            WorkerBroadcastMessage::Leave { host, port } => {
                self.remove_node(&(host, port)).await;
            }
        }
    }

    /// Initialize the broadcast agent.
    pub async fn init_broadcast(self: Arc<Self>) -> Result<Arc<Self>, AntsError> {
        self.broadcaster.start().await;

        tokio::spawn({
            let arc_self = Arc::clone(&self);
            let arc_broadcaster = Arc::downgrade(&self.broadcaster);
            async move {
                // Stop the loop if the broadcaster is dropped.
                while let Some(broadcaster) = arc_broadcaster.upgrade() {
                    let delivery = broadcaster.get_delivery().await;
                    arc_self.process_broadcast(delivery.body).await;
                }
            }
        });

        self.announce_join().await?;

        Ok(self)
    }
}
