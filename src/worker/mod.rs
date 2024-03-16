//! The main class of the library: a worker that works or distributes work.

mod model;
pub use model::*;

pub mod token;

pub mod proto {
    //! The gRPC protocol for the worker.

    tonic::include_proto!("ants");
}
