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

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use futures_util::{SinkExt, StreamExt};
use nautilus_network::{
    Message as NetworkMessage, RECONNECTED,
    error::SendError,
    ratelimiter::RateLimiter,
    websocket::{
        AuthTracker, MessageHandler, TransportBackend, WebSocketClient, WebSocketConfig,
        channel_epoch_message_handler, channel_message_handler,
    },
};
use parking_lot::Mutex;
use rstest::{fixture, rstest};
use tokio_tungstenite::{accept_async, tungstenite::Message};
use turmoil::net;

use crate::common::turmoil::{
    log_soak_seed, seed_sweep_from_env, seeded_builder, seeded_builder_with_duration,
    stressed_builder,
};

// 2-second budget in simulated time, covering reconnect timings across these tests.
const POLL_ITERS: u32 = 200;
const POLL_STEP: Duration = Duration::from_millis(10);
const BASIC_CONNECT_SEED: u64 = 0x57EB_0001;
const RECONNECTION_SEED: u64 = 0x57EB_0002;
const NETWORK_PARTITION_SEED: u64 = 0x57EB_0003;
const DISCONNECT_DURING_RECONNECT_SEED: u64 = 0x57EB_0004;
const DISCONNECT_DURING_BACKOFF_SEED: u64 = 0x57EB_0005;
const PROXY_REJECTION_SEED: u64 = 0x57EB_0006;
const CONNECTION_EPOCH_RECONNECTION_SEED: u64 = 0x57EB_0007;
const QUEUED_WRITE_DROP_SEED: u64 = 0x57EB_2001;
const RECONNECT_ACTIVE_DROP_SEED: u64 = 0x57EB_2002;
const ALTERNATING_TEXT_BINARY_SEED: u64 = 0x57EB_2003;
const HANDSHAKE_DROP_SEED: u64 = 0x57EB_3001;
const FIRST_READ_TASK_DROP_SEED: u64 = 0x57EB_3002;
const PARTITION_DURING_RECONNECT_SEED: u64 = 0x57EB_3003;
const PARTITION_DURING_BACKOFF_SEED: u64 = 0x57EB_3004;
const SILENT_UNTIL_IDLE_TIMEOUT_SEED: u64 = 0x57EB_3005;
const NO_READ_BACKPRESSURE_SEED: u64 = 0x57EB_3006;
const DISCONNECT_WHILE_SEND_WAITS_FOR_RECONNECT_SEED: u64 = 0x57EB_3007;
const DISCONNECT_WHILE_WAITING_FOR_AUTH_SEED: u64 = 0x57EB_3008;
const MAX_RECONNECT_ATTEMPTS_WHILE_WAITING_FOR_AUTH_SEED: u64 = 0x57EB_3009;
const STREAM_NOTIFY_CLOSED_WHILE_WAITING_FOR_AUTH_SEED: u64 = 0x57EB_300A;
const STREAM_DEAD_WRITE_WHILE_WAITING_FOR_AUTH_SEED: u64 = 0x57EB_300B;
const RECONNECTABLE_DROP_WHILE_WAITING_FOR_AUTH_SEED: u64 = 0x57EB_300C;
const UNSTABLE_RECONNECT_SEED: u64 = 0x57EB_300D;
const STABLE_RECONNECT_SEED: u64 = 0x57EB_300E;
const UNSTABLE_RECONNECT_DELAY_MS: u64 = 750;
const HEARTBEAT_PING_SEED: u64 = 0x57EB_3010;
const SERVER_PING_PONG_SEED: u64 = 0x57EB_3011;
const SERVER_CLOSE_FRAME_SEED: u64 = 0x57EB_3012;
const MISSING_PONG_SEED: u64 = 0x57EB_3013;
const HEARTBEAT_TEXT_NO_RESPONSE_SEED: u64 = 0x57EB_3014;
const HEARTBEAT_NON_PONG_TRAFFIC_SEED: u64 = 0x57EB_3015;
const HEARTBEAT_PROTOCOL_NO_TIMEOUT_SEED: u64 = 0x57EB_3016;
const RECONNECT_STORM_SEED: u64 = 0x57EB_3017;
const TEXT_HEARTBEAT: &str = "heartbeat";
const NON_PONG_FRAME: &str = "server-activity";
const NON_PONG_FRAME_COUNT: usize = 6;
const LARGE_WEBSOCKET_MESSAGE_LEN: usize = 16 * 1024;
const BACKPRESSURE_TCP_CAPACITY: usize = 4;
const BACKPRESSURE_MESSAGE_COUNT: usize = 16;

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

async fn recv_application_text(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<Message>,
) -> Option<String> {
    for _ in 0..POLL_ITERS {
        while let Ok(msg) = rx.try_recv() {
            if let Message::Text(text) = msg
                && text.as_str() != RECONNECTED
            {
                return Some(text.to_string());
            }
        }
        tokio::time::sleep(POLL_STEP).await;
    }
    None
}

async fn recv_connection_message(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<(u64, Message)>,
) -> Option<(u64, Message)> {
    for _ in 0..POLL_ITERS {
        if let Ok(message) = rx.try_recv() {
            return Some(message);
        }
        tokio::time::sleep(POLL_STEP).await;
    }
    None
}

fn tracked_channel_message_handler(
    reconnected: Arc<AtomicBool>,
) -> (
    MessageHandler,
    tokio::sync::mpsc::UnboundedReceiver<Message>,
) {
    let (handler, receiver) = channel_message_handler();
    let tracked_handler = Arc::new(move |message: NetworkMessage| {
        if matches!(&message, NetworkMessage::Text(text) if text == RECONNECTED) {
            reconnected.store(true, Ordering::SeqCst);
        }
        handler(message);
    });
    (tracked_handler, receiver)
}

async fn wait_for<F>(condition: F) -> bool
where
    F: FnMut() -> bool,
{
    wait_for_within(POLL_ITERS, condition).await
}

