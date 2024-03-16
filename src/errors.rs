//! Error types.
//!

use thiserror::Error;

#[derive(Error, Debug)]
pub enum AntsError {
    #[error("Invalid node address: {0}")]
    InvalidNodeAddress(String),
    #[error("Termination signal received from {0}.")]
    Termination(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serde error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("Tonic error: {0}")]
    Tonic(#[from] tonic::Status),
    #[error("Tonic server start up error: {0}")]
    TonicServerStartUpError(String),
    #[error("Tokio error: {0}")]
    Tokio(#[from] tokio::task::JoinError),
    #[error("Worker error: {0}")]
    WorkerError(String),
    #[error("No nodes available to execute the task.")]
    NoNodesAvailable,
    #[error("Inner function was called without a reservation.")]
    CalledWithoutReservation,
    #[error("Inner function is still executing and cannot be called again.")]
    CalledWhileBusy,
    #[error("Reservation token does not match, cannot release reservation.")]
    ReservationTokenMismatch,
    #[error("Task exeuction error: {0}")]
    TaskExecutionError(String),
    #[error("Work data is not valid: {0}")]
    InvalidWorkData(String),
    #[error("Work result is not valid: {0}")]
    InvalidWorkResult(String),
    #[error("Remote worker is not available at http://{0}:{1}: {2}")]
    ConnectionError(String, u16, String),
}
