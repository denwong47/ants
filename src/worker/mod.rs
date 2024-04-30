//! The main class of the library: a worker that works or distributes work.

mod model;
pub use model::*;

mod simple_consensus;

pub mod token;

mod broadcast;
pub use broadcast::*;

mod drop;

pub mod proto {
    //! The gRPC protocol for the worker.

    tonic::include_proto!("ants");
}
