//! The main worker model.
//!

use core::cmp::Reverse;
use serde::{de::DeserializeOwned, Serialize};
use std::{
    collections::BinaryHeap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
};
use tokio::sync::Mutex;
use tonic::{Request, Response, Status};

pub type NodeAddress = (String, u16);
pub type NodeRecord = (tokio::time::Instant, NodeAddress);

use super::{
    proto::{self, worker_ant_server::WorkerAnt as WorkerAntServerTrait},
    token,
};
use crate::AntsError;

/// The timeout for reserving a node.
///
/// This should be reasonably short, provided that we expected the node to be
/// in the same subnet. If it does not return within this time, it is likely to be
/// unreachable.
static RESERVE_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(500);

const RESERVE_ATTEMPTS: usize = 32;
/// A worker that can do work or distribute work.
pub struct Worker<T, R, F, FO, E>
where
    T: DeserializeOwned + Serialize + std::marker::Sync + std::marker::Send,
    R: DeserializeOwned + Serialize + std::marker::Sync + std::marker::Send,
    FO: std::future::Future<Output = Result<R, E>>,
    F: Fn(T) -> FO,
    E: std::fmt::Display + std::fmt::Debug,
    Self: std::marker::Sync + std::marker::Send,
{
    pub address: NodeAddress,
    pub nodes: Mutex<BinaryHeap<Reverse<NodeRecord>>>, // Min-heap, so we can pop the node that was least used.
    pub reservation: Arc<AtomicU64>,
    pub busy: Arc<AtomicBool>,
    pub func: Box<F>,
    pub work_timeout: tokio::time::Duration,
    _phantom: std::marker::PhantomData<(T, R)>,
}

