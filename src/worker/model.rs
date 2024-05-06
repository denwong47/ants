//! The main worker model.
//!

use serde::{de::DeserializeOwned, Serialize};
use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, OnceLock,
    },
};
use tokio::sync::Notify;
use tonic::{Request, Response, Status};

use super::{
    proto::{self, worker_ant_server::WorkerAnt as WorkerAntServerTrait},
    token as reservation_token, WorkerBroadcastMessage,
};
use crate::{
    nodes::{NodeAddress, NodeList, NodeMetadata},
    postbox::{traits::*, PostBox},
    token as message_token, AntsError,
};

/// The timeout for reserving a node.
///
/// This should be reasonably short, provided that we expected the node to be
/// in the same subnet. If it does not return within this time, it is likely to be
/// unreachable.
static RESERVE_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(500);

const RESERVE_ATTEMPTS: usize = 32;

/// A worker that can do work or distribute work.
///
/// A unit of work is defined by an async function that takes a type `T` and returns
/// a `Result<R, E>`. All workers are expected to be stateless, and work performed
/// by any worker is expected to be the same as any other worker.
///
/// The work is also expected to be blocking for the duration of the work, and
/// no concurrent work can be performed. This is usually the case when the work
/// involves physical devices such as GPU, or the work being extremely CPU
/// bound, rendering concurrent work to be ineffective.
///
/// The worker should be instantiated with a list of nodes that it can forward
/// work to, alongside the aforementioned function. The worker will then listen
/// on gRPC for work requests. Any calls to [`find_worker_and_work`] will
/// attempt to reserve the worker, and do work if possible, or forward the work
/// to another worker if not.
///
/// During the negotiation of work with another worker, the worker will attempt
/// to reserve the other worker before forwarding the work. If the other worker
/// is reserved, the worker will attempt to reserve the next worker in the list.
///
/// Reservation works by sending a `reserve` request to the worker, and the
/// worker will respond with a token if it is not reserved. A subsequent `work`
/// request must include the token to be accepted; by the end of the work, the
/// worker will release the reservation. If the worker does not receive a
/// `work` request within a [`token::TIMEOUT`], the worker will release the
/// reservation automatically.
pub struct Worker<T, R, F, FO, E>
where
    T: DeserializeOwned + Serialize + std::marker::Sync + std::marker::Send + 'static,
    R: DeserializeOwned + Serialize + std::marker::Sync + std::marker::Send + 'static,
    FO: std::future::Future<Output = Result<R, E>>
        + std::marker::Sync
        + std::marker::Send
        + 'static,
    F: Fn(T) -> FO + 'static,
    E: std::fmt::Display + std::fmt::Debug + 'static,
    Self: std::marker::Sync + std::marker::Send,
{
    pub address: NodeAddress,
    pub nodes: Arc<NodeList>,
    pub reservation: Arc<AtomicU64>,
    pub busy: Arc<AtomicBool>,
    pub func: Box<F>,
    pub work_timeout: tokio::time::Duration,

    /// The broadcast agent for the worker.
    pub broadcaster: Arc<multicast::MulticastAgent<WorkerBroadcastMessage>>,

    /// The postbox for the worker; this is used to stage received messages for delivery.
    pub postbox: Arc<PostBox>,
    _phantom: std::marker::PhantomData<(T, R)>,

    /// The handle for the gRPC listeners.
    _listener_handle: OnceLock<tokio::task::JoinHandle<Result<(), AntsError>>>,

    /// A flag to signal termination.
    pub(crate) _termination_flag: Arc<Notify>,

    /// An atomic cyclical reference to itself.
    ///
    /// This is used for the [`Worker`] to spawn tasks that can refer back to itself,
    /// while respecting the ``'static`` lifetime of the [`Worker`].
    _cyclical: OnceLock<Arc<Self>>,
}

