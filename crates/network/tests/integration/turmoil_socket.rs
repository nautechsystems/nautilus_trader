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

//! Turmoil integration tests for the `SocketClient`.
//!
//! These tests use turmoil's network simulation to test the actual production
//! `SocketClient` code under various network conditions.

#![cfg(feature = "turmoil")]

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use nautilus_network::socket::{SocketClient, SocketConfig};
use parking_lot::Mutex;
use rstest::{fixture, rstest};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::tungstenite::stream::Mode;
use turmoil::net;

use crate::common::turmoil::{seeded_builder, seeded_builder_with_duration, stressed_builder};

// 2-second budget in simulated time, covering reconnect timings across these tests.
const POLL_ITERS: u32 = 200;
const POLL_STEP: Duration = Duration::from_millis(10);
const BASIC_CONNECT_SEED: u64 = 0x51C0_0001;
const RECONNECTION_SEED: u64 = 0x51C0_0002;
const NETWORK_PARTITION_SEED: u64 = 0x51C0_0003;
const CLOSE_DURING_RECONNECT_SEED: u64 = 0x51C0_0004;
const CLOSE_DURING_BACKOFF_SEED: u64 = 0x51C0_0005;
const UNSTABLE_RECONNECT_SEED: u64 = 0x51C0_0006;
const STABLE_RECONNECT_SEED: u64 = 0x51C0_0007;
const RECONNECT_STORM_SEED: u64 = 0x51C0_0008;
const UNSTABLE_RECONNECT_DELAY_MS: u64 = 750;

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

type ReceivedMessages = Arc<Mutex<Vec<String>>>;

fn attach_message_capture(config: &mut SocketConfig, received: &ReceivedMessages) {
    let received = Arc::clone(received);
    config.message_handler = Some(Arc::new(move |data: &[u8]| {
        received
            .lock()
            .push(String::from_utf8_lossy(data).to_string());
    }));
}

fn captured_messages(received: &ReceivedMessages) -> Vec<String> {
    received.lock().clone()
}

async fn echo_once_then_drop_server() -> Result<(), Box<dyn std::error::Error>> {
    let listener = net::TcpListener::bind("0.0.0.0:8080").await?;

    loop {
        let (mut stream, _) = listener.accept().await?;

        tokio::spawn(async move {
            let mut buffer = vec![0; 1024];
            if let Ok(n) = stream.read(&mut buffer).await
                && n > 0
            {
                if !buffer.starts_with(b"close\r\n") {
                    let _ = stream.write_all(&buffer[..n]).await;
                }
                let _ = stream.shutdown().await;
            }
        });
    }
}

async fn drop_each_connection_server(
    accepted: Arc<AtomicUsize>,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = net::TcpListener::bind("0.0.0.0:8080").await?;

    loop {
        let (stream, _) = listener.accept().await?;
        accepted.fetch_add(1, Ordering::SeqCst);
        drop(stream);
    }
}

async fn drop_each_connection_timed_server(
    accept_times: Arc<Mutex<Vec<tokio::time::Instant>>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = net::TcpListener::bind("0.0.0.0:8080").await?;

    loop {
        let (stream, _) = listener.accept().await?;
        accept_times.lock().push(tokio::time::Instant::now());
        drop(stream);
    }
}

async fn hold_stable_reconnect_server(
    accepted: Arc<AtomicUsize>,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = net::TcpListener::bind("0.0.0.0:8080").await?;

    loop {
        let (stream, _) = listener.accept().await?;
        let connection = accepted.fetch_add(1, Ordering::SeqCst);

        match connection {
            0 => drop(stream),
            1 => {
                tokio::time::sleep(Duration::from_secs(11)).await;
                drop(stream);
            }
            _ => {
                let _held_stream = stream;
                std::future::pending::<()>().await;
            }
        }
    }
}

/// Default test socket configuration.
#[fixture]
fn socket_config() -> SocketConfig {
    SocketConfig {
        url: "server:8080".to_string(),
        mode: Mode::Plain,
        suffix: b"\r\n".to_vec(),
        message_handler: None,
        heartbeat: None,
        connect_timeout_ms: Some(2_000),
        reconnect_delay_initial_ms: Some(50),
        reconnect_delay_max_ms: Some(500),
        reconnect_backoff_factor: Some(1.5),
        reconnect_jitter_ms: Some(10),
        connection_max_retries: None,
        reconnect_max_attempts: None,
        heartbeat_timeout_secs: None,
        certs_dir: None,
    }
}

