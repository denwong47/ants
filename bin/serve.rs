//! Host a local server to listen and do work.
//!
//! This is a simple example to demonstrate how to use the [`ants`] library.

use axum::{
    extract::Json,
    routing::{get, post},
    Router,
};
use clap::Parser;
use std::sync::OnceLock;

use ants::{
    config,
    example::{TaskBody, TaskResponse},
    AntsError, CliArgs, Worker,
};

static CLI_ARGS: OnceLock<CliArgs> = OnceLock::new();

/// Not actually doing any work, just to simulate a long running task.
async fn do_work(body: String) -> Result<String, AntsError> {
    logger::debug!("Simulating work for '{body}'...");
    // We use the BLOCKING executor to simulate a CPU bound task.
    // This thread won't be available for other tasks until the sleep is done.
    std::thread::sleep(std::time::Duration::from_secs(5));
    logger::debug!("Finished work for '{body}'...");
    Ok(format!("Work done: {}", body))
}

/// Trait to let a worker handle routes.
#[tonic::async_trait]
trait WorkerRoutes {
    async fn send_route(&self, body: TaskBody) -> (axum::http::StatusCode, Json<TaskResponse>);
}

/// Implement the trait for the worker.
#[tonic::async_trait]
impl<F, FO> WorkerRoutes for Worker<String, String, F, FO, AntsError>
where
    FO: std::future::Future<Output = Result<String, AntsError>>
        + std::marker::Sync
        + std::marker::Send
        + 'static,
    F: Fn(String) -> FO + 'static,
    Self: std::marker::Sync + std::marker::Send,
{
    async fn send_route(&self, body: TaskBody) -> (axum::http::StatusCode, Json<TaskResponse>) {
        match self.find_worker_and_work(body.body).await {
            Ok((worker, response)) => (
                axum::http::StatusCode::OK,
                Json(TaskResponse {
                    success: true,
                    worker,
                    body: Some(response),
                    error: None,
                }),
            ),
            Err(e) => (
                axum::http::StatusCode::BAD_GATEWAY,
                Json(TaskResponse {
                    success: false,
                    worker: self.name(),
                    body: None,
                    error: Some(format!("{:?}", e)),
                }),
            ),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), AntsError> {
    let args = CliArgs::parse();

    let worker = Worker::new_and_init(
        args.host.clone(),
        args.grpc_port,
        args.node_addresses()?,
        config::DEFAULT_MULTICAST_HOST.to_owned(),
        config::DEFAULT_MULTICAST_PORT,
        do_work,
        tokio::time::Duration::from_secs(6),
    )
    .await?;

    logger::info!(
        "Worker created: {:?}, aware of {} nodes.",
        worker.name(),
        worker.nodes.len().await
    );

    let args = CLI_ARGS.get_or_init(|| args);

    let app_worker = worker.clone();
    let app = Router::new()
        .route("/", get(|| async { "Hello, world!" }))
        .route(
            "/send",
            post(|Json(body): Json<TaskBody>| async move { app_worker.send_route(body).await }),
        );

    let listener = tokio::net::TcpListener::bind((args.host.as_str(), args.port))
        .await
        .unwrap();

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            logger::info!("SIGTERM received, gracefully shutting down.");
            Ok(())
        },
        _ = async move {
            logger::info!("Axum listening on http://{}:{}...", args.host, args.port);
            axum::serve(listener, app).await
        } => {
            logger::info!("Axum server has shut down, terminating.");
            Err(AntsError::Termination("Axum server has shut down.".to_owned()))
        },
    }
    .expect("Failed to run the server.");

    worker.teardown();
    Ok(())
}
