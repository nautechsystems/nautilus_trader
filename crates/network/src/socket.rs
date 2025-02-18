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

//! High-performance raw TCP client implementation with TLS capability, automatic reconnection
//! with exponential backoff and state management.

use std::{
    path::Path,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc,
    },
    time::Duration,
};

use nautilus_cryptography::providers::install_cryptographic_provider;
use pyo3::prelude::*;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf},
    net::TcpStream,
    sync::Mutex,
};
use tokio_tungstenite::{
    tungstenite::{client::IntoClientRequest, stream::Mode, Error},
    MaybeTlsStream,
};

use crate::{
    backoff::ExponentialBackoff,
    mode::ConnectionMode,
    tls::{create_tls_config_from_certs_dir, tcp_tls, Connector},
};

type TcpWriter = WriteHalf<MaybeTlsStream<TcpStream>>;
type SharedTcpWriter = Arc<Mutex<WriteHalf<MaybeTlsStream<TcpStream>>>>;
type TcpReader = ReadHalf<MaybeTlsStream<TcpStream>>;

/// Configuration for TCP socket connection.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.network")
)]
pub struct SocketConfig {
    /// The URL to connect to.
    pub url: String,
    /// The connection mode {Plain, TLS}.
    pub mode: Mode,
    /// The sequence of bytes which separates lines.
    pub suffix: Vec<u8>,
    /// The Python function to handle incoming messages.
    pub handler: Arc<PyObject>,
    /// The optional heartbeat with period and beat message.
    pub heartbeat: Option<(u64, Vec<u8>)>,
    /// The timeout (milliseconds) for reconnection attempts.
    pub reconnect_timeout_ms: Option<u64>,
    /// The initial reconnection delay (milliseconds) for reconnects.
    pub reconnect_delay_initial_ms: Option<u64>,
    /// The maximum reconnect delay (milliseconds) for exponential backoff.
    pub reconnect_delay_max_ms: Option<u64>,
    /// The exponential backoff factor for reconnection delays.
    pub reconnect_backoff_factor: Option<f64>,
    /// The maximum jitter (milliseconds) added to reconnection delays.
    pub reconnect_jitter_ms: Option<u64>,
    /// The path to the certificates directory.
    pub certs_dir: Option<String>,
}

/// Creates a TcpStream with the server.
///
/// The stream can be encrypted with TLS or Plain. The stream is split into
/// read and write ends:
/// - The read end is passed to the task that keeps receiving
///   messages from the server and passing them to a handler.
/// - The write end is wrapped in an `Arc<Mutex>` and used to send messages
///   or heart beats.
///
/// The heartbeat is optional and can be configured with an interval and data to
/// send.
///
/// The client uses a suffix to separate messages on the byte stream. It is
/// appended to all sent messages and heartbeats. It is also used to split
/// the received byte stream.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.network")
)]
struct SocketClientInner {
    config: SocketConfig,
    connector: Option<Connector>,
    read_task: Arc<tokio::task::JoinHandle<()>>,
    heartbeat_task: Option<tokio::task::JoinHandle<()>>,
    writer: SharedTcpWriter,
    connection_mode: Arc<AtomicU8>,
    reconnect_timeout: Duration,
    backoff: ExponentialBackoff,
}

impl SocketClientInner {
    pub async fn connect_url(config: SocketConfig) -> anyhow::Result<Self> {
        install_cryptographic_provider();

        let SocketConfig {
            url,
            mode,
            heartbeat,
            suffix,
            handler,
            reconnect_timeout_ms,
            reconnect_delay_initial_ms,
            reconnect_delay_max_ms,
            reconnect_backoff_factor,
            reconnect_jitter_ms,
            certs_dir,
        } = &config;
        let connector = if let Some(dir) = certs_dir {
            let config = create_tls_config_from_certs_dir(Path::new(dir))?;
            Some(Connector::Rustls(Arc::new(config)))
        } else {
            None
        };

        let (reader, writer) = Self::tls_connect_with_server(url, *mode, connector.clone()).await?;
        let writer = Arc::new(Mutex::new(writer));

        let connection_mode = Arc::new(AtomicU8::new(ConnectionMode::Active.as_u8()));

        let handler = Python::with_gil(|py| handler.clone_ref(py));
        let read_task = Arc::new(Self::spawn_read_task(reader, handler, suffix.clone()));

        // Optionally spawn a heartbeat task to periodically ping server
        let heartbeat_task = heartbeat.as_ref().map(|heartbeat| {
            Self::spawn_heartbeat_task(
                connection_mode.clone(),
                heartbeat.clone(),
                writer.clone(),
                suffix.clone(),
            )
        });

        let reconnect_timeout = Duration::from_millis(reconnect_timeout_ms.unwrap_or(10_000));
        let backoff = ExponentialBackoff::new(
            Duration::from_millis(reconnect_delay_initial_ms.unwrap_or(2_000)),
            Duration::from_millis(reconnect_delay_max_ms.unwrap_or(30_000)),
            reconnect_backoff_factor.unwrap_or(1.5),
            reconnect_jitter_ms.unwrap_or(100),
            true, // immediate-first
        );

        Ok(Self {
            config,
            connector,
            read_task,
            heartbeat_task,
            writer,
            connection_mode,
            reconnect_timeout,
            backoff,
        })
    }