/// Echo server for testing.
async fn echo_server() -> Result<(), Box<dyn std::error::Error>> {
    let listener = net::TcpListener::bind("0.0.0.0:8080").await?;

    loop {
        if let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buffer = vec![0; 1024];

                loop {
                    match stream.read(&mut buffer).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            // Check for termination message
                            if buffer.starts_with(b"close\r\n") {
                                let _ = stream.shutdown().await;
                                break;
                            }
                            // Echo back the data
                            if stream.write_all(&buffer[..n]).await.is_err() {
                                break;
                            }
                        }
                    }
                }
            });
        }
    }
}

#[rstest]
fn test_turmoil_real_socket_basic_connect(socket_config: SocketConfig) {
    let mut socket_config = socket_config;
    let received = Arc::new(Mutex::new(Vec::new()));
    attach_message_capture(&mut socket_config, &received);

    let mut sim = seeded_builder(BASIC_CONNECT_SEED).build();

    sim.host("server", echo_server);

    sim.client("client", async move {
        let client = SocketClient::builder()
            .config(socket_config)
            .connect()
            .await
            .expect("Should connect");

        // Verify client is active
        assert!(client.is_active(), "Client should be active after connect");

        client
            .send_bytes(b"hello".to_vec())
            .await
            .expect("Should send data");
        assert!(
            wait_for(|| captured_messages(&received) == ["hello"]).await,
            "Client should receive echoed hello"
        );

        client
            .send_bytes(b"close".to_vec())
            .await
            .expect("Should send close");

        client.close().await;
        assert!(client.is_closed(), "Client should be closed");

        Ok(())
    });

    sim.run().unwrap();
}

#[rstest]
fn test_turmoil_real_socket_reconnection(mut socket_config: SocketConfig) {
    socket_config.connect_timeout_ms = Some(5_000);
    socket_config.reconnect_delay_initial_ms = Some(100);
    let received = Arc::new(Mutex::new(Vec::new()));
    attach_message_capture(&mut socket_config, &received);
    let reconnections = Arc::new(AtomicUsize::new(0));
    let reconnections_for_handler = Arc::clone(&reconnections);
    let post_reconnection = Arc::new(move || {
        reconnections_for_handler.fetch_add(1, Ordering::SeqCst);
    });

    let mut sim = seeded_builder(RECONNECTION_SEED).build();

    // Server that accepts one connection, closes it, then accepts another
    sim.host("server", || async {
        let listener = net::TcpListener::bind("0.0.0.0:8080").await?;

        // Accept first connection
        if let Ok((mut stream, _)) = listener.accept().await {
            let mut buffer = vec![0; 1024];
            let _ = stream.read(&mut buffer).await;
            let _ = stream.write_all(b"first\r\n").await;
            drop(stream);
        }

        // Accept second connection and run echo loop
        if let Ok((mut stream, _)) = listener.accept().await {
            let mut buffer = vec![0; 1024];
            loop {
                match stream.read(&mut buffer).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if buffer.starts_with(b"close\r\n") {
                            break;
                        }

                        if stream.write_all(&buffer[..n]).await.is_err() {
                            break;
                        }
                    }
                }
            }
        }

        Ok::<(), Box<dyn std::error::Error>>(())
    });

    sim.client("client", async move {
        let client = SocketClient::builder()
            .config(socket_config)
            .post_reconnection(post_reconnection)
            .connect()
            .await
            .expect("Should connect");

        assert_eq!(reconnections.load(Ordering::SeqCst), 0);

        client
            .send_bytes(b"first_msg".to_vec())
            .await
            .expect("Should send first message");
        assert!(
            wait_for(|| captured_messages(&received) == ["first"]).await,
            "Client should receive first message before reconnect"
        );

        // Server closes after echoing; wait for the client to cycle through
        // reconnection and return to an active state before the next send.
        assert!(
            wait_for(|| client.is_reconnecting() || !client.is_active()).await,
            "Client should observe server disconnect"
        );
        assert!(
            wait_for(|| client.is_active()).await,
            "Client should reconnect after server close"
        );
        assert_eq!(reconnections.load(Ordering::SeqCst), 1);

        client
            .send_bytes(b"second_msg".to_vec())
            .await
            .expect("Should send second message after reconnect");
        assert!(
            wait_for(|| captured_messages(&received) == ["first", "second_msg"]).await,
            "Client should receive post-reconnect echo"
        );

        client.send_bytes(b"close".to_vec()).await.ok();
        client.close().await;

        assert_eq!(reconnections.load(Ordering::SeqCst), 1);

        Ok(())
    });

    sim.run().unwrap();
}

