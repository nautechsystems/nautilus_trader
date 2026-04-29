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

//! Turmoil integration tests for the sockudo `WebSocketClient` backend.
//!
//! Mirrors `turmoil_websocket.rs` but selects [`TransportBackend::Sockudo`] so the
//! sockudo handshake helpers and adapter are exercised over a turmoil
//! `TcpStream`. The server side uses tungstenite's `accept_async`: both backends
//! speak the same wire protocol, and sockudo only ships a client API.

#![cfg(all(feature = "turmoil", feature = "transport-sockudo"))]

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use nautilus_network::{
    RECONNECTED,
    websocket::{TransportBackend, WebSocketClient, WebSocketConfig, channel_message_handler},
};
use rstest::{fixture, rstest};
use tokio_tungstenite::{accept_async, tungstenite::Message};
use turmoil::{Builder, net};

const POLL_ITERS: u32 = 200;
const POLL_STEP: Duration = Duration::from_millis(10);

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
        backend: TransportBackend::Sockudo,
        proxy_url: None,
    }
}

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
fn test_turmoil_real_sockudo_basic_connect(websocket_config: WebSocketConfig) {
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
fn test_turmoil_real_sockudo_reconnection(mut websocket_config: WebSocketConfig) {
    websocket_config.reconnect_timeout_ms = Some(5_000);
    websocket_config.reconnect_delay_initial_ms = Some(100);

    let mut sim = Builder::new().build();

    sim.host("server", || async {
        let listener = net::TcpListener::bind("0.0.0.0:8080").await?;

        if let Ok((stream, _)) = listener.accept().await
            && let Ok(mut ws) = accept_async(stream).await
        {
            let _ = ws.send(Message::Text("first".to_string().into())).await;
            drop(ws);
        }

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
fn test_turmoil_real_sockudo_network_partition(mut websocket_config: WebSocketConfig) {
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

/// The Sockudo backend rejects `proxy_url` outright (proxying is only wired
/// for the tungstenite path); the simulator must surface that uniformly so
/// callers see the gap immediately.
#[rstest]
fn test_turmoil_sockudo_rejects_proxy_url(mut websocket_config: WebSocketConfig) {
    websocket_config.proxy_url = Some("http://proxy:9999".to_string());

    let mut sim = Builder::new().build();
    sim.host("server", ws_echo_server);
    sim.client("client", async move {
        let (handler, _rx) = channel_message_handler();
        let err =
            WebSocketClient::connect(websocket_config, Some(handler), None, None, vec![], None)
                .await
                .expect_err("sockudo should reject proxy_url");
        let msg = err.to_string();
        assert!(
            msg.contains("proxy_url is not supported"),
            "expected proxy rejection error, was {msg:?}"
        );
        Ok(())
    });

    sim.run().unwrap();
}

/// `wss://` cannot be modelled under the simulator (turmoil has no TLS), so
/// the sockudo backend must reject it up front rather than failing later in
/// the handshake.
#[rstest]
fn test_turmoil_sockudo_rejects_wss(mut websocket_config: WebSocketConfig) {
    websocket_config.url = "wss://server:8443".to_string();

    let mut sim = Builder::new().build();
    sim.client("client", async move {
        let (handler, _rx) = channel_message_handler();
        let err =
            WebSocketClient::connect(websocket_config, Some(handler), None, None, vec![], None)
                .await
                .expect_err("turmoil should reject wss");
        let msg = err.to_string();
        assert!(
            msg.contains("turmoil") || msg.contains("wss"),
            "expected turmoil-specific error, was {msg:?}"
        );
        Ok(())
    });

    sim.run().unwrap();
}