    pub async fn tls_connect_with_server(
        url: &str,
        mode: Mode,
        connector: Option<Connector>,
    ) -> Result<(TcpReader, TcpWriter), Error> {
        tracing::debug!("Connecting to server");
        let stream = TcpStream::connect(url).await?;
        tracing::debug!("Making TLS connection");
        let request = url.into_client_request()?;
        tcp_tls(&request, mode, stream, connector)
            .await
            .map(tokio::io::split)
    }

    /// Reconnect with server.
    ///
    /// Make a new connection with server. Use the new read and write halves
    /// to update the shared writer and the read and heartbeat tasks.
    async fn reconnect(&mut self) -> Result<(), Error> {
        tracing::debug!("Reconnecting");

        tokio::time::timeout(self.reconnect_timeout, async {
            // Clean up existing tasks
            shutdown(
                self.read_task.clone(),
                self.heartbeat_task.take(),
                self.writer.clone(),
            )
            .await;

            let SocketConfig {
                url,
                mode,
                heartbeat,
                suffix,
                handler,
                reconnect_timeout_ms: _,
                reconnect_delay_initial_ms: _,
                reconnect_backoff_factor: _,
                reconnect_delay_max_ms: _,
                reconnect_jitter_ms: _,
                certs_dir: _,
            } = &self.config;
            // Create a fresh connection
            let connector = self.connector.clone();
            let (reader, writer) = Self::tls_connect_with_server(url, *mode, connector).await?;
            let writer = Arc::new(Mutex::new(writer));
            self.writer = writer.clone();

            // Spawn new read task
            let handler_for_read = Python::with_gil(|py| handler.clone_ref(py));
            self.read_task = Arc::new(Self::spawn_read_task(
                reader,
                handler_for_read,
                suffix.clone(),
            ));

            // Optionally spawn new heartbeat task
            self.heartbeat_task = heartbeat.as_ref().map(|heartbeat| {
                Self::spawn_heartbeat_task(
                    self.connection_mode.clone(),
                    heartbeat.clone(),
                    writer.clone(),
                    suffix.clone(),
                )
            });

            self.connection_mode
                .store(ConnectionMode::Active.as_u8(), Ordering::SeqCst);

            tracing::debug!("Reconnect succeeded");
            Ok(())
        })
        .await
        .map_err(|_| {
            Error::Io(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!(
                    "reconnection timed out after {}s",
                    self.reconnect_timeout.as_secs_f64()
                ),
            ))
        })?
    }

    /// Check if the client is still alive.
    ///
    /// The client is connected if the read task has not finished. It is expected
    /// that in case of any failure client or server side. The read task will be
    /// shutdown. There might be some delay between the connection being closed
    /// and the client detecting it.
    #[inline]
    #[must_use]
    pub fn is_alive(&self) -> bool {
        !self.read_task.is_finished()
    }

    #[must_use]
    fn spawn_read_task(
        mut reader: TcpReader,
        handler: PyObject,
        suffix: Vec<u8>,
    ) -> tokio::task::JoinHandle<()> {
        tracing::debug!("Started task 'read'");

        tokio::task::spawn(async move {
            let mut buf = Vec::new();

            loop {
                match reader.read_buf(&mut buf).await {
                    // Connection has been terminated or vector buffer is complete
                    Ok(0) => {
                        tracing::debug!("Connection closed by server");
                        break;
                    }
                    Err(e) => {
                        tracing::debug!("Connection ended: {e}");
                        break;
                    }
                    // Received bytes of data
                    Ok(bytes) => {
                        tracing::trace!("Received <binary> {bytes} bytes");

                        // While received data has a line break
                        // drain it and pass it to the handler
                        while let Some((i, _)) = &buf
                            .windows(suffix.len())
                            .enumerate()
                            .find(|(_, pair)| pair.eq(&suffix))
                        {
                            let mut data: Vec<u8> = buf.drain(0..i + suffix.len()).collect();
                            data.truncate(data.len() - suffix.len());

                            if let Err(e) =
                                Python::with_gil(|py| handler.call1(py, (data.as_slice(),)))
                            {
                                tracing::error!("Call to handler failed: {e}");
                                break;
                            }
                        }
                    }
                };
            }

            tracing::debug!("Completed task 'read'");
        })
    }

    fn spawn_heartbeat_task(
        connection_state: Arc<AtomicU8>,
        heartbeat: (u64, Vec<u8>),
        writer: SharedTcpWriter,
        suffix: Vec<u8>,
    ) -> tokio::task::JoinHandle<()> {
        tracing::debug!("Started task 'heartbeat'");
        let (interval_secs, mut message) = heartbeat;

        tokio::task::spawn(async move {
            let interval = Duration::from_secs(interval_secs);
            message.extend(suffix);

            loop {
                tokio::time::sleep(interval).await;

                match ConnectionMode::from_u8(connection_state.load(Ordering::SeqCst)) {
                    ConnectionMode::Active => {
                        let mut guard = writer.lock().await;
                        match guard.write_all(&message).await {
                            Ok(()) => tracing::trace!("Sent heartbeat"),
                            Err(e) => tracing::error!("Failed to send heartbeat: {e}"),
                        }
                    }
                    ConnectionMode::Reconnect => continue,
                    ConnectionMode::Disconnect | ConnectionMode::Closed => break,
                }
            }

            tracing::debug!("Completed task 'heartbeat'");
        })
    }
}

