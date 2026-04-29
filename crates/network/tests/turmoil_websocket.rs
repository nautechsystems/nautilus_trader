// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

//! Turmoil integration tests for the `WebSocketClient`.
//!
//! These tests use turmoil's network simulation to test the actual production
//! `WebSocketClient` code under various network conditions.

#![cfg(feature = "turmoil")]

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use nautilus_network::{
    RECONNECTED,
    websocket::{TransportBackend, WebSocketClient, WebSocketConfig, channel_message_handler},
};
use rstest::{fixture, rstest};
use tokio_tungstenite::{accept_async, tungstenite::Message};
use turmoil::{Builder, net};

// 2-second budget in simulated time, covering reconnect timings across these tests.
const POLL_ITERS: u32 = 200;
const POLL_STEP: Duration = Duration::from_millis(10);

// Small sleep steps advance turmoil's simulated clock so the receiver drains
// between ticks instead of relying on a single fixed wait.
async fn recv_text(rx: &mut tokio::sync::mpsc::UnboundedReceiver<Message>, expected: &str) -> bool {
    for _ in 0..POLL_ITERS {
        while let Ok(msg) = rx.try_recv() {
            if matches!(&msg, Message::Text(text) if text.as_str() == expected) {
                return true;
            }
        }
        tokio::time::sleep(POLL_STEP).await;
    }
    false
}

async fn wait_for<F>(mut condition: F) -> bool
where
    F: FnMut() -> bool,
{
    for _ in 0..POLL_ITERS {
        if condition() {
            return true;
        }
        tokio::time::sleep(POLL_STEP).await;
    }
    false
}

/// Default test WebSocket configuration.
#[fixture]
fn websocket_config() -> WebSocketConfig {
    WebSocketConfig {
        url: "ws://server:8080".to_string(),
        headers: vec![],
        heartbeat: None,
        heartbeat_msg: None,
        reconnect_timeout_ms: Some(2_000),
        reconnect_delay_initial_ms: Some(50),
        reconnect_delay_max_ms: Some(500),
        reconnect_backoff_factor: Some(1.5),
        reconnect_jitter_ms: Some(10),
        reconnect_max_attempts: None,
        idle_timeout_ms: None,
        backend: TransportBackend::Tungstenite,
        proxy_url: None,
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

        let client =
            WebSocketClient::connect(websocket_config, Some(handler), None, None, vec![], None)
                .await
                .expect("Should connect");

        assert!(client.is_active(), "Client should be active after connect");

        client
            .send_text("hello".to_string(), None)
            .await
            .expect("Should send text");

        assert!(
            recv_text(&mut rx, "hello").await,
            "Should receive echoed hello"
        );

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
            let _ = ws.send(Message::Text("first".to_string().into())).await;
            drop(ws);
        }

        // Accept second connection and run echo loop
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

        let client =
            WebSocketClient::connect(websocket_config, Some(handler), None, None, vec![], None)
                .await
                .expect("Should connect");

        assert!(
            recv_text(&mut rx, "first").await,
            "Should receive first message before server closes"
        );

        // Server drop triggers reconnect; the client emits `RECONNECTED` on the
        // message channel once the new connection is fully established.
        assert!(
            recv_text(&mut rx, RECONNECTED).await,
            "Client should emit RECONNECTED after server close"
        );

        client
            .send_text("second_msg".to_string(), None)
            .await
            .expect("Should send after reconnect");

        assert!(
            recv_text(&mut rx, "second_msg").await,
            "Should receive echoed second message"
        );

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

        let client =
            WebSocketClient::connect(websocket_config, Some(handler), None, None, vec![], None)
                .await
                .expect("Should connect");

        client
            .send_text("before_partition".to_string(), None)
            .await
            .expect("Should send before partition");

        assert!(
            recv_text(&mut rx, "before_partition").await,
            "Should receive echoed before_partition"
        );

        turmoil::partition("client", "server");
        tokio::time::sleep(Duration::from_millis(200)).await;
        turmoil::repair("client", "server");

        // Either the connection survived the partition or reconnect restored it;
        // poll until the client is active again before sending.
        assert!(
            wait_for(|| client.is_active()).await,
            "Client should be active after partition repair"
        );

        client
            .send_text("after_partition".to_string(), None)
            .await
            .expect("Should send after partition repair");

        assert!(
            recv_text(&mut rx, "after_partition").await,
            "Should receive echoed after_partition"
        );

        client.disconnect().await;

        Ok(())
    });

    sim.run().unwrap();
}