impl<T, R, F, FO, E> Worker<T, R, F, FO, E>
where
    T: DeserializeOwned + Serialize + std::marker::Sync + std::marker::Send,
    R: DeserializeOwned + Serialize + std::marker::Sync + std::marker::Send,
    FO: std::future::Future<Output = Result<R, E>> + std::marker::Sync + std::marker::Send,
    F: Fn(T) -> FO,
    E: std::fmt::Display + std::fmt::Debug,
    Self: std::marker::Sync + std::marker::Send,
{
    /// Create a new worker.
    pub fn new(
        host: String,
        port: u16,
        nodes: Vec<NodeAddress>,
        func: F,
        timeout: tokio::time::Duration,
    ) -> Self {
        let nodes_heap =
            BinaryHeap::from_iter(nodes.into_iter().filter_map(|(their_host, their_port)| {
                // Filter out ourselves.
                if (&their_host, &their_port) != (&host, &port) {
                    Some(Reverse((
                        tokio::time::Instant::now(),
                        (their_host, their_port),
                    )))
                } else {
                    None
                }
            }));

        Worker {
            address: (host, port),
            nodes: Mutex::new(nodes_heap),
            reservation: Arc::new(AtomicU64::new(0)),
            busy: Arc::new(AtomicBool::new(false)),
            func: Box::new(func),
            work_timeout: timeout,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Get the name of this worker.
    pub fn name(&self) -> String {
        format!("worker://{}:{}", self.address.0, self.address.1)
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
            .map_err(|err| AntsError::ConnectionError(to.0, to.1, err.to_string()))
    }

    /// Get the next node to use.
    pub async fn next_node(&self) -> Option<NodeAddress> {
        let record = self.nodes.lock().await.pop();
        record.map(|Reverse((_, node))| node)
    }

    /// Get the next node with the duration since the last call.
    pub async fn next_node_with_duration(&self) -> Option<(tokio::time::Duration, NodeAddress)> {
        let record = self.nodes.lock().await.pop();
        record.map(|Reverse((time, node))| (time.elapsed(), node))
    }

    /// Add a node back to the heap.
    pub async fn add_node(&self, node: NodeAddress) {
        self.nodes
            .lock()
            .await
            .push(Reverse((tokio::time::Instant::now(), node)));
    }

    /// Attempt to reserve a node, and return the reservation token if successful.
    pub async fn reserve_node(&self) -> Result<(NodeAddress, u64), AntsError> {
        for tries in 0..RESERVE_ATTEMPTS {
            if let Some((duration, address)) = self.next_node_with_duration().await {
                // DDoS prevention.
                if tries > 0 && duration < RESERVE_TIMEOUT {
                    eprintln!(
                        "The top node was queried {duration:?} ago, waiting for {RESERVE_TIMEOUT:?} before trying again."
                    );
                    tokio::time::sleep(RESERVE_TIMEOUT - duration).await;
                }

                // Put the node back in the heap, but this time with a new timestamp.
                self.add_node(address.clone()).await;

                // this needs to contact the node and reserve it.
                let mut client = match tokio::select! {
                    _ = tokio::time::sleep(RESERVE_TIMEOUT) => {
                        Err(AntsError::ConnectionError(
                            address.0.clone(),
                            address.1,
                            format!("did not respond within {RESERVE_TIMEOUT:?}")
                        ))
                    },
                    connection_result = self.build_client(address.clone()) => {
                        connection_result
                    }
                } {
                    Ok(client) => client,
                    Err(err) => {
                        eprintln!(
                            "Failed to connect to node {}:{} due to {}, trying next node.",
                            &address.0, &address.1, err
                        );
                        continue;
                    }
                };

                let response = client
                    .reserve(Request::new(proto::Empty {}))
                    .await?
                    .into_inner();

                if response.success {
                    return Ok((address, response.token));
                } else {
                    eprintln!(
                        "Node {}:{} is reserved, trying next node.",
                        &address.0, &address.1
                    );
                }
            } else {
                // There is literally no nodes in the heap.
                return Err(AntsError::NoNodesAvailable);
            }
        }

        Err(AntsError::NoNodesAvailable)
    }

    /// Reserve this node.
    pub fn reserve(&self) -> Option<token::ReservationToken> {
        // TODO This function does not know who is reserving it. Change this?
        let token = token::generate_token();
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
                    tokio::time::sleep(token::TIMEOUT).await;

                    if busy_ptr.load(Ordering::Acquire) {
                        eprintln!("Reservation timed out, but the node has started Work; will allow Work to release reservation instead.");
                    } else {
                        match reservation_ptr.compare_exchange(
                            token,
                            0,
                            Ordering::Release,
                            Ordering::Relaxed,
                        ) {
                            Ok(_) => eprintln!("Reservation timed out and released."),
                            Err(_) => eprintln!("Reservation timed out, but was already released."),
                        }
                    }
                });

                eprintln!("Reserved node with token {}.", token);
                Some(token)
            }
            Err(_) => None,
        }
    }

    /// Release this node.
    pub fn release(&self, token: u64) -> Result<token::ReservationToken, AntsError> {
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
                    let result = '_get_result: {
                        let (address, token) = self.reserve_node().await?;

                        let mut client = self.build_client(address.clone()).await?;

                        tokio::select! {
                            _ = tokio::time::sleep(self.work_timeout) => {
                                Err(AntsError::ConnectionError(
                                    address.0.clone(),
                                    address.1,
                                    format!("did not respond within {work_timeout:?}", work_timeout=self.work_timeout)
                                ))
                            },
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
                        }
                    };

                    if let Ok(response) = result {
                        let work_reply = response.into_inner();
                        if work_reply.success {
                            return serde_json::from_str(work_reply.body.as_str())
                                .map_err(|err| AntsError::InvalidWorkResult(err.to_string()))
                                .map(|r| (work_reply.worker, r));
                        } else {
                            eprintln!(
                                "Work failed on node {} due to {}, trying next node.",
                                &work_reply.worker, work_reply.error
                            );
                        }
                    } else {
                        eprintln!(
                            "Work failed on node due to {}, trying next node.",
                            result.err().unwrap()
                        );
                    }
                }
            }
        }
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
    /// Start the worker server to listen.
    pub async fn start(self: &Arc<Self>) -> Result<(), AntsError> {
        eprintln!(
            "Starting RPC server on {}:{}...",
            self.address.0, self.address.1
        );
        tonic::transport::Server::builder()
            .add_service(proto::worker_ant_server::WorkerAntServer::from_arc(
                self.clone(),
            ))
            .serve(std::net::SocketAddr::new(
                std::net::IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0)),
                self.address.1,
            ))
            .await
            .map_err(|err| AntsError::TonicServerStartUpError(err.to_string()))
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
    async fn work(
        &self,
        request: Request<proto::WorkRequest>,
    ) -> Result<Response<proto::WorkReply>, Status> {
        let work_request = request.into_inner();
        let token = work_request.token;
        let parsed_body: Result<T, _> = serde_json::from_str(work_request.body.as_str());

        Ok(Response::new(match parsed_body {
            Ok(body) => {
                let result = self.reserve_and_work(token, body).await;

                match result {
                    Ok(r) => serde_json::to_string(&r)
                        .map(|body| proto::WorkReply {
                            token,
                            success: true,
                            error: "".to_string(),
                            body,
                            worker: self.name(),
                        })
                        .unwrap_or_else(|err| proto::WorkReply {
                            token,
                            success: false,
                            error: AntsError::InvalidWorkResult(err.to_string()).to_string(),
                            body: "".to_string(),
                            worker: self.name(),
                        }),
                    Err(err) => proto::WorkReply {
                        token,
                        success: false,
                        error: AntsError::TaskExecutionError(err.to_string()).to_string(),
                        body: "".to_string(),
                        worker: self.name(),
                    },
                }
            }
            Err(err) => proto::WorkReply {
                token,
                success: false,
                error: AntsError::InvalidWorkData(err.to_string()).to_string(),
                body: "".to_string(),
                worker: self.name(),
            },
        }))
    }
}