/// Polls for longer than [`POLL_ITERS`], for conditions gated on a whole-second timer.
async fn wait_for_within<F>(iters: u32, mut condition: F) -> bool
where
    F: FnMut() -> bool,
{
    for _ in 0..iters {
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
    websocket_config_for_backend(TransportBackend::Tungstenite)
}

fn websocket_config_for_backend(backend: TransportBackend) -> WebSocketConfig {
    WebSocketConfig {
        url: "ws://server:8080".to_string(),
        headers: vec![],
        heartbeat_interval_secs: None,
        heartbeat_payload: None,
        connect_timeout_ms: Some(2_000),
        reconnect_delay_initial_ms: Some(50),
        reconnect_delay_max_ms: Some(500),
        reconnect_backoff_factor: Some(1.5),
        reconnect_jitter_ms: Some(10),
        reconnect_max_attempts: None,
        heartbeat_timeout_secs: None,
        idle_timeout_ms: None,
        backend,
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

async fn ws_echo_once_then_drop_server() -> Result<(), Box<dyn std::error::Error>> {
    let listener = net::TcpListener::bind("0.0.0.0:8080").await?;

    loop {
        let (stream, _) = listener.accept().await?;

        tokio::spawn(async move {
            if let Ok(mut ws_stream) = accept_async(stream).await
                && let Some(Ok(msg)) = ws_stream.next().await
            {
                match msg {
                    Message::Text(text) => {
                        let _ = ws_stream.send(Message::Text(text)).await;
                    }
                    Message::Binary(data) => {
                        let _ = ws_stream.send(Message::Binary(data)).await;
                    }
                    Message::Ping(ping_data) => {
                        let _ = ws_stream.send(Message::Pong(ping_data)).await;
                    }
                    Message::Close(_) => {
                        let _ = ws_stream.close(None).await;
                    }
                    Message::Pong(_) | Message::Frame(_) => {}
                }
            }
        });
    }
}

async fn ws_drop_each_connection_server(
    accepted: Arc<AtomicUsize>,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = net::TcpListener::bind("0.0.0.0:8080").await?;

    loop {
        let (stream, _) = listener.accept().await?;
        let mut websocket = accept_async(stream).await?;
        accepted.fetch_add(1, Ordering::SeqCst);
        let _ = websocket.close(None).await;
    }
}

async fn ws_drop_each_connection_timed_server(
    accept_times: Arc<Mutex<Vec<tokio::time::Instant>>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = net::TcpListener::bind("0.0.0.0:8080").await?;

    loop {
        let (stream, _) = listener.accept().await?;
        let mut websocket = accept_async(stream).await?;
        accept_times.lock().push(tokio::time::Instant::now());
        let _ = websocket.close(None).await;
    }
}

async fn ws_hold_stable_reconnect_server(
    accepted: Arc<AtomicUsize>,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = net::TcpListener::bind("0.0.0.0:8080").await?;

    loop {
        let (stream, _) = listener.accept().await?;
        let mut websocket = accept_async(stream).await?;
        let connection = accepted.fetch_add(1, Ordering::SeqCst);

        match connection {
            0 => {
                let _ = websocket.close(None).await;
            }
            1 => {
                tokio::time::sleep(Duration::from_secs(11)).await;
                let _ = websocket.close(None).await;
            }
            _ => {
                let _held_websocket = websocket;
                std::future::pending::<()>().await;
            }
        }
    }
}

#[rstest]
fn test_turmoil_real_websocket_basic_connect(websocket_config: WebSocketConfig) {
    let mut sim = seeded_builder(BASIC_CONNECT_SEED).build();

    sim.host("server", ws_echo_server);

    sim.client("client", async move {
        let (handler, mut rx) = channel_message_handler();

        let client = WebSocketClient::builder()
            .config(websocket_config)
            .message_handler(handler)
            .connect()
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
    websocket_config.connect_timeout_ms = Some(5_000);
    websocket_config.reconnect_delay_initial_ms = Some(100);

    let mut sim = seeded_builder(RECONNECTION_SEED).build();

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

        let client = WebSocketClient::builder()
            .config(websocket_config)
            .message_handler(handler)
            .connect()
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
fn test_turmoil_connection_epoch_owns_messages_and_sends(mut websocket_config: WebSocketConfig) {
    websocket_config.connect_timeout_ms = Some(5_000);
    websocket_config.reconnect_delay_initial_ms = Some(25);
    websocket_config.reconnect_delay_max_ms = Some(100);
    websocket_config.reconnect_backoff_factor = Some(1.0);
    websocket_config.reconnect_jitter_ms = Some(0);

    let mut sim = seeded_builder(CONNECTION_EPOCH_RECONNECTION_SEED).build();

    sim.host("server", || async {
        let listener = net::TcpListener::bind("0.0.0.0:8080").await?;

        if let Ok((stream, _)) = listener.accept().await
            && let Ok(mut ws) = accept_async(stream).await
        {
            ws.send(Message::Text("first".into())).await?;
            ws.send(Message::Binary(vec![0x00, 0x7f, 0x80, 0xff].into()))
                .await?;
            let message = ws
                .next()
                .await
                .expect("initial connection should receive a bound send")?;
            assert_eq!(message, Message::Text("epoch-0".into()));
            ws.send(message).await?;
        }

        tokio::time::sleep(Duration::from_millis(100)).await;

        if let Ok((stream, _)) = listener.accept().await
            && let Ok(mut ws) = accept_async(stream).await
        {
            while let Some(message) = ws.next().await {
                let message = message?;
                if matches!(message, Message::Close(_)) {
                    break;
                }
                let send_replacement_binary =
                    matches!(&message, Message::Text(text) if text.as_str() == "epoch-1");
                ws.send(message).await?;
                if send_replacement_binary {
                    ws.send(Message::Binary(vec![0xfe, 0xed, 0xfa, 0xce].into()))
                        .await?;
                }
            }
        }

        Ok::<(), Box<dyn std::error::Error>>(())
    });

    sim.client("client", async move {
        let (handler, mut rx) = channel_epoch_message_handler();
        let rate_limiter = Arc::new(RateLimiter::new_with_quota(None, vec![]));
        let client = WebSocketClient::epoch_builder()
            .config(websocket_config)
            .epoch_handler(handler)
            .rate_limiter(rate_limiter)
            .connect()
            .await
            .expect("epoch-aware client should connect");
        let connection_epoch = client.connection_epoch_atomic();

        assert_eq!(client.connection_epoch(), 0);
        assert_eq!(connection_epoch.load(Ordering::Acquire), 0);
        assert_eq!(
            recv_connection_message(&mut rx).await,
            Some((0, Message::Text("first".into()))),
        );
        assert_eq!(
            recv_connection_message(&mut rx).await,
            Some((0, Message::Binary(vec![0x00, 0x7f, 0x80, 0xff].into()),)),
        );

        client
            .send_text_on_connection("epoch-0".to_string(), None, 0)
            .await
            .expect("initial epoch should own the initial writer");
        assert_eq!(
            recv_connection_message(&mut rx).await,
            Some((0, Message::Text("epoch-0".into()))),
        );
        assert!(
            wait_for(|| client.is_reconnecting()).await,
            "client should enter reconnect before the replacement handshake"
        );

        let stale_result = client
            .send_text_on_connection("stale".to_string(), None, 0)
            .await;
        assert!(matches!(stale_result, Err(SendError::ConnectionChanged),));
        assert_eq!(
            recv_connection_message(&mut rx).await,
            Some((1, Message::Text(RECONNECTED.into()))),
        );
        assert_eq!(client.connection_epoch(), 1);
        assert_eq!(connection_epoch.load(Ordering::Acquire), 1);

        client
            .send_text_on_connection("epoch-1".to_string(), None, 1)
            .await
            .expect("replacement epoch should own the replacement writer");
        assert_eq!(
            recv_connection_message(&mut rx).await,
            Some((1, Message::Text("epoch-1".into()))),
        );
        assert_eq!(
            recv_connection_message(&mut rx).await,
            Some((1, Message::Binary(vec![0xfe, 0xed, 0xfa, 0xce].into()),)),
        );
        assert_eq!(
            rx.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty),
            "stale bound send must not reach either connection",
        );

        client.disconnect().await;
        assert!(client.is_disconnected());

        Ok(())
    });

    sim.run().unwrap();
}

#[rstest]
fn test_turmoil_websocket_unstable_reconnects_exhaust_attempts(
    mut websocket_config: WebSocketConfig,
) {
    websocket_config.reconnect_delay_initial_ms = Some(UNSTABLE_RECONNECT_DELAY_MS);
    websocket_config.reconnect_delay_max_ms = Some(UNSTABLE_RECONNECT_DELAY_MS);
    websocket_config.reconnect_backoff_factor = Some(1.0);
    websocket_config.reconnect_jitter_ms = Some(0);
    websocket_config.reconnect_max_attempts = Some(3);

    let accepted = Arc::new(AtomicUsize::new(0));
    let server_accepted = Arc::clone(&accepted);
    let mut builder =
        seeded_builder_with_duration(UNSTABLE_RECONNECT_SEED, Duration::from_secs(10));
    builder.min_message_latency(Duration::ZERO);
    builder.max_message_latency(Duration::ZERO);
    let mut sim = builder.build();

    sim.host("server", move || {
        ws_drop_each_connection_server(Arc::clone(&server_accepted))
    });

    sim.client("client", async move {
        let (handler, _rx) = channel_message_handler();
        let client = WebSocketClient::builder()
            .config(websocket_config)
            .message_handler(handler)
            .connect()
            .await
            .expect("Initial WebSocket connection should succeed");
        let started_at = tokio::time::Instant::now();

        assert!(
            wait_for(|| client.is_closed()).await,
            "Rapidly dropped reconnects should exhaust the attempt limit"
        );
        assert!(
            started_at.elapsed() >= Duration::from_millis(UNSTABLE_RECONNECT_DELAY_MS),
            "Rapidly dropped reconnects should retain the backoff progression"
        );
        assert_eq!(
            accepted.load(Ordering::SeqCst),
            4,
            "Server should accept the initial connection and three reconnect attempts"
        );

        Ok(())
    });

    sim.run().unwrap();
}

#[rstest]
fn test_turmoil_websocket_stable_reconnect_resets_attempts(mut websocket_config: WebSocketConfig) {
    websocket_config.reconnect_delay_initial_ms = Some(50);
    websocket_config.reconnect_delay_max_ms = Some(200);
    websocket_config.reconnect_backoff_factor = Some(2.0);
    websocket_config.reconnect_jitter_ms = Some(0);
    websocket_config.reconnect_max_attempts = Some(1);

    let accepted = Arc::new(AtomicUsize::new(0));
    let server_accepted = Arc::clone(&accepted);
    let mut sim =
        seeded_builder_with_duration(STABLE_RECONNECT_SEED, Duration::from_secs(20)).build();

    sim.host("server", move || {
        ws_hold_stable_reconnect_server(Arc::clone(&server_accepted))
    });

    sim.client("client", async move {
        let (handler, _rx) = channel_message_handler();
        let client = WebSocketClient::builder()
            .config(websocket_config)
            .message_handler(handler)
            .connect()
            .await
            .expect("Initial WebSocket connection should succeed");

        tokio::time::sleep(Duration::from_secs(13)).await;

        assert!(
            wait_for(|| accepted.load(Ordering::SeqCst) >= 3).await,
            "A stable reconnect should reset the attempt limit for the next drop"
        );
        assert!(
            client.is_active(),
            "Client should remain active on the connection after the reset"
        );

        client.disconnect().await;
        Ok(())
    });

    sim.run().unwrap();
}

#[rstest]
fn test_turmoil_websocket_reconnect_storm_attempts_are_floored(
    mut websocket_config: WebSocketConfig,
) {
    // A venue cycling connections faster than the stability threshold must not get an
    // immediate reconnect: once three attempts land inside the rolling window, each
    // further attempt waits at least one second regardless of the configured backoff.
    websocket_config.reconnect_delay_initial_ms = Some(25);
    websocket_config.reconnect_delay_max_ms = Some(25);
    websocket_config.reconnect_backoff_factor = Some(1.0);
    websocket_config.reconnect_jitter_ms = Some(0);
    websocket_config.reconnect_max_attempts = None;

    let accept_times = Arc::new(Mutex::new(Vec::new()));
    let server_accept_times = Arc::clone(&accept_times);
    let mut builder = seeded_builder_with_duration(RECONNECT_STORM_SEED, Duration::from_secs(20));
    builder.min_message_latency(Duration::ZERO);
    builder.max_message_latency(Duration::ZERO);
    let mut sim = builder.build();

    sim.host("server", move || {
        ws_drop_each_connection_timed_server(Arc::clone(&server_accept_times))
    });

    sim.client("client", async move {
        let (handler, _rx) = channel_message_handler();
        let client = WebSocketClient::builder()
            .config(websocket_config)
            .message_handler(handler)
            .connect()
            .await
            .expect("Initial WebSocket connection should succeed");

        tokio::time::sleep(Duration::from_secs(15)).await;
        client.disconnect().await;

        Ok(())
    });

    sim.run().unwrap();

    let times = accept_times.lock();
    assert!(
        times.len() >= 5,
        "Expected the initial connection and several reconnects, was {}",
        times.len()
    );
    assert!(
        times.len() <= 25,
        "Floored attempts should total well under the unfloored ~600, was {}",
        times.len()
    );

    for pair in times.windows(2).skip(3) {
        let gap = pair[1].duration_since(pair[0]);
        assert!(
            gap >= Duration::from_millis(900),
            "Reconnect attempts should space at least ~1s once the window trips, was {gap:?}"
        );
    }
}

#[rstest]
fn test_turmoil_real_websocket_network_partition(mut websocket_config: WebSocketConfig) {
    websocket_config.connect_timeout_ms = Some(3_000);

    let mut sim = seeded_builder(NETWORK_PARTITION_SEED).build();

    sim.host("server", ws_echo_server);

    sim.client("client", async move {
        let (handler, mut rx) = channel_message_handler();

        let client = WebSocketClient::builder()
            .config(websocket_config)
            .message_handler(handler)
            .connect()
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
    websocket_config.connect_timeout_ms = Some(5_000);
    websocket_config.reconnect_delay_initial_ms = Some(100);

    let mut sim = seeded_builder(DISCONNECT_DURING_RECONNECT_SEED).build();

    sim.host("server", ws_echo_server);

    sim.client("client", async move {
        let (handler, _rx) = channel_message_handler();

        let client = WebSocketClient::builder()
            .config(websocket_config)
            .message_handler(handler)
            .connect()
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
    websocket_config.connect_timeout_ms = Some(1_000);
    websocket_config.reconnect_delay_initial_ms = Some(10_000); // Long backoff
    websocket_config.reconnect_delay_max_ms = Some(10_000);
    websocket_config.reconnect_backoff_factor = Some(1.0);
    websocket_config.reconnect_jitter_ms = Some(0);

    let mut sim =
        seeded_builder_with_duration(DISCONNECT_DURING_BACKOFF_SEED, Duration::from_secs(30))
            .build();

    sim.host("server", ws_echo_server);

    sim.client("client", async move {
        let (handler, _rx) = channel_message_handler();

        let client = WebSocketClient::builder()
            .config(websocket_config)
            .message_handler(handler)
            .connect()
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

    let mut sim = seeded_builder(PROXY_REJECTION_SEED).build();
    sim.host("server", ws_echo_server);
    sim.client("client", async move {
        let (handler, _rx) = channel_message_handler();
        let err = WebSocketClient::builder()
            .config(websocket_config)
            .message_handler(handler)
            .connect()
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

#[rstest]
#[case::seed_a(0x57EB_1001)]
#[case::seed_b(0x57EB_1002)]
#[case::seed_c(0x57EB_1003)]
fn test_turmoil_websocket_repeated_drops_preserve_message_order(
    websocket_config: WebSocketConfig,
    #[case] seed: u64,
) {
    run_websocket_repeated_drops_preserve_message_order(websocket_config, seed, "tungstenite");
}

#[rstest]
fn test_turmoil_websocket_queued_write_drop_preserves_later_message_order() {
    run_websocket_queued_write_drop_preserves_later_message_order(
        websocket_config_for_backend(TransportBackend::Tungstenite),
        QUEUED_WRITE_DROP_SEED,
        "websocket/tungstenite",
    );
}

#[cfg(feature = "transport-sockudo")]
#[rstest]
fn test_turmoil_websocket_sockudo_queued_write_drop_preserves_later_message_order() {
    run_websocket_queued_write_drop_preserves_later_message_order(
        websocket_config_for_backend(TransportBackend::Sockudo),
        QUEUED_WRITE_DROP_SEED,
        "websocket/sockudo",
    );
}

#[rstest]
fn test_turmoil_websocket_reconnect_active_drop_preserves_later_message_order() {
    run_websocket_reconnect_active_drop_preserves_later_message_order(
        websocket_config_for_backend(TransportBackend::Tungstenite),
        RECONNECT_ACTIVE_DROP_SEED,
        "websocket/tungstenite",
    );
}

#[cfg(feature = "transport-sockudo")]
#[rstest]
fn test_turmoil_websocket_sockudo_reconnect_active_drop_preserves_later_message_order() {
    run_websocket_reconnect_active_drop_preserves_later_message_order(
        websocket_config_for_backend(TransportBackend::Sockudo),
        RECONNECT_ACTIVE_DROP_SEED,
        "websocket/sockudo",
    );
}

#[rstest]
fn test_turmoil_websocket_alternating_text_binary_preserves_message_order() {
    run_websocket_alternating_text_binary_preserves_message_order(
        websocket_config_for_backend(TransportBackend::Tungstenite),
        ALTERNATING_TEXT_BINARY_SEED,
        "websocket/tungstenite",
    );
}

#[cfg(feature = "transport-sockudo")]
#[rstest]
fn test_turmoil_websocket_sockudo_alternating_text_binary_preserves_message_order() {
    run_websocket_alternating_text_binary_preserves_message_order(
        websocket_config_for_backend(TransportBackend::Sockudo),
        ALTERNATING_TEXT_BINARY_SEED,
        "websocket/sockudo",
    );
}

#[rstest]
fn test_turmoil_websocket_handshake_drop_reaches_active_state() {
    run_websocket_handshake_drop_reaches_active_state(
        websocket_config_for_backend(TransportBackend::Tungstenite),
        HANDSHAKE_DROP_SEED,
        "websocket/tungstenite",
    );
}

#[cfg(feature = "transport-sockudo")]
#[rstest]
fn test_turmoil_websocket_sockudo_handshake_drop_reaches_active_state() {
    run_websocket_handshake_drop_reaches_active_state(
        websocket_config_for_backend(TransportBackend::Sockudo),
        HANDSHAKE_DROP_SEED,
        "websocket/sockudo",
    );
}

#[rstest]
fn test_turmoil_websocket_first_read_task_drop_reaches_active_state() {
    run_websocket_first_read_task_drop_reaches_active_state(
        websocket_config_for_backend(TransportBackend::Tungstenite),
        FIRST_READ_TASK_DROP_SEED,
        "websocket/tungstenite",
    );
}

#[cfg(feature = "transport-sockudo")]
#[rstest]
fn test_turmoil_websocket_sockudo_first_read_task_drop_reaches_active_state() {
    run_websocket_first_read_task_drop_reaches_active_state(
        websocket_config_for_backend(TransportBackend::Sockudo),
        FIRST_READ_TASK_DROP_SEED,
        "websocket/sockudo",
    );
}

#[rstest]
fn test_turmoil_websocket_partition_while_reconnecting_reaches_active_state() {
    run_websocket_partition_while_reconnecting_reaches_active_state(
        websocket_config_for_backend(TransportBackend::Tungstenite),
        PARTITION_DURING_RECONNECT_SEED,
        "websocket/tungstenite",
    );
}

#[cfg(feature = "transport-sockudo")]
#[rstest]
fn test_turmoil_websocket_sockudo_partition_while_reconnecting_reaches_active_state() {
    run_websocket_partition_while_reconnecting_reaches_active_state(
        websocket_config_for_backend(TransportBackend::Sockudo),
        PARTITION_DURING_RECONNECT_SEED,
        "websocket/sockudo",
    );
}

#[rstest]
fn test_turmoil_websocket_partition_during_backoff_sleep_reaches_active_state() {
    run_websocket_partition_during_backoff_sleep_reaches_active_state(
        websocket_config_for_backend(TransportBackend::Tungstenite),
        PARTITION_DURING_BACKOFF_SEED,
        "websocket/tungstenite",
    );
}

#[cfg(feature = "transport-sockudo")]
#[rstest]
fn test_turmoil_websocket_sockudo_partition_during_backoff_sleep_reaches_active_state() {
    run_websocket_partition_during_backoff_sleep_reaches_active_state(
        websocket_config_for_backend(TransportBackend::Sockudo),
        PARTITION_DURING_BACKOFF_SEED,
        "websocket/sockudo",
    );
}

#[rstest]
fn test_turmoil_websocket_silent_until_idle_timeout_reconnects_to_active_state() {
    run_websocket_silent_until_idle_timeout_reconnects_to_active_state(
        websocket_config_for_backend(TransportBackend::Tungstenite),
        SILENT_UNTIL_IDLE_TIMEOUT_SEED,
        "websocket/tungstenite",
    );
}

#[cfg(feature = "transport-sockudo")]
#[rstest]
fn test_turmoil_websocket_sockudo_silent_until_idle_timeout_reconnects_to_active_state() {
    run_websocket_silent_until_idle_timeout_reconnects_to_active_state(
        websocket_config_for_backend(TransportBackend::Sockudo),
        SILENT_UNTIL_IDLE_TIMEOUT_SEED,
        "websocket/sockudo",
    );
}

#[rstest]
fn test_turmoil_websocket_dark_until_derived_heartbeat_timeout_reconnects_to_active_state() {
    run_websocket_dark_until_derived_heartbeat_timeout_reconnects_to_active_state(
        websocket_config_for_backend(TransportBackend::Tungstenite),
        SILENT_UNTIL_IDLE_TIMEOUT_SEED,
        "websocket/tungstenite",
    );
}

#[cfg(feature = "transport-sockudo")]
#[rstest]
fn test_turmoil_websocket_sockudo_dark_until_derived_heartbeat_timeout_reconnects_to_active_state()
{
    run_websocket_dark_until_derived_heartbeat_timeout_reconnects_to_active_state(
        websocket_config_for_backend(TransportBackend::Sockudo),
        SILENT_UNTIL_IDLE_TIMEOUT_SEED,
        "websocket/sockudo",
    );
}

#[rstest]
fn test_turmoil_websocket_no_read_backpressure_reconnects_to_active_state() {
    run_websocket_no_read_backpressure_reconnects_to_active_state(
        websocket_config_for_backend(TransportBackend::Tungstenite),
        NO_READ_BACKPRESSURE_SEED,
        "websocket/tungstenite",
    );
}

#[cfg(feature = "transport-sockudo")]
#[rstest]
fn test_turmoil_websocket_sockudo_no_read_backpressure_reconnects_to_active_state() {
    run_websocket_no_read_backpressure_reconnects_to_active_state(
        websocket_config_for_backend(TransportBackend::Sockudo),
        NO_READ_BACKPRESSURE_SEED,
        "websocket/sockudo",
    );
}

#[rstest]
fn test_turmoil_websocket_disconnect_while_send_waits_for_reconnect_closes_send() {
    run_websocket_disconnect_while_send_waits_for_reconnect_closes_send(
        websocket_config_for_backend(TransportBackend::Tungstenite),
        DISCONNECT_WHILE_SEND_WAITS_FOR_RECONNECT_SEED,
        "websocket/tungstenite",
    );
}

#[cfg(feature = "transport-sockudo")]
#[rstest]
fn test_turmoil_websocket_sockudo_disconnect_while_send_waits_for_reconnect_closes_send() {
    run_websocket_disconnect_while_send_waits_for_reconnect_closes_send(
        websocket_config_for_backend(TransportBackend::Sockudo),
        DISCONNECT_WHILE_SEND_WAITS_FOR_RECONNECT_SEED,
        "websocket/sockudo",
    );
}

#[rstest]
fn test_turmoil_websocket_disconnect_while_waiting_for_auth_closes_client() {
    run_websocket_disconnect_while_waiting_for_auth_closes_client(
        websocket_config_for_backend(TransportBackend::Tungstenite),
        DISCONNECT_WHILE_WAITING_FOR_AUTH_SEED,
        "websocket/tungstenite",
    );
}

#[cfg(feature = "transport-sockudo")]
#[rstest]
fn test_turmoil_websocket_sockudo_disconnect_while_waiting_for_auth_closes_client() {
    run_websocket_disconnect_while_waiting_for_auth_closes_client(
        websocket_config_for_backend(TransportBackend::Sockudo),
        DISCONNECT_WHILE_WAITING_FOR_AUTH_SEED,
        "websocket/sockudo",
    );
}

#[rstest]
fn test_turmoil_websocket_max_reconnect_attempts_while_waiting_for_auth_closes_client() {
    run_websocket_max_reconnect_attempts_while_waiting_for_auth_closes_client(
        websocket_config_for_backend(TransportBackend::Tungstenite),
        MAX_RECONNECT_ATTEMPTS_WHILE_WAITING_FOR_AUTH_SEED,
        "websocket/tungstenite",
    );
}

#[cfg(feature = "transport-sockudo")]
#[rstest]
fn test_turmoil_websocket_sockudo_max_reconnect_attempts_while_waiting_for_auth_closes_client() {
    run_websocket_max_reconnect_attempts_while_waiting_for_auth_closes_client(
        websocket_config_for_backend(TransportBackend::Sockudo),
        MAX_RECONNECT_ATTEMPTS_WHILE_WAITING_FOR_AUTH_SEED,
        "websocket/sockudo",
    );
}

#[rstest]
fn test_turmoil_websocket_stream_notify_closed_while_waiting_for_auth_closes_client() {
    run_websocket_stream_notify_closed_while_waiting_for_auth_closes_client(
        websocket_config_for_backend(TransportBackend::Tungstenite),
        STREAM_NOTIFY_CLOSED_WHILE_WAITING_FOR_AUTH_SEED,
        "websocket/tungstenite",
    );
}

#[cfg(feature = "transport-sockudo")]
#[rstest]
fn test_turmoil_websocket_sockudo_stream_notify_closed_while_waiting_for_auth_closes_client() {
    run_websocket_stream_notify_closed_while_waiting_for_auth_closes_client(
        websocket_config_for_backend(TransportBackend::Sockudo),
        STREAM_NOTIFY_CLOSED_WHILE_WAITING_FOR_AUTH_SEED,
        "websocket/sockudo",
    );
}

#[rstest]
fn test_turmoil_websocket_stream_dead_write_while_waiting_for_auth_closes_client() {
    run_websocket_stream_dead_write_while_waiting_for_auth_closes_client(
        websocket_config_for_backend(TransportBackend::Tungstenite),
        STREAM_DEAD_WRITE_WHILE_WAITING_FOR_AUTH_SEED,
        "websocket/tungstenite",
    );
}

#[cfg(feature = "transport-sockudo")]
#[rstest]
fn test_turmoil_websocket_sockudo_stream_dead_write_while_waiting_for_auth_closes_client() {
    run_websocket_stream_dead_write_while_waiting_for_auth_closes_client(
        websocket_config_for_backend(TransportBackend::Sockudo),
        STREAM_DEAD_WRITE_WHILE_WAITING_FOR_AUTH_SEED,
        "websocket/sockudo",
    );
}

#[rstest]
fn test_turmoil_websocket_reconnectable_drop_while_waiting_for_auth_waits_for_reauth() {
    run_websocket_reconnectable_drop_while_waiting_for_auth_waits_for_reauth(
        websocket_config_for_backend(TransportBackend::Tungstenite),
        RECONNECTABLE_DROP_WHILE_WAITING_FOR_AUTH_SEED,
        "websocket/tungstenite",
    );
}

#[cfg(feature = "transport-sockudo")]
#[rstest]
fn test_turmoil_websocket_sockudo_reconnectable_drop_while_waiting_for_auth_waits_for_reauth() {
    run_websocket_reconnectable_drop_while_waiting_for_auth_waits_for_reauth(
        websocket_config_for_backend(TransportBackend::Sockudo),
        RECONNECTABLE_DROP_WHILE_WAITING_FOR_AUTH_SEED,
        "websocket/sockudo",
    );
}

#[rstest]
fn test_turmoil_websocket_heartbeat_pings_reach_server() {
    run_websocket_heartbeat_pings_reach_server(
        websocket_config_for_backend(TransportBackend::Tungstenite),
        HEARTBEAT_PING_SEED,
        "websocket/tungstenite",
    );
}

#[cfg(feature = "transport-sockudo")]
#[rstest]
fn test_turmoil_websocket_sockudo_heartbeat_pings_reach_server() {
    run_websocket_heartbeat_pings_reach_server(
        websocket_config_for_backend(TransportBackend::Sockudo),
        HEARTBEAT_PING_SEED,
        "websocket/sockudo",
    );
}

#[rstest]
fn test_turmoil_websocket_missing_pong_reconnects() {
    run_websocket_missing_pong_reconnects(
        websocket_config_for_backend(TransportBackend::Tungstenite),
        MISSING_PONG_SEED,
        "websocket/tungstenite",
    );
}

#[rstest]
fn test_turmoil_websocket_no_heartbeat_has_no_implicit_timeout() {
    run_websocket_no_heartbeat_has_no_implicit_timeout(
        websocket_config_for_backend(TransportBackend::Tungstenite),
        HEARTBEAT_PROTOCOL_NO_TIMEOUT_SEED,
        "websocket/tungstenite",
    );
}

#[cfg(feature = "transport-sockudo")]
#[rstest]
fn test_turmoil_websocket_sockudo_no_heartbeat_has_no_implicit_timeout() {
    run_websocket_no_heartbeat_has_no_implicit_timeout(
        websocket_config_for_backend(TransportBackend::Sockudo),
        HEARTBEAT_PROTOCOL_NO_TIMEOUT_SEED,
        "websocket/sockudo",
    );
}

#[cfg(feature = "transport-sockudo")]
#[rstest]
fn test_turmoil_websocket_sockudo_missing_pong_reconnects() {
    run_websocket_missing_pong_reconnects(
        websocket_config_for_backend(TransportBackend::Sockudo),
        MISSING_PONG_SEED,
        "websocket/sockudo",
    );
}

#[rstest]
fn test_turmoil_websocket_unanswered_text_heartbeat_times_out() {
    run_websocket_unanswered_text_heartbeat_times_out(
        websocket_config_for_backend(TransportBackend::Tungstenite),
        HEARTBEAT_TEXT_NO_RESPONSE_SEED,
        "websocket/tungstenite",
    );
}

#[cfg(feature = "transport-sockudo")]
#[rstest]
fn test_turmoil_websocket_sockudo_unanswered_text_heartbeat_times_out() {
    run_websocket_unanswered_text_heartbeat_times_out(
        websocket_config_for_backend(TransportBackend::Sockudo),
        HEARTBEAT_TEXT_NO_RESPONSE_SEED,
        "websocket/sockudo",
    );
}

#[rstest]
fn test_turmoil_websocket_non_pong_frames_prevent_heartbeat_timeout() {
    run_websocket_non_pong_frames_prevent_heartbeat_timeout(
        websocket_config_for_backend(TransportBackend::Tungstenite),
        HEARTBEAT_NON_PONG_TRAFFIC_SEED,
        "websocket/tungstenite",
    );
}

#[cfg(feature = "transport-sockudo")]
#[rstest]
fn test_turmoil_websocket_sockudo_non_pong_frames_prevent_heartbeat_timeout() {
    run_websocket_non_pong_frames_prevent_heartbeat_timeout(
        websocket_config_for_backend(TransportBackend::Sockudo),
        HEARTBEAT_NON_PONG_TRAFFIC_SEED,
        "websocket/sockudo",
    );
}

#[rstest]
fn test_turmoil_websocket_server_ping_gets_pong() {
    run_websocket_server_ping_gets_pong(
        websocket_config_for_backend(TransportBackend::Tungstenite),
        SERVER_PING_PONG_SEED,
        "websocket/tungstenite",
    );
}

#[cfg(feature = "transport-sockudo")]
#[rstest]
fn test_turmoil_websocket_sockudo_server_ping_gets_pong() {
    run_websocket_server_ping_gets_pong(
        websocket_config_for_backend(TransportBackend::Sockudo),
        SERVER_PING_PONG_SEED,
        "websocket/sockudo",
    );
}

#[rstest]
fn test_turmoil_websocket_server_close_frame_triggers_reconnect() {
    run_websocket_server_close_frame_triggers_reconnect(
        websocket_config_for_backend(TransportBackend::Tungstenite),
        SERVER_CLOSE_FRAME_SEED,
        "websocket/tungstenite",
    );
}

#[cfg(feature = "transport-sockudo")]
#[rstest]
fn test_turmoil_websocket_sockudo_server_close_frame_triggers_reconnect() {
    run_websocket_server_close_frame_triggers_reconnect(
        websocket_config_for_backend(TransportBackend::Sockudo),
        SERVER_CLOSE_FRAME_SEED,
        "websocket/sockudo",
    );
}

#[rstest]
#[ignore = "continuous seed sweep; run scripts/soak-network-turmoil.sh"]
fn test_turmoil_websocket_repeated_drops_backend_pair_soak() {
    for (iteration, seed) in seed_sweep_from_env() {
        log_soak_seed("websocket/tungstenite", iteration, seed);
        run_websocket_repeated_drops_preserve_message_order(
            websocket_config_for_backend(TransportBackend::Tungstenite),
            seed,
            "websocket/tungstenite",
        );

        #[cfg(feature = "transport-sockudo")]
        {
            log_soak_seed("websocket/sockudo", iteration, seed);
            run_websocket_repeated_drops_preserve_message_order(
                websocket_config_for_backend(TransportBackend::Sockudo),
                seed,
                "websocket/sockudo",
            );
        }
    }
}

fn run_websocket_heartbeat_pings_reach_server(
    mut websocket_config: WebSocketConfig,
    seed: u64,
    label: &'static str,
) {
    websocket_config.heartbeat_interval_secs = Some(1);

    let mut sim = seeded_builder_with_duration(seed, Duration::from_secs(30)).build();
    let pings = Arc::new(AtomicUsize::new(0));
    let server_pings = Arc::clone(&pings);

    sim.host("server", move || {
        let server_pings = Arc::clone(&server_pings);
        async move { ws_ping_counting_server(server_pings).await }
    });

    sim.client("client", async move {
        let reconnected = Arc::new(AtomicBool::new(false));
        let (handler, _rx) = tracked_channel_message_handler(Arc::clone(&reconnected));
        websocket_config.heartbeat_timeout_secs = Some(2);
        let client = WebSocketClient::builder()
            .config(websocket_config)
            .message_handler(handler)
            .connect()
            .await
            .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} should connect: {e}"));

        // Heartbeat cadence is 1s; allow up to 10s of simulated time for 3 pings
        let mut received_enough = false;

        for _ in 0..1_000 {
            if pings.load(Ordering::SeqCst) >= 3 {
                received_enough = true;
                break;
            }
            tokio::time::sleep(POLL_STEP).await;
        }

        assert!(
            received_enough,
            "{label} seed {seed:#018x} server should receive heartbeat pings, received {}",
            pings.load(Ordering::SeqCst)
        );
        assert!(
            client.is_active(),
            "{label} seed {seed:#018x} should stay active when the quiet peer returns pongs"
        );
        assert!(
            !reconnected.load(Ordering::SeqCst),
            "{label} seed {seed:#018x} should not reconnect while pongs arrive"
        );

        client.disconnect().await;

        Ok(())
    });

    sim.run()
        .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} simulation failed: {e:?}"));
}

/// Without a heartbeat there is nothing to guarantee inbound frames, so no window is derived and a
/// peer that has gone dark is left alone. The paired case, where a heartbeat is configured and the
/// derived window does tear it down, is covered by
/// `run_websocket_dark_until_derived_heartbeat_timeout_reconnects_to_active_state`.
fn run_websocket_no_heartbeat_has_no_implicit_timeout(
    mut websocket_config: WebSocketConfig,
    seed: u64,
    label: &'static str,
) {
    websocket_config.heartbeat_interval_secs = None;
    websocket_config.heartbeat_timeout_secs = None;

    let mut sim = seeded_builder_with_duration(seed, Duration::from_secs(30)).build();
    let first_connection_held = Arc::new(AtomicBool::new(false));
    let release_first_connection = Arc::new(AtomicBool::new(false));
    let server_first_connection_held = Arc::clone(&first_connection_held);
    let server_release_first_connection = Arc::clone(&release_first_connection);

    sim.host("server", move || {
        let server_first_connection_held = Arc::clone(&server_first_connection_held);
        let server_release_first_connection = Arc::clone(&server_release_first_connection);
        async move {
            ws_hold_first_connection_until_release_then_echo_server(
                server_first_connection_held,
                server_release_first_connection,
            )
            .await
        }
    });

    sim.client("client", async move {
        let reconnected = Arc::new(AtomicBool::new(false));
        let (handler, _rx) = tracked_channel_message_handler(Arc::clone(&reconnected));
        let client = WebSocketClient::builder()
            .config(websocket_config)
            .message_handler(handler)
            .connect()
            .await
            .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} should connect: {e}"));

        assert!(
            wait_for(|| first_connection_held.load(Ordering::SeqCst)).await,
            "{label} seed {seed:#018x} should hold the first connection without reads"
        );

        for _ in 0..400 {
            tokio::time::sleep(POLL_STEP).await;
        }

        assert!(
            client.is_active(),
            "{label} seed {seed:#018x} should stay active with no heartbeat configured"
        );
        assert!(
            !reconnected.load(Ordering::SeqCst),
            "{label} seed {seed:#018x} should derive no liveness window without a heartbeat"
        );
        release_first_connection.store(true, Ordering::SeqCst);
        client.disconnect().await;

        Ok(())
    });

    sim.run()
        .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} simulation failed: {e:?}"));
}

fn run_websocket_missing_pong_reconnects(
    mut websocket_config: WebSocketConfig,
    seed: u64,
    label: &'static str,
) {
    websocket_config.heartbeat_interval_secs = Some(1);
    websocket_config.reconnect_delay_initial_ms = Some(25);
    websocket_config.reconnect_delay_max_ms = Some(100);
    websocket_config.reconnect_backoff_factor = Some(1.0);
    websocket_config.reconnect_jitter_ms = Some(0);

    let mut sim = seeded_builder_with_duration(seed, Duration::from_secs(30)).build();
    let first_connection_held = Arc::new(AtomicBool::new(false));
    let release_first_connection = Arc::new(AtomicBool::new(false));
    let server_first_connection_held = Arc::clone(&first_connection_held);
    let server_release_first_connection = Arc::clone(&release_first_connection);

    sim.host("server", move || {
        let server_first_connection_held = Arc::clone(&server_first_connection_held);
        let server_release_first_connection = Arc::clone(&server_release_first_connection);
        async move {
            ws_hold_first_connection_until_release_then_echo_server(
                server_first_connection_held,
                server_release_first_connection,
            )
            .await
        }
    });

    sim.client("client", async move {
        let reconnected = Arc::new(AtomicBool::new(false));
        let (handler, _rx) = tracked_channel_message_handler(Arc::clone(&reconnected));
        websocket_config.heartbeat_timeout_secs = Some(2);
        let client = WebSocketClient::builder()
            .config(websocket_config)
            .message_handler(handler)
            .connect()
            .await
            .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} should connect: {e}"));

        assert!(
            wait_for(|| first_connection_held.load(Ordering::SeqCst)).await,
            "{label} seed {seed:#018x} should hold the first connection without reads"
        );

        let mut recovered = false;

        for _ in 0..500 {
            if reconnected.load(Ordering::SeqCst) && client.is_active() {
                recovered = true;
                break;
            }
            tokio::time::sleep(POLL_STEP).await;
        }

        assert!(
            recovered,
            "{label} seed {seed:#018x} should reconnect when heartbeat pongs stop"
        );
        release_first_connection.store(true, Ordering::SeqCst);
        client.disconnect().await;

        Ok(())
    });

    sim.run()
        .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} simulation failed: {e:?}"));
}

/// A text keepalive the peer never answers is a dead peer, so the derived window tears it down.
/// This is the text-payload counterpart to the Ping-frame case covered by
/// `run_websocket_dark_until_derived_heartbeat_timeout_reconnects_to_active_state`.
fn run_websocket_unanswered_text_heartbeat_times_out(
    mut websocket_config: WebSocketConfig,
    seed: u64,
    label: &'static str,
) {
    websocket_config.heartbeat_interval_secs = Some(1);
    websocket_config.heartbeat_payload = Some(TEXT_HEARTBEAT.to_string());

    let mut sim = seeded_builder_with_duration(seed, Duration::from_secs(30)).build();
    let heartbeats = Arc::new(AtomicUsize::new(0));
    let server_heartbeats = Arc::clone(&heartbeats);

    sim.host("server", move || {
        let server_heartbeats = Arc::clone(&server_heartbeats);
        async move { ws_text_heartbeat_counting_server(server_heartbeats).await }
    });

    sim.client("client", async move {
        let reconnected = Arc::new(AtomicBool::new(false));
        let (handler, _rx) = tracked_channel_message_handler(Arc::clone(&reconnected));
        let client = WebSocketClient::builder()
            .config(websocket_config)
            .message_handler(handler)
            .connect()
            .await
            .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} should connect: {e}"));

        let mut received_enough = false;

        for _ in 0..1_000 {
            if heartbeats.load(Ordering::SeqCst) >= 3 {
                received_enough = true;
                break;
            }
            tokio::time::sleep(POLL_STEP).await;
        }

        assert!(
            received_enough,
            "{label} seed {seed:#018x} server should receive three text heartbeats, received {}",
            heartbeats.load(Ordering::SeqCst)
        );
        assert!(
            wait_for_within(1_000, || reconnected.load(Ordering::SeqCst)).await,
            "{label} seed {seed:#018x} should tear down a text heartbeat the peer never answers"
        );

        client.disconnect().await;

        Ok(())
    });

    sim.run()
        .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} simulation failed: {e:?}"));
}

fn run_websocket_non_pong_frames_prevent_heartbeat_timeout(
    mut websocket_config: WebSocketConfig,
    seed: u64,
    label: &'static str,
) {
    websocket_config.heartbeat_interval_secs = Some(1);

    let mut sim = seeded_builder_with_duration(seed, Duration::from_secs(30)).build();
    sim.host("server", ws_send_text_without_reading_server);

    sim.client("client", async move {
        let reconnected = Arc::new(AtomicBool::new(false));
        let (handler, mut rx) = tracked_channel_message_handler(Arc::clone(&reconnected));
        websocket_config.heartbeat_timeout_secs = Some(2);
        let client = WebSocketClient::builder()
            .config(websocket_config)
            .message_handler(handler)
            .connect()
            .await
            .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} should connect: {e}"));
        let mut received = Vec::with_capacity(NON_PONG_FRAME_COUNT);

        for _ in 0..NON_PONG_FRAME_COUNT {
            if let Some(message) = recv_application_text(&mut rx).await {
                received.push(message);
            }
        }

        assert_eq!(
            received,
            vec![NON_PONG_FRAME.to_string(); NON_PONG_FRAME_COUNT],
            "{label} seed {seed:#018x} should receive non-Pong frames across the timeout window"
        );
        assert!(
            client.is_active(),
            "{label} seed {seed:#018x} should stay active while non-Pong frames arrive"
        );
        assert!(
            !reconnected.load(Ordering::SeqCst),
            "{label} seed {seed:#018x} should count every frame as heartbeat activity"
        );

        client.disconnect().await;

        Ok(())
    });

    sim.run()
        .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} simulation failed: {e:?}"));
}

fn run_websocket_server_ping_gets_pong(
    websocket_config: WebSocketConfig,
    seed: u64,
    label: &'static str,
) {
    let mut sim = seeded_builder_with_duration(seed, Duration::from_secs(30)).build();
    let pong_received = Arc::new(AtomicBool::new(false));
    let server_pong_received = Arc::clone(&pong_received);

    sim.host("server", move || {
        let server_pong_received = Arc::clone(&server_pong_received);
        async move { ws_ping_until_pong_server(server_pong_received).await }
    });

    sim.client("client", async move {
        let (handler, _rx) = channel_message_handler();
        let client = WebSocketClient::builder()
            .config(websocket_config)
            .message_handler(handler)
            .connect()
            .await
            .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} should connect: {e}"));

        // A quiet client must auto-reply to server pings at the transport layer;
        // for sockudo this pins the pending_flush nudge that flushes pongs
        // queued by the read path
        assert!(
            wait_for(|| pong_received.load(Ordering::SeqCst)).await,
            "{label} seed {seed:#018x} server should receive a pong from the quiet client"
        );

        client.disconnect().await;

        Ok(())
    });

    sim.run()
        .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} simulation failed: {e:?}"));
}

fn run_websocket_server_close_frame_triggers_reconnect(
    mut websocket_config: WebSocketConfig,
    seed: u64,
    label: &'static str,
) {
    websocket_config.connect_timeout_ms = Some(5_000);
    websocket_config.reconnect_delay_initial_ms = Some(25);
    websocket_config.reconnect_delay_max_ms = Some(100);
    websocket_config.reconnect_backoff_factor = Some(1.0);
    websocket_config.reconnect_jitter_ms = Some(0);

    let mut sim = seeded_builder_with_duration(seed, Duration::from_secs(30)).build();

    sim.host("server", ws_close_frame_then_echo_server);

    sim.client("client", async move {
        let (handler, mut rx) = channel_message_handler();
        let client = WebSocketClient::builder()
            .config(websocket_config)
            .message_handler(handler)
            .connect()
            .await
            .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} should connect: {e}"));

        // First connection receives a protocol Close frame; the client must
        // tear it down and reconnect; the second connection announces itself
        assert!(
            recv_text(&mut rx, "after-close").await,
            "{label} seed {seed:#018x} should reconnect after a server close frame"
        );
        assert!(
            wait_for(|| client.is_active()).await,
            "{label} seed {seed:#018x} should be active after the close-frame reconnect"
        );

        client.disconnect().await;

        Ok(())
    });

    sim.run()
        .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} simulation failed: {e:?}"));
}

