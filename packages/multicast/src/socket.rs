//! Unified interface for the creation of sockets.

use std::net::SocketAddr;
use std::{
    io,
    net::{IpAddr, Ipv4Addr},
};

use socket2::{Domain, Protocol, SockAddr, Socket, Type};

use crate::logger;

/// The async socket type used in this crate.
pub use tokio_socket2::TokioSocket2 as AsyncSocket;

/// A helper function to describe a [`SockAddr`].
///
/// This is distinct from [`describe_socket_addr`] which is the [`std::net`] equivalent.
pub fn describe_sock_addr(sock_addr: &SockAddr) -> String {
    sock_addr
        .as_socket()
        .map(|sock_addr| describe_socket_addr(&sock_addr))
        .unwrap_or_else(|| "(Unknown source)".to_owned())
}

/// A helper function to describe a [`SocketAddr`].
///
/// This is distinct from [`describe_sock_addr`] which is the [`socket2`] equivalent.
pub fn describe_socket_addr(socket_addr: &SocketAddr) -> String {
    format!(
        "{ip}:{port}",
        ip = socket_addr.ip(),
        port = socket_addr.port()
    )
}

/// Create a generic UDP socket that can be used for multicast communication.
///
/// The resultant socket can be used for both sending and receiving multicast messages.
///
/// By default, the socket will be:
/// - non-blocking,
/// - allow the reuse of the address, and
/// - bound to the given address.
pub fn create_udp(addr: &SocketAddr) -> io::Result<AsyncSocket> {
    let domain = Domain::for_address(*addr);

    let socket = Socket::new(domain, Type::DGRAM, Some(Protocol::UDP))?;

    socket.set_nonblocking(true)?;
    socket.set_reuse_address(true)?;
    socket.bind(&SockAddr::from(*addr))?;

    AsyncSocket::new(socket)
}

/// A short hand function to create a UDP socket bound to all IPv4 interfaces.
///
/// This is useful for creating a sender socket.
pub fn create_udp_all_v4_interfaces(port: u16) -> io::Result<AsyncSocket> {
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    create_udp(&addr)
}

/// A helper function to set a socket to join a multicast address.
///
/// The resultant socket will listen for multicast messages on all interfaces.
pub fn join_multicast(asocket: &AsyncSocket, addr: &SocketAddr) -> io::Result<()> {
    let socket = asocket.get_ref();

    let ip_addr = addr.ip();

    if !ip_addr.is_multicast() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Address {ip_addr} is not a multicast address"),
        ));
    }

    match ip_addr {
        IpAddr::V4(ref mdns_v4) => {
            socket.join_multicast_v4(mdns_v4, &Ipv4Addr::new(0, 0, 0, 0))?;
        }
        IpAddr::V6(ref mdns_v6) => {
            // This does not work on macOS which REQUIRES an interface to be specified.
            socket.join_multicast_v6(mdns_v6, 0)?;
            socket.set_only_v6(true)?;
        }
    };

    Ok(())
}

/// A helper function to send a multicast message.
pub async fn send_multicast(
    asocket: &AsyncSocket,
    addr: &SocketAddr,
    data: &[u8],
) -> io::Result<usize> {
    logger::debug!(
        "Sending {} bytes to {:?}.",
        data.len(),
        describe_sock_addr(&SockAddr::from(*addr))
    );
    asocket
        .write(|socket| socket.send_to(data, &SockAddr::from(*addr)))
        .await
}

/// A helper function to receive a multicast message.
pub async fn receive_multicast(
    asocket: &AsyncSocket,
    buffer_size: usize,
) -> io::Result<(Vec<u8>, SockAddr)> {
    let mut inner_buffer = vec![core::mem::MaybeUninit::uninit(); buffer_size];
    logger::debug!("Waiting for message...");
    let result = asocket
        .read(|socket| socket.recv_from(&mut inner_buffer))
        .await;
    logger::debug!("Received message.");

    result.map(|(size, addr)| {
        logger::debug!(
            "Received {} bytes from {:?}.",
            size,
            describe_sock_addr(&addr)
        );

        // Only take the initialized part of the buffer.
        (
            (0..size)
                .map(|i| unsafe { inner_buffer[i].assume_init() })
                .collect::<Vec<_>>(),
            addr,
        )
    })
}

/// Do not run these tests in CI.
#[cfg(all(test, not(feature = "ci_tests")))]
mod test {
    use super::*;
    use crate::_tests;
    use serial_test::serial;

    const REPEAT: usize = 3;

    #[tokio::test]
    #[serial]
    async fn hello_world() {
        let addr = _tests::MULTICAST_ADDRESS;

        let sender = create_udp_all_v4_interfaces(0).expect("Failed to create sender socket!");
        let receiver = create_udp(&addr).expect("Failed to create receiver socket!");
        join_multicast(&receiver, &addr).expect("Failed to join multicast group!");

        let listener = tokio::spawn(async move {
            let mut received = Vec::with_capacity(REPEAT);

            for _ in 0..REPEAT {
                let (data, _addr) = receive_multicast(&receiver, 1024)
                    .await
                    .expect("Failed to receive multicast message!");
                logger::debug!(
                    "Received message from {:?}: {:?}",
                    describe_sock_addr(&_addr),
                    data
                );

                received.push(data);
            }

            received
        });

        tokio::time::sleep(tokio::time::Duration::from_nanos(50)).await;

        let data = b"Hello, world!";

        for _ in 0..REPEAT {
            let _sent = send_multicast(&sender, &addr, data)
                .await
                .expect("Failed to send multicast message!");
            logger::debug!("Sent {} bytes.", _sent);
        }

        // Wait for the listener to finish, with a timeout.
        // Convert all errors to io::Error.
        for received_data in tokio::time::timeout(tokio::time::Duration::from_secs(1), listener)
            .await
            .map_err(|timeout| io::Error::new(io::ErrorKind::TimedOut, timeout))
            .and_then(|result| {
                result.map_err(|join_error| {
                    io::Error::new(io::ErrorKind::Other, join_error.to_string())
                })
            })
            .expect("Failed to receive messages!")
        {
            assert_eq!(data, received_data.as_slice());
            logger::debug!("Received message passed assertion.")
        }
    }
}
