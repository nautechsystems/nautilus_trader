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

//! Turmoil-compatible socket tests with dependency injection.
//!
//! This demonstrates how to make our networking components work with turmoil
//! through dependency injection of network types.

use std::time::Duration;

use nautilus_network::{backoff::ExponentialBackoff, net::TcpConnector, socket::SocketConfig};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::tungstenite::stream::Mode;
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

/// A test-specific socket client that uses dependency injection for networking.
struct TestSocketClient<C: TcpConnector> {
    config: SocketConfig,
    connector: C,
}

impl<C: TcpConnector> TestSocketClient<C> {
    const fn new(config: SocketConfig, connector: C) -> Self {
        Self { config, connector }
    }

    async fn connect(&self) -> Result<C::Stream, Box<dyn std::error::Error>> {
        let stream = self.connector.connect(&self.config.url).await?;
        Ok(stream)
    }

    async fn send_data(
        &self,
        mut stream: C::Stream,
        data: &[u8],
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        stream.write_all(data).await?;

        let mut buffer = vec![0; 1024];
        let n = stream.read(&mut buffer).await?;
        buffer.truncate(n);
        Ok(buffer)
    }

    async fn send_data_with_backoff(
        &self,
        data: &[u8],
        backoff: &mut nautilus_network::backoff::ExponentialBackoff,
        max_attempts: usize,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        for _attempt in 0..max_attempts {
            if let Ok(stream) = self.connect().await {
                if let Ok(response) = self.send_data(stream, data).await {
                    backoff.reset();
                    return Ok(response);
                } else {
                    let delay = backoff.next_duration();
                    tokio::time::sleep(delay).await;
                }
            } else {
                let delay = backoff.next_duration();
                tokio::time::sleep(delay).await;
            }
        }
        Err("Max attempts reached".into())
    }
}

/// Echo server that responds to messages.
async fn echo_server() -> Result<(), Box<dyn std::error::Error>> {
    let listener = net::TcpListener::bind("0.0.0.0:8080").await?;

    loop {
        if let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buffer = vec![0; 1024];

                while let Ok(n) = stream.read(&mut buffer).await {
                    if n == 0 {
                        break;
                    }
                    let _ = stream.write_all(&buffer[..n]).await;
                }
            });
        }
    }
}

#[test]
fn test_turmoil_socket_with_dependency_injection() {
    let mut sim = Builder::new().build();

    sim.host("server", echo_server);

    sim.client("client", async {
        let config = SocketConfig {
            url: "server:8080".to_string(),
            mode: Mode::Plain,
            suffix: b"\\r\\n".to_vec(),
            #[cfg(feature = "python")]
            py_handler: None,
            heartbeat: None,
            reconnect_timeout_ms: Some(2_000),
            reconnect_delay_initial_ms: Some(50),
            reconnect_delay_max_ms: Some(500),
            reconnect_backoff_factor: Some(1.5),
            reconnect_jitter_ms: Some(10),
            certs_dir: None,
        };

        // Use turmoil connector for testing
        let client = TestSocketClient::new(config, TurmoilTcpConnector);

        // Connect and send test data
        let stream = client.connect().await.expect("Should connect");
        let response = client
            .send_data(stream, b"hello turmoil")
            .await
            .expect("Should send data");

        // Verify echo response
        assert_eq!(response, b"hello turmoil");

        Ok(())
    });

    sim.run().unwrap();
}