fn run_websocket_repeated_drops_preserve_message_order(
    mut websocket_config: WebSocketConfig,
    seed: u64,
    label: &'static str,
) {
    websocket_config.connect_timeout_ms = Some(5_000);
    websocket_config.reconnect_delay_initial_ms = Some(25);
    websocket_config.reconnect_delay_max_ms = Some(100);
    websocket_config.reconnect_backoff_factor = Some(1.0);
    websocket_config.reconnect_jitter_ms = Some(0);

    let mut sim = stressed_builder(seed, Duration::from_secs(20)).build();

    sim.host("server", ws_echo_once_then_drop_server);

    sim.client("client", async move {
        let (handler, mut rx) = channel_message_handler();

        let client = WebSocketClient::builder()
            .config(websocket_config)
            .message_handler(handler)
            .connect()
            .await
            .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} should connect: {e}"));

        let expected = (0..6)
            .map(|i| format!("drop-reconnect-{i}"))
            .collect::<Vec<_>>();
        let mut received = Vec::with_capacity(expected.len());

        for (index, msg) in expected.iter().enumerate() {
            client
                .send_text(msg.clone(), None)
                .await
                .expect("Should enqueue message");

            let received_msg = recv_application_text(&mut rx)
                .await
                .unwrap_or_else(|| panic!("{label} seed {seed:#018x} should receive echoed text"));
            assert_eq!(
                &received_msg, msg,
                "{label} seed {seed:#018x} should receive echoed message {index}"
            );
            received.push(received_msg);

            if index + 1 < expected.len() {
                assert!(
                    wait_for(|| client.is_reconnecting() || !client.is_active()).await,
                    "{label} seed {seed:#018x} should observe drop after message {index}"
                );
                assert!(
                    wait_for(|| client.is_active()).await,
                    "{label} seed {seed:#018x} should reconnect after message {index}"
                );
            }
        }

        assert_eq!(
            received, expected,
            "{label} seed {seed:#018x} should preserve message order"
        );

        client.disconnect().await;
        assert!(
            client.is_disconnected(),
            "{label} seed {seed:#018x} should disconnect after scenario"
        );

        Ok(())
    });

    sim.run()
        .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} simulation failed: {e:?}"));
}

