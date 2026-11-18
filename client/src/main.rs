use protogen::pb::relay_service_client::RelayServiceClient;
use protogen::pb::{HttpHeader, HttpRequestMsg, HttpResponseMsg};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let target = std::env::var("TARGET_URL")
        .expect("set TARGET_URL, e.g. TARGET_URL=http://target-api:9000");
    let server_addr =
        std::env::var("SERVER_ADDR").unwrap_or_else(|_| "http://127.0.0.1:50051".into());

    let mut client = RelayServiceClient::connect(server_addr).await?;

    // Outbound channel: responses to the server go through here.
    // Tonic expects a stream of HttpResponseMsg, not Result.
    let (tx, rx) = mpsc::channel::<HttpResponseMsg>(32);

    let mut incoming = client.forward(ReceiverStream::new(rx)).await?.into_inner();

    println!("connected to server; forwarding requests to {target}");

    // Consume the HTTP requests broadcast by the server and forward them.
    // Each client receives EVERY request; this worker answers each one
    // (here: always forwards to its TARGET_URL and replies).
    while let Some(req) = incoming.message().await? {
        let target = target.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            let resp = forward_to_api(&target, req).await;
            let _ = tx.send(resp).await;
        });
    }

    Ok(())
}

async fn forward_to_api(target: &str, req: HttpRequestMsg) -> HttpResponseMsg {
    let id = req.id;
    let method = reqwest::Method::from_bytes(req.method.to_uppercase().as_bytes())
        .unwrap_or(reqwest::Method::GET);
    let url = format!("{}{}", target.trim_end_matches('/'), req.path);

    let mut request = reqwest::Client::new().request(method, &url);
    for h in &req.headers {
        // Hop-by-hop: reqwest sets these itself and rejects duplicates.
        if matches!(
            h.key.as_str(),
            "host" | "content-length" | "transfer-encoding" | "connection"
        ) {
            continue;
        }
        request = request.header(h.key.as_str(), h.value.as_str());
    }
    if !req.body.is_empty() {
        request = request.body(req.body);
    }

    match request.send().await {
        Ok(resp) => {
            let status = resp.status().as_u16() as u32;
            let headers: Vec<HttpHeader> = resp
                .headers()
                .iter()
                .filter_map(|(k, v)| {
                    Some(HttpHeader {
                        key: k.as_str().to_string(),
                        value: v.to_str().ok()?.to_string(),
                    })
                })
                .collect();
            let body = resp.bytes().await.map(|b| b.to_vec()).unwrap_or_default();
            HttpResponseMsg {
                id,
                status,
                headers,
                body,
            }
        }
        Err(_) => HttpResponseMsg {
            id,
            status: 502,
            headers: vec![],
            body: b"the client could not reach the target API".to_vec(),
        },
    }
}