/// Shutdown socket connection.
///
/// The client must be explicitly shutdown before dropping otherwise
/// the connection might still be alive for some time before terminating.
/// Closing the connection is an async call which cannot be done by the
/// drop method so it must be done explicitly.
async fn shutdown(
    read_task: Arc<tokio::task::JoinHandle<()>>,
    heartbeat_task: Option<tokio::task::JoinHandle<()>>,
    writer: SharedTcpWriter,
) {
    tracing::debug!("Shutting down inner client");

    let timeout = Duration::from_secs(5);
    if tokio::time::timeout(timeout, async {
        // Final close of writer
        let mut writer = writer.lock().await;
        if let Err(e) = writer.shutdown().await {
            tracing::error!("Error on shutdown: {e}");
        }
        drop(writer);

        tokio::time::sleep(Duration::from_millis(100)).await;

        // Abort tasks
        if !read_task.is_finished() {
            read_task.abort();
            tracing::debug!("Aborted read task");
        }
        if let Some(task) = heartbeat_task {
            if !task.is_finished() {
                task.abort();
                tracing::debug!("Aborted heartbeat task");
            }
        }
    })
    .await
    .is_err()
    {
        tracing::error!("Shutdown timed out after {}s", timeout.as_secs());
    }

    tracing::debug!("Closed");
}

impl Drop for SocketClientInner {
    fn drop(&mut self) {
        if !self.read_task.is_finished() {
            self.read_task.abort();
        }

        // Cancel heart beat task
        if let Some(ref handle) = self.heartbeat_task.take() {
            if !handle.is_finished() {
                handle.abort();
            }
        }
    }
}

#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.network")
)]
pub struct SocketClient {
    pub(crate) writer: SharedTcpWriter,
    pub(crate) controller_task: tokio::task::JoinHandle<()>,
    pub(crate) connection_mode: Arc<AtomicU8>,
    pub(crate) suffix: Vec<u8>,
}

