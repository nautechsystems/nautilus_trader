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
//! and state management.

use std::{
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        Arc,
    },
    time::Duration,
};

use nautilus_cryptography::providers::install_cryptographic_provider;
use pyo3::prelude::*;
use tokio::{
    io::{split, AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf},
    net::TcpStream,
    sync::Mutex,
    task,
    time::sleep,
};
use tokio_tungstenite::{
    tungstenite::{client::IntoClientRequest, stream::Mode, Error},
    MaybeTlsStream,
};

use crate::tls::tcp_tls;

type TcpWriter = WriteHalf<MaybeTlsStream<TcpStream>>;
type SharedTcpWriter = Arc<Mutex<WriteHalf<MaybeTlsStream<TcpStream>>>>;
type TcpReader = ReadHalf<MaybeTlsStream<TcpStream>>;

/// Connection state for the Socket client.
///
/// - ACTIVE: Normal operation, all tasks running
/// - RECONNECTING: In process of reconnecting, tasks paused
/// - CLOSED: Connection terminated, cleanup in progress
///
/// Connection state transitions:
/// ACTIVE <-> RECONNECTING: During reconnection attempts
/// ACTIVE/RECONNECTING -> CLOSED: Only when controller task terminates
const CONNECTION_ACTIVE: u8 = 0;
const CONNECTION_RECONNECTING: u8 = 1;
const CONNECTION_CLOSED: u8 = 2;

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
    pub max_reconnection_tries: Option<u64>,
}

/// Creates a TcpStream with the server.
///
/// The stream can be encrypted with TLS or Plain. The stream is split into
/// read and write ends.
/// * The read end is passed to task that keeps receiving
///   messages from the server and passing them to a handler.
/// * The write end is wrapped in an Arc Mutex and used to send messages
///   or heart beats
///
/// The heartbeat is optional and can be configured with an interval and data to
/// send.
///
/// The client uses a suffix to separate messages on the byte stream. It is
/// appended to all sent messages and heartbeats. It is also used the split
/// the received byte stream.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.network")
)]
struct SocketClientInner {
    config: SocketConfig,
    read_task: Arc<task::JoinHandle<()>>,
    heartbeat_task: Option<task::JoinHandle<()>>,
    writer: SharedTcpWriter,
    reconnection_lock: Arc<Mutex<()>>,
    connection_state: Arc<AtomicU8>,
}

impl SocketClientInner {
    pub async fn connect_url(config: SocketConfig) -> Result<Self, Error> {
        install_cryptographic_provider();

        let SocketConfig {
            url,
            mode,
            heartbeat,
            suffix,
            handler,
            max_reconnection_tries: _,
        } = &config;
        let (reader, writer) = Self::tls_connect_with_server(url, *mode).await?;
        let writer = Arc::new(Mutex::new(writer));

        let connection_state = Arc::new(AtomicU8::new(CONNECTION_ACTIVE));
        let reconnection_lock = Arc::new(Mutex::new(()));

        let handler1 = Python::with_gil(|py| handler.clone_ref(py));
        // Keep receiving messages from socket pass them as arguments to handler
        let read_task = Arc::new(Self::spawn_read_task(reader, handler1, suffix.clone()));

        // Optionally create heartbeat task
        let heartbeat_task = Self::spawn_heartbeat_task(
            connection_state.clone(),
            heartbeat.clone(),
            writer.clone(),
            suffix.clone(),
        );

        Ok(Self {
            config,
            read_task,
            heartbeat_task,
            writer,
            reconnection_lock,
            connection_state,
        })
    }

    pub async fn tls_connect_with_server(
        url: &str,
        mode: Mode,
    ) -> Result<(TcpReader, TcpWriter), Error> {
        tracing::debug!("Connecting to server");
        let stream = TcpStream::connect(url).await?;
        tracing::debug!("Making TLS connection");
        let request = url.into_client_request()?;
        tcp_tls(&request, mode, stream, None).await.map(split)
    }

