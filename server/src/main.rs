mod http;
mod service;
mod state;

use axum::routing::any;
use axum::Router;
use protogen::pb::relay_service_server::RelayServiceServer;
use service::Relay;
use state::AppState;

fn listen_port(name: &str, default: u16) -> u16 {
    let Ok(raw) = std::env::var(name) else {
        return default;
    };
    match raw.parse::<u16>() {
        Ok(port) if port != 0 => port,
        _ => {
            eprintln!("{name} must be a port from 1 to 65535, got {raw}");
            std::process::exit(1);
        }
    }
}

#[tokio::main]
async fn main() {
    let http_port = listen_port("HTTP_PORT", 8080);
    let grpc_port = listen_port("GRPC_PORT", 50051);

    let state = AppState::default();
    let relay = Relay {
        state: state.clone(),
    };

    // HTTP gateway: every route falls through to the handler.
    // `/*rest` is the axum 0.7 wildcard; fallback covers `/`.
    let app = Router::new()
        .route("/*rest", any(http::handle))
        .fallback(any(http::handle))
        .with_state(state);

    let http_task = tokio::spawn(async move {
        let http_addr = format!("0.0.0.0:{http_port}");
        let listener = tokio::net::TcpListener::bind(&http_addr).await.unwrap();
        println!("HTTP gateway listening on {http_addr}");
        axum::serve(listener, app).await.unwrap();
    });

    let grpc_task = tokio::spawn(async move {
        let grpc_addr = std::net::SocketAddr::from(([0, 0, 0, 0], grpc_port));
        println!("gRPC broadcast service listening on {grpc_addr}");
        tonic::transport::Server::builder()
            .add_service(RelayServiceServer::new(relay))
            .serve(grpc_addr)
            .await
            .unwrap();
    });

    let _ = tokio::join!(http_task, grpc_task);
}
