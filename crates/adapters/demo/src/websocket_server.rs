use futures::SinkExt;
use futures::StreamExt;
use tokio::task;
use tokio::time::Duration;

struct NegativeStreamServer {
    task: tokio::task::JoinHandle<()>,
    port: u16,
}

impl NegativeStreamServer {
    async fn setup() -> Self {
        let server = tokio::net::TcpListener::bind(format!("127.0.0.1:0"))
            .await
            .unwrap();
        let port = server.local_addr().unwrap().port();

        let task = task::spawn(async move {
            // Keep accepting connections
            loop {
                let (conn, _) = server.accept().await.unwrap();
                let websocket = tokio_tungstenite::accept_async(conn).await.unwrap();
                let (mut sender, mut receiver) = websocket.split();

                // Create a counter for negative values
                let counter = std::sync::Arc::new(std::sync::atomic::AtomicI32::new(0));
                let counter_clone = counter.clone();
                let counter_clone_2 = counter.clone();

                // Task to send negative numbers every second
                let sender_task = task::spawn(async move {
                    loop {
                        let value = counter_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        let message = tokio_tungstenite::tungstenite::protocol::Message::Text(
                            format!("{}", -value).into(),
                        );

                        if let Err(err) = sender.send(message).await {
                            eprintln!("Error sending message: {}", err);
                            break;
                        }

                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                });

                // Task to handle incoming messages
                task::spawn(async move {
                    while let Some(Ok(msg)) = receiver.next().await {
                        match msg {
                            tokio_tungstenite::tungstenite::protocol::Message::Text(txt) => {
                                if txt == "SKIP" {
                                    counter_clone_2
                                        .fetch_add(5, std::sync::atomic::Ordering::SeqCst);
                                } else if txt == "close-now" {
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }

                    // Cancel the sender task when we're done
                    sender_task.abort();
                });
            }
        });

        Self { task, port }
    }
}

impl Drop for NegativeStreamServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}