#[rstest]
fn test_turmoil_socket_unstable_reconnects_exhaust_attempts(mut socket_config: SocketConfig) {
    socket_config.reconnect_delay_initial_ms = Some(UNSTABLE_RECONNECT_DELAY_MS);
    socket_config.reconnect_delay_max_ms = Some(UNSTABLE_RECONNECT_DELAY_MS);
    socket_config.reconnect_backoff_factor = Some(1.0);
    socket_config.reconnect_jitter_ms = Some(0);
    socket_config.reconnect_max_attempts = Some(3);

    let accepted = Arc::new(AtomicUsize::new(0));
    let server_accepted = Arc::clone(&accepted);
    let mut builder =
        seeded_builder_with_duration(UNSTABLE_RECONNECT_SEED, Duration::from_secs(10));
    builder.min_message_latency(Duration::ZERO);
    builder.max_message_latency(Duration::ZERO);
    let mut sim = builder.build();

    sim.host("server", move || {
        drop_each_connection_server(Arc::clone(&server_accepted))
    });

    sim.client("client", async move {
        let client = SocketClient::builder()
            .config(socket_config)
            .connect()
            .await
            .expect("Initial socket connection should succeed");
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
fn test_turmoil_socket_stable_reconnect_resets_attempts(mut socket_config: SocketConfig) {
    socket_config.reconnect_delay_initial_ms = Some(50);
    socket_config.reconnect_delay_max_ms = Some(200);
    socket_config.reconnect_backoff_factor = Some(2.0);
    socket_config.reconnect_jitter_ms = Some(0);
    socket_config.reconnect_max_attempts = Some(1);

    let accepted = Arc::new(AtomicUsize::new(0));
    let server_accepted = Arc::clone(&accepted);
    let mut sim =
        seeded_builder_with_duration(STABLE_RECONNECT_SEED, Duration::from_secs(20)).build();

    sim.host("server", move || {
        hold_stable_reconnect_server(Arc::clone(&server_accepted))
    });

    sim.client("client", async move {
        let client = SocketClient::builder()
            .config(socket_config)
            .connect()
            .await
            .expect("Initial socket connection should succeed");

        tokio::time::sleep(Duration::from_secs(13)).await;

        assert!(
            wait_for(|| accepted.load(Ordering::SeqCst) >= 3).await,
            "A stable reconnect should reset the attempt limit for the next drop"
        );
        assert!(
            client.is_active(),
            "Client should remain active on the connection after the reset"
        );

        client.close().await;
        Ok(())
    });

    sim.run().unwrap();
}

#[rstest]
fn test_turmoil_socket_reconnect_storm_attempts_are_floored(mut socket_config: SocketConfig) {
    // A venue cycling connections faster than the stability threshold must not get an
    // immediate reconnect: once three attempts land inside the rolling window, each
    // further attempt waits at least one second regardless of the configured backoff.
    socket_config.reconnect_delay_initial_ms = Some(25);
    socket_config.reconnect_delay_max_ms = Some(25);
    socket_config.reconnect_backoff_factor = Some(1.0);
    socket_config.reconnect_jitter_ms = Some(0);
    socket_config.reconnect_max_attempts = None;

    let accept_times = Arc::new(Mutex::new(Vec::new()));
    let server_accept_times = Arc::clone(&accept_times);
    let mut builder = seeded_builder_with_duration(RECONNECT_STORM_SEED, Duration::from_secs(20));
    builder.min_message_latency(Duration::ZERO);
    builder.max_message_latency(Duration::ZERO);
    let mut sim = builder.build();

    sim.host("server", move || {
        drop_each_connection_timed_server(Arc::clone(&server_accept_times))
    });

    sim.client("client", async move {
        let client = SocketClient::builder()
            .config(socket_config)
            .connect()
            .await
            .expect("Initial socket connection should succeed");

        tokio::time::sleep(Duration::from_secs(15)).await;
        client.close().await;
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
fn test_turmoil_real_socket_network_partition(mut socket_config: SocketConfig) {
    socket_config.connect_timeout_ms = Some(3_000);
    let received = Arc::new(Mutex::new(Vec::new()));
    attach_message_capture(&mut socket_config, &received);

    let mut sim = seeded_builder(NETWORK_PARTITION_SEED).build();

    sim.host("server", echo_server);

    sim.client("client", async move {
        let client = SocketClient::builder()
            .config(socket_config)
            .connect()
            .await
            .expect("Should connect");

        client
            .send_bytes(b"before_partition".to_vec())
            .await
            .expect("Should send before partition");
        assert!(
            wait_for(|| captured_messages(&received) == ["before_partition"]).await,
            "Client should receive echoed before_partition"
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
            .send_bytes(b"after_partition".to_vec())
            .await
            .expect("Should send after partition repair");
        assert!(
            wait_for(|| {
                captured_messages(&received) == ["before_partition", "after_partition"]
            })
            .await,
            "Client should receive echoed after_partition"
        );

        client.send_bytes(b"close".to_vec()).await.ok();
        client.close().await;

        Ok(())
    });

    sim.run().unwrap();
}

#[rstest]
fn test_turmoil_real_socket_close_during_reconnect(mut socket_config: SocketConfig) {
    socket_config.connect_timeout_ms = Some(5_000);
    socket_config.reconnect_delay_initial_ms = Some(100);

    let mut sim = seeded_builder(CLOSE_DURING_RECONNECT_SEED).build();

    sim.host("server", echo_server);

    sim.client("client", async move {
        let client = SocketClient::builder()
            .config(socket_config)
            .connect()
            .await
            .expect("Should connect");

        assert!(client.is_active(), "Client should be active after connect");

        turmoil::partition("client", "server");
        tokio::time::sleep(Duration::from_millis(200)).await;

        client.close().await;

        assert!(
            client.is_closed(),
            "Client should be closed after close during reconnect"
        );
        assert!(
            !client.is_active(),
            "Client should not be active after close"
        );

        Ok(())
    });

    sim.run().unwrap();
}

#[rstest]
fn test_turmoil_real_socket_disconnect_during_backoff(mut socket_config: SocketConfig) {
    socket_config.connect_timeout_ms = Some(1_000);
    socket_config.reconnect_delay_initial_ms = Some(10_000); // Long backoff
    socket_config.reconnect_delay_max_ms = Some(10_000);
    socket_config.reconnect_backoff_factor = Some(1.0);
    socket_config.reconnect_jitter_ms = Some(0);

    let mut sim =
        seeded_builder_with_duration(CLOSE_DURING_BACKOFF_SEED, Duration::from_secs(30)).build();

    sim.host("server", echo_server);

    sim.client("client", async move {
        let client = SocketClient::builder()
            .config(socket_config)
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
        client.close().await;
        let elapsed = start.elapsed();

        assert!(client.is_closed(), "Client should be closed");
        assert!(
            elapsed < Duration::from_secs(3),
            "Close should interrupt backoff, took {elapsed:?}"
        );

        Ok(())
    });

    sim.run().unwrap();
}

#[rstest]
#[case::seed_a(0x51C0_1001)]
#[case::seed_b(0x51C0_1002)]
#[case::seed_c(0x51C0_1003)]
fn test_turmoil_socket_repeated_drops_preserve_message_order(
    mut socket_config: SocketConfig,
    #[case] seed: u64,
) {
    socket_config.connect_timeout_ms = Some(5_000);
    socket_config.reconnect_delay_initial_ms = Some(25);
    socket_config.reconnect_delay_max_ms = Some(100);
    socket_config.reconnect_backoff_factor = Some(1.0);
    socket_config.reconnect_jitter_ms = Some(0);
    let received = Arc::new(Mutex::new(Vec::new()));
    attach_message_capture(&mut socket_config, &received);

    let mut sim = stressed_builder(seed, Duration::from_secs(20)).build();

    sim.host("server", echo_once_then_drop_server);

    sim.client("client", async move {
        let client = SocketClient::builder()
            .config(socket_config)
            .connect()
            .await
            .expect("Should connect");

        let expected = (0..6)
            .map(|i| format!("drop-reconnect-{i}"))
            .collect::<Vec<_>>();

        for (index, msg) in expected.iter().enumerate() {
            client
                .send_bytes(msg.as_bytes().to_vec())
                .await
                .expect("Should enqueue message");

            assert!(
                wait_for(|| captured_messages(&received).len() == index + 1).await,
                "Client should receive echoed message {index}"
            );

            if index + 1 < expected.len() {
                assert!(
                    wait_for(|| client.is_reconnecting() || !client.is_active()).await,
                    "Client should observe drop after message {index}"
                );
                assert!(
                    wait_for(|| client.is_active()).await,
                    "Client should reconnect after message {index}"
                );
            }
        }

        assert_eq!(
            captured_messages(&received),
            expected,
            "Repeated reconnects should preserve message order"
        );

        client.close().await;
        assert!(client.is_closed(), "Client should close after scenario");

        Ok(())
    });

    sim.run().unwrap();
}
