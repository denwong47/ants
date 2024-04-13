//! Unit test related configurations and functions.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

/// The multicast address used in unit tests in this crate.
pub const MULTICAST_ADDRESS: SocketAddr =
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(224, 0, 0, 251)), 65432);

/// A simple struct to test serialization and deserialization.
#[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
pub struct TestStruct {
    field1: u64,
    field2: i32,
}

/// A secret value used in unit tests.
pub const SECRET: TestStruct = TestStruct {
    field1: u64::MAX / 2,
    field2: 42,
};
