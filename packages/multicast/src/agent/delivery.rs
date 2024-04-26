//! A separate module for the multicast agent's acknowledgement system.
//!

use super::MulticastAgent;
use crate::delivery::MulticastDelivery;

#[cfg(doc)]
use tokio::sync::Notify;

impl<T> MulticastAgent<T>
where
    T: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + 'static,
{
    /// Wait for a delivery to be made.
    ///
    /// This is to wait for a [`MulticastDelivery`] to be inserted into the agent's
    /// [`Self::output`] queue. Contrary to awaiting that queue directly, this method
    /// does not return the [`MulticastDelivery`] itself, and leaves the queue untouched.
    /// If you are interested in the actual delivery instead of
    /// the its occurrence, you should use [`Self::get_delivery`].
    pub async fn wait_for_delivery(&self) {
        self._delivery_flag.notified().await;
    }

    /// Get a delivery from the agent's output queue.
    ///
    /// This method will return a [`MulticastDelivery`] from the agent's output queue.
    /// If the queue is empty, the method will wait until a delivery is made.
    pub async fn get_delivery(&self) -> MulticastDelivery<T> {
        self.output.pop().await
    }
}
