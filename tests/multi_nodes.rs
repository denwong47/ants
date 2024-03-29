use ants::{AntsError, Worker};
use std::sync::Arc;

const BASE_HOST: &str = "127.0.0.1";
const BASE_PORT: u16 = 50051;
const PORT_INCREMENT: u16 = 1;

const WAIT_FOR_WORKER: std::time::Duration = std::time::Duration::from_secs(1);

const WORK_TIMEOUT: tokio::time::Duration = tokio::time::Duration::from_secs(2);

/// The task to be executed by the worker.
/// This function calculates 2 raised to the power of the .
async fn pow_2(input: u32) -> Result<u32, AntsError> {
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    2_u32.checked_pow(input).ok_or_else(|| {
        AntsError::InvalidWorkResult(format!("Result for 2^{input} is out of bounds."))
    })
}

/// Test multiple nodes.
async fn test_multi_nodes(worker_count: usize) {
    let nodes = (0..worker_count)
        .map(|id| {
            (
                BASE_HOST.to_owned(),
                BASE_PORT + (id as u16) * PORT_INCREMENT,
            )
        })
        .collect::<Vec<_>>();

    let workers = (0..worker_count)
        .map(|id| {
            Worker::new_arc(
                BASE_HOST.to_owned(),
                BASE_PORT + (id as u16) * PORT_INCREMENT,
                // Cloning here is fine. You are not supposed to run more than one
                // worker in the same process anyway, so in production usage
                // this clone shouldn't be needed.
                nodes.clone(),
                pow_2,
                WORK_TIMEOUT,
            )
        })
        .collect::<Vec<_>>();

    let inner_worker = Arc::clone(workers.first().unwrap());

    let results = tokio::select! {
        _ = async move { futures::future::join_all(workers.iter().map(
            |worker| worker.start()
        )).await } => {
            panic!("Workers should not return.")
        },
        results = async {
            eprintln!("Waiting for workers to start.");
            tokio::time::sleep(WAIT_FOR_WORKER).await;
            eprintln!("Workers should be running now.");

            futures::future::join_all(
                (0..worker_count+1)
                .map(
                    |_| Arc::clone(&inner_worker)
                )
                .map(
                    |worker| async move {
                        worker.find_worker_and_work(16).await
                    }
                )
            )
            .await
        } => {
            results
        }
    };

    let commissioned_workers = results
        .into_iter()
        .map(|result| {
            let (worker, work_result) = result.expect("Worker should have been succeeded.");
            assert_eq!(work_result, 65536);
            worker
        })
        .collect::<std::collections::HashSet<_>>();

    // Make sure all workers were used.
    assert_eq!(commissioned_workers.len(), worker_count);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_4_nodes() {
    test_multi_nodes(4).await;
}
