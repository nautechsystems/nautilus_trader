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

//! Turmoil integration tests for the SocketClient.
//!
//! These tests use turmoil's network simulation to test the actual production
//! SocketClient code under various network conditions.

#![cfg(feature = "turmoil")]

use std::time::Duration;

use nautilus_network::socket::{SocketClient, SocketConfig};
use rstest::{fixture, rstest};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::tungstenite::stream::Mode;
use turmoil::{Builder, net};

/// Default test socket configuration.
#[fixture]
fn socket_config() -> SocketConfig {
    SocketConfig {
        url: "server:8080".to_string(),
        mode: Mode::Plain,
        suffix: b"\r\n".to_vec(),
        message_handler: None,
        heartbeat: None,
        reconnect_timeout_ms: Some(2_000),
        reconnect_delay_initial_ms: Some(50),
        reconnect_delay_max_ms: Some(500),
        reconnect_backoff_factor: Some(1.5),
        reconnect_jitter_ms: Some(10),
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
                        Ok(0) => break,
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
                        Err(_) => break,
                    }
                }
            });
        }
    }
}

#[rstest]
fn test_turmoil_real_socket_basic_connect(socket_config: SocketConfig) {
    let mut sim = Builder::new().build();

    sim.host("server", echo_server);

    sim.client("client", async move {
        let client = SocketClient::connect(socket_config, None, None, None)
            .await
            .expect("Should connect");

        // Verify client is active
        assert!(client.is_active(), "Client should be active after connect");

        // Send a test message
        client
            .send_bytes(b"hello".to_vec())
            .await
            .expect("Should send data");

        // Wait a bit
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Send close message
        client
            .send_bytes(b"close".to_vec())
            .await
            .expect("Should send close");

        // Close the client
        client.close().await;
        assert!(client.is_closed(), "Client should be closed");

        Ok(())
    });

    sim.run().unwrap();
}

#[rstest]
fn test_turmoil_real_socket_reconnection(mut socket_config: SocketConfig) {
    socket_config.reconnect_timeout_ms = Some(5_000);
    socket_config.reconnect_delay_initial_ms = Some(100);

    let mut sim = Builder::new().build();

    // Server that accepts one connection, closes it, then accepts another
    sim.host("server", || async {
        let listener = net::TcpListener::bind("0.0.0.0:8080").await?;

        // Accept first connection
        if let Ok((mut stream, _)) = listener.accept().await {
            // Read one message
            let mut buffer = vec![0; 1024];
            let _ = stream.read(&mut buffer).await;
            // Echo it back
            let _ = stream.write_all(b"first\r\n").await;
            // Close the connection
            drop(stream);
        }

        // Wait a bit before accepting second connection
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Accept second connection and run echo server
        if let Ok((mut stream, _)) = listener.accept().await {
            let mut buffer = vec![0; 1024];
            loop {
                match stream.read(&mut buffer).await {
                    Ok(0) => break,
                    Ok(n) => {
                        if buffer.starts_with(b"close\r\n") {
                            break;
                        }
                        if stream.write_all(&buffer[..n]).await.is_err() {
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
        let client = SocketClient::connect(socket_config, None, None, None)
            .await
            .expect("Should connect");

        // Send first message
        client
            .send_bytes(b"first_msg".to_vec())
            .await
            .expect("Should send first message");

        // Wait for server to close connection
        tokio::time::sleep(Duration::from_millis(300)).await;

        // Client should detect disconnection and attempt reconnection
        // Send another message after reconnection
        client
            .send_bytes(b"second_msg".to_vec())
            .await
            .expect("Should send second message after reconnect");

        // Close
        client.send_bytes(b"close".to_vec()).await.ok();
        client.close().await;

        Ok(())
    });

    sim.run().unwrap();
}

#[rstest]
fn test_turmoil_real_socket_network_partition(mut socket_config: SocketConfig) {
    socket_config.reconnect_timeout_ms = Some(3_000);

    let mut sim = Builder::new().build();

    sim.host("server", echo_server);

    sim.client("client", async move {
        let client = SocketClient::connect(socket_config, None, None, None)
            .await
            .expect("Should connect");

        // Send message before partition
        client
            .send_bytes(b"before_partition".to_vec())
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
            .send_bytes(b"after_partition".to_vec())
            .await
            .expect("Should send after partition repair");

        // Close
        client.send_bytes(b"close".to_vec()).await.ok();
        client.close().await;

        Ok(())
    });

    sim.run().unwrap();
}
