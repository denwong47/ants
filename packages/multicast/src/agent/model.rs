//! The main multicast struct that handles sending and receiving multicast messages.
//!
//!

use crate::{build_error, delivery::*, logger, proto::*, socket::*};
use deadqueue::unlimited::Queue;
use std::{
    io,
    net::SocketAddr,
    sync::{Arc, Mutex, OnceLock},
};
use tokio::sync::{watch, Notify, OnceCell, RwLock};

use uuid::Uuid;

use fxhash::FxHashMap;

use super::SystemMessage;

/// The default packet size for receiving multicast messages.
///
/// Any message larger than this size will be truncated, and likely result in
/// an error when deserializing the message.
///
/// By default it is set to 2043 bytes, which is the maximum size of a
/// multicast message in IP over InfiniBand (IPoIB) networks.
pub const DEFAULT_PACKET_SIZE: usize = 2043;

/// The default time-to-live for multicast messages.
///
/// This value is used when sending messages to the multicast address. Upon
/// receiving a message that is not a loopback, the agent will mark the message
/// as seen and re-broadcast it with a reduced TTL to all its interfaces. This
/// reduces the likelihood of the message being dropped by unreliable links,
/// while not being duplicated indefinitely.
pub const DEFAULT_TTL: u32 = 1;

/// The default address when locally indicating a message has originated from
/// this very process. It is not a valid address and should not be used for
/// sending or receiving messages.
pub const LOOPBACK_ADDRESS: SocketAddr = SocketAddr::new(
    std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)),
    0,
);

/// A map of acknowledgements for each UUID to their senders and the time they were received.
pub type AcknowledgementRecords = FxHashMap<Uuid, Mutex<Vec<(SocketAddr, tokio::time::Instant)>>>;

/// The main multicast struct that handles sending and receiving multicast messages.
///
/// This struct expects all messages to be in the same type `T`, which is serializable
/// and deserializable using [`serde`]. It is up to the downstream application to design
/// the message format. If more than one type of message is required, it is recommended
/// to use a tagged enum to differentiate between the types while allowing deserialization
/// under the same type.
///
/// To create a new [`MulticastAgent`], use the [`Self::new`] method. It is recommended to
/// wrap the agent in an [`Arc`] to allow for easy sharing between multiple threads.
/// To start listening for messages, call the [`Self::start`] method. This will spawn a new
/// task that listens for incoming messages. To stop listening for messages, call the
/// [`Self::stop`] method. Alternatively, if the [`Arc`] reduces to zero strong references,
/// the agent will also stop listening for messages.
///
/// To fetch messages from the agent, use the [`Self::output`] [`Queue`]. This
/// [`Queue`] will contain all the unique messages that have been received but
/// not yet processed.
pub struct MulticastAgent<T>
where
    T: serde::Serialize + serde::de::DeserializeOwned + Send + Sync,
{
    /// The socket used to receive multicast messages.
    listener: Arc<AsyncSocket>,

    /// The handle that listens for incoming messages.
    listener_handle: OnceLock<tokio::task::JoinHandle<()>>,

    /// The socket used to send multicast messages.
    sender: OnceCell<AsyncSocket>,

    /// The multicast address to send and receive messages from.
    multicast_addr: SocketAddr,

    /// The set of [`Uuid`]s that have been seen.
    seen: RwLock<FxHashMap<Uuid, (SocketAddr, tokio::time::Instant)>>,

    /// If an acknowledgement is returned, it will be stored here.
    pub(crate) acknowledgements: RwLock<AcknowledgementRecords>,

    /// The queue of messages that have been received but not yet processed.
    pub output: Queue<MulticastDelivery<T>>,

    /// The maximum size of a packet that can be received.
    packet_size: usize,

    /// The time-to-live for multicast messages.
    ///
    /// This value is used when sending messages to the multicast address. Upon
    /// receiving a message that is not a loopback, the agent will mark the message
    /// as seen and re-broadcast it with a reduced TTL to all its interfaces. This
    /// reduces the likelihood of the message being dropped by unreliable links,
    /// while not being duplicated indefinitely.
    ttl: u32,

    /// A flag to indicate if the agent should terminate.
    _terminate_flag: Arc<Notify>,

    /// A [`Notify`] whenever an acknowledgement is received.
    pub(crate) _acknowledgement_flag: Arc<Notify>,

    /// A [`Notify`] whever a message is delivered.
    pub(crate) _delivery_flag: Arc<Notify>,

    /// A secret [`Uuid`] used for self-testing.
    ///
    /// This contains a [`watch::Sender`] that will be used to store the [`Uuid`] of the
    /// incoming self-test message, which the caller can then use to monitor and verify
    /// the message.
    pub(crate) _self_test_received: RwLock<OnceCell<watch::Sender<Option<Uuid>>>>,
}