impl<T, R, F, FO, E> Worker<T, R, F, FO, E>
where
    T: DeserializeOwned + Serialize + std::marker::Sync + std::marker::Send + 'static,
    R: DeserializeOwned + Serialize + std::marker::Sync + std::marker::Send + 'static,
    FO: std::future::Future<Output = Result<R, E>>
        + std::marker::Sync
        + std::marker::Send
        + 'static,
    F: Fn(T) -> FO + 'static,
    E: std::fmt::Display + std::fmt::Debug + 'static,
    Self: std::marker::Sync + std::marker::Send,
{
    /// Create a new worker.
    ///
    /// # Warning
    ///
    /// This function is not recommended for use, as it does not initialise the
    /// internal cyclical reference. Use [`Self::new_arc`] instead.
    pub fn new(
        host: String,
        port: u16,
        nodes: Vec<NodeAddress>,
        broadcast_host: String,
        broadcast_port: u16,
        func: F,
        timeout: tokio::time::Duration,
    ) -> Result<Self, AntsError> {
        let nodes = NodeList::from_vec(nodes);

        let broadcaster_socket_addr = SocketAddr::new(
            broadcast_host.parse().map_err(|_| {
                AntsError::MulticasterAddressError(format!(
                    "{} is not a valid IP address.",
                    broadcast_host
                ))
            })?,
            broadcast_port,
        );

        if !broadcaster_socket_addr.ip().is_multicast() {
            return Err(AntsError::MulticasterAddressError(format!(
                "{}:{} is not a multicast address.",
                broadcast_host, broadcast_port
            )));
        }

        let broadcaster = multicast::MulticastAgent::new(broadcaster_socket_addr)
            .map_err(AntsError::MulticasterNotAvailable)?;

        let worker = Worker {
            address: (host, port),
            nodes: Arc::new(nodes),
            reservation: Arc::new(AtomicU64::new(0)),
            busy: Arc::new(AtomicBool::new(false)),
            func: Box::new(func),
            work_timeout: timeout,
            broadcaster: Arc::new(broadcaster),
            postbox: PostBox::new_arc(),
            _listener_handle: OnceLock::new(),
            _termination_flag: Arc::new(Notify::new()),
            _phantom: std::marker::PhantomData,
            _cyclical: OnceLock::new(),
        };

        Ok(worker)
    }

    /// Intialize the internal cyclical [`Arc`] reference, and return a reference to it.
    pub fn init_arc(self) -> Arc<Self> {
        let arc_self = Arc::new(self);
        Arc::clone(arc_self._cyclical.get_or_init(|| arc_self.clone()))
    }

    /// Get the internal cyclical [`Arc`] reference.
    ///
    /// This can only be used if the worker has an initialised cyclical reference using
    /// [`Self::init_arc`] or [`Self::new_arc`].
    ///
    /// # Panics
    ///
    /// This function will panic if the cyclical reference is not initialised.
    pub fn cyclical(&self) -> Arc<Self> {
        Arc::clone(
            self._cyclical
                .get()
                .expect("Cyclical reference not initialised."),
        )
    }

    /// Create a new worker wrapped in an [`Arc`].
    ///
    /// Since the [`Worker`] is expected to be static and not bound to any particular
    /// scope, creating [`Arc`] wrapped instances is the most common way to use this
    /// struct.
    ///
    /// This will also initialise the internal cyclical reference as well as the broadcast
    /// agent.
    #[allow(clippy::too_many_arguments)]
    pub async fn new_and_init(
        host: String,
        port: u16,
        nodes: Vec<NodeAddress>,
        broadcast_host: String,
        broadcast_port: u16,
        func: F,
        timeout: tokio::time::Duration,
    ) -> Result<Arc<Self>, AntsError> {
        let arc_self = Self::new(
            host,
            port,
            nodes,
            broadcast_host,
            broadcast_port,
            func,
            timeout,
        )?
        .init_arc()
        .init_broadcast()
        .await?
        .init_listener()
        .await;

        Ok(arc_self)
    }

    /// Get the name of this worker.
    pub fn name(&self) -> String {
        format!("worker://{}:{}", self.address.0, self.address.1)
    }

    /// Get the host of this worker.
    pub fn host(&self) -> String {
        self.address.0.clone()
    }

    /// Get the port of this worker.
    pub fn port(&self) -> u16 {
        self.address.1
    }

    /// Build a client to the given node.
    async fn build_client(
        &self,
        to: NodeAddress,
    ) -> Result<proto::worker_ant_client::WorkerAntClient<tonic::transport::Channel>, AntsError>
    {
        let addr = format!("http://{}:{}", to.0, to.1);
        proto::worker_ant_client::WorkerAntClient::connect(addr)
            .await
            .map_err(|err| AntsError::ConnectionError(to.0, to.1, format!("{:?}", err)))
    }

    /// Get the next node to use from the [`NodeList`].
    pub async fn next_node(&self) -> Option<NodeAddress> {
        self.nodes.next().await
    }

    /// Get the next node with the metadata from the [`NodeList`].
    pub async fn next_node_with_metadata(&self) -> Option<(NodeMetadata, NodeAddress)> {
        self.nodes.next_with_metadata().await
    }

    /// Get the next node with the duration since the last call.
    pub async fn next_node_with_duration(&self) -> Option<(tokio::time::Duration, NodeAddress)> {
        self.next_node_with_metadata()
            .await
            .map(|(metadata, node)| (metadata.last_used().elapsed(), node))
    }

    /// Add a node back to the [`NodeList`].
    pub async fn add_node(&self, node: NodeAddress) -> bool {
        self.nodes.insert(node).await
    }

    /// Check if the node is in the [`NodeList`].
    pub async fn has_node(&self, node: &NodeAddress) -> bool {
        self.nodes.contains(node).await
    }

    /// Remove a node from the [`NodeList`].
    pub async fn remove_node(&self, node: &NodeAddress) {
        self.nodes.remove(node).await;
    }

    /// Attempt to reserve a node, and return the reservation token if successful.
    pub async fn reserve_node(&self) -> Result<(NodeAddress, u64), AntsError> {
        for tries in 0..RESERVE_ATTEMPTS {
            if let Some(checked_out_node) = self.nodes.checkout().await {
                // DDoS prevention.
                if tries > 0 && checked_out_node.since_last_used() < RESERVE_TIMEOUT {
                    logger::info!(
                        "The top node was queried {duration:?} ago, waiting for {timeout:?} before trying again.",
                        duration = checked_out_node.since_last_used(),
                        timeout = RESERVE_TIMEOUT,
                    );
                    tokio::time::sleep(RESERVE_TIMEOUT - checked_out_node.since_last_used()).await;
                }

                // this needs to contact the node and reserve it.
                // Hold the node address during this step, preventing any other
                // threads from attempting to reserve it.
                let mut client = match tokio::select! {
                    _ = tokio::time::sleep(RESERVE_TIMEOUT) => {
                        // TODO The node did not respond. Downrate the node.
                        Err(AntsError::ConnectionError(
                            checked_out_node.host().to_owned(),
                            checked_out_node.port(),
                            format!("did not respond within {RESERVE_TIMEOUT:?}")
                        ))
                    },
                    connection_result = self.build_client(checked_out_node.address().clone()) => {
                        connection_result
                    }
                } {
                    Ok(client) => {
                        // Put the node back in the heap, but this time with a new timestamp.
                        client
                    }
                    Err(_err) => {
                        // TODO The node had a connection error. Downrate the node.

                        // Put the node back in the heap, but this time with a new timestamp.
                        let _address = checked_out_node.complete().await;
                        logger::warn!(
                            "Failed to connect to node {}:{} due to {}, trying next node.",
                            &_address.0,
                            &_address.1,
                            _err
                        );
                        continue;
                    }
                };

                let reservation_result = client.reserve(Request::new(proto::Empty {})).await;

                // Put the node back in the heap, but this time with a new timestamp.
                // While this node will be flagged as "reservable", it will not be
                // possible to do so while our reservation is in place.
                let address = checked_out_node.complete().await;

                // We can't short circuit `reservation_result?` here because this will
                // break the retry loop immediately.
                match reservation_result {
                    Ok(response) => {
                        // Now that we have put the node back in the heap, we can check the
                        // reservation result.
                        let reservation_reply = response.into_inner();

                        if reservation_reply.success {
                            return Ok((address, reservation_reply.token));
                        } else {
                            logger::debug!(
                                "Node {}:{} is reserved, trying next node.",
                                &address.0,
                                &address.1
                            );
                        }
                    }
                    Err(_status) => {
                        logger::warn!(
                            "Failed to reserve node {}:{} due to {}, trying next node.",
                            &address.0,
                            &address.1,
                            _status
                        );
                    }
                }
            } else {
                // There is literally no nodes in the heap.
                //
                // This needs to be re-think - if all available nodes are checked out
                // at once for reservation, should we just wait for one to be available?
                logger::warn!(
                    "No nodes available in the heap at all. This could be due to all \
                    nodes being checked out for reservation, or no nodes were added to \
                    the Worker at the first place. We will wait for {timeout:?} \
                    before trying again.",
                    timeout = RESERVE_TIMEOUT
                );
                tokio::time::sleep(RESERVE_TIMEOUT).await;
            }
        }

        Err(AntsError::NoNodesAvailable)
    }

    /// Reserve this node.
    pub fn reserve(&self) -> Option<reservation_token::ReservationToken> {
        // TODO This function does not know who is reserving it. Change this?
        let token = reservation_token::generate_token();
        match self
            .reservation
            .compare_exchange(0, token, Ordering::Release, Ordering::Relaxed)
        {
            Ok(_) => {
                let reservation_ptr = self.reservation.clone();
                let busy_ptr = self.busy.clone();

                // Prevent node poisoning if the reservation is made, but work
                // is not started after a timeout.
                tokio::spawn(async move {
                    tokio::time::sleep(reservation_token::TIMEOUT).await;

                    if busy_ptr.load(Ordering::Acquire) {
                        logger::trace!("Reservation timed out, but the node has started Work; will allow Work to release reservation instead.");
                    } else {
                        match reservation_ptr.compare_exchange(
                            token,
                            0,
                            Ordering::Release,
                            Ordering::Relaxed,
                        ) {
                            Ok(_) => {
                                logger::trace!("Reservation timed out and released.")
                            }
                            Err(_) => {
                                logger::trace!("Reservation timed out, but was already released.")
                            }
                        }
                    }
                });

                logger::info!(
                    "Reserved {name} with token {token}.",
                    name = self.name(),
                    token = token
                );
                Some(token)
            }
            Err(_) => None,
        }
    }

    /// Release this node.
    pub fn release(&self, token: u64) -> Result<reservation_token::ReservationToken, AntsError> {
        match self
            .reservation
            .compare_exchange(token, 0, Ordering::Release, Ordering::Relaxed)
        {
            Ok(_) => Ok(token),
            Err(_) => Err(AntsError::ReservationTokenMismatch),
        }
    }

    /// Call the internal function, flag itself as busy in the process.
    ///
    /// This is an internal function that does not check the reservation token,
    /// and shoudl not be used in the public API.
    ///
    /// This also does not release the reservation. It is up to the caller to
    /// release and remove the reservation, so that it can be sent back to the
    /// origin node.
    async fn call_inner(&self, body: T) -> Result<R, AntsError> {
        self.busy
            .compare_exchange(false, true, Ordering::Release, Ordering::Relaxed)
            .map_err(|_| AntsError::CalledWhileBusy)?;

        // DON'T ? HERE! We have to release the busy flag first.
        let result = (self.func)(body)
            .await
            .map_err(|err| AntsError::TaskExecutionError(err.to_string()));

        self.busy.store(false, Ordering::Release);

        result
    }

    /// Hold the reservation and do work, before releasing it.
    async fn reserve_and_work(&self, token: u64, body: T) -> Result<R, AntsError> {
        if token != self.reservation.load(Ordering::Acquire) {
            return Err(AntsError::ReservationTokenMismatch);
        }

        let result = self.call_inner(body).await;

        self.release(token)?;

        result
    }

    /// Attempt to reserve itself and do work; if not possible, iterate
    /// through the nodes and attempt to reserve them and do work instead.
    pub async fn find_worker_and_work(&self, body: T) -> Result<(String, R), AntsError> {
        loop {
            match self.reserve() {
                Some(token) => {
                    return self
                        .reserve_and_work(token, body)
                        .await
                        .map(|r| (self.name(), r))
                }
                None => {
                    // Make a named block to get a `Result`. Any `Err`s will immediately
                    // short circuit the block.
                    let result: Result<_, AntsError> = '_get_result: {
                        let (address, token) = self.reserve_node().await?;

                        let mut client = self.build_client(address.clone()).await?;

                        let reservation_result = tokio::select! {
                            result = client.work(Request::new(proto::WorkRequest {
                                token,
                                body: serde_json::to_string(&body).map_err(
                                    |err| AntsError::InvalidWorkData(err.to_string())
                                )?,
                                host: self.address.0.clone(),
                                port: self.address.1 as u32,
                            })) => { result.map_err(
                                AntsError::Tonic
                            ) }
                            _ = tokio::time::sleep(self.work_timeout) => {
                                Err(AntsError::ConnectionError(
                                    address.0.clone(),
                                    address.1,
                                    format!("did not respond within {work_timeout:?}", work_timeout=self.work_timeout)
                                ))
                            },
                        }?;

                        Ok((token, reservation_result))
                    };

                    if let Ok((token, response)) = result {
                        let work_reply = response.into_inner();
                        if work_reply.success && work_reply.token == token {
                            // We have successfully reserved and sent work to a node.
                            // We will now create a message, and wait for the work
                            // result to be delivered.
                            let message = self
                                .postbox
                                .create_message::<proto::DeliverRequest>((
                                    token,
                                    work_reply.task_id,
                                ))
                                .await;

                            logger::trace!(
                                "Waiting for message {key:?} to be delivered...",
                                key = message.key()
                            );

                            // Wait for the message to be delivered.
                            message.wait_for(self.work_timeout).await?;

                            // Get the message body.
                            let inner: &proto::DeliverRequest = message.get_body()?;

                            if inner.success {
                                return Ok((
                                    inner.worker.clone(),
                                    serde_json::from_str(inner.body.as_str()).map_err(|err| {
                                        AntsError::InvalidWorkResult(err.to_string())
                                    })?,
                                ));
                            } else {
                                logger::warn!(
                                    "Work result received, which reported that work failed on \
                                    node {} due to {}, trying next node.",
                                    &inner.worker,
                                    inner.error
                                );
                            }
                        } else if work_reply.token != token {
                            logger::error!(
                                "Work failed on node {} due to token mismatch, trying next node.",
                                &work_reply.worker
                            );
                        } else {
                            logger::error!(
                                "Work failed on node {} due to {}, trying next node.",
                                &work_reply.worker,
                                work_reply.message
                            );
                        }
                    } else {
                        logger::error!(
                            "Work request failed on node due to {}, trying next node.",
                            result.err().unwrap()
                        );
                    }
                }
            }
        }
    }

    /// Subscribe to the termination signal. This will return a future that will
    /// only resolve when the worker is terminated.
    pub async fn wait_for_termination(&self) {
        self._termination_flag.notified().await;
    }
}