fn run_websocket_reconnect_active_drop_preserves_later_message_order(
    mut websocket_config: WebSocketConfig,
    seed: u64,
    label: &'static str,
) {
    websocket_config.connect_timeout_ms = Some(5_000);
    websocket_config.reconnect_delay_initial_ms = Some(25);
    websocket_config.reconnect_delay_max_ms = Some(100);
    websocket_config.reconnect_backoff_factor = Some(1.0);
    websocket_config.reconnect_jitter_ms = Some(0);

    let mut sim = stressed_builder(seed, Duration::from_secs(20)).build();
    let drop_reconnected_connection = Arc::new(AtomicBool::new(false));
    let server_drop_reconnected_connection = Arc::clone(&drop_reconnected_connection);

    sim.host("server", move || {
        let server_drop_reconnected_connection = Arc::clone(&server_drop_reconnected_connection);
        async move {
            ws_echo_first_then_drop_when_reconnect_active_then_echo_server(
                server_drop_reconnected_connection,
            )
            .await
        }
    });

    sim.client("client", async move {
        let (handler, mut rx) =
            tracked_channel_message_handler(Arc::clone(&drop_reconnected_connection));

        let client = WebSocketClient::builder()
            .config(websocket_config)
            .message_handler(handler)
            .connect()
            .await
            .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} should connect: {e}"));

        let first_msg = "before-reconnect-active-drop".to_string();
        client
            .send_text(first_msg.clone(), None)
            .await
            .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} should enqueue first: {e}"));

        assert!(
            recv_text(&mut rx, &first_msg).await,
            "{label} seed {seed:#018x} should receive first echo"
        );

        assert!(
            recv_text(&mut rx, RECONNECTED).await,
            "{label} seed {seed:#018x} should complete the first reconnect"
        );
        assert!(
            recv_text(&mut rx, RECONNECTED).await,
            "{label} seed {seed:#018x} should reconnect again after active drop"
        );

        let expected = (0..4)
            .map(|i| format!("after-active-drop-{i}"))
            .collect::<Vec<_>>();
        let mut received = Vec::with_capacity(expected.len());

        for msg in &expected {
            client
                .send_text(msg.clone(), None)
                .await
                .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} should enqueue {msg}: {e}"));
        }

        while received.len() < expected.len() {
            let received_msg = recv_application_text(&mut rx).await.unwrap_or_else(|| {
                panic!("{label} seed {seed:#018x} should receive later application text")
            });

            let expected_msg = &expected[received.len()];
            assert_eq!(
                &received_msg, expected_msg,
                "{label} seed {seed:#018x} should preserve later message order"
            );
            received.push(received_msg);
        }

        assert_eq!(
            received, expected,
            "{label} seed {seed:#018x} should preserve later message sequence"
        );

        client.disconnect().await;
        assert!(
            client.is_disconnected(),
            "{label} seed {seed:#018x} should disconnect after scenario"
        );

        Ok(())
    });

    sim.run()
        .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} simulation failed: {e:?}"));
}