impl<T> MulticastAgent<T>
where
    T: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + 'static,
{
    /// Creates a new [`MulticastAgent`] that listens on the given address.
    pub fn new(multicast_addr: SocketAddr) -> Result<Self, std::io::Error> {
        let listener = Arc::new(create_udp(&multicast_addr)?);

        join_multicast(&listener, &multicast_addr)?;

        Ok(Self {
            listener,
            listener_handle: OnceLock::new(),
            sender: OnceCell::new(),
            multicast_addr,
            seen: RwLock::new(FxHashMap::default()),
            acknowledgements: RwLock::new(FxHashMap::default()),
            output: Queue::new(),
            ttl: DEFAULT_TTL,
            packet_size: DEFAULT_PACKET_SIZE,
            _terminate_flag: Arc::new(Notify::new()),
            _acknowledgement_flag: Arc::new(Notify::new()),
            _delivery_flag: Arc::new(Notify::new()),
            _self_test_received: RwLock::new(OnceCell::new()),
        })
    }

    /// Sets the maximum size of a packet that can be received.
    pub fn with_packet_size(mut self, packet_size: usize) -> Self {
        self.packet_size = packet_size;
        self
    }

    /// Sets the time-to-live for multicast messages.
    pub fn with_ttl(mut self, ttl: u32) -> Self {
        self.ttl = ttl;
        self
    }

    /// Checks if the given [`Uuid`] has been seen before.
    pub async fn has_seen(&self, uuid: &Uuid) -> bool {
        self.seen.read().await.contains_key(uuid)
    }

    /// Marks the given [`Uuid`] as seen.
    ///
    /// If the [`Uuid`] has already been seen, this method will not update
    /// the seen records to the provided address, before returning `false`.
    /// Otherwise, it will return `true`.
    ///
    /// If two calls to this method are made concurrently with the same [`Uuid`],
    /// it is possible that the second call will overwrite the first call; there
    /// is no guarantee which address will be stored in the seen records. In such
    /// a scenario, however, this method will correctly return `false` for the
    /// second call.
    pub async fn mark_seen(&self, uuid: &Uuid, addr: SocketAddr) -> bool {
        if !self.seen.read().await.contains_key(uuid) {
            // There is a problem here with the atomicity of the operation:
            // it is possible that another write operation has occurred between
            // the check and the write, which would cause the second write to
            // overwrite the first.
            // Since we do not really care about the exact value of the address,
            // we can make do with this compromise.
            let replaced = self
                .seen
                .write()
                .await
                .insert(*uuid, (addr, tokio::time::Instant::now()));

            // If the value was replaced, then the UUID was already seen.
            replaced.is_none()
        } else {
            false
        }
    }

    /// Sends a payload to the multicast address.
    ///
    /// A new [`Uuid`] will be generated for the message, and the message will be
    /// treated as an [`multicast_message::Kind::Origin`] message. The [`Self::ttl`]
    /// value will be used as the time-to-live for the message.
    ///
    /// The method will return a tuple containing the [`Uuid`] of the message and
    /// the size of the message in bytes.
    pub async fn send(&self, payload: T) -> io::Result<(Uuid, usize)> {
        let uuid = Uuid::new_v4();
        let message = MulticastMessage::from_serializable(
            &uuid,
            multicast_message::Kind::Origin,
            &payload,
            Some(self.ttl),
        )?;

        self.send_message(&uuid, message, false)
            .await
            .map(|size| (uuid, size))
    }

    /// Sends a [`MulticastMessage`] to the multicast address.
    ///
    /// This is an internal method that will send the given pre-constructed
    /// [`MulticastMessage`] to the multicast address. [`Uuid`] will need to be
    /// specified manually to avoid repeated parsing of [`Uuid`] from the message.
    ///
    /// # Note
    ///
    ///
    pub async fn send_message(
        &self,
        uuid: &Uuid,
        message: MulticastMessage,
        loopback: bool,
    ) -> io::Result<usize> {
        let buffer = message.to_bytes();
        let sender = self
            .sender
            .get_or_init(|| async {
                create_udp_all_v4_interfaces(0).expect("Failed to create sender socket.")
            })
            .await;

        if !loopback {
            tokio::join!(
                // Mark the message as seen before sending it.
                self.mark_seen(uuid, LOOPBACK_ADDRESS),
                self.expects_acknowledgement_for(uuid),
            );
        }
        // If we are looping back the message, we do not need to mark it seen or acknowledge it;
        // the message will be received by the listener anyway.

        // Send the message to the multicast address.
        send_multicast(sender, &self.multicast_addr, &buffer).await
    }

    /// Receive a message from the multicast address.
    ///
    /// This is an internal method that will await the next message from the
    /// multicast address. It will return the message as a [`MulticastDelivery`]
    /// if successful.
    ///
    /// If a duplicated message is received, it will be skipped and this method
    /// will return an [`io::Error`] with a message indicating the duplication.
    pub async fn receive_message(&self) -> io::Result<Option<MulticastDelivery<T>>> {
        let (bytes, sock_addr) = receive_multicast(&self.listener, self.packet_size).await?;
        logger::debug!("Received {} bytes from {:?}.", bytes.len(), sock_addr);
        let received = tokio::time::Instant::now();

        let message = MulticastMessage::from_bytes(&bytes)?;
        let uuid = message.uuid()?;
        let socket_addr = sock_addr
            .as_socket()
            .ok_or_else(|| build_error(&format!("Failed to convert {sock_addr:?} to socket.")))?;
        let message_kind = multicast_message::Kind::try_from(message.kind)
            .map_err(|_| build_error(&format!("Invalid message kind: {}", message.kind)))?;

        logger::debug!(
            "Message {} from {} is of kind {:?}.",
            &uuid,
            describe_socket_addr(&socket_addr),
            message_kind
        );
        if message_kind == multicast_message::Kind::Acknowledge {
            // The message is an acknowledgement.
            // Record the acknowledgement regardless of whether the UUID is seen.
            // Who knows, may be the message may come afterwards?
            self.acknowledge(&uuid, &socket_addr).await;
            // Notify the waiting thread that an acknowledgement has been received.
            self._acknowledgement_flag.notify_waiters();
            return Ok(None);
        } else if !self.mark_seen(&uuid, socket_addr).await {
            logger::debug!(
                "Received duplicate message {} from {}; skipping.",
                uuid,
                describe_socket_addr(&socket_addr)
            );
            return Err(build_error(&format!(
                "Received duplicate message {} from {}.",
                uuid,
                describe_socket_addr(&socket_addr)
            )));
        }

        logger::info!(
            "Received message {} from {}.",
            uuid,
            describe_socket_addr(&socket_addr)
        );

        let echo_opt = message.echo();

        match tokio::try_join!(
            // Record the acknowledgement if the message contains one.
            '_acknowledgement: {
                async {
                    // Action on acknowledgement.
                    if message_kind == multicast_message::Kind::Origin
                        || message_kind == multicast_message::Kind::Echo
                    {
                        // We may need to send an acknowledgement for this message.
                        match self.send_acknowledgement(&uuid).await {
                            Ok(_size) => {
                                logger::debug!(
                                    "Sent acknowledgement for message {} with size {}.",
                                    uuid,
                                    _size
                                );
                            }
                            Err(err) => {
                                logger::warn!(
                                    "Failed to send acknowledgement for message {}: {}",
                                    uuid,
                                    err
                                );
                            }
                        }
                    }

                    Ok(())
                }
            },
            // Re-broadcast the message if it has TTL > 0.
            '_rebroadcast: {
                async {
                    // The message is new; check if we should re-broadcast it.
                    if let Some(echo) = echo_opt {
                        // Re-broadcast the message, do not loop back.
                        match self.send_message(&uuid, echo, false).await {
                            Ok(_) => {
                                logger::info!(
                                    "Re-broadcasted message {} from {}.",
                                    uuid,
                                    describe_socket_addr(&socket_addr)
                                );
                            }
                            Err(err) => {
                                logger::warn!(
                                    "Failed to re-broadcast message {} from {}: {}",
                                    uuid,
                                    describe_socket_addr(&socket_addr),
                                    err
                                );
                            }
                        };
                    }

                    Ok(())
                }
            },
            // Convert the message to a delivery.
            '_convert_to_delivery: {
                async {
                    if multicast_message::Kind::try_from(message.kind)
                        != Ok(multicast_message::Kind::System)
                    {
                        // Non-system message.
                        MulticastDelivery::try_from((message, socket_addr, received)).map(Some)
                    } else {
                        // System message.
                        let system_message: SystemMessage = serde_json::from_str(&message.body)
                            .map_err(|err| {
                                build_error(&format!(
                                    "Failed to deserialize system message: {}",
                                    err
                                ))
                            })?;

                        self.resolve_system_message(system_message).await?;
                        Ok(Option::<MulticastDelivery<T>>::None)
                    }
                }
            },
        ) {
            Ok((_, _, delivery)) => {
                if delivery.is_some() {
                    // Notify all the waiting threads that a delivery has been made.
                    self._delivery_flag.notify_waiters();
                    logger::debug!("Converted message {} to delivery.", uuid);
                } else {
                    logger::debug!("Received system message {}.", uuid);
                }
                Ok(delivery)
            }
            Err(err) => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Failed to process message: {}", err),
            )),
        }
    }

    /// Starts listening for messages on the multicast address.
    pub async fn start<'h>(self: &Arc<Self>) {
        let weak_self = Arc::downgrade(self);
        let arc_notify = Arc::clone(&self._terminate_flag);

        self.listener_handle.get_or_init(||
            tokio::spawn(async move {
                // This is the listener task that will receive messages.
                // It only holds a weak reference to the agent, so that it can terminate
                // when the agent is dropped.
                let recv_messages = {async move {
                    loop {
                        if let Some(arc_self) = weak_self.upgrade() {
                            let delivery = arc_self.receive_message().await;

                            match delivery {
                                Ok(Some(delivery)) => {
                                    logger::debug!("Pushing delivery {} to queue.", delivery.uuid);
                                    arc_self.output.push(delivery);
                                },
                                Ok(None) => {
                                    logger::debug!("Received non-deliverable message.");
                                },
                                Err(_err) => {}
                            }
                        } else {
                            logger::info!("Terminating multicast listener due to dropped reference.");
                            break;
                        }
                    }
                }};

                tokio::select! {
                    _ = arc_notify.notified() => {
                        logger::info!("Terminating multicast listener due to termination notification.", );
                    },
                    _ = recv_messages => {},
                }
            })
        );
    }

    /// Check if the agent is listening for messages.
    ///
    /// # Note
    ///
    /// If you intend to start the agent if not already, simply call the [`Self::start`] method,
    /// which will not error if the agent is already listening.
    ///
    /// This method is useful for checking only.
    pub fn is_listening(&self) -> bool {
        self.listener_handle.get().is_some()
    }
}

