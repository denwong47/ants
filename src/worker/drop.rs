//! Destructor for the worker.
//!
//! This defines all the teardown logic for the worker.

use super::{Worker, WorkerBroadcastMessage};
use serde::{de::DeserializeOwned, Serialize};

use crate::AntsError;

impl<T, R, F, FO, E> Drop for Worker<T, R, F, FO, E>
where
    T: DeserializeOwned + Serialize + std::marker::Sync + std::marker::Send,
    R: DeserializeOwned + Serialize + std::marker::Sync + std::marker::Send,
    FO: std::future::Future<Output = Result<R, E>> + std::marker::Sync + std::marker::Send,
    F: Fn(T) -> FO,
    E: std::fmt::Display + std::fmt::Debug,
    Self: std::marker::Sync + std::marker::Send,
{
    fn drop(&mut self) {
        let arc_broadcaster = self.cyclical().broadcaster.clone();
        let host = self.host();
        let port = self.port();

        // Spawn a task to do all the teardown work.
        tokio::spawn(async move {
            arc_broadcaster
                .send(WorkerBroadcastMessage::Leave { host, port })
                .await
                .map_err(|err| {
                    AntsError::MulticastSendError(format!(
                        "Failed to broadcast leave message: {:?}",
                        err
                    ))
                })
                .map(|_| ())
        });
    }
}
