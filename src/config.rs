//! CLI configuration to parse CLI arguments.
//!
use crate::{nodes::NodeAddress, AntsError};
use clap::Parser;

/// Default Multicast address.
pub const DEFAULT_MULTICAST_HOST: &str = "224.0.0.121";

/// Default Multicast port.
pub const DEFAULT_MULTICAST_PORT: u16 = 50061;

#[derive(Parser, Debug, Clone)]
pub struct CliArgs {
    #[clap(long, default_value = "0.0.0.0")]
    pub host: String,
    #[clap(short, long, default_value_t = 5355)]
    pub port: u16,
    #[clap(long, default_value_t = 50051)]
    pub grpc_port: u16,
    pub nodes: Vec<String>,
    #[clap(long, default_value = DEFAULT_MULTICAST_HOST)]
    pub multicast_host: String,
    #[clap(long, default_value_t = DEFAULT_MULTICAST_PORT)]
    pub multicast_port: u16,
}

impl CliArgs {
    /// Return the nodes as a vector of [`NodeAddress`] instances.
    pub fn node_addresses(&self) -> Result<Vec<NodeAddress>, AntsError> {
        Result::from_iter(self.nodes.iter().map(
            // Convert a "0.0.0.0:50051" string to a NodeAddress instance.
            |node| {
                let parts: Vec<&str> = node.split(':').collect();
                Ok((
                    parts[0].to_string(),
                    parts[1]
                        .parse()
                        .map_err(|_| AntsError::InvalidNodeAddress(node.to_string()))?,
                ))
            },
        ))
    }
}