impl<T, R, F, FO, E> Worker<T, R, F, FO, E>
where
    T: DeserializeOwned + Serialize + std::marker::Sync + std::marker::Send + 'static,
    R: DeserializeOwned + Serialize + std::marker::Sync + std::marker::Send + 'static,
    FO: std::future::Future<Output = Result<R, E>>
        + std::marker::Sync
        + std::marker::Send
        + 'static,
    F: Fn(T) -> FO + 'static,
    E: std::fmt::Display + std::fmt::Debug + 'static,
    Self: std::marker::Sync + std::marker::Send,
{
    /// Initialize the broadcast agent in the background.
    ///
    /// This chain method will start the broadcast agent in the background, and return
    /// the same [`Arc<Worker>`] instance for further chaining.
    pub async fn init_listener(self: Arc<Self>) -> Arc<Self> {
        let listener_self = Arc::clone(&self);
        self._listener_handle
            .get_or_init(|| tokio::spawn(listener_self.listener()));
        self
    }

    /// Start the worker server to listen.
    ///
    /// This method will start the gRPC server to listen for incoming requests, and not
    /// return until the server is shut down.
    ///
    /// There is typically no need to call this method directly, as it is called by
    /// [`Self::init_listener`] and in turn [`Self::new_and_init`].
    pub async fn listener(self: Arc<Self>) -> Result<(), AntsError> {
        logger::info!(
            "Starting RPC server on {}:{}...",
            self.address.0,
            self.address.1
        );
        tokio::select! {
            _ = self.wait_for_termination() => {
                // If the worker is terminated, we will return Ok(()).
                logger::debug!("Shutting down RPC server on {}:{}...", self.address.0, self.address.1);
                Ok(())
            },
            err = tonic::transport::Server::builder()
                .add_service(proto::worker_ant_server::WorkerAntServer::from_arc(
                    self.clone(),
                ))
                .serve(std::net::SocketAddr::new(
                    std::net::IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0)),
                    self.address.1,
                ))
            => {
                // If the server fails to start, we will return an [`AntsError`].
                err.map_err(|err| AntsError::TonicServerStartUpError(err.to_string()))
            }
        }
    }
}