fn run_websocket_handshake_drop_reaches_active_state(
    mut websocket_config: WebSocketConfig,
    seed: u64,
    label: &'static str,
) {
    websocket_config.connect_timeout_ms = Some(5_000);
    websocket_config.reconnect_delay_initial_ms = Some(25);
    websocket_config.reconnect_delay_max_ms = Some(100);
    websocket_config.reconnect_backoff_factor = Some(1.0);
    websocket_config.reconnect_jitter_ms = Some(0);

    let mut sim = stressed_builder(seed, Duration::from_secs(20)).build();
    let handshake_dropped = Arc::new(AtomicBool::new(false));
    let server_handshake_dropped = Arc::clone(&handshake_dropped);

    sim.host("server", move || {
        let server_handshake_dropped = Arc::clone(&server_handshake_dropped);
        async move { ws_drop_reconnect_handshake_then_echo_server(server_handshake_dropped).await }
    });

    sim.client("client", async move {
        let (handler, _rx) = channel_message_handler();

        let client = WebSocketClient::builder()
            .config(websocket_config)
            .message_handler(handler)
            .connect()
            .await
            .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} should connect: {e}"));

        assert!(
            wait_for(|| handshake_dropped.load(Ordering::SeqCst)).await,
            "{label} seed {seed:#018x} should drop a reconnect handshake"
        );
        assert!(
            wait_for(|| client.is_active()).await,
            "{label} seed {seed:#018x} should reconnect after handshake drop"
        );

        client.disconnect().await;
        assert!(
            client.is_disconnected(),
            "{label} seed {seed:#018x} should disconnect after scenario"
        );

        Ok(())
    });

    sim.run()
        .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} simulation failed: {e:?}"));
}

fn run_websocket_first_read_task_drop_reaches_active_state(
    mut websocket_config: WebSocketConfig,
    seed: u64,
    label: &'static str,
) {
    websocket_config.connect_timeout_ms = Some(5_000);
    websocket_config.reconnect_delay_initial_ms = Some(25);
    websocket_config.reconnect_delay_max_ms = Some(100);
    websocket_config.reconnect_backoff_factor = Some(1.0);
    websocket_config.reconnect_jitter_ms = Some(0);

    let mut sim = stressed_builder(seed, Duration::from_secs(20)).build();
    let first_connection_dropped = Arc::new(AtomicBool::new(false));
    let server_first_connection_dropped = Arc::clone(&first_connection_dropped);

    sim.host("server", move || {
        let server_first_connection_dropped = Arc::clone(&server_first_connection_dropped);
        async move {
            ws_drop_first_connection_before_read_then_echo_server(server_first_connection_dropped)
                .await
        }
    });

    sim.client("client", async move {
        let reconnected = Arc::new(AtomicBool::new(false));
        let (handler, _rx) = tracked_channel_message_handler(Arc::clone(&reconnected));

        let client = WebSocketClient::builder()
            .config(websocket_config)
            .message_handler(handler)
            .connect()
            .await
            .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} should connect: {e}"));

        assert!(
            wait_for(|| first_connection_dropped.load(Ordering::SeqCst)).await,
            "{label} seed {seed:#018x} should drop the first connection before reads"
        );
        assert!(
            wait_for(|| client.is_reconnecting() || reconnected.load(Ordering::SeqCst)).await,
            "{label} seed {seed:#018x} should enter reconnect after first read task drop"
        );
        assert!(
            wait_for(|| reconnected.load(Ordering::SeqCst) && client.is_active()).await,
            "{label} seed {seed:#018x} should become active after first read task drop"
        );

        client.disconnect().await;
        assert!(
            client.is_disconnected(),
            "{label} seed {seed:#018x} should disconnect after scenario"
        );

        Ok(())
    });

    sim.run()
        .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} simulation failed: {e:?}"));
}

fn run_websocket_partition_while_reconnecting_reaches_active_state(
    mut websocket_config: WebSocketConfig,
    seed: u64,
    label: &'static str,
) {
    websocket_config.connect_timeout_ms = Some(1_000);
    websocket_config.reconnect_delay_initial_ms = Some(25);
    websocket_config.reconnect_delay_max_ms = Some(100);
    websocket_config.reconnect_backoff_factor = Some(1.0);
    websocket_config.reconnect_jitter_ms = Some(0);

    let mut sim = stressed_builder(seed, Duration::from_secs(20)).build();
    let first_connection_dropped = Arc::new(AtomicBool::new(false));
    let reconnect_repaired = Arc::new(AtomicBool::new(false));
    let server_first_connection_dropped = Arc::clone(&first_connection_dropped);
    let server_reconnect_repaired = Arc::clone(&reconnect_repaired);

    sim.host("server", move || {
        let server_first_connection_dropped = Arc::clone(&server_first_connection_dropped);
        let server_reconnect_repaired = Arc::clone(&server_reconnect_repaired);
        async move {
            ws_drop_first_connection_wait_for_repair_then_echo_server(
                server_first_connection_dropped,
                server_reconnect_repaired,
            )
            .await
        }
    });

    sim.client("client", async move {
        let reconnected = Arc::new(AtomicBool::new(false));
        let (handler, _rx) = tracked_channel_message_handler(Arc::clone(&reconnected));

        let client = WebSocketClient::builder()
            .config(websocket_config)
            .message_handler(handler)
            .connect()
            .await
            .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} should connect: {e}"));

        assert!(
            wait_for(|| first_connection_dropped.load(Ordering::SeqCst)).await,
            "{label} seed {seed:#018x} should drop the first connection"
        );
        assert!(
            wait_for(|| client.is_reconnecting()).await,
            "{label} seed {seed:#018x} should enter reconnect before partition"
        );

        turmoil::partition("client", "server");
        tokio::time::sleep(Duration::from_millis(1_200)).await;
        assert!(
            client.is_reconnecting(),
            "{label} seed {seed:#018x} should stay reconnecting while partitioned"
        );

        turmoil::repair("client", "server");
        reconnect_repaired.store(true, Ordering::SeqCst);

        assert!(
            wait_for(|| reconnected.load(Ordering::SeqCst) && client.is_active()).await,
            "{label} seed {seed:#018x} should become active after partition repair"
        );

        client.disconnect().await;
        assert!(
            client.is_disconnected(),
            "{label} seed {seed:#018x} should disconnect after scenario"
        );

        Ok(())
    });

    sim.run()
        .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} simulation failed: {e:?}"));
}

fn run_websocket_partition_during_backoff_sleep_reaches_active_state(
    mut websocket_config: WebSocketConfig,
    seed: u64,
    label: &'static str,
) {
    websocket_config.connect_timeout_ms = Some(1_000);
    websocket_config.reconnect_delay_initial_ms = Some(1_000);
    websocket_config.reconnect_delay_max_ms = Some(1_000);
    websocket_config.reconnect_backoff_factor = Some(1.0);
    websocket_config.reconnect_jitter_ms = Some(0);

    let mut sim = stressed_builder(seed, Duration::from_secs(20)).build();
    let first_handshake_dropped = Arc::new(AtomicBool::new(false));
    let second_handshake_dropped = Arc::new(AtomicBool::new(false));
    let server_first_handshake_dropped = Arc::clone(&first_handshake_dropped);
    let server_second_handshake_dropped = Arc::clone(&second_handshake_dropped);

    sim.host("server", move || {
        let server_first_handshake_dropped = Arc::clone(&server_first_handshake_dropped);
        let server_second_handshake_dropped = Arc::clone(&server_second_handshake_dropped);
        async move {
            ws_drop_two_reconnect_handshakes_then_echo_server(
                server_first_handshake_dropped,
                server_second_handshake_dropped,
            )
            .await
        }
    });

    sim.client("client", async move {
        let reconnected = Arc::new(AtomicBool::new(false));
        let (handler, _rx) = tracked_channel_message_handler(Arc::clone(&reconnected));

        let client = WebSocketClient::builder()
            .config(websocket_config)
            .message_handler(handler)
            .connect()
            .await
            .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} should connect: {e}"));

        assert!(
            wait_for(|| first_handshake_dropped.load(Ordering::SeqCst)).await,
            "{label} seed {seed:#018x} should drop the first reconnect handshake"
        );
        assert!(
            wait_for(|| second_handshake_dropped.load(Ordering::SeqCst)).await,
            "{label} seed {seed:#018x} should drop the immediate reconnect handshake"
        );
        assert!(
            wait_for(|| client.is_reconnecting()).await,
            "{label} seed {seed:#018x} should enter reconnect before backoff partition"
        );

        tokio::time::sleep(Duration::from_millis(200)).await;
        turmoil::partition("client", "server");
        tokio::time::sleep(Duration::from_millis(2_500)).await;
        assert!(
            !reconnected.load(Ordering::SeqCst),
            "{label} seed {seed:#018x} should not reconnect while partitioned"
        );
        assert!(
            client.is_reconnecting(),
            "{label} seed {seed:#018x} should stay reconnecting after partitioned retry"
        );

        turmoil::repair("client", "server");

        assert!(
            wait_for(|| reconnected.load(Ordering::SeqCst) && client.is_active()).await,
            "{label} seed {seed:#018x} should become active after backoff partition repair"
        );

        client.disconnect().await;
        assert!(
            client.is_disconnected(),
            "{label} seed {seed:#018x} should disconnect after scenario"
        );

        Ok(())
    });

    sim.run()
        .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} simulation failed: {e:?}"));
}

fn run_websocket_silent_until_idle_timeout_reconnects_to_active_state(
    mut websocket_config: WebSocketConfig,
    seed: u64,
    label: &'static str,
) {
    websocket_config.idle_timeout_ms = Some(500);
    run_websocket_silent_until_liveness_timeout_reconnects_to_active_state(
        websocket_config,
        seed,
        label,
    );
}

/// A configured heartbeat with no explicit timeout must still tear down a peer that has gone dark,
/// because that derived window is what gives most adapters any dead-peer detection at all.
fn run_websocket_dark_until_derived_heartbeat_timeout_reconnects_to_active_state(
    mut websocket_config: WebSocketConfig,
    seed: u64,
    label: &'static str,
) {
    websocket_config.heartbeat_interval_secs = Some(1);
    websocket_config.heartbeat_timeout_secs = None;
    websocket_config.connect_timeout_ms = Some(5_000);
    websocket_config.reconnect_delay_initial_ms = Some(25);
    websocket_config.reconnect_delay_max_ms = Some(100);
    websocket_config.reconnect_backoff_factor = Some(1.0);
    websocket_config.reconnect_jitter_ms = Some(0);

    let mut sim = stressed_builder(seed, Duration::from_secs(30)).build();
    let first_connection_dark = Arc::new(AtomicBool::new(false));
    let server_first_connection_dark = Arc::clone(&first_connection_dark);

    sim.host("server", move || {
        let server_first_connection_dark = Arc::clone(&server_first_connection_dark);
        async move { ws_dark_first_connection_then_echo_server(server_first_connection_dark).await }
    });

    sim.client("client", async move {
        let reconnected = Arc::new(AtomicBool::new(false));
        let (handler, _rx) = tracked_channel_message_handler(Arc::clone(&reconnected));

        let client = WebSocketClient::builder()
            .config(websocket_config)
            .message_handler(handler)
            .connect()
            .await
            .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} should connect: {e}"));

        assert!(
            wait_for(|| first_connection_dark.load(Ordering::SeqCst)).await,
            "{label} seed {seed:#018x} should enter the dark first connection"
        );
        assert!(
            wait_for_within(1_000, || reconnected.load(Ordering::SeqCst) && client.is_active())
                .await,
            "{label} seed {seed:#018x} should become active after derived heartbeat-timeout reconnect"
        );

        client.disconnect().await;
        assert!(
            client.is_disconnected(),
            "{label} seed {seed:#018x} should disconnect after scenario"
        );

        Ok(())
    });

    sim.run()
        .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} simulation failed: {e:?}"));
}

fn run_websocket_silent_until_liveness_timeout_reconnects_to_active_state(
    mut websocket_config: WebSocketConfig,
    seed: u64,
    label: &'static str,
) {
    websocket_config.connect_timeout_ms = Some(5_000);
    websocket_config.reconnect_delay_initial_ms = Some(25);
    websocket_config.reconnect_delay_max_ms = Some(100);
    websocket_config.reconnect_backoff_factor = Some(1.0);
    websocket_config.reconnect_jitter_ms = Some(0);

    let mut sim = stressed_builder(seed, Duration::from_secs(20)).build();
    let first_connection_silent = Arc::new(AtomicBool::new(false));
    let server_first_connection_silent = Arc::clone(&first_connection_silent);

    sim.host("server", move || {
        let server_first_connection_silent = Arc::clone(&server_first_connection_silent);
        async move {
            ws_silent_first_connection_then_echo_server(server_first_connection_silent).await
        }
    });

    sim.client("client", async move {
        let reconnected = Arc::new(AtomicBool::new(false));
        let (handler, _rx) = tracked_channel_message_handler(Arc::clone(&reconnected));

        let client = WebSocketClient::builder()
            .config(websocket_config)
            .message_handler(handler)
            .connect()
            .await
            .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} should connect: {e}"));

        assert!(
            wait_for(|| first_connection_silent.load(Ordering::SeqCst)).await,
            "{label} seed {seed:#018x} should enter the silent first connection"
        );
        assert!(
            wait_for(|| reconnected.load(Ordering::SeqCst) && client.is_active()).await,
            "{label} seed {seed:#018x} should become active after idle-timeout reconnect"
        );

        client.disconnect().await;
        assert!(
            client.is_disconnected(),
            "{label} seed {seed:#018x} should disconnect after scenario"
        );

        Ok(())
    });

    sim.run()
        .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} simulation failed: {e:?}"));
}

