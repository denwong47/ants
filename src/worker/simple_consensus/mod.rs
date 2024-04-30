//! Simple consensus algorithm to repeat the same work until a majority of nodes agree.
//!
//! This module provides a simple consensus algorithm that repeats the same work until a majority
//! of nodes agree. This is useful for guard against non-deterministic behavior in the network or
//! message corruption.
//!
//! If there is only one node available, then the worker will be asked to do the same work until it
//! agrees with itself, with a minimum of 2 attempts. This is a compromise, as we cannot guarantee
//! the node is correct, but at least we can be assured that the node is consistent.
//!
//! If the maximum retries are reached, then a [`Err`] is returned.

use std::hash::Hash;

use crate::AntsError;

use super::Worker;

use serde::{de::DeserializeOwned, Serialize};

pub mod traits;
use traits::Consensus;

impl<T, R, F, FO, E> Worker<T, R, F, FO, E>
where
    T: DeserializeOwned + Serialize + std::marker::Sync + std::marker::Send + Clone + 'static,
    R: DeserializeOwned + Serialize + std::marker::Sync + std::marker::Send + Eq + Hash + 'static,
    FO: std::future::Future<Output = Result<R, E>>
        + std::marker::Sync
        + std::marker::Send
        + 'static,
    F: Fn(T) -> FO + 'static,
    E: std::fmt::Display + std::fmt::Debug + 'static,
    Self: std::marker::Sync + std::marker::Send,
{
    /// Find workers to do the same work and reach a consensus.
    ///
    /// This function will find workers to do the same work and reach a consensus. The work is
    /// provided as an argument, and the minimum consensus is the number of workers that must agree
    /// on the same result. The maximum retries is the number of times the work will be repeated
    /// if minimum consensus is not reached; if the maximum retries is reached, then a [`Err`]
    /// wrapping an [`AntsError::ConsensusNotReached`] will be returned.
    ///
    /// If the minimum consensus is less than 2, then it will be set to 2.
    /// If the maximum retries is less than the minimum consensus, then it will
    /// be set to the minimum consensus.
    ///
    /// Contrary to the [`Self::find_worker_and_work`] method, this method will not report the
    /// worker that produced the result.
    pub async fn find_workers_and_reach_consensus(
        &self,
        body: T,
        minimum_consensus: usize,
        max_retries: usize,
    ) -> Result<R, AntsError> {
        let minimum_consensus = minimum_consensus.max(2); // We need at least 2 nodes to reach consensus.
        let mut max_retries = max_retries.max(minimum_consensus); // We need to at least have as many retries as the minimum consensus.

        let mut results = Vec::<R>::with_capacity(max_retries);
        let mut workers = Vec::<String>::with_capacity(max_retries);

        logger::info!(
            "Attempting to reach a consensus of {minimum_consensus} with a maximum of {max_retries} retries.",
            minimum_consensus = minimum_consensus,
            max_retries = max_retries
        );

        while let Err(AntsError::ConsensusPending(agreed)) =
            results.iter().consensus(minimum_consensus)
        {
            let attempts = (minimum_consensus - agreed).min(max_retries);

            if attempts == 0 {
                break;
            }

            // This should assert to be positive, as we have already checked
            // that no consensus has been reached.
            let handles = (0..attempts)
                .map(|_| self.find_worker_and_work(body.clone()))
                .collect::<Vec<_>>();

            logger::info!(
                "Waiting for {attempts} workers to finish work; current consensus: {agreed}.",
                attempts = handles.len(),
                agreed = agreed
            );

            // Wait for all the workers to finish.
            futures::future::join_all(handles)
                .await
                .into_iter()
                .for_each(|result| match result {
                    Ok((worker, result)) => {
                        workers.push(worker);
                        results.push(result);
                        max_retries -= 1;
                    }
                    Err(_err) => {
                        logger::error!(
                            "Error while finding worker and work, will continue finding: {}",
                            _err
                        );
                    }
                });
        }

        let results_len = results.len();
        results
            .into_iter()
            .consensus(minimum_consensus)
            .map_err(|err| {
                if let AntsError::ConsensusPending(_) = err {
                    // Transform the error into a consensus not reached error, which
                    // describes in more detail the state of the consensus.
                    AntsError::ConsensusNotReached(minimum_consensus, results_len, workers)
                } else {
                    // If the error is not a consensus pending error, then return the error.
                    err
                }
            })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::config;

    const BASE_HOST: &str = "127.0.0.1";
    const BASE_PORT: u16 = 51051;

    const WORK_TIMEOUT: tokio::time::Duration = tokio::time::Duration::from_secs(3);

    /// A worker that does some work.
    async fn correct_work(value: i32) -> Result<i32, String> {
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        logger::success!("Correct worker returned good value.");
        Ok(value.wrapping_add(1))
    }

    /// A worker that does the same work, but returns the negative value.
    async fn rogue_work(value: i32) -> Result<i32, String> {
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        logger::error!("Rogue worker returned wrong value.");
        Ok(value.wrapping_add(1).wrapping_mul(-1))
    }

    const CORRECT_COUNT: usize = 3;
    const ROGUE_COUNT: usize = 2;

    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    #[serial_test::serial]
    async fn consensus() {
        let _rogue_workers = Worker::new_and_init_multiple(
            ROGUE_COUNT,
            BASE_HOST.to_owned(),
            BASE_PORT,
            vec![],
            config::DEFAULT_MULTICAST_HOST.to_owned(),
            config::DEFAULT_MULTICAST_PORT,
            rogue_work,
            WORK_TIMEOUT,
        )
        .await
        .expect("Failed to create rogue workers.");
        let _correct_workers = Worker::new_and_init_multiple(
            CORRECT_COUNT,
            BASE_HOST.to_owned(),
            BASE_PORT + ROGUE_COUNT as u16,
            vec![],
            config::DEFAULT_MULTICAST_HOST.to_owned(),
            config::DEFAULT_MULTICAST_PORT,
            correct_work,
            WORK_TIMEOUT,
        )
        .await
        .expect("Failed to create correct workers.");

        // Use a rogue worker to find the correct workers.
        let inner_worker = std::sync::Arc::clone(_correct_workers.first().unwrap());

        let _results = tokio::select!(
            _ = futures::future::join_all(
                _rogue_workers.iter().map(
                    |worker| worker.start()
                )
            ) => {
                panic!("Rogue workers' listening futures should not return.")
            },
            _ = futures::future::join_all(
                _correct_workers.iter().map(
                    |worker| worker.start()
                )
            ) => {
                panic!("Correct workers' listening futures should not return.")
            },
            answer = async move {
                // Let all the multicast handshake finish.
                tokio::time::sleep(
                    tokio::time::Duration::from_millis(500)
                ).await;
                logger::info!(
                    "Starting consensus test."
                );
                inner_worker.find_workers_and_reach_consensus(1024, 6, 12).await.expect(
                    "Workers failed to work and reach consensus."
                )
            } => {
                answer
            }
        );

        // The consensus should be 1025, as the correct workers will agree on the correct value.
        assert_eq!(_results, 1025);
    }
}
