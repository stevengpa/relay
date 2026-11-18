use protogen::pb::relay_service_server::RelayService;
use protogen::pb::{HttpRequestMsg, HttpResponseMsg};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status, Streaming};

use crate::state::AppState;

pub struct Relay {
    pub state: AppState,
}

#[tonic::async_trait]
impl RelayService for Relay {
    // The server emits HttpRequestMsg (the proto: returns stream HttpRequestMsg).
    type ForwardStream = ReceiverStream<Result<HttpRequestMsg, Status>>;

    /// Each gRPC client opens this bidirectional stream (its own connection).
    /// Out: HttpRequestMsg broadcast to every client.
    /// In: HttpResponseMsg correlated by id with the HTTP caller.
    async fn forward(
        &self,
        request: Request<Streaming<HttpResponseMsg>>,
    ) -> Result<Response<Self::ForwardStream>, Status> {
        let mut incoming = request.into_inner();
        let (tx, rx) = mpsc::channel::<Result<HttpRequestMsg, Status>>(32);

        // Register this client as a broadcast recipient
        self.state.clients.lock().await.push(tx.clone());

        // Task that reads the client's responses and completes the oneshots
        let state = self.state.clone();
        tokio::spawn(async move {
            while let Ok(Some(resp)) = incoming.message().await {
                // Only the FIRST responder for a given id gets the sender;
                // duplicate responses (from other clients) are discarded.
                let sender = state.pending.lock().await.remove(&resp.id);
                if let Some(sender) = sender {
                    let _ = sender.send(Ok(resp));
                }
            }
            // Client disconnected: remove its channel from the broadcast registry
            state.clients.lock().await.retain(|c| !c.same_channel(&tx));
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }
}
