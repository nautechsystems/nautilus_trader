use std::net::{SocketAddr, TcpListener};

use axum::{Router, routing::get, serve};

fn get_unique_port() -> u16 {
    // Create a temporary TcpListener to get an available port
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind temporary TcpListener");
    let port = listener.local_addr().unwrap().port();

    // Close the listener to free up the port
    drop(listener);

    port
}

pub async fn start_positive_stream_http_server()
-> Result<SocketAddr, Box<dyn std::error::Error + Send + Sync>> {
    let port = get_unique_port();
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}"))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        serve(listener, create_positive_stream_router())
            .await
            .unwrap();
    });

    Ok(addr)
}

fn create_positive_stream_router() -> Router {
    // Create a counter state that will be shared across requests
    let counter = std::sync::Arc::new(std::sync::atomic::AtomicI32::new(0));

    // Clone the counter for the handler
    let counter_clone = counter.clone();
    let counter_clone_2 = counter;

    Router::new()
        .route(
            "/get",
            get(async move || {
                // Increment the counter and return the new value
                let value = counter_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                format!("{value}")
            }),
        )
        .route(
            "/skip",
            get(async move || {
                // Increment the counter and return the new value
                let value = counter_clone_2.fetch_add(5, std::sync::atomic::Ordering::SeqCst);
                format!("{value}")
            }),
        )
}
