use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use protogen::pb::{HttpRequestMsg, HttpResponseMsg};
use tokio::sync::{mpsc, oneshot, Mutex};
use tonic::Status;

/// Shared state between the HTTP gateway and the gRPC service.
#[derive(Clone, Default)]
pub struct AppState {
    /// In-flight HTTP requests, by id → channel that returns the response.
    /// With broadcast, the FIRST client to answer that id consumes it
    /// (the `oneshot` is removed from the map) and later responses are dropped.
    pub pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Result<HttpResponseMsg, Status>>>>>,
    /// One outbound channel per connected gRPC client.
    /// Each HTTP request is SENT TO ALL of them (broadcast).
    ///
    /// The item is `Result<_, Status>` because Tonic's outbound stream
    /// requires that type; the gateway always sends `Ok`.
    pub clients: Arc<Mutex<Vec<mpsc::Sender<Result<HttpRequestMsg, Status>>>>>,
    /// Unique ids used to correlate a request with its response.
    pub next_id: Arc<AtomicU64>,
}