fn run_websocket_no_read_backpressure_reconnects_to_active_state(
    mut websocket_config: WebSocketConfig,
    seed: u64,
    label: &'static str,
) {
    websocket_config.connect_timeout_ms = Some(5_000);
    websocket_config.reconnect_delay_initial_ms = Some(25);
    websocket_config.reconnect_delay_max_ms = Some(100);
    websocket_config.reconnect_backoff_factor = Some(1.0);
    websocket_config.reconnect_jitter_ms = Some(0);

    let mut builder = stressed_builder(seed, Duration::from_secs(20));
    builder.tcp_capacity(BACKPRESSURE_TCP_CAPACITY);
    let mut sim = builder.build();

    let first_connection_held = Arc::new(AtomicBool::new(false));
    let release_first_connection = Arc::new(AtomicBool::new(false));
    let server_first_connection_held = Arc::clone(&first_connection_held);
    let server_release_first_connection = Arc::clone(&release_first_connection);

    sim.host("server", move || {
        let server_first_connection_held = Arc::clone(&server_first_connection_held);
        let server_release_first_connection = Arc::clone(&server_release_first_connection);
        async move {
            ws_hold_first_connection_until_release_then_echo_server(
                server_first_connection_held,
                server_release_first_connection,
            )
            .await
        }
    });

    sim.client("client", async move {
        let reconnected = Arc::new(AtomicBool::new(false));
        let (handler, _rx) = tracked_channel_message_handler(Arc::clone(&reconnected));

        let client = WebSocketClient::builder()
            .config(websocket_config)
            .message_handler(handler)
            .connect()
            .await
            .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} should connect: {e}"));

        assert!(
            wait_for(|| first_connection_held.load(Ordering::SeqCst)).await,
            "{label} seed {seed:#018x} should hold the first connection without reads"
        );

        for index in 0..BACKPRESSURE_MESSAGE_COUNT {
            client
                .send_bytes(
                    patterned_bytes(index as u8, LARGE_WEBSOCKET_MESSAGE_LEN),
                    None,
                )
                .await
                .unwrap_or_else(|e| {
                    panic!(
                        "{label} seed {seed:#018x} should enqueue backpressure message {index}: {e}"
                    )
                });
        }

        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(
            client.is_active(),
            "{label} seed {seed:#018x} should stay active while the no-read peer is connected"
        );
        assert!(
            !reconnected.load(Ordering::SeqCst),
            "{label} seed {seed:#018x} should not reconnect before the no-read peer releases"
        );

        release_first_connection.store(true, Ordering::SeqCst);

        assert!(
            wait_for(|| client.is_reconnecting() || reconnected.load(Ordering::SeqCst)).await,
            "{label} seed {seed:#018x} should enter reconnect after no-read peer releases"
        );
        assert!(
            wait_for(|| reconnected.load(Ordering::SeqCst) && client.is_active()).await,
            "{label} seed {seed:#018x} should become active after no-read backpressure clears"
        );

        client.disconnect().await;
        assert!(
            client.is_disconnected(),
            "{label} seed {seed:#018x} should disconnect after scenario"
        );

        Ok(())
    });

    sim.run()
        .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} simulation failed: {e:?}"));
}

fn run_websocket_disconnect_while_send_waits_for_reconnect_closes_send(
    mut websocket_config: WebSocketConfig,
    seed: u64,
    label: &'static str,
) {
    websocket_config.connect_timeout_ms = Some(5_000);
    websocket_config.reconnect_delay_initial_ms = Some(25);
    websocket_config.reconnect_delay_max_ms = Some(100);
    websocket_config.reconnect_backoff_factor = Some(1.0);
    websocket_config.reconnect_jitter_ms = Some(0);

    let mut sim = stressed_builder(seed, Duration::from_secs(20)).build();
    let first_connection_dropped = Arc::new(AtomicBool::new(false));
    let reconnect_attempt_waiting = Arc::new(AtomicBool::new(false));
    let release_reconnect_attempt = Arc::new(AtomicBool::new(false));
    let server_first_connection_dropped = Arc::clone(&first_connection_dropped);
    let server_reconnect_attempt_waiting = Arc::clone(&reconnect_attempt_waiting);
    let server_release_reconnect_attempt = Arc::clone(&release_reconnect_attempt);

    sim.host("server", move || {
        let server_first_connection_dropped = Arc::clone(&server_first_connection_dropped);
        let server_reconnect_attempt_waiting = Arc::clone(&server_reconnect_attempt_waiting);
        let server_release_reconnect_attempt = Arc::clone(&server_release_reconnect_attempt);
        async move {
            ws_drop_first_connection_then_hold_reconnect_handshake_until_release_server(
                server_first_connection_dropped,
                server_reconnect_attempt_waiting,
                server_release_reconnect_attempt,
            )
            .await
        }
    });

    sim.client("client", async move {
        let (handler, _rx) = channel_message_handler();

        let client = Arc::new(
            WebSocketClient::builder()
                .config(websocket_config)
                .message_handler(handler)
                .connect()
                .await
                .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} should connect: {e}")),
        );

        assert!(
            wait_for(|| first_connection_dropped.load(Ordering::SeqCst)).await,
            "{label} seed {seed:#018x} should drop the first connection"
        );
        assert!(
            wait_for(|| client.is_reconnecting()).await,
            "{label} seed {seed:#018x} should enter reconnect before sending"
        );
        assert!(
            wait_for(|| reconnect_attempt_waiting.load(Ordering::SeqCst)).await,
            "{label} seed {seed:#018x} should hold the reconnect handshake"
        );

        let send_client = Arc::clone(&client);
        let send_handle = tokio::spawn(async move {
            send_client
                .send_text("send-waiting-for-reconnect".to_string(), None)
                .await
        });

        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(
            !send_handle.is_finished(),
            "{label} seed {seed:#018x} send should wait for reconnect before disconnect"
        );

        client.disconnect().await;

        let send_result = tokio::time::timeout(Duration::from_secs(2), send_handle)
            .await
            .unwrap_or_else(|_| panic!("{label} seed {seed:#018x} send should finish promptly"))
            .unwrap_or_else(|e| {
                panic!("{label} seed {seed:#018x} send task should not panic: {e}")
            });

        assert!(
            matches!(send_result, Err(SendError::Closed)),
            "{label} seed {seed:#018x} send should close on disconnect, was: {send_result:?}"
        );
        assert!(
            client.is_disconnected(),
            "{label} seed {seed:#018x} should finish controller after disconnect"
        );
        assert!(
            client.is_closed(),
            "{label} seed {seed:#018x} should enter closed state after disconnect"
        );
        assert!(
            !client.is_reconnecting(),
            "{label} seed {seed:#018x} should not stay reconnecting after disconnect"
        );
        assert!(
            !client.is_active(),
            "{label} seed {seed:#018x} should not stay active after disconnect"
        );

        release_reconnect_attempt.store(true, Ordering::SeqCst);

        Ok(())
    });

    sim.run()
        .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} simulation failed: {e:?}"));
}

fn run_websocket_disconnect_while_waiting_for_auth_closes_client(
    mut websocket_config: WebSocketConfig,
    seed: u64,
    label: &'static str,
) {
    websocket_config.connect_timeout_ms = Some(5_000);
    websocket_config.reconnect_delay_initial_ms = Some(25);
    websocket_config.reconnect_delay_max_ms = Some(100);
    websocket_config.reconnect_backoff_factor = Some(1.0);
    websocket_config.reconnect_jitter_ms = Some(0);

    let mut sim = stressed_builder(seed, Duration::from_secs(20)).build();
    let first_connection_dropped = Arc::new(AtomicBool::new(false));
    let server_first_connection_dropped = Arc::clone(&first_connection_dropped);

    sim.host("server", move || {
        let server_first_connection_dropped = Arc::clone(&server_first_connection_dropped);
        async move {
            ws_drop_first_connection_before_read_then_echo_server(server_first_connection_dropped)
                .await
        }
    });

    sim.client("client", async move {
        let tracker = AuthTracker::new();
        let reconnected = Arc::new(AtomicBool::new(false));
        let (handler, _rx) = tracked_channel_message_handler(Arc::clone(&reconnected));

        let client = WebSocketClient::builder()
            .config(websocket_config)
            .message_handler(handler)
            .connect()
            .await
            .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} should connect: {e}"));

        client.set_auth_tracker(tracker.clone(), true);
        tracker.succeed();

        assert!(
            wait_for(|| first_connection_dropped.load(Ordering::SeqCst)).await,
            "{label} seed {seed:#018x} should drop the first connection"
        );
        assert!(
            wait_for(|| reconnected.load(Ordering::SeqCst) && client.is_active()).await,
            "{label} seed {seed:#018x} should reconnect before auth wait"
        );

        let _auth_receiver = tracker.begin();
        let wait_tracker = tracker.clone();
        let auth_wait = tokio::spawn(async move {
            wait_tracker
                .wait_for_authenticated(Duration::from_secs(10))
                .await
        });

        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(
            !auth_wait.is_finished(),
            "{label} seed {seed:#018x} auth wait should be pending before disconnect"
        );

        client.disconnect().await;

        let authenticated = tokio::time::timeout(Duration::from_secs(2), auth_wait)
            .await
            .unwrap_or_else(|_| {
                panic!("{label} seed {seed:#018x} auth wait should finish promptly")
            })
            .unwrap_or_else(|e| {
                panic!("{label} seed {seed:#018x} auth wait task should not panic: {e}")
            });

        assert!(
            !authenticated,
            "{label} seed {seed:#018x} auth wait should be interrupted by disconnect"
        );
        assert!(
            client.is_disconnected(),
            "{label} seed {seed:#018x} should finish controller after disconnect"
        );
        assert!(
            client.is_closed(),
            "{label} seed {seed:#018x} should enter closed state after disconnect"
        );
        assert!(
            !client.is_reconnecting(),
            "{label} seed {seed:#018x} should not stay reconnecting after disconnect"
        );
        assert!(
            !client.is_active(),
            "{label} seed {seed:#018x} should not stay active after disconnect"
        );

        Ok(())
    });

    sim.run()
        .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} simulation failed: {e:?}"));
}

fn run_websocket_max_reconnect_attempts_while_waiting_for_auth_closes_client(
    mut websocket_config: WebSocketConfig,
    seed: u64,
    label: &'static str,
) {
    websocket_config.connect_timeout_ms = Some(500);
    websocket_config.reconnect_delay_initial_ms = Some(25);
    websocket_config.reconnect_delay_max_ms = Some(25);
    websocket_config.reconnect_backoff_factor = Some(1.0);
    websocket_config.reconnect_jitter_ms = Some(0);
    websocket_config.reconnect_max_attempts = Some(1);

    let mut sim = stressed_builder(seed, Duration::from_secs(20)).build();
    let first_connection_dropped = Arc::new(AtomicBool::new(false));
    let reconnect_attempt_waiting = Arc::new(AtomicBool::new(false));
    let release_reconnect_attempt = Arc::new(AtomicBool::new(false));
    let server_first_connection_dropped = Arc::clone(&first_connection_dropped);
    let server_reconnect_attempt_waiting = Arc::clone(&reconnect_attempt_waiting);
    let server_release_reconnect_attempt = Arc::clone(&release_reconnect_attempt);

    sim.host("server", move || {
        let server_first_connection_dropped = Arc::clone(&server_first_connection_dropped);
        let server_reconnect_attempt_waiting = Arc::clone(&server_reconnect_attempt_waiting);
        let server_release_reconnect_attempt = Arc::clone(&server_release_reconnect_attempt);
        async move {
            ws_drop_first_connection_then_hold_reconnect_handshake_until_release_server(
                server_first_connection_dropped,
                server_reconnect_attempt_waiting,
                server_release_reconnect_attempt,
            )
            .await
        }
    });

    sim.client("client", async move {
        let tracker = AuthTracker::new();
        let (handler, _rx) = channel_message_handler();

        let client = WebSocketClient::builder()
            .config(websocket_config)
            .message_handler(handler)
            .connect()
            .await
            .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} should connect: {e}"));

        client.set_auth_tracker(tracker.clone(), true);
        tracker.succeed();

        assert!(
            wait_for(|| first_connection_dropped.load(Ordering::SeqCst)).await,
            "{label} seed {seed:#018x} should drop the first connection"
        );
        assert!(
            wait_for(|| reconnect_attempt_waiting.load(Ordering::SeqCst)).await,
            "{label} seed {seed:#018x} should hold the reconnect handshake"
        );

        let _auth_receiver = tracker.begin();
        let wait_tracker = tracker.clone();
        let auth_wait = tokio::spawn(async move {
            wait_tracker
                .wait_for_authenticated(Duration::from_secs(10))
                .await
        });

        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            !auth_wait.is_finished(),
            "{label} seed {seed:#018x} auth wait should be pending before max attempts"
        );

        let authenticated = tokio::time::timeout(Duration::from_secs(3), auth_wait)
            .await
            .unwrap_or_else(|_| {
                panic!("{label} seed {seed:#018x} auth wait should finish promptly")
            })
            .unwrap_or_else(|e| {
                panic!("{label} seed {seed:#018x} auth wait task should not panic: {e}")
            });

        assert!(
            !authenticated,
            "{label} seed {seed:#018x} auth wait should be interrupted by max attempts"
        );
        assert!(
            wait_for(|| client.is_disconnected() && client.is_closed()).await,
            "{label} seed {seed:#018x} should finish controller after max attempts"
        );
        assert!(
            !client.is_reconnecting(),
            "{label} seed {seed:#018x} should not stay reconnecting after max attempts"
        );
        assert!(
            !client.is_active(),
            "{label} seed {seed:#018x} should not stay active after max attempts"
        );

        release_reconnect_attempt.store(true, Ordering::SeqCst);

        Ok(())
    });

    sim.run()
        .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} simulation failed: {e:?}"));
}

fn run_websocket_stream_notify_closed_while_waiting_for_auth_closes_client(
    websocket_config: WebSocketConfig,
    seed: u64,
    label: &'static str,
) {
    let mut sim = stressed_builder(seed, Duration::from_secs(20)).build();

    sim.host("server", ws_echo_server);

    sim.client("client", async move {
        let tracker = AuthTracker::new();
        let (_reader, client) = WebSocketClient::stream_builder()
            .config(websocket_config)
            .connect()
            .await
            .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} should connect: {e}"));

        client.set_auth_tracker(tracker.clone(), true);
        tracker.succeed();

        let _auth_receiver = tracker.begin();
        let wait_tracker = tracker.clone();
        let auth_wait = tokio::spawn(async move {
            wait_tracker
                .wait_for_authenticated(Duration::from_secs(10))
                .await
        });

        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            !auth_wait.is_finished(),
            "{label} seed {seed:#018x} auth wait should be pending before notify_closed"
        );

        client.notify_closed();

        let authenticated = tokio::time::timeout(Duration::from_secs(2), auth_wait)
            .await
            .unwrap_or_else(|_| {
                panic!("{label} seed {seed:#018x} auth wait should finish promptly")
            })
            .unwrap_or_else(|e| {
                panic!("{label} seed {seed:#018x} auth wait task should not panic: {e}")
            });

        assert!(
            !authenticated,
            "{label} seed {seed:#018x} auth wait should be interrupted by notify_closed"
        );
        assert!(
            wait_for(|| client.is_disconnected() && client.is_closed()).await,
            "{label} seed {seed:#018x} should finish controller after notify_closed"
        );
        assert!(
            !client.is_reconnecting(),
            "{label} seed {seed:#018x} should not reconnect after notify_closed"
        );
        assert!(
            !client.is_active(),
            "{label} seed {seed:#018x} should not stay active after notify_closed"
        );

        Ok(())
    });

    sim.run()
        .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} simulation failed: {e:?}"));
}