impl<T, R, F, FO, E> Worker<T, R, F, FO, E>
where
    T: DeserializeOwned + Serialize + std::marker::Sync + std::marker::Send,
    R: DeserializeOwned + Serialize + std::marker::Sync + std::marker::Send,
    FO: std::future::Future<Output = Result<R, E>> + std::marker::Sync + std::marker::Send,
    F: Fn(T) -> FO + Copy, // Copy is required for the `Fn` trait.
    E: std::fmt::Display + std::fmt::Debug,
    Self: std::marker::Sync + std::marker::Send,
{
    /// Create a number of new workers wrapped in an [`Arc`].
    ///
    /// Each worker will have a port number that is incremented by 1 from the
    /// previous worker.
    ///
    /// This is mostly designed for testing purposes, as there is not much purpose
    /// in creating multiple workers on the same machine if the work is CPU or
    /// device-bound.
    ///
    /// # See Also
    ///
    /// See [`Self::new_and_init`] for more information.
    #[allow(clippy::too_many_arguments)]
    pub async fn new_and_init_multiple(
        count: usize,
        host: String,
        port: u16,
        nodes: Vec<NodeAddress>,
        broadcast_host: String,
        broadcast_port: u16,
        func: F,
        timeout: tokio::time::Duration,
    ) -> Result<Vec<Arc<Self>>, AntsError> {
        futures::future::try_join_all((0..count).map(|id| {
            Self::new_and_init(
                host.clone(),
                port + id as u16,
                nodes.clone(),
                broadcast_host.clone(),
                broadcast_port,
                func,
                timeout,
            )
        }))
        .await
    }
}

