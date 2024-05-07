//! Shared testing parameters.
//!
//! This module contains shared testing parameters for the tests for the whole
//! [`ants`] library, which includes unit and integration tests.

use crate::{config, AntsError, Worker};
use std::sync::Arc;

/// The base host for the workers.
pub const BASE_HOST: &str = "127.0.0.1";

/// The base port for the workers.
pub const BASE_PORT: u16 = 50051;

/// The time to wait for the workers to start.
pub const WAIT_FOR_WORKER: tokio::time::Duration = tokio::time::Duration::from_secs(1);

/// The time to wait for the workers to complete the work.
pub const WORK_TIMEOUT: tokio::time::Duration = tokio::time::Duration::from_secs(2);

/// The task to be executed by the worker.
/// This function calculates 2 raised to the power of the input.
pub async fn pow_2(input: u32) -> Result<u32, AntsError> {
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    2_u32.checked_pow(input).ok_or_else(|| {
        AntsError::InvalidWorkResult(format!("Result for 2^{input} is out of bounds."))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test multiple nodes.
    #[cfg(test)]
    async fn test_multi_nodes(worker_count: usize) {
        // Let the nodes discover each other by multicast.
        let nodes = vec![];

        let workers = Worker::new_and_init_multiple(
            worker_count,
            BASE_HOST.to_owned(),
            BASE_PORT,
            // Cloning here is fine. You are not supposed to run more than one
            // worker in the same process anyway, so in production usage
            // this clone shouldn't be needed.
            nodes,
            config::DEFAULT_MULTICAST_HOST.to_owned(),
            config::DEFAULT_MULTICAST_PORT,
            pow_2,
            WORK_TIMEOUT,
        )
        .await
        .expect("Failed to create workers.");

        let inner_worker = Arc::clone(workers.first().unwrap());

        let results = async {
            logger::debug!("Waiting for workers to start.");
            tokio::time::sleep(WAIT_FOR_WORKER).await;
            logger::debug!("Workers should be running now.");

            futures::future::join_all(
                (0..worker_count + 1)
                    .map(|_| Arc::clone(&inner_worker))
                    .map(|worker| async move { worker.find_worker_and_work(16).await }),
            )
            .await
        }
        .await;

        let commissioned_workers = results
            .into_iter()
            .map(|result| {
                let (worker, work_result) = result.expect("Worker should have been succeeded.");
                assert_eq!(work_result, 65536);
                worker
            })
            .collect::<std::collections::HashSet<_>>();

        logger::info!(
            "Commissioned workers: {:?}",
            commissioned_workers.iter().collect::<Vec<_>>()
        );

        // Make sure all workers were used.
        assert_eq!(commissioned_workers.len(), worker_count);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    #[serial_test::serial]
    async fn test_4_nodes() {
        test_multi_nodes(4).await;
    }
}