fn run_websocket_stream_dead_write_while_waiting_for_auth_closes_client(
    websocket_config: WebSocketConfig,
    seed: u64,
    label: &'static str,
) {
    let mut sim = stressed_builder(seed, Duration::from_secs(20)).build();
    let first_connection_dropped = Arc::new(AtomicBool::new(false));
    let server_first_connection_dropped = Arc::clone(&first_connection_dropped);

    sim.host("server", move || {
        let server_first_connection_dropped = Arc::clone(&server_first_connection_dropped);
        async move {
            ws_drop_first_connection_before_read_then_echo_server(server_first_connection_dropped)
                .await
        }
    });

    sim.client("client", async move {
        let tracker = AuthTracker::new();
        let (_reader, client) = WebSocketClient::stream_builder()
            .config(websocket_config)
            .connect()
            .await
            .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} should connect: {e}"));

        client.set_auth_tracker(tracker.clone(), true);
        tracker.succeed();

        assert!(
            wait_for(|| first_connection_dropped.load(Ordering::SeqCst)).await,
            "{label} seed {seed:#018x} should drop the stream connection"
        );

        let _auth_receiver = tracker.begin();
        let wait_tracker = tracker.clone();
        let auth_wait = tokio::spawn(async move {
            wait_tracker
                .wait_for_authenticated(Duration::from_secs(10))
                .await
        });

        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            !auth_wait.is_finished(),
            "{label} seed {seed:#018x} auth wait should be pending before dead write"
        );

        for attempt in 0..20 {
            if !client.is_active() || auth_wait.is_finished() {
                break;
            }
            let _ = client
                .send_text(format!("stream-dead-write-{attempt}"), None)
                .await;
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        let authenticated = tokio::time::timeout(Duration::from_secs(2), auth_wait)
            .await
            .unwrap_or_else(|_| {
                panic!("{label} seed {seed:#018x} auth wait should finish promptly")
            })
            .unwrap_or_else(|e| {
                panic!("{label} seed {seed:#018x} auth wait task should not panic: {e}")
            });

        assert!(
            !authenticated,
            "{label} seed {seed:#018x} auth wait should be interrupted by stream close"
        );
        assert!(
            wait_for(|| client.is_disconnected() && client.is_closed()).await,
            "{label} seed {seed:#018x} should finish controller after stream close"
        );
        assert!(
            !client.is_reconnecting(),
            "{label} seed {seed:#018x} stream mode should not stay reconnecting"
        );
        assert!(
            !client.is_active(),
            "{label} seed {seed:#018x} stream mode should not stay active"
        );

        Ok(())
    });

    sim.run()
        .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} simulation failed: {e:?}"));
}

fn run_websocket_reconnectable_drop_while_waiting_for_auth_waits_for_reauth(
    mut websocket_config: WebSocketConfig,
    seed: u64,
    label: &'static str,
) {
    websocket_config.connect_timeout_ms = Some(5_000);
    websocket_config.reconnect_delay_initial_ms = Some(25);
    websocket_config.reconnect_delay_max_ms = Some(100);
    websocket_config.reconnect_backoff_factor = Some(1.0);
    websocket_config.reconnect_jitter_ms = Some(0);
    websocket_config.idle_timeout_ms = Some(500);

    let mut sim = stressed_builder(seed, Duration::from_secs(20)).build();
    let first_connection_silent = Arc::new(AtomicBool::new(false));
    let server_first_connection_silent = Arc::clone(&first_connection_silent);

    sim.host("server", move || {
        let server_first_connection_silent = Arc::clone(&server_first_connection_silent);
        async move {
            ws_silent_first_connection_then_echo_server(server_first_connection_silent).await
        }
    });

    sim.client("client", async move {
        let tracker = AuthTracker::new();
        let reconnected = Arc::new(AtomicBool::new(false));
        let (handler, _rx) = tracked_channel_message_handler(Arc::clone(&reconnected));

        let client = WebSocketClient::builder()
            .config(websocket_config)
            .message_handler(handler)
            .connect()
            .await
            .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} should connect: {e}"));

        client.set_auth_tracker(tracker.clone(), true);
        tracker.succeed();

        assert!(
            wait_for(|| first_connection_silent.load(Ordering::SeqCst)).await,
            "{label} seed {seed:#018x} should enter the silent first connection"
        );

        let _auth_receiver = tracker.begin();
        let wait_tracker = tracker.clone();
        let auth_wait = tokio::spawn(async move {
            wait_tracker
                .wait_for_authenticated(Duration::from_secs(10))
                .await
        });

        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            !auth_wait.is_finished(),
            "{label} seed {seed:#018x} auth wait should be pending before reconnect"
        );
        assert!(
            wait_for(|| reconnected.load(Ordering::SeqCst) && client.is_active()).await,
            "{label} seed {seed:#018x} should reconnect after idle timeout"
        );
        assert!(
            !auth_wait.is_finished(),
            "{label} seed {seed:#018x} auth wait should remain pending until re-auth"
        );

        tracker.succeed();

        let authenticated = tokio::time::timeout(Duration::from_secs(2), auth_wait)
            .await
            .unwrap_or_else(|_| {
                panic!("{label} seed {seed:#018x} auth wait should finish after re-auth")
            })
            .unwrap_or_else(|e| {
                panic!("{label} seed {seed:#018x} auth wait task should not panic: {e}")
            });

        assert!(
            authenticated,
            "{label} seed {seed:#018x} auth wait should complete after re-auth"
        );
        assert!(
            client.is_active(),
            "{label} seed {seed:#018x} should stay active after re-auth"
        );
        assert!(
            !client.is_reconnecting(),
            "{label} seed {seed:#018x} should not stay reconnecting after re-auth"
        );
        assert!(
            !client.is_closed(),
            "{label} seed {seed:#018x} should not close on reconnectable drop"
        );

        client.disconnect().await;
        assert!(
            client.is_disconnected(),
            "{label} seed {seed:#018x} should disconnect after scenario"
        );

        Ok(())
    });

    sim.run()
        .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} simulation failed: {e:?}"));
}

fn run_websocket_alternating_text_binary_preserves_message_order(
    mut websocket_config: WebSocketConfig,
    seed: u64,
    label: &'static str,
) {
    websocket_config.connect_timeout_ms = Some(5_000);
    websocket_config.reconnect_delay_initial_ms = Some(25);
    websocket_config.reconnect_delay_max_ms = Some(100);
    websocket_config.reconnect_backoff_factor = Some(1.0);
    websocket_config.reconnect_jitter_ms = Some(0);

    let mut sim = stressed_builder(seed, Duration::from_secs(20)).build();

    sim.host("server", ws_echo_server);

    sim.client("client", async move {
        let (handler, mut rx) = channel_message_handler();

        let client = WebSocketClient::builder()
            .config(websocket_config)
            .message_handler(handler)
            .connect()
            .await
            .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} should connect: {e}"));

        let expected = alternating_text_binary_messages();
        let mut received = Vec::with_capacity(expected.len());

        for (index, msg) in expected.iter().enumerate() {
            match msg {
                ApplicationMessage::Text(text) => client
                    .send_text(text.clone(), None)
                    .await
                    .unwrap_or_else(|e| {
                        panic!("{label} seed {seed:#018x} should enqueue text {index}: {e}")
                    }),
                ApplicationMessage::Binary(data) => client
                    .send_bytes(data.clone(), None)
                    .await
                    .unwrap_or_else(|e| {
                        panic!("{label} seed {seed:#018x} should enqueue binary {index}: {e}")
                    }),
            }
        }

        while received.len() < expected.len() {
            let received_msg = recv_application_message(&mut rx).await.unwrap_or_else(|| {
                panic!("{label} seed {seed:#018x} should receive application message")
            });

            let expected_msg = &expected[received.len()];
            assert_eq!(
                &received_msg, expected_msg,
                "{label} seed {seed:#018x} should preserve text/binary order"
            );
            received.push(received_msg);
        }

        assert_eq!(
            received, expected,
            "{label} seed {seed:#018x} should preserve full text/binary sequence"
        );

        client.disconnect().await;
        assert!(
            client.is_disconnected(),
            "{label} seed {seed:#018x} should disconnect after scenario"
        );

        Ok(())
    });

    sim.run()
        .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} simulation failed: {e:?}"));
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ApplicationMessage {
    Text(String),
    Binary(Vec<u8>),
}

async fn recv_application_message(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<Message>,
) -> Option<ApplicationMessage> {
    for _ in 0..POLL_ITERS {
        while let Ok(msg) = rx.try_recv() {
            match msg {
                Message::Text(text) if text.as_str() != RECONNECTED => {
                    return Some(ApplicationMessage::Text(text.to_string()));
                }
                Message::Binary(data) => {
                    return Some(ApplicationMessage::Binary(data.to_vec()));
                }
                _ => {}
            }
        }
        tokio::time::sleep(POLL_STEP).await;
    }
    None
}

fn alternating_text_binary_messages() -> Vec<ApplicationMessage> {
    vec![
        ApplicationMessage::Text("small-text-0".to_string()),
        ApplicationMessage::Binary(vec![0x00, 0x7f, 0x80, 0xff]),
        ApplicationMessage::Text(repeated_text("large-text-0:", LARGE_WEBSOCKET_MESSAGE_LEN)),
        ApplicationMessage::Binary(patterned_bytes(0x10, LARGE_WEBSOCKET_MESSAGE_LEN)),
        ApplicationMessage::Text("small-text-1".to_string()),
        ApplicationMessage::Binary(vec![0xfe, 0xed, 0xfa, 0xce]),
        ApplicationMessage::Text(repeated_text(
            "large-text-1:",
            LARGE_WEBSOCKET_MESSAGE_LEN + 257,
        )),
        ApplicationMessage::Binary(patterned_bytes(0x40, LARGE_WEBSOCKET_MESSAGE_LEN + 257)),
    ]
}

fn repeated_text(pattern: &str, len: usize) -> String {
    let mut text = String::with_capacity(len);
    while text.len() < len {
        text.push_str(pattern);
    }
    text.truncate(len);
    text
}

fn patterned_bytes(start: u8, len: usize) -> Vec<u8> {
    (0..len)
        .scan(start, |byte, _| {
            let value = *byte;
            *byte = byte.wrapping_add(31);
            Some(value)
        })
        .collect()
}

fn run_websocket_queued_write_drop_preserves_later_message_order(
    mut websocket_config: WebSocketConfig,
    seed: u64,
    label: &'static str,
) {
    websocket_config.connect_timeout_ms = Some(5_000);
    websocket_config.reconnect_delay_initial_ms = Some(25);
    websocket_config.reconnect_delay_max_ms = Some(100);
    websocket_config.reconnect_backoff_factor = Some(1.0);
    websocket_config.reconnect_jitter_ms = Some(0);

    let mut sim = stressed_builder(seed, Duration::from_secs(20)).build();

    sim.host("server", ws_drop_first_write_then_echo_server);

    sim.client("client", async move {
        let (handler, mut rx) = channel_message_handler();

        let client = WebSocketClient::builder()
            .config(websocket_config)
            .message_handler(handler)
            .connect()
            .await
            .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} should connect: {e}"));

        let in_flight_msg = "queued-before-drop".to_string();
        client
            .send_text(in_flight_msg.clone(), None)
            .await
            .unwrap_or_else(|e| {
                panic!("{label} seed {seed:#018x} should enqueue pre-drop message: {e}")
            });

        assert!(
            recv_text(&mut rx, RECONNECTED).await,
            "{label} seed {seed:#018x} should reconnect after pre-echo drop"
        );

        let expected = (0..4)
            .map(|i| format!("after-queued-drop-{i}"))
            .collect::<Vec<_>>();
        let mut received = Vec::with_capacity(expected.len());

        for msg in &expected {
            client
                .send_text(msg.clone(), None)
                .await
                .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} should enqueue {msg}: {e}"));
        }

        while received.len() < expected.len() {
            let received_msg = recv_application_text(&mut rx).await.unwrap_or_else(|| {
                panic!("{label} seed {seed:#018x} should receive later application text")
            });

            if received_msg == in_flight_msg {
                continue;
            }

            let expected_msg = &expected[received.len()];
            assert_eq!(
                &received_msg, expected_msg,
                "{label} seed {seed:#018x} should preserve later message order"
            );
            received.push(received_msg);
        }

        assert_eq!(
            received, expected,
            "{label} seed {seed:#018x} should preserve later message sequence"
        );

        client.disconnect().await;
        assert!(
            client.is_disconnected(),
            "{label} seed {seed:#018x} should disconnect after scenario"
        );

        Ok(())
    });

    sim.run()
        .unwrap_or_else(|e| panic!("{label} seed {seed:#018x} simulation failed: {e:?}"));
}