#[rstest]
fn test_turmoil_real_websocket_disconnect_during_reconnect(mut websocket_config: WebSocketConfig) {
    websocket_config.reconnect_timeout_ms = Some(5_000);
    websocket_config.reconnect_delay_initial_ms = Some(100);

    let mut sim = Builder::new().build();

    sim.host("server", ws_echo_server);

    sim.client("client", async move {
        let (handler, _rx) = channel_message_handler();

        let client =
            WebSocketClient::connect(websocket_config, Some(handler), None, None, vec![], None)
                .await
                .expect("Should connect");

        assert!(client.is_active(), "Client should be active after connect");

        turmoil::partition("client", "server");
        tokio::time::sleep(Duration::from_millis(200)).await;

        client.disconnect().await;

        assert!(
            client.is_disconnected(),
            "Client should be disconnected after disconnect during reconnect"
        );
        assert!(
            !client.is_active(),
            "Client should not be active after disconnect"
        );

        Ok(())
    });

    sim.run().unwrap();
}

#[rstest]
fn test_turmoil_real_websocket_disconnect_during_backoff(mut websocket_config: WebSocketConfig) {
    websocket_config.reconnect_timeout_ms = Some(1_000);
    websocket_config.reconnect_delay_initial_ms = Some(10_000); // Long backoff
    websocket_config.reconnect_delay_max_ms = Some(10_000);
    websocket_config.reconnect_backoff_factor = Some(1.0);
    websocket_config.reconnect_jitter_ms = Some(0);

    let mut sim = Builder::new()
        .simulation_duration(Duration::from_secs(30))
        .build();

    sim.host("server", ws_echo_server);

    sim.client("client", async move {
        let (handler, _rx) = channel_message_handler();

        let client =
            WebSocketClient::connect(websocket_config, Some(handler), None, None, vec![], None)
                .await
                .expect("Should connect");

        assert!(client.is_active());

        // Partition to force reconnect
        turmoil::partition("client", "server");
        tokio::time::sleep(Duration::from_millis(300)).await;

        // Client should be reconnecting; reconnect attempt fails, enters 10s backoff
        tokio::time::sleep(Duration::from_millis(1_500)).await;

        let start = tokio::time::Instant::now();
        client.disconnect().await;
        let elapsed = start.elapsed();

        assert!(client.is_disconnected(), "Client should be disconnected");
        assert!(
            elapsed < Duration::from_secs(3),
            "Disconnect should interrupt backoff, took {elapsed:?}"
        );

        Ok(())
    });

    sim.run().unwrap();
}

/// HTTP `CONNECT` proxy tunneling cannot be modelled in the turmoil
/// simulator (no `tokio-tungstenite` adapter for the proxy hop). The
/// simulator-specific stub must reject `proxy_url` clearly so callers see
/// the gap immediately rather than silently bypassing the proxy.
#[rstest]
fn test_turmoil_websocket_rejects_proxy_url(mut websocket_config: WebSocketConfig) {
    websocket_config.proxy_url = Some("http://proxy:9999".to_string());

    let mut sim = Builder::new().build();
    sim.host("server", ws_echo_server);
    sim.client("client", async move {
        let (handler, _rx) = channel_message_handler();
        let err =
            WebSocketClient::connect(websocket_config, Some(handler), None, None, vec![], None)
                .await
                .expect_err("turmoil should reject proxy_url");
        let msg = err.to_string();
        assert!(
            msg.contains("turmoil"),
            "expected turmoil-specific error, was {msg:?}"
        );
        Ok(())
    });

    sim.run().unwrap();
}
