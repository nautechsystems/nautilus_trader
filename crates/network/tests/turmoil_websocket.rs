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

//! Turmoil integration tests for the WebSocketClient.
//!
//! These tests use turmoil's network simulation to test the actual production
//! WebSocketClient code under various network conditions.

#![cfg(feature = "turmoil")]

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use nautilus_network::websocket::{WebSocketClient, WebSocketConfig, channel_message_handler};
use rstest::{fixture, rstest};
use tokio_tungstenite::{accept_async, tungstenite::Message};
use turmoil::{Builder, net};

/// Default test WebSocket configuration.
#[fixture]
fn websocket_config() -> WebSocketConfig {
    WebSocketConfig {
        url: "ws://server:8080".to_string(),
        headers: vec![],
        message_handler: None,
        heartbeat: None,
        heartbeat_msg: None,
        ping_handler: None,
        reconnect_timeout_ms: Some(2_000),
        reconnect_delay_initial_ms: Some(50),
        reconnect_delay_max_ms: Some(500),
        reconnect_backoff_factor: Some(1.5),
        reconnect_jitter_ms: Some(10),
    }
}

/// WebSocket echo server for testing.
async fn ws_echo_server() -> Result<(), Box<dyn std::error::Error>> {
    let listener = net::TcpListener::bind("0.0.0.0:8080").await?;

    loop {
        let (stream, _) = listener.accept().await?;

        tokio::spawn(async move {
            if let Ok(mut ws_stream) = accept_async(stream).await {
                while let Some(msg) = ws_stream.next().await {
                    match msg {
                        Ok(Message::Text(text)) => {
                            if text == "close_me" {
                                let _ = ws_stream.close(None).await;
                                break;
                            }
                            let _ = ws_stream.send(Message::Text(text)).await;
                        }
                        Ok(Message::Binary(data)) => {
                            let _ = ws_stream.send(Message::Binary(data)).await;
                        }
                        Ok(Message::Ping(ping_data)) => {
                            let _ = ws_stream.send(Message::Pong(ping_data)).await;
                        }
                        Ok(Message::Close(_)) => {
                            let _ = ws_stream.close(None).await;
                            break;
                        }
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
            }
        });
    }
}

#[rstest]
fn test_turmoil_real_websocket_basic_connect(websocket_config: WebSocketConfig) {
    let mut sim = Builder::new().build();

    sim.host("server", ws_echo_server);

    sim.client("client", async move {
        let (handler, mut rx) = channel_message_handler();
        let config = WebSocketConfig {
            message_handler: Some(handler),
            ..websocket_config
        };

        let client = WebSocketClient::connect(config, None, vec![], None)
            .await
            .expect("Should connect");

        // Verify client is active
        assert!(client.is_active(), "Client should be active after connect");

        // Send a test message
        client
            .send_text("hello".to_string(), None)
            .await
            .expect("Should send text");

        // Wait for echo
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Check we received the echo
        if let Ok(msg) = rx.try_recv() {
            assert!(matches!(msg, Message::Text(ref text) if text.as_str() == "hello"));
        }

        // Close the client
        client.disconnect().await;
        assert!(client.is_disconnected(), "Client should be disconnected");

        Ok(())
    });

    sim.run().unwrap();
}

#[rstest]
fn test_turmoil_real_websocket_reconnection(mut websocket_config: WebSocketConfig) {
    websocket_config.reconnect_timeout_ms = Some(5_000);
    websocket_config.reconnect_delay_initial_ms = Some(100);

    let mut sim = Builder::new().build();

    // Server that accepts one connection, closes it, then accepts another
    sim.host("server", || async {
        let listener = net::TcpListener::bind("0.0.0.0:8080").await?;

        // Accept first connection
        if let Ok((stream, _)) = listener.accept().await
            && let Ok(mut ws) = accept_async(stream).await
        {
            // Send one message then close
            let _ = ws.send(Message::Text("first".to_string().into())).await;
            drop(ws);
        }

        // Wait a bit before accepting second connection
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Accept second connection and run echo server
        if let Ok((stream, _)) = listener.accept().await
            && let Ok(mut ws) = accept_async(stream).await
        {
            while let Some(msg) = ws.next().await {
                match msg {
                    Ok(Message::Text(text)) if text == "close_me" => break,
                    Ok(msg) => {
                        if ws.send(msg).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        }

        Ok::<(), Box<dyn std::error::Error>>(())
    });

    sim.client("client", async move {
        let (handler, mut rx) = channel_message_handler();
        let config = WebSocketConfig {
            message_handler: Some(handler),
            ..websocket_config
        };

        let client = WebSocketClient::connect(config, None, vec![], None)
            .await
            .expect("Should connect");

        // Wait to receive first message
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Wait for server to close connection and reconnection
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Send another message after reconnection
        client
            .send_text("second_msg".to_string(), None)
            .await
            .expect("Should send after reconnect");

        // Wait for echo
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Check we received messages
        let mut received_second = false;
        while let Ok(msg) = rx.try_recv() {
            if matches!(msg, Message::Text(ref text) if text.as_str() == "second_msg") {
                received_second = true;
            }
        }
        assert!(received_second, "Should receive echoed second message");

        // Close
        client.disconnect().await;

        Ok(())
    });

    sim.run().unwrap();
}

#[rstest]
fn test_turmoil_real_websocket_network_partition(mut websocket_config: WebSocketConfig) {
    websocket_config.reconnect_timeout_ms = Some(3_000);

    let mut sim = Builder::new().build();

    sim.host("server", ws_echo_server);

    sim.client("client", async move {
        let (handler, mut rx) = channel_message_handler();
        let config = WebSocketConfig {
            message_handler: Some(handler),
            ..websocket_config
        };

        let client = WebSocketClient::connect(config, None, vec![], None)
            .await
            .expect("Should connect");

        // Send message before partition
        client
            .send_text("before_partition".to_string(), None)
            .await
            .expect("Should send before partition");

        tokio::time::sleep(Duration::from_millis(100)).await;

        // Create network partition
        turmoil::partition("client", "server");

        // Wait a bit
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Repair partition
        turmoil::repair("client", "server");

        // Wait for reconnection
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Should be able to send after repair
        client
            .send_text("after_partition".to_string(), None)
            .await
            .expect("Should send after partition repair");

        tokio::time::sleep(Duration::from_millis(100)).await;

        // Check we received messages
        let mut received_after = false;
        while let Ok(msg) = rx.try_recv() {
            if matches!(msg, Message::Text(ref text) if text.as_str() == "after_partition") {
                received_after = true;
            }
        }
        assert!(received_after, "Should receive message after partition");

        // Close
        client.disconnect().await;

        Ok(())
    });

    sim.run().unwrap();
}
