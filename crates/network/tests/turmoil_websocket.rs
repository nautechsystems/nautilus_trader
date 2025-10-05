// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Turmoil-compatible WebSocket tests with dependency injection.
//!
//! This demonstrates how to make WebSocket components work with turmoil
//! through dependency injection of network types.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use nautilus_network::net::TcpConnector;
use rstest::rstest;
use tokio_tungstenite::{WebSocketStream, accept_async, client_async, tungstenite::Message};
use turmoil::{Builder, net};

/// Turmoil TCP connector for testing.
#[derive(Default, Clone, Debug)]
pub struct TurmoilTcpConnector;

impl TcpConnector for TurmoilTcpConnector {
    type Stream = turmoil::net::TcpStream;

    fn connect(
        &self,
        addr: &str,
    ) -> impl std::future::Future<Output = std::io::Result<Self::Stream>> + Send {
        turmoil::net::TcpStream::connect(addr.to_string())
    }
}

/// A test-specific WebSocket client that uses dependency injection for networking.
struct TestWebSocketClient {
    connector: TurmoilTcpConnector,
}

impl TestWebSocketClient {
    const fn new(connector: TurmoilTcpConnector) -> Self {
        Self { connector }
    }

    async fn connect(
        &self,
        addr: &str,
    ) -> Result<WebSocketStream<turmoil::net::TcpStream>, Box<dyn std::error::Error>> {
        let stream = self.connector.connect(addr).await?;
        let url = format!("ws://{addr}/");
        let (ws_stream, _response) = client_async(url, stream).await?;
        Ok(ws_stream)
    }
}

/// WebSocket echo server that responds to messages.
async fn ws_echo_server() -> Result<(), Box<dyn std::error::Error>> {
    let listener = net::TcpListener::bind("0.0.0.0:8080").await?;

    loop {
        let (stream, _) = listener.accept().await?;

        tokio::spawn(async move {
            if let Ok(ws_stream) = accept_async(stream).await {
                let (mut ws_sender, mut ws_receiver) = ws_stream.split();

                while let Some(msg) = ws_receiver.next().await {
                    match msg {
                        Ok(Message::Text(text)) => {
                            if text == "close_me" {
                                let _ = ws_sender.close().await;
                                break;
                            }
                            let _ = ws_sender.send(Message::Text(text)).await;
                        }
                        Ok(Message::Binary(data)) => {
                            let _ = ws_sender.send(Message::Binary(data)).await;
                        }
                        Ok(Message::Ping(ping_data)) => {
                            let _ = ws_sender.send(Message::Pong(ping_data)).await;
                        }
                        Ok(Message::Close(_)) => {
                            let _ = ws_sender.close().await;
                            break;
                        }
                        Ok(_) => {} // Ignore other message types
                        Err(_) => break,
                    }
                }
            } else {
                // WebSocket handshake failed
            }
        });
    }
}

#[rstest]
fn test_turmoil_websocket_with_dependency_injection() {
    let mut sim = Builder::new().build();

    sim.host("server", ws_echo_server);

    sim.client("client", async {
        let client = TestWebSocketClient::new(TurmoilTcpConnector);

        // Try to connect to the server
        let mut ws_stream = client
            .connect("server:8080")
            .await
            .expect("WebSocket handshake should succeed");

        ws_stream
            .send(Message::Text("hello turmoil".into()))
            .await
            .expect("send text");
        let echo = ws_stream
            .next()
            .await
            .expect("expected echo")
            .expect("websocket frame");
        assert_eq!(echo, Message::Text("hello turmoil".into()));

        ws_stream
            .send(Message::Binary(b"abc".to_vec().into()))
            .await
            .expect("send binary");
        let binary_echo = ws_stream
            .next()
            .await
            .expect("expected binary echo")
            .expect("websocket frame");
        assert_eq!(binary_echo, Message::Binary(b"abc".to_vec().into()));

        ws_stream
            .send(Message::Ping(b"ping".to_vec().into()))
            .await
            .expect("send ping");
        let pong = ws_stream
            .next()
            .await
            .expect("expected pong")
            .expect("websocket frame");
        assert_eq!(pong, Message::Pong(b"ping".to_vec().into()));

        Ok(())
    });

    sim.run().unwrap();
}

#[rstest]
fn test_turmoil_websocket_network_partition() {
    let mut sim = Builder::new().build();

    sim.host("server", ws_echo_server);

    sim.client("client", async {
        let client = TestWebSocketClient::new(TurmoilTcpConnector);

        // Initial connection succeeds and can exchange a message
        let mut initial_stream = client
            .connect("server:8080")
            .await
            .expect("initial connect");
        initial_stream
            .send(Message::Text("before_partition".into()))
            .await
            .expect("send before partition");
        let reply = initial_stream
            .next()
            .await
            .expect("echo before partition")
            .expect("frame");
        assert_eq!(reply, Message::Text("before_partition".into()));
        drop(initial_stream);

        tokio::time::sleep(Duration::from_millis(50)).await;

        // Create network partition
        turmoil::partition("client", "server");

        // Connection should fail during partition
        let partition_result = client.connect("server:8080").await;
        assert!(
            partition_result.is_err(),
            "Connection should fail during partition"
        );

        tokio::time::sleep(Duration::from_millis(200)).await;

        // Repair partition
        turmoil::repair("client", "server");

        // Wait for network to stabilize
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Should be able to attempt connection again after repair
        let mut repaired_stream = client
            .connect("server:8080")
            .await
            .expect("connect after repair");
        repaired_stream
            .send(Message::Text("after_partition".into()))
            .await
            .expect("send after repair");
        let repaired_reply = repaired_stream
            .next()
            .await
            .expect("echo after repair")
            .expect("frame");
        assert_eq!(repaired_reply, Message::Text("after_partition".into()));
        drop(repaired_stream);

        Ok(())
    });

    sim.run().unwrap();
}
