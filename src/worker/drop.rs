//! Destructor for the worker.
//!
//! This defines all the teardown logic for the worker.

use std::sync::{Arc, Barrier as BlockingBarrier};

use super::{Worker, WorkerBroadcastMessage};
use serde::{de::DeserializeOwned, Serialize};

use crate::AntsError;

impl<T, R, F, FO, E> Worker<T, R, F, FO, E>
where
    T: DeserializeOwned + Serialize + std::marker::Sync + std::marker::Send + Clone,
    R: DeserializeOwned + Serialize + std::marker::Sync + std::marker::Send,
    FO: std::future::Future<Output = Result<R, E>> + std::marker::Sync + std::marker::Send,
    F: Fn(T) -> FO,
    E: std::fmt::Display + std::fmt::Debug,
    Self: std::marker::Sync + std::marker::Send,
{
    /// Teardown the worker.
    ///
    /// If a worker has any [`std::sync::Arc`] strong references in static lifetime upon
    /// termination, then the [`Drop`] trait will not be called. This may have adverse
    /// effects on the other workers in the cluster as they will not be notified of the
    /// worker's termination.
    ///
    /// Manually calling this function will ensure that the worker is properly removed
    /// from the cluster, and all listeners are gracefully terminated.
    pub fn teardown(&self) {
        logger::debug!("Tearing down worker {}.", self.name());
        let arc_broadcaster = self.cyclical().broadcaster.clone();
        let _name = self.name();
        let host = self.host();
        let port = self.port();

        self._termination_flag.notify_waiters();

        let child_signal = Arc::new(BlockingBarrier::new(2));
        let parent_signal = Arc::clone(&child_signal);

        // Spawn a task to do all the teardown work.
        tokio::spawn(async move {
            logger::debug!(
                "Announcing {name} is leaving the multicast group.",
                name = _name
            );
            arc_broadcaster
                .send(WorkerBroadcastMessage::Leave { host, port })
                .await
                .map_err(|err| {
                    AntsError::MulticastSendError(format!(
                        "Failed to broadcast leave message: {:?}",
                        err
                    ))
                })
                .map(|_|
                    // Notify the parent that the child has finished.
                    child_signal.wait())
        });

        // Wait for the child to finish.
        parent_signal.wait();
    }
}
impl<T, R, F, FO, E> Drop for Worker<T, R, F, FO, E>
where
    T: DeserializeOwned + Serialize + std::marker::Sync + std::marker::Send + Clone,
    R: DeserializeOwned + Serialize + std::marker::Sync + std::marker::Send,
    FO: std::future::Future<Output = Result<R, E>> + std::marker::Sync + std::marker::Send,
    F: Fn(T) -> FO,
    E: std::fmt::Display + std::fmt::Debug,
    Self: std::marker::Sync + std::marker::Send,
{
    fn drop(&mut self) {
        self.teardown();
    }
}