#[tonic::async_trait]
impl<T, R, F, FO, E> WorkerAntServerTrait for Worker<T, R, F, FO, E>
where
    T: DeserializeOwned + Serialize + std::marker::Sync + std::marker::Send + 'static,
    R: DeserializeOwned + Serialize + std::marker::Sync + std::marker::Send + 'static,
    FO: std::future::Future<Output = Result<R, E>>
        + std::marker::Sync
        + std::marker::Send
        + 'static,
    F: Fn(T) -> FO + 'static,
    E: std::fmt::Display + std::fmt::Debug + 'static,
    Self: std::marker::Sync + std::marker::Send,
{
    /// Ping a worker to see if it is alive.
    async fn ping(&self, _: Request<proto::Empty>) -> Result<Response<proto::PingReply>, Status> {
        Ok(Response::new(proto::PingReply {
            status: if self.reservation.load(Ordering::Acquire) == 0 {
                0 // Not reserved.
            } else {
                1 // Reserved.
            },
        }))
    }

    /// Receive a request to reserve this worker.
    async fn reserve(
        &self,
        _: Request<proto::Empty>,
    ) -> Result<Response<proto::ReserveReply>, Status> {
        if let Some(token) = self.reserve() {
            Ok(Response::new(proto::ReserveReply {
                success: true,
                token,
            }))
        } else {
            Ok(Response::new(proto::ReserveReply {
                success: false,
                token: 0,
            }))
        }
    }

    /// Receive a request to release a reservation on this worker.
    ///
    /// The correct token must be provided, or the reservation will not be
    /// released.
    async fn release(
        &self,
        request: Request<proto::ReleaseRequest>,
    ) -> Result<Response<proto::ReleaseReply>, Status> {
        let token = request.into_inner().token;
        match self.release(token) {
            Ok(_) => Ok(Response::new(proto::ReleaseReply { success: true })),
            Err(_) => Ok(Response::new(proto::ReleaseReply { success: false })),
        }
    }

    /// Receive a request to do work.
    ///
    /// This will only work if the worker is reserved, and the correct token
    /// is provided.
    ///
    /// # Panics
    ///
    /// If the [`Worker`] is not instantiated with a cyclical reference, this
    /// function will panic.
    async fn work(
        &self,
        request: Request<proto::WorkRequest>,
    ) -> Result<Response<proto::WorkReply>, Status> {
        let work_request = request.into_inner();
        let address = (work_request.host, work_request.port as u16);
        let token = work_request.token;
        let task_id = message_token::generate_token();
        let parsed_body: Result<T, _> = serde_json::from_str(work_request.body.as_str());

        let arc_self = self.cyclical();

        Ok(Response::new(match parsed_body {
            Ok(body) => {
                let _handle = tokio::spawn(async move {
                    let result = arc_self.reserve_and_work(token, body).await;
                    let deliver_request = match result {
                        Ok(r) => serde_json::to_string(&r)
                            .map(|body| proto::DeliverRequest {
                                token,
                                task_id,
                                success: true,
                                error: "".to_string(),
                                body,
                                worker: arc_self.name(),
                            })
                            .unwrap_or_else(|err| proto::DeliverRequest {
                                token,
                                task_id,
                                success: false,
                                error: AntsError::InvalidWorkResult(err.to_string()).to_string(),
                                body: "".to_string(),
                                worker: arc_self.name(),
                            }),
                        Err(_err) => proto::DeliverRequest {
                            token,
                            task_id,
                            success: false,
                            error: AntsError::TaskExecutionError(_err.to_string()).to_string(),
                            body: "".to_string(),
                            worker: arc_self.name(),
                        },
                    };

                    let client = arc_self.build_client(address.clone()).await;
                    if let Err(_err) = client {
                        logger::warn!(
                            "Failed to connect to node {}:{} to deliver work result due to: {}. Work result will be dropped.",
                            address.0, address.1, _err
                        );
                        return;
                    }

                    let result = client
                        .unwrap()
                        .deliver(Request::new(deliver_request))
                        .await
                        .map_err(AntsError::Tonic);

                    // If the delivery fails, we just log it for now.
                    // FIXME How should we handle this?
                    if let Err(_err) = result {
                        logger::error!("Failed to deliver work result: {:?}", _err);
                    } else if let Ok(response) = result {
                        let deliver_reply = response.into_inner();
                        if deliver_reply.success {
                            logger::info!(
                                "Work result of {task_id} for reservation #{token} delivered successfully.",
                                task_id=deliver_reply.task_id,
                                token=deliver_reply.token,
                            );
                        } else {
                            logger::error!(
                                "Work result of {task_id} for reservation #{token} delivered, but the host reported it could not be set on the message.",
                                task_id=deliver_reply.task_id,
                                token=deliver_reply.token,
                            );
                        }
                    }
                });

                proto::WorkReply {
                    token,
                    task_id,
                    success: true,
                    message: "Task spawned successfully.".to_owned(),
                    worker: self.name(),
                }
            }
            Err(_err) => proto::WorkReply {
                token,
                task_id,
                success: false,
                message: AntsError::InvalidWorkData(_err.to_string()).to_string(),
                worker: self.name(),
            },
        }))
    }

    async fn deliver(
        &self,
        request: Request<proto::DeliverRequest>,
    ) -> Result<Response<proto::DeliverReply>, Status> {
        let deliver_request = request.into_inner();

        let message_key = ("Deliver", (deliver_request.token, deliver_request.task_id));

        let token = deliver_request.token;
        let task_id = deliver_request.task_id;
        let set_body_result = self.postbox.set_body(&message_key, deliver_request).await;
        let set_body_success = set_body_result.is_ok();

        if set_body_result.is_err() {
            logger::error!(
                "Failed to set the body of message {task_id} for reservation #{token} due to: {err:?}",
                task_id=task_id,
                token=token,
                err=set_body_result.unwrap_err(),
            );
        }

        Ok(Response::new(proto::DeliverReply {
            token,
            task_id,
            // We don't care if the delivery fails, as the worker has already done its job.
            // We can report this back to the worker, who will do nothing about it.
            success: set_body_success,
        }))
    }
}