async fn ws_echo_first_then_drop_when_reconnect_active_then_echo_server(
    drop_reconnected_connection: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = net::TcpListener::bind("0.0.0.0:8080").await?;
    let mut connection_index = 0;

    loop {
        let (stream, _) = listener.accept().await?;
        let current_connection = connection_index;
        connection_index += 1;
        let drop_reconnected_connection = Arc::clone(&drop_reconnected_connection);

        tokio::spawn(async move {
            if let Ok(mut ws_stream) = accept_async(stream).await {
                match current_connection {
                    0 => {
                        if let Some(Ok(msg)) = ws_stream.next().await {
                            match msg {
                                Message::Text(text) => {
                                    let _ = ws_stream.send(Message::Text(text)).await;
                                }
                                Message::Binary(data) => {
                                    let _ = ws_stream.send(Message::Binary(data)).await;
                                }
                                Message::Ping(ping_data) => {
                                    let _ = ws_stream.send(Message::Pong(ping_data)).await;
                                }
                                Message::Close(_) => {
                                    let _ = ws_stream.close(None).await;
                                }
                                Message::Pong(_) | Message::Frame(_) => {}
                            }
                        }
                    }
                    1 => {
                        while !drop_reconnected_connection.load(Ordering::SeqCst) {
                            tokio::time::sleep(POLL_STEP).await;
                        }
                    }
                    _ => {
                        while let Some(msg) = ws_stream.next().await {
                            match msg {
                                Ok(Message::Text(text)) => {
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
                                Ok(Message::Pong(_) | Message::Frame(_)) => {}
                                Err(_) => break,
                            }
                        }
                    }
                }
            }
        });
    }
}

async fn ws_drop_first_write_then_echo_server() -> Result<(), Box<dyn std::error::Error>> {
    let listener = net::TcpListener::bind("0.0.0.0:8080").await?;
    let mut drop_next_connection = true;

    loop {
        let (stream, _) = listener.accept().await?;
        let drop_before_echo = drop_next_connection;
        drop_next_connection = false;

        tokio::spawn(async move {
            if let Ok(mut ws_stream) = accept_async(stream).await {
                if drop_before_echo {
                    let _ = ws_stream.next().await;
                    return;
                }

                while let Some(msg) = ws_stream.next().await {
                    match msg {
                        Ok(Message::Text(text)) => {
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
                        Ok(Message::Pong(_) | Message::Frame(_)) => {}
                        Err(_) => break,
                    }
                }
            }
        });
    }
}

async fn ws_drop_reconnect_handshake_then_echo_server(
    handshake_dropped: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = net::TcpListener::bind("0.0.0.0:8080").await?;
    let mut connection_index = 0;

    loop {
        let (stream, _) = listener.accept().await?;
        let current_connection = connection_index;
        connection_index += 1;
        let handshake_dropped = Arc::clone(&handshake_dropped);

        tokio::spawn(async move {
            match current_connection {
                0 => {
                    let _ = accept_async(stream).await;
                }
                1 => {
                    drop(stream);
                    handshake_dropped.store(true, Ordering::SeqCst);
                }
                _ => {
                    if let Ok(mut ws_stream) = accept_async(stream).await {
                        while let Some(msg) = ws_stream.next().await {
                            match msg {
                                Ok(Message::Text(text)) => {
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
                                Ok(Message::Pong(_) | Message::Frame(_)) => {}
                                Err(_) => break,
                            }
                        }
                    }
                }
            }
        });
    }
}

async fn ws_drop_two_reconnect_handshakes_then_echo_server(
    first_handshake_dropped: Arc<AtomicBool>,
    second_handshake_dropped: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = net::TcpListener::bind("0.0.0.0:8080").await?;
    let mut connection_index = 0;

    loop {
        let (stream, _) = listener.accept().await?;
        let current_connection = connection_index;
        connection_index += 1;
        let first_handshake_dropped = Arc::clone(&first_handshake_dropped);
        let second_handshake_dropped = Arc::clone(&second_handshake_dropped);

        tokio::spawn(async move {
            match current_connection {
                0 => {
                    let _ = accept_async(stream).await;
                }
                1 => {
                    drop(stream);
                    first_handshake_dropped.store(true, Ordering::SeqCst);
                }
                2 => {
                    drop(stream);
                    second_handshake_dropped.store(true, Ordering::SeqCst);
                }
                _ => {
                    if let Ok(mut ws_stream) = accept_async(stream).await {
                        while let Some(msg) = ws_stream.next().await {
                            match msg {
                                Ok(Message::Text(text)) => {
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
                                Ok(Message::Pong(_) | Message::Frame(_)) => {}
                                Err(_) => break,
                            }
                        }
                    }
                }
            }
        });
    }
}

async fn ws_drop_first_connection_before_read_then_echo_server(
    first_connection_dropped: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = net::TcpListener::bind("0.0.0.0:8080").await?;
    let mut connection_index = 0;

    loop {
        let (stream, _) = listener.accept().await?;
        let current_connection = connection_index;
        connection_index += 1;
        let first_connection_dropped = Arc::clone(&first_connection_dropped);

        tokio::spawn(async move {
            match current_connection {
                0 => {
                    if let Ok(ws_stream) = accept_async(stream).await {
                        first_connection_dropped.store(true, Ordering::SeqCst);
                        drop(ws_stream);
                    }
                }
                _ => {
                    if let Ok(mut ws_stream) = accept_async(stream).await {
                        while let Some(msg) = ws_stream.next().await {
                            match msg {
                                Ok(Message::Text(text)) => {
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
                                Ok(Message::Pong(_) | Message::Frame(_)) => {}
                                Err(_) => break,
                            }
                        }
                    }
                }
            }
        });
    }
}

async fn ws_drop_first_connection_wait_for_repair_then_echo_server(
    first_connection_dropped: Arc<AtomicBool>,
    reconnect_repaired: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = net::TcpListener::bind("0.0.0.0:8080").await?;
    let mut connection_index = 0;

    loop {
        let (stream, _) = listener.accept().await?;
        let current_connection = connection_index;
        connection_index += 1;
        let first_connection_dropped = Arc::clone(&first_connection_dropped);
        let reconnect_repaired = Arc::clone(&reconnect_repaired);

        tokio::spawn(async move {
            match current_connection {
                0 => {
                    if let Ok(ws_stream) = accept_async(stream).await {
                        first_connection_dropped.store(true, Ordering::SeqCst);
                        drop(ws_stream);
                    }
                }
                _ => {
                    while !reconnect_repaired.load(Ordering::SeqCst) {
                        tokio::time::sleep(POLL_STEP).await;
                    }

                    if let Ok(mut ws_stream) = accept_async(stream).await {
                        while let Some(msg) = ws_stream.next().await {
                            match msg {
                                Ok(Message::Text(text)) => {
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
                                Ok(Message::Pong(_) | Message::Frame(_)) => {}
                                Err(_) => break,
                            }
                        }
                    }
                }
            }
        });
    }
}

async fn ws_silent_first_connection_then_echo_server(
    first_connection_silent: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = net::TcpListener::bind("0.0.0.0:8080").await?;
    let mut connection_index = 0;

    loop {
        let (stream, _) = listener.accept().await?;
        let current_connection = connection_index;
        connection_index += 1;
        let first_connection_silent = Arc::clone(&first_connection_silent);

        tokio::spawn(async move {
            match current_connection {
                0 => {
                    if let Ok(mut ws_stream) = accept_async(stream).await {
                        first_connection_silent.store(true, Ordering::SeqCst);

                        while let Some(msg) = ws_stream.next().await {
                            match msg {
                                Ok(Message::Close(_)) | Err(_) => break,
                                Ok(_) => {}
                            }
                        }
                    }
                }
                _ => {
                    if let Ok(mut ws_stream) = accept_async(stream).await {
                        while let Some(msg) = ws_stream.next().await {
                            match msg {
                                Ok(Message::Text(text)) => {
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
                                Ok(Message::Pong(_) | Message::Frame(_)) => {}
                                Err(_) => break,
                            }
                        }
                    }
                }
            }
        });
    }
}

/// Completes the handshake on the first connection then stops servicing the stream entirely.
///
/// Unlike the silent server, this never polls the stream, so tokio-tungstenite's automatic pong
/// never runs and the peer emits no frame of any kind. That is the half-open connection the
/// heartbeat timeout exists to catch: the socket stays open and writes keep succeeding.
async fn ws_dark_first_connection_then_echo_server(
    first_connection_dark: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = net::TcpListener::bind("0.0.0.0:8080").await?;
    let mut connection_index = 0;

    loop {
        let (stream, _) = listener.accept().await?;
        let current_connection = connection_index;
        connection_index += 1;
        let first_connection_dark = Arc::clone(&first_connection_dark);

        tokio::spawn(async move {
            match current_connection {
                0 => {
                    if let Ok(ws_stream) = accept_async(stream).await {
                        first_connection_dark.store(true, Ordering::SeqCst);

                        // Hold the stream without polling it so no pong is ever emitted.
                        let _ws_stream = ws_stream;
                        std::future::pending::<()>().await;
                    }
                }
                _ => {
                    if let Ok(mut ws_stream) = accept_async(stream).await {
                        while let Some(msg) = ws_stream.next().await {
                            match msg {
                                Ok(Message::Text(text)) => {
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
                                Ok(Message::Pong(_) | Message::Frame(_)) => {}
                                Err(_) => break,
                            }
                        }
                    }
                }
            }
        });
    }
}

async fn ws_drop_first_connection_then_hold_reconnect_handshake_until_release_server(
    first_connection_dropped: Arc<AtomicBool>,
    reconnect_attempt_waiting: Arc<AtomicBool>,
    release_reconnect_attempt: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = net::TcpListener::bind("0.0.0.0:8080").await?;
    let mut connection_index = 0;

    loop {
        let (stream, _) = listener.accept().await?;
        let current_connection = connection_index;
        connection_index += 1;
        let first_connection_dropped = Arc::clone(&first_connection_dropped);
        let reconnect_attempt_waiting = Arc::clone(&reconnect_attempt_waiting);
        let release_reconnect_attempt = Arc::clone(&release_reconnect_attempt);

        tokio::spawn(async move {
            match current_connection {
                0 => {
                    if let Ok(ws_stream) = accept_async(stream).await {
                        first_connection_dropped.store(true, Ordering::SeqCst);
                        drop(ws_stream);
                    }
                }
                _ => {
                    reconnect_attempt_waiting.store(true, Ordering::SeqCst);
                    while !release_reconnect_attempt.load(Ordering::SeqCst) {
                        tokio::time::sleep(POLL_STEP).await;
                    }
                    drop(stream);
                }
            }
        });
    }
}

async fn ws_hold_first_connection_until_release_then_echo_server(
    first_connection_held: Arc<AtomicBool>,
    release_first_connection: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = net::TcpListener::bind("0.0.0.0:8080").await?;
    let mut connection_index = 0;

    loop {
        let (stream, _) = listener.accept().await?;
        let current_connection = connection_index;
        connection_index += 1;
        let first_connection_held = Arc::clone(&first_connection_held);
        let release_first_connection = Arc::clone(&release_first_connection);

        tokio::spawn(async move {
            match current_connection {
                0 => {
                    if let Ok(mut ws_stream) = accept_async(stream).await {
                        first_connection_held.store(true, Ordering::SeqCst);

                        while !release_first_connection.load(Ordering::SeqCst) {
                            tokio::time::sleep(POLL_STEP).await;
                        }

                        for _ in 0..BACKPRESSURE_MESSAGE_COUNT {
                            if ws_stream.next().await.is_none() {
                                return;
                            }
                        }
                    }
                }
                _ => {
                    if let Ok(mut ws_stream) = accept_async(stream).await {
                        while let Some(msg) = ws_stream.next().await {
                            match msg {
                                Ok(Message::Text(text)) => {
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
                                Ok(Message::Pong(_) | Message::Frame(_)) => {}
                                Err(_) => break,
                            }
                        }
                    }
                }
            }
        });
    }
}

async fn ws_ping_counting_server(
    pings: Arc<AtomicUsize>,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = net::TcpListener::bind("0.0.0.0:8080").await?;

    loop {
        let (stream, _) = listener.accept().await?;
        let mut websocket = accept_async(stream).await?;

        while let Some(Ok(msg)) = websocket.next().await {
            match msg {
                Message::Ping(_) => {
                    pings.fetch_add(1, Ordering::SeqCst);
                }
                Message::Close(_) => {
                    let _ = websocket.close(None).await;
                    break;
                }
                _ => {}
            }
        }
    }
}

async fn ws_text_heartbeat_counting_server(
    heartbeats: Arc<AtomicUsize>,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = net::TcpListener::bind("0.0.0.0:8080").await?;

    loop {
        let (stream, _) = listener.accept().await?;
        let heartbeats = Arc::clone(&heartbeats);

        tokio::spawn(async move {
            if let Ok(mut websocket) = accept_async(stream).await {
                while let Some(message) = websocket.next().await {
                    match message {
                        Ok(Message::Text(text)) if text == TEXT_HEARTBEAT => {
                            heartbeats.fetch_add(1, Ordering::SeqCst);
                        }
                        Ok(Message::Close(_)) => {
                            let _ = websocket.close(None).await;
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

async fn ws_send_text_without_reading_server() -> Result<(), Box<dyn std::error::Error>> {
    let listener = net::TcpListener::bind("0.0.0.0:8080").await?;

    loop {
        let (stream, _) = listener.accept().await?;

        tokio::spawn(async move {
            if let Ok(mut websocket) = accept_async(stream).await {
                loop {
                    tokio::time::sleep(Duration::from_millis(500)).await;

                    if websocket
                        .send(Message::Text(NON_PONG_FRAME.into()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }
        });
    }
}

async fn ws_ping_until_pong_server(
    pong_received: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = net::TcpListener::bind("0.0.0.0:8080").await?;
    let (stream, _) = listener.accept().await?;
    let mut websocket = accept_async(stream).await?;

    loop {
        if websocket
            .send(Message::Ping(Vec::new().into()))
            .await
            .is_err()
        {
            break;
        }

        match tokio::time::timeout(Duration::from_millis(100), websocket.next()).await {
            Ok(Some(Ok(Message::Pong(_)))) => {
                pong_received.store(true, Ordering::SeqCst);
            }
            Ok(Some(Ok(Message::Close(_)))) => {
                let _ = websocket.close(None).await;
                break;
            }
            Ok(Some(Ok(_))) => {}
            Ok(Some(Err(_)) | None) => break,
            Err(_) => {}
        }
    }

    Ok(())
}

async fn ws_close_frame_then_echo_server() -> Result<(), Box<dyn std::error::Error>> {
    let listener = net::TcpListener::bind("0.0.0.0:8080").await?;

    // First connection: complete the upgrade then send a protocol Close frame
    let (stream, _) = listener.accept().await?;
    let mut websocket = accept_async(stream).await?;
    let _ = websocket.close(None).await;
    // Drive the close handshake to completion (bounded in case the peer drops)
    let _ = tokio::time::timeout(Duration::from_millis(500), async {
        while websocket.next().await.is_some() {}
    })
    .await;

    // Second connection: announce and echo
    let (stream, _) = listener.accept().await?;
    let mut websocket = accept_async(stream).await?;
    websocket.send(Message::Text("after-close".into())).await?;

    while let Some(Ok(msg)) = websocket.next().await {
        match msg {
            Message::Text(_) | Message::Binary(_) => {
                if websocket.send(msg).await.is_err() {
                    break;
                }
            }
            Message::Close(_) => {
                let _ = websocket.close(None).await;
                break;
            }
            _ => {}
        }
    }

    Ok(())
}
