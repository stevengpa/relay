use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use protogen::pb::{HttpHeader, HttpRequestMsg};
use std::sync::atomic::Ordering;

use crate::state::AppState;

/// Gateway: capture any HTTP request and broadcast it
/// to every connected gRPC client.
pub async fn handle(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    // 1. Snapshot of live clients (drop closed channels on the fly)
    let clients = {
        let mut clients = state.clients.lock().await;
        clients.retain(|c| !c.is_closed());
        clients.clone()
    };

    if clients.is_empty() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "no gRPC clients connected",
        )
            .into_response();
    }

    // 2. Register the pending response
    let id = state.next_id.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = tokio::sync::oneshot::channel();
    state.pending.lock().await.insert(id, tx);

    // 3. Serialize the HTTP request and broadcast it to every client
    let msg = HttpRequestMsg {
        id,
        method: method.as_str().to_string(),
        path: uri
            .path_and_query()
            .map(|pq| pq.as_str().to_string())
            .unwrap_or_else(|| uri.path().to_string()),
        headers: headers
            .iter()
            .filter_map(|(k, v)| {
                Some(HttpHeader {
                    key: k.as_str().to_string(),
                    value: v.to_str().ok()?.to_string(),
                })
            })
            .collect(),
        body: body.to_vec(),
    };

    for client in &clients {
        // send consumes the value, so clone per recipient.
        // A failed send does not abort the broadcast:
        // that client simply will not take part (its channel was already
        // closed, or the cleanup task in service.rs will drop it).
        if client.send(Ok(msg.clone())).await.is_err() {
            eprintln!("broadcast: failed to send to a client (disconnected)");
        }
    }

    // 4. Wait for the FIRST response that comes back on any gRPC stream
    match rx.await {
        Ok(Ok(resp)) => {
            let status = StatusCode::from_u16(resp.status as u16)
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            let mut builder = Response::builder().status(status);
            for h in &resp.headers {
                builder = builder.header(h.key.as_str(), h.value.as_str());
            }
            builder.body(axum::body::Body::from(resp.body)).unwrap()
        }
        _ => (StatusCode::BAD_GATEWAY, "no client returned a response").into_response(),
    }
}
