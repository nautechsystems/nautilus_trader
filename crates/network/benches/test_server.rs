use axum::{Router, routing::get};

#[tokio::main]
async fn main() {
    // Construct our SocketAddr to listen on...
    let router = Router::new().route("/", get(|| async { "Hello World" }));

    // Create a listener and serve...
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    let server = axum::serve(listener, router);

    // And run forever...
    if let Err(e) = server.await {
        eprintln!("server error: {e}");
    }
}