impl SocketClient {
    pub async fn connect(
        config: SocketConfig,
        post_connection: Option<PyObject>,
        post_reconnection: Option<PyObject>,
        post_disconnection: Option<PyObject>,
    ) -> anyhow::Result<Self> {
        let suffix = config.suffix.clone();
        let inner = SocketClientInner::connect_url(config).await?;
        let writer = inner.writer.clone();
        let connection_mode = inner.connection_mode.clone();

        let controller_task = Self::spawn_controller_task(
            inner,
            connection_mode.clone(),
            post_reconnection,
            post_disconnection,
        );

        if let Some(handler) = post_connection {
            Python::with_gil(|py| match handler.call0(py) {
                Ok(_) => tracing::debug!("Called `post_connection` handler"),
                Err(e) => tracing::error!("Error calling `post_connection` handler: {e}"),
            });
        }

        Ok(Self {
            writer,
            controller_task,
            connection_mode,
            suffix,
        })
    }

    /// Returns the current connection mode.
    #[must_use]
    pub fn connection_mode(&self) -> ConnectionMode {
        ConnectionMode::from_atomic(&self.connection_mode)
    }

    /// Check if the client connection is active.
    ///
    /// Returns `true` if the client is connected and has not been signalled to disconnect.
    /// The client will automatically retry connection based on its configuration.
    #[inline]
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.connection_mode().is_active()
    }

    /// Check if the client is reconnecting.
    ///
    /// Returns `true` if the client lost connection and is attempting to reestablish it.
    /// The client will automatically retry connection based on its configuration.
    #[inline]
    #[must_use]
    pub fn is_reconnecting(&self) -> bool {
        self.connection_mode().is_reconnect()
    }

    /// Check if the client is disconnecting.
    ///
    /// Returns `true` if the client is in disconnect mode.
    #[inline]
    #[must_use]
    pub fn is_disconnecting(&self) -> bool {
        self.connection_mode().is_disconnect()
    }

    /// Check if the client is closed.
    ///
    /// Returns `true` if the client has been explicitly disconnected or reached
    /// maximum reconnection attempts. In this state, the client cannot be reused
    /// and a new client must be created for further connections.
    #[inline]
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.connection_mode().is_closed()
    }

    /// Close the client.
    ///
    /// Controller task will periodically check the disconnect mode
    /// and shutdown the client if it is not alive.
    pub async fn close(&self) {
        self.connection_mode
            .store(ConnectionMode::Disconnect.as_u8(), Ordering::SeqCst);

        match tokio::time::timeout(Duration::from_secs(5), async {
            while !self.is_closed() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }

            if !self.controller_task.is_finished() {
                self.controller_task.abort();
                tracing::debug!("Aborted controller task");
            }
        })
        .await
        {
            Ok(()) => {
                tracing::debug!("Controller task finished");
            }
            Err(_) => {
                tracing::error!("Timeout waiting for controller task to finish");
            }
        }
    }

    pub async fn send_bytes(&self, data: &[u8]) -> Result<(), std::io::Error> {
        if self.is_closed() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                "Not connected",
            ));
        }

        let timeout = Duration::from_secs(2);
        let check_interval = Duration::from_millis(1);

        if !self.is_active() {
            tracing::debug!("Waiting for client to become ACTIVE before sending (2s)...");
            match tokio::time::timeout(timeout, async {
                while !self.is_active() {
                    if matches!(
                        self.connection_mode(),
                        ConnectionMode::Disconnect | ConnectionMode::Closed
                    ) {
                        return Err("Client disconnected waiting to send");
                    }

                    tokio::time::sleep(check_interval).await;
                }

                Ok(())
            })
            .await
            {
                Ok(Ok(())) => tracing::debug!("Client now active"),
                Ok(Err(e)) => {
                    tracing::error!("Cannot send data ({}): {e}", String::from_utf8_lossy(data));
                    return Ok(());
                }
                Err(_) => {
                    tracing::error!(
                        "Cannot send data ({}): timeout waiting to become ACTIVE",
                        String::from_utf8_lossy(data)
                    );
                    return Ok(());
                }
            }
        }

        let mut writer = self.writer.lock().await;
        writer.write_all(data).await?;
        writer.write_all(&self.suffix).await
    }

    fn spawn_controller_task(
        mut inner: SocketClientInner,
        connection_mode: Arc<AtomicU8>,
        post_reconnection: Option<PyObject>,
        post_disconnection: Option<PyObject>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::task::spawn(async move {
            tracing::debug!("Starting task 'controller'");

            let check_interval = Duration::from_millis(10);

            loop {
                tokio::time::sleep(check_interval).await;
                let mode = ConnectionMode::from_atomic(&connection_mode);

                if mode.is_disconnect() {
                    tracing::debug!("Disconnecting");
                    shutdown(
                        inner.read_task.clone(),
                        inner.heartbeat_task.take(),
                        inner.writer.clone(),
                    )
                    .await;

                    if let Some(ref handler) = post_disconnection {
                        Python::with_gil(|py| match handler.call0(py) {
                            Ok(_) => tracing::debug!("Called `post_disconnection` handler"),
                            Err(e) => {
                                tracing::error!("Error calling `post_disconnection` handler: {e}");
                            }
                        });
                    }
                    break; // Controller finished
                }

                if mode.is_reconnect() || (mode.is_active() && !inner.is_alive()) {
                    match inner.reconnect().await {
                        Ok(()) => {
                            tracing::debug!("Reconnected successfully");
                            inner.backoff.reset();

                            if let Some(ref handler) = post_reconnection {
                                Python::with_gil(|py| match handler.call0(py) {
                                    Ok(_) => tracing::debug!("Called `post_reconnection` handler"),
                                    Err(e) => tracing::error!(
                                        "Error calling `post_reconnection` handler: {e}"
                                    ),
                                });
                            }
                        }
                        Err(e) => {
                            let duration = inner.backoff.next_duration();
                            tracing::warn!("Reconnect attempt failed: {e}",);
                            if !duration.is_zero() {
                                tracing::warn!("Backing off for {}s...", duration.as_secs_f64());
                            }
                            tokio::time::sleep(duration).await;
                        }
                    }
                }
            }
            inner
                .connection_mode
                .store(ConnectionMode::Closed.as_u8(), Ordering::SeqCst);
        })
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
#[cfg(target_os = "linux")] // Only run network tests on Linux (CI stability)
mod tests {
    use nautilus_core::python::IntoPyObjectNautilusExt;
    use std::{ffi::CString, net::TcpListener};

    use nautilus_common::testing::wait_until_async;
    use pyo3::prepare_freethreaded_python;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpStream,
        task,
        time::{sleep, Duration},
    };

    use super::*;

    fn create_handler() -> PyObject {
        let code_raw = r"
class Counter:
    def __init__(self):
        self.count = 0
        self.check = False

    def handler(self, bytes):
        msg = bytes.decode()
        if msg == 'ping':
            self.count += 1
        elif msg == 'heartbeat message':
            self.check = True

    def get_check(self):
        return self.check

    def get_count(self):
        return self.count

counter = Counter()
";
        let code = CString::new(code_raw).unwrap();
        let filename = CString::new("test".to_string()).unwrap();
        let module = CString::new("test".to_string()).unwrap();
        Python::with_gil(|py| {
            let pymod = PyModule::from_code(py, &code, &filename, &module).unwrap();
            let counter = pymod.getattr("counter").unwrap().into_py_any_unwrap(py);

            counter
                .getattr(py, "handler")
                .unwrap()
                .into_py_any_unwrap(py)
        })
    }

    fn bind_test_server() -> (u16, TcpListener) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind ephemeral port");
        let port = listener.local_addr().unwrap().port();
        (port, listener)
    }

    async fn run_echo_server(mut socket: TcpStream) {
        let mut buf = Vec::new();
        loop {
            match socket.read_buf(&mut buf).await {
                Ok(0) => {
                    break;
                }
                Ok(_n) => {
                    while let Some(idx) = buf.windows(2).position(|w| w == b"\r\n") {
                        let mut line = buf.drain(..idx + 2).collect::<Vec<u8>>();
                        // Remove trailing \r\n
                        line.truncate(line.len() - 2);

                        if line == b"close" {
                            let _ = socket.shutdown().await;
                            return;
                        }

                        let mut echo_data = line;
                        echo_data.extend_from_slice(b"\r\n");
                        if socket.write_all(&echo_data).await.is_err() {
                            break;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Server read error: {e}");
                    break;
                }
            }
        }
    }

    #[tokio::test]
    async fn test_basic_send_receive() {
        prepare_freethreaded_python();

        let (port, listener) = bind_test_server();
        let server_task = task::spawn(async move {
            let (socket, _) = tokio::net::TcpListener::from_std(listener)
                .unwrap()
                .accept()
                .await
                .unwrap();
            run_echo_server(socket).await;
        });

        let config = SocketConfig {
            url: format!("127.0.0.1:{port}"),
            mode: Mode::Plain,
            suffix: b"\r\n".to_vec(),
            handler: Arc::new(create_handler()),
            heartbeat: None,
            reconnect_timeout_ms: None,
            reconnect_delay_initial_ms: None,
            reconnect_backoff_factor: None,
            reconnect_delay_max_ms: None,
            reconnect_jitter_ms: None,
            certs_dir: None,
        };

        let client = SocketClient::connect(config, None, None, None)
            .await
            .expect("Client connect failed unexpectedly");

        client.send_bytes(b"Hello").await.unwrap();
        client.send_bytes(b"World").await.unwrap();

        // Wait a bit for the server to echo them back
        sleep(Duration::from_millis(100)).await;

        client.send_bytes(b"close").await.unwrap();
        server_task.await.unwrap();
        assert!(!client.is_closed());
    }

    #[tokio::test]
    async fn test_reconnect_fail_exhausted() {
        prepare_freethreaded_python();

        let (port, listener) = bind_test_server();
        drop(listener); // We drop it immediately -> no server is listening

        let config = SocketConfig {
            url: format!("127.0.0.1:{port}"),
            mode: Mode::Plain,
            suffix: b"\r\n".to_vec(),
            handler: Arc::new(create_handler()),
            heartbeat: None,
            reconnect_timeout_ms: None,
            reconnect_delay_initial_ms: None,
            reconnect_backoff_factor: None,
            reconnect_delay_max_ms: None,
            reconnect_jitter_ms: None,
            certs_dir: None,
        };

        let client_res = SocketClient::connect(config, None, None, None).await;
        assert!(
            client_res.is_err(),
            "Should fail quickly with no server listening"
        );
    }

    #[tokio::test]
    async fn test_user_disconnect() {
        prepare_freethreaded_python();

        let (port, listener) = bind_test_server();
        let server_task = task::spawn(async move {
            let (socket, _) = tokio::net::TcpListener::from_std(listener)
                .unwrap()
                .accept()
                .await
                .unwrap();
            let mut buf = [0u8; 1024];
            let _ = socket.try_read(&mut buf);

            loop {
                sleep(Duration::from_secs(1)).await;
            }
        });

        let config = SocketConfig {
            url: format!("127.0.0.1:{port}"),
            mode: Mode::Plain,
            suffix: b"\r\n".to_vec(),
            handler: Arc::new(create_handler()),
            heartbeat: None,
            reconnect_timeout_ms: None,
            reconnect_delay_initial_ms: None,
            reconnect_backoff_factor: None,
            reconnect_delay_max_ms: None,
            reconnect_jitter_ms: None,
            certs_dir: None,
        };

        let client = SocketClient::connect(config, None, None, None)
            .await
            .unwrap();

        client.close().await;
        assert!(client.is_closed());
        server_task.abort();
    }

    #[tokio::test]
    async fn test_heartbeat() {
        prepare_freethreaded_python();

        let (port, listener) = bind_test_server();
        let received = Arc::new(Mutex::new(Vec::new()));
        let received2 = received.clone();

        let server_task = task::spawn(async move {
            let (socket, _) = tokio::net::TcpListener::from_std(listener)
                .unwrap()
                .accept()
                .await
                .unwrap();

            let mut buf = Vec::new();
            loop {
                match socket.try_read_buf(&mut buf) {
                    Ok(0) => break,
                    Ok(_) => {
                        while let Some(idx) = buf.windows(2).position(|w| w == b"\r\n") {
                            let mut line = buf.drain(..idx + 2).collect::<Vec<u8>>();
                            line.truncate(line.len() - 2);
                            received2.lock().await.push(line);
                        }
                    }
                    Err(_) => {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                }
            }
        });

        // Heartbeat every 1 second
        let heartbeat = Some((1, b"ping".to_vec()));

        let config = SocketConfig {
            url: format!("127.0.0.1:{port}"),
            mode: Mode::Plain,
            suffix: b"\r\n".to_vec(),
            handler: Arc::new(create_handler()),
            heartbeat,
            reconnect_timeout_ms: None,
            reconnect_delay_initial_ms: None,
            reconnect_backoff_factor: None,
            reconnect_delay_max_ms: None,
            reconnect_jitter_ms: None,
            certs_dir: None,
        };

        let client = SocketClient::connect(config, None, None, None)
            .await
            .unwrap();

        // Wait ~3 seconds to collect some heartbeats
        sleep(Duration::from_secs(3)).await;

        {
            let lock = received.lock().await;
            let pings = lock
                .iter()
                .filter(|line| line == &&b"ping".to_vec())
                .count();
            assert!(
                pings >= 2,
                "Expected at least 2 heartbeat pings; got {pings}"
            );
        }

        client.close().await;
        server_task.abort();
    }

    #[tokio::test]
    async fn test_python_handler_error() {
        prepare_freethreaded_python();

        let (port, listener) = bind_test_server();
        let server_task = task::spawn(async move {
            let (socket, _) = tokio::net::TcpListener::from_std(listener)
                .unwrap()
                .accept()
                .await
                .unwrap();
            run_echo_server(socket).await;
        });

        let code_raw = r#"
def handler(bytes_data):
    txt = bytes_data.decode()
    if "ERR" in txt:
        raise ValueError("Simulated error in handler")
    return
"#;
        let code = CString::new(code_raw).unwrap();
        let filename = CString::new("test".to_string()).unwrap();
        let module = CString::new("test".to_string()).unwrap();

        let handler = Python::with_gil(|py| {
            let pymod = PyModule::from_code(py, &code, &filename, &module).unwrap();
            let func = pymod.getattr("handler").unwrap();
            Arc::new(func.into_py_any_unwrap(py))
        });

        let config = SocketConfig {
            url: format!("127.0.0.1:{port}"),
            mode: Mode::Plain,
            suffix: b"\r\n".to_vec(),
            handler,
            heartbeat: None,
            reconnect_timeout_ms: None,
            reconnect_delay_initial_ms: None,
            reconnect_backoff_factor: None,
            reconnect_delay_max_ms: None,
            reconnect_jitter_ms: None,
            certs_dir: None,
        };

        let client = SocketClient::connect(config, None, None, None)
            .await
            .expect("Client connect failed unexpectedly");

        client.send_bytes(b"hello").await.unwrap();
        sleep(Duration::from_millis(100)).await;

        client.send_bytes(b"ERR").await.unwrap();
        sleep(Duration::from_secs(1)).await;

        assert!(client.is_active());

        client.close().await;

        assert!(client.is_closed());
        server_task.abort();
    }

    #[tokio::test]
    async fn test_reconnect_success() {
        prepare_freethreaded_python();

        let (port, listener) = bind_test_server();
        let listener = tokio::net::TcpListener::from_std(listener).unwrap();

        // Spawn a server task that:
        // 1. Accepts the first connection and then drops it after a short delay (simulate disconnect)
        // 2. Waits a bit and then accepts a new connection and runs the echo server
        let server_task = task::spawn(async move {
            // Accept first connection
            let (mut socket, _) = listener.accept().await.expect("First accept failed");

            // Wait briefly and then force-close the connection
            sleep(Duration::from_millis(500)).await;
            let _ = socket.shutdown().await;

            // Wait for the client's reconnect attempt
            sleep(Duration::from_millis(500)).await;

            // Run the echo server on the new connection
            let (socket, _) = listener.accept().await.expect("Second accept failed");
            run_echo_server(socket).await;
        });

        let config = SocketConfig {
            url: format!("127.0.0.1:{port}"),
            mode: Mode::Plain,
            suffix: b"\r\n".to_vec(),
            handler: Arc::new(create_handler()),
            heartbeat: None,
            reconnect_timeout_ms: Some(5_000),
            reconnect_delay_initial_ms: Some(500),
            reconnect_delay_max_ms: Some(5_000),
            reconnect_backoff_factor: Some(2.0),
            reconnect_jitter_ms: Some(50),
            certs_dir: None,
        };

        let client = SocketClient::connect(config, None, None, None)
            .await
            .expect("Client connect failed unexpectedly");

        // Initially, the client should be active
        assert!(client.is_active(), "Client should start as active");

        // Wait until the client loses connection (i.e. not active),
        // then wait until it reconnects (active again).
        wait_until_async(|| async { client.is_active() }, Duration::from_secs(10)).await;

        client
            .send_bytes(b"TestReconnect")
            .await
            .expect("Send failed");

        client.close().await;
        server_task.abort();
    }
}