impl<T> MulticastAgent<T>
where
    T: serde::Serialize + serde::de::DeserializeOwned + Send + Sync,
{
    /// Stops listening for messages on the multicast address.
    pub fn stop(&self) {
        logger::debug!("Stopping multicast listener...");
        self._terminate_flag.notify_one();
    }
}

impl<T> Drop for MulticastAgent<T>
where
    T: serde::Serialize + serde::de::DeserializeOwned + Send + Sync,
{
    fn drop(&mut self) {
        logger::info!("Drop triggered on MulticastAgent.");
        self.stop()
    }
}

/// Do not run these tests in CI.
#[cfg(all(test, not(feature = "ci_tests")))]
mod tests {
    use super::*;
    use crate::_tests::{TestStruct, MULTICAST_ADDRESS, SECRET};

    use serial_test::serial;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[serial]
    async fn single_multicast_agent() {
        let multicast_addr = MULTICAST_ADDRESS;

        let agent_factory = || {
            Arc::new(
                MulticastAgent::<TestStruct>::new(multicast_addr)
                    .expect("Failed to create multicast agent.")
                    .with_packet_size(1024),
            )
        };

        let agent = agent_factory();

        agent.start().await;

        let uuid = Uuid::new_v4();
        let message = MulticastMessage::from_serializable(
            &uuid,
            multicast_message::Kind::Origin,
            &SECRET,
            None,
        )
        .expect("Failed to create message.");
        let uuid = message.uuid().expect("Failed to get UUID.");
        // Loop back the message, so that we can receive it.

        for _count in 0..5 {
            agent
                .send_message(&uuid, message.clone(), true)
                .await
                .expect("Failed to send message.");
            logger::info!("Sent multicast message #{count}.", count = _count)
        }

        logger::info!("Awaiting message from queue...");
        let _delivery =
            tokio::time::timeout(tokio::time::Duration::from_secs(1), agent.output.pop())
                .await
                .expect("Failed to wait for message from queue, or the message was not received.");

        assert_eq!(agent.output.len(), 0);

        logger::info!("Received message: {:?}", _delivery);
        logger::info!("Reference count: {}", Arc::strong_count(&agent));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[serial]
    async fn multi_multicast_agents() {
        let multicast_addr = MULTICAST_ADDRESS;

        let agent_factory = || {
            Arc::new(
                MulticastAgent::<TestStruct>::new(multicast_addr)
                    .expect("Failed to create multicast agent.")
                    .with_packet_size(1024)
                    .with_ttl(1),
            )
        };

        let agents = [agent_factory(), agent_factory()];

        for agent in agents.iter() {
            agent.start().await;
        }

        for _count in 0..5 {
            let (_uuid, _size) = agents[0]
                // Do not loop back the message, so that only the other agent can receive it.
                .send(SECRET)
                .await
                .expect("Failed to send message.");
            logger::debug!(
                "Sent multicast message #{count} with UUID {uuid} containing {size} bytes.",
                count = _count,
                uuid = _uuid,
                size = _size,
            );
        }

        logger::debug!("Awaiting message from queue...");
        let delivery =
            tokio::time::timeout(tokio::time::Duration::from_secs(1), agents[1].output.pop())
                .await
                .expect("Failed to wait for message from queue, or the message was not received.");
        // The first agent should not receive the message, and ignored the echo.
        assert_eq!(agents[0].output.len(), 0);
        // The second agent should receive the message, which has now been popped.
        assert_eq!(agents[1].output.len(), 0);

        logger::debug!("Received message: {:?}", delivery);

        tokio::try_join!(
            tokio::time::timeout(
                tokio::time::Duration::from_secs(1),
                agents[0].wait_for_acknowledgement()
            ),
            tokio::time::timeout(
                tokio::time::Duration::from_secs(1),
                agents[1].wait_for_acknowledgement()
            ),
        )
        .expect("Failed to wait for acknowledgements, or the acknowledgements were not received.");

        assert_eq!(
            agents[0].count_acknowledgements_for(&delivery.uuid).await,
            Some(1)
        );
        assert_eq!(
            agents[1].count_acknowledgements_for(&delivery.uuid).await,
            Some(1)
        );
    }
}