    #[must_use]
    fn spawn_read_task(
        mut reader: TcpReader,
        handler: PyObject,
        suffix: Vec<u8>,
    ) -> task::JoinHandle<()> {
        tracing::debug!("Started task 'read'");

        task::spawn(async move {
            let mut buf = Vec::new();

            loop {
                match reader.read_buf(&mut buf).await {
                    // Connection has been terminated or vector buffer is complete
                    Ok(0) => {
                        tracing::error!("Cannot read anymore bytes");
                        break;
                    }
                    Err(e) => {
                        tracing::error!("Failed with error: {e}");
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
        })
    }

    /// Optionally spawn a heartbeat task to periodically ping the server.
    fn spawn_heartbeat_task(
        connection_state: Arc<AtomicU8>,
        heartbeat: Option<(u64, Vec<u8>)>,
        writer: SharedTcpWriter,
        suffix: Vec<u8>,
    ) -> Option<task::JoinHandle<()>> {
        heartbeat.map(|(duration, mut message)| {
            task::spawn(async move {
                let duration = Duration::from_secs(duration);
                message.extend(suffix);
                while connection_state.load(Ordering::SeqCst) == CONNECTION_ACTIVE {
                    sleep(duration).await;
                    if connection_state.load(Ordering::SeqCst) != CONNECTION_ACTIVE {
                        break;
                    }
                    tracing::debug!("Sending heartbeat");
                    let mut guard = writer.lock().await;
                    match guard.write_all(&message).await {
                        Ok(()) => tracing::debug!("Sent heartbeat"),
                        Err(e) => tracing::error!("Failed to send heartbeat: {e}"),
                    }
                }
            })
        })
    }

    /// Reconnect with server.
    ///
    /// Make a new connection with server. Use the new read and write halves
    /// to update the shared writer and the read and heartbeat tasks.
    async fn reconnect(&mut self) -> Result<(), Error> {
        // TODO: Expose reconnect timeout as config option
        let timeout = Duration::from_secs(30);
        tracing::debug!("Reconnecting client");

        tokio::time::timeout(timeout, async {
            let state_guard = {
                let guard = self.reconnection_lock.lock().await;
                self.connection_state
                    .store(CONNECTION_RECONNECTING, Ordering::SeqCst);
                guard
            };

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
                max_reconnection_tries: _,
            } = &self.config;
            // Create a fresh connection
            let (reader, new_writer) = Self::tls_connect_with_server(url, *mode).await?;

            let new_writer_arc = Arc::new(Mutex::new(new_writer));
            self.writer = new_writer_arc.clone();

            // Spawn new read task
            let handler_for_read = Python::with_gil(|py| handler.clone_ref(py));
            self.read_task = Arc::new(Self::spawn_read_task(
                reader,
                handler_for_read,
                suffix.clone(),
            ));

            // Spawn new heartbeat task
            self.heartbeat_task = Self::spawn_heartbeat_task(
                self.connection_state.clone(),
                heartbeat.clone(),
                new_writer_arc,
                suffix.clone(),
            );

            drop(state_guard);
            self.connection_state
                .store(CONNECTION_ACTIVE, Ordering::SeqCst);

            tracing::debug!("Reconnect succeeded");
            Ok(())
        })
        .await
        .map_err(|_| {
            Error::Io(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("reconnection timed out after {}s", timeout.as_secs()),
            ))
        })?
    }

    /// Check if the client is still connected.
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
}

/// Shutdown socket connection.
///
/// The client must be explicitly shutdown before dropping otherwise
/// the connection might still be alive for some time before terminating.
/// Closing the connection is an async call which cannot be done by the
/// drop method so it must be done explicitly.
async fn shutdown(
    read_task: Arc<task::JoinHandle<()>>,
    heartbeat_task: Option<task::JoinHandle<()>>,
    writer: SharedTcpWriter,
) {
    tracing::debug!("Closing");

    let timeout = Duration::from_secs(5);
    if tokio::time::timeout(timeout, async {
        // Final close of writer
        let mut writer = writer.lock().await;
        if let Err(e) = writer.shutdown().await {
            tracing::error!("Error on shutdown: {e}");
        }
        drop(writer);

        sleep(Duration::from_millis(100)).await;

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
    pub(crate) controller_task: task::JoinHandle<()>,
    pub(crate) disconnect_mode: Arc<AtomicBool>,
    pub(crate) suffix: Vec<u8>,
}

impl SocketClient {
    pub async fn connect(
        config: SocketConfig,
        post_connection: Option<PyObject>,
        post_reconnection: Option<PyObject>,
        post_disconnection: Option<PyObject>,
    ) -> Result<Self, Error> {
        let suffix = config.suffix.clone();
        let max_reconnection_tries = config.max_reconnection_tries;
        let inner = SocketClientInner::connect_url(config).await?;
        let writer = inner.writer.clone();
        let disconnect_mode = Arc::new(AtomicBool::new(false));

        let controller_task = Self::spawn_controller_task(
            inner,
            disconnect_mode.clone(),
            post_reconnection,
            post_disconnection,
            max_reconnection_tries,
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
            disconnect_mode,
            suffix,
        })
    }

    /// Set disconnect mode to true.
    ///
    /// Controller task will periodically check the disconnect mode
    /// and shutdown the client if it is not alive.
    pub async fn disconnect(&self) {
        self.disconnect_mode.store(true, Ordering::SeqCst);

        match tokio::time::timeout(Duration::from_secs(5), async {
            while !self.is_disconnected() {
                sleep(Duration::from_millis(10)).await;
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
        let mut writer = self.writer.lock().await;
        writer.write_all(data).await?;
        writer.write_all(&self.suffix).await
    }

    #[must_use]
    pub fn is_disconnected(&self) -> bool {
        self.controller_task.is_finished()
    }

    fn spawn_controller_task(
        mut inner: SocketClientInner,
        disconnect_mode: Arc<AtomicBool>,
        post_reconnection: Option<PyObject>,
        post_disconnection: Option<PyObject>,
        max_reconnection_tries: Option<u64>,
    ) -> task::JoinHandle<()> {
        task::spawn(async move {
            let check_interval = Duration::from_millis(100);
            let retry_interval = Duration::from_millis(1000);
            let mut retry_counter: u64 = 0;

            loop {
                sleep(check_interval).await;

                // Check if client needs to disconnect
                let disconnect = disconnect_mode.load(Ordering::SeqCst);
                match (disconnect, inner.is_alive()) {
                    (false, false) => match inner.reconnect().await {
                        Ok(()) => {
                            tracing::debug!("Reconnected successfully");
                            retry_counter = 0;

                            if let Some(ref handler) = post_reconnection {
                                Python::with_gil(|py| match handler.call0(py) {
                                    Ok(_) => tracing::debug!("Called `post_reconnection` handler"),
                                    Err(e) => {
                                        tracing::error!(
                                            "Error calling `post_reconnection` handler: {e}"
                                        );
                                    }
                                });
                            }
                        }
                        Err(e) => {
                            retry_counter += 1;

                            if let Some(max) = max_reconnection_tries {
                                tracing::warn!("Reconnect failed {e}. Retry {retry_counter}/{max}");

                                if retry_counter >= max {
                                    tracing::error!("Reached max reconnection tries");
                                    break;
                                }
                            } else {
                                tracing::warn!(
                                    "Reconnect failed {e}. Retry {retry_counter} (infinite)"
                                );
                            }

                            sleep(retry_interval).await;
                        }
                    },
                    (true, true) => {
                        tracing::debug!("Shutting down inner client");
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
                                    tracing::error!(
                                        "Error calling `post_disconnection` handler: {e}"
                                    );
                                }
                            });
                        }
                        break;
                    }
                    (true, false) => {
                        tracing::debug!("Inner client is disconnected");
                        tracing::debug!("Shutting down inner client to clean up running tasks");
                        shutdown(
                            inner.read_task.clone(),
                            inner.heartbeat_task.take(),
                            inner.writer.clone(),
                        )
                        .await;
                    }
                    _ => (),
                }
            }
            inner
                .connection_state
                .store(CONNECTION_CLOSED, Ordering::SeqCst);
        })
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
#[cfg(target_os = "linux")] // Only run network tests on Linux (CI stability)
mod tests {
    use std::{ffi::CString, net::TcpListener};

    use pyo3::prepare_freethreaded_python;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpStream,
        task,
        time::{sleep, Duration},
    };

    use super::*;

    fn create_handler() -> PyObject {
        let code = r#"
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
"#;

        let code = CString::new(code).unwrap();
        let filename = CString::new("test".to_string()).unwrap();
        let module = CString::new("test".to_string()).unwrap();
        Python::with_gil(|py| {
            let pymod = PyModule::from_code(py, &code, &filename, &module).unwrap();
            let counter = pymod.getattr("counter").unwrap().into_py(py);
            let handler = counter.getattr(py, "handler").unwrap().into_py(py);
            handler
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
            max_reconnection_tries: Some(1),
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
        assert!(!client.is_disconnected());
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
            max_reconnection_tries: Some(2),
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
            max_reconnection_tries: None,
        };

        let client = SocketClient::connect(config, None, None, None)
            .await
            .unwrap();

        client.disconnect().await;
        assert!(client.is_disconnected());
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
            handler: Arc::new(create_handler().into()),
            heartbeat,
            max_reconnection_tries: None,
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

        client.disconnect().await;
        server_task.abort();
    }

    //     #[tokio::test]
    //     async fn test_python_handler_error() {
    //         prepare_freethreaded_python();
    //
    //         let (port, listener) = bind_test_server();
    //         let server_task = task::spawn(async move {
    //             let (socket, _) = tokio::net::TcpListener::from_std(listener)
    //                 .unwrap()
    //                 .accept()
    //                 .await
    //                 .unwrap();
    //             run_echo_server(socket).await;
    //         });
    //
    //         let handler = Python::with_gil(|py| {
    //             let code = r#"
    // def handler(bytes_data):
    //     txt = bytes_data.decode()
    //     if "ERR" in txt:
    //         raise ValueError("Simulated error in handler")
    //     return
    // "#;
    //             let module =
    //                 PyModule::from_code(py, code, "error_handler.py", "error_handler").unwrap();
    //             let func = module.getattr("handler").unwrap();
    //             Arc::new(func.into_py(py))
    //         });
    //
    //         let config = SocketConfig {
    //             url: format!("127.0.0.1:{port}"),
    //             mode: Mode::Plain,
    //             suffix: b"\r\n".to_vec(),
    //             handler,
    //             heartbeat: None,
    //             max_reconnection_tries: Some(1),
    //         };
    //
    //         let client = SocketClient::connect(config, None, None, None)
    //             .await
    //             .expect("Client connect failed unexpectedly");
    //
    //         client.send_bytes(b"hello").await.unwrap();
    //         sleep(Duration::from_millis(100)).await;
    //
    //         client.send_bytes(b"ERR").await.unwrap();
    //         sleep(Duration::from_secs(1)).await;
    //
    //         assert!(!client.is_disconnected());
    //
    //         client.disconnect().await;
    //
    //         assert!(client.is_disconnected());
    //         server_task.abort();
    //     }
}