#[test]
fn test_turmoil_socket_network_partition() {
    let mut sim = Builder::new().build();

    sim.host("server", echo_server);

    sim.client("client", async {
        let config = SocketConfig {
            url: "server:8080".to_string(),
            mode: Mode::Plain,
            suffix: b"\\r\\n".to_vec(),
            #[cfg(feature = "python")]
            py_handler: None,
            heartbeat: None,
            reconnect_timeout_ms: Some(2_000),
            reconnect_delay_initial_ms: Some(100),
            reconnect_delay_max_ms: Some(800),
            reconnect_backoff_factor: Some(1.8),
            reconnect_jitter_ms: Some(20),
            certs_dir: None,
        };

        let client = TestSocketClient::new(config, TurmoilTcpConnector);

        // Initial connection and message
        let stream = client.connect().await.expect("Should connect initially");
        let response = client
            .send_data(stream, b"before_partition")
            .await
            .expect("Should send initially");
        assert_eq!(response, b"before_partition");

        tokio::time::sleep(Duration::from_millis(50)).await;

        // Create network partition
        turmoil::partition("client", "server");

        // Connection should fail during partition
        let partition_result = client.connect().await;
        assert!(
            partition_result.is_err(),
            "Connection should fail during partition"
        );

        tokio::time::sleep(Duration::from_millis(200)).await;

        // Repair partition
        turmoil::repair("client", "server");

        // Wait for network to stabilize
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Should be able to connect again after repair
        let stream = client.connect().await.expect("Should connect after repair");
        let response = client
            .send_data(stream, b"after_partition")
            .await
            .expect("Should send after repair");
        assert_eq!(response, b"after_partition");

        Ok(())
    });

    sim.run().unwrap();
}

#[test]
fn test_exponential_backoff_under_network_instability() {
    let mut sim = Builder::new().build();

    sim.host("server", echo_server);

    sim.client("client", async {
        let config = SocketConfig {
            url: "server:8080".to_string(),
            mode: Mode::Plain,
            suffix: b"\\r\\n".to_vec(),
            #[cfg(feature = "python")]
            py_handler: None,
            heartbeat: None,
            reconnect_timeout_ms: Some(5_000),
            reconnect_delay_initial_ms: Some(50),
            reconnect_delay_max_ms: Some(500),
            reconnect_backoff_factor: Some(2.0),
            reconnect_jitter_ms: Some(10),
            certs_dir: None,
        };

        let client = TestSocketClient::new(config, TurmoilTcpConnector);

        let mut backoff = ExponentialBackoff::new(
            Duration::from_millis(50),  // initial delay
            Duration::from_millis(500), // max delay
            2.0,                        // factor
            10,                         // jitter
            true,                       // immediate first
        )
        .unwrap();

        // Test successful communication first
        let stream = client.connect().await.expect("Should connect");
        let response = client
            .send_data(stream, b"test_message")
            .await
            .expect("Should send data");
        assert_eq!(response, b"test_message");

        // Create network partition to test backoff
        turmoil::partition("client", "server");

        // Wait briefly
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Repair partition
        turmoil::repair("client", "server");
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Should eventually succeed with backoff (limiting attempts to prevent infinite loops)
        let response = client
            .send_data_with_backoff(b"after_instability", &mut backoff, 5)
            .await
            .expect("Should eventually succeed");
        assert_eq!(response, b"after_instability");

        Ok(())
    });

    sim.run().unwrap();
}

#[test]
fn test_multiple_clients_concurrent() {
    let mut sim = Builder::new().build();

    sim.host("server", echo_server);

    // Test multiple clients concurrently
    for client_id in 0..3_u32 {
        sim.client(format!("client_{client_id}"), async move {
            let config = SocketConfig {
                url: "server:8080".to_string(),
                mode: Mode::Plain,
                suffix: b"\\r\\n".to_vec(),
                #[cfg(feature = "python")]
                py_handler: None,
                heartbeat: None,
                reconnect_timeout_ms: Some(2_000),
                reconnect_delay_initial_ms: Some(50),
                reconnect_delay_max_ms: Some(500),
                reconnect_backoff_factor: Some(1.5),
                reconnect_jitter_ms: Some(10),
                certs_dir: None,
            };

            let client = TestSocketClient::new(config, TurmoilTcpConnector);

            // Each client sends multiple requests
            for req_id in 0..3 {
                let message = format!("client_{client_id}_req_{req_id}");
                let stream = client.connect().await.expect("Should connect");
                let response = client
                    .send_data(stream, message.as_bytes())
                    .await
                    .expect("Should send data");
                assert_eq!(response, message.as_bytes());

                // Add small delay between requests
                tokio::time::sleep(Duration::from_millis(50)).await;
            }

            Ok(())
        });
    }

    sim.run().unwrap();
}
