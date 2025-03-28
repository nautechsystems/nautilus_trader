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

//! **Key features**:
//! - Connection state tracking (ACTIVE/RECONNECTING/DISCONNECTING/CLOSED)
//! - Synchronized reconnection with backoff
//! - Split read/write architecture
//! - Python callback integration
//!
//! **Design**:
//! - Single reader, multiple writer model
//! - Read half runs in dedicated task
//! - Write half runs in dedicated task connected with channel
//! - Controller task manages lifecycle

use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    time::Duration,
};

use bytes::Bytes;
use nautilus_cryptography::providers::install_cryptographic_provider;
use pyo3::prelude::*;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf},
    net::TcpStream,
};
use tokio_tungstenite::{
    MaybeTlsStream,
    tungstenite::{Error, client::IntoClientRequest, stream::Mode},
};

use crate::{
    backoff::ExponentialBackoff,
    fix::process_fix_buffer,
    mode::ConnectionMode,
    tls::{Connector, create_tls_config_from_certs_dir, tcp_tls},
};

type TcpWriter = WriteHalf<MaybeTlsStream<TcpStream>>;
type TcpReader = ReadHalf<MaybeTlsStream<TcpStream>>;
pub type TcpMessageHandler = dyn Fn(&[u8]) + Send + Sync;

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
    /// The optional Python function to handle incoming messages.
    pub py_handler: Option<Arc<PyObject>>,
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

/// Represents a command for the writer task.
#[derive(Debug)]
pub enum WriterCommand {
    /// Update the writer reference with a new one after reconnection.
    Update(TcpWriter),
    /// Send data to the server.
    Send(Bytes),
}

/// Creates a TcpStream with the server.
///
/// The stream can be encrypted with TLS or Plain. The stream is split into
/// read and write ends:
/// - The read end is passed to the task that keeps receiving
///   messages from the server and passing them to a handler.
/// - The write end is passed to a task which receives messages over a channel
///   to send to the server.
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
    write_task: tokio::task::JoinHandle<()>,
    writer_tx: tokio::sync::mpsc::UnboundedSender<WriterCommand>,
    heartbeat_task: Option<tokio::task::JoinHandle<()>>,
    connection_mode: Arc<AtomicU8>,
    reconnect_timeout: Duration,
    backoff: ExponentialBackoff,
    handler: Option<Arc<TcpMessageHandler>>,
}

impl SocketClientInner {
    pub async fn connect_url(
        config: SocketConfig,
        handler: Option<Arc<TcpMessageHandler>>,
    ) -> anyhow::Result<Self> {
        install_cryptographic_provider();

        let SocketConfig {
            url,
            mode,
            heartbeat,
            suffix,
            py_handler,
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
        tracing::debug!("Connected");

        let connection_mode = Arc::new(AtomicU8::new(ConnectionMode::Active.as_u8()));

        let read_task = Arc::new(Self::spawn_read_task(
            connection_mode.clone(),
            reader,
            handler.clone(),
            py_handler.clone(),
            suffix.clone(),
        ));

        let (writer_tx, writer_rx) = tokio::sync::mpsc::unbounded_channel::<WriterCommand>();

        let write_task =
            Self::spawn_write_task(connection_mode.clone(), writer, writer_rx, suffix.clone());

        // Optionally spawn a heartbeat task to periodically ping server
        let heartbeat_task = heartbeat.as_ref().map(|heartbeat| {
            Self::spawn_heartbeat_task(
                connection_mode.clone(),
                heartbeat.clone(),
                writer_tx.clone(),
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
            write_task,
            writer_tx,
            heartbeat_task,
            connection_mode,
            reconnect_timeout,
            backoff,
            handler,
        })
    }

    pub async fn tls_connect_with_server(
        url: &str,
        mode: Mode,
        connector: Option<Connector>,
    ) -> Result<(TcpReader, TcpWriter), Error> {
        tracing::debug!("Connecting to {url}");
        let tcp_result = TcpStream::connect(url).await;

        match tcp_result {
            Ok(stream) => {
                tracing::debug!("TCP connection established, proceeding with TLS");
                let request = url.into_client_request()?;
                tcp_tls(&request, mode, stream, connector)
                    .await
                    .map(tokio::io::split)
            }
            Err(e) => {
                tracing::error!("TCP connection failed: {e:?}");
                Err(Error::Io(e))
            }
        }
    }

    /// Reconnect with server.
    ///
    /// Makes a new connection with server, uses the new read and write halves
    /// to update the reader and writer.
    async fn reconnect(&mut self) -> Result<(), Error> {
        tracing::debug!("Reconnecting");

        tokio::time::timeout(self.reconnect_timeout, async {
            let SocketConfig {
                url,
                mode,
                heartbeat: _,
                suffix,
                py_handler,
                reconnect_timeout_ms: _,
                reconnect_delay_initial_ms: _,
                reconnect_backoff_factor: _,
                reconnect_delay_max_ms: _,
                reconnect_jitter_ms: _,
                certs_dir: _,
            } = &self.config;
            // Create a fresh connection
            let connector = self.connector.clone();
            let (reader, new_writer) = Self::tls_connect_with_server(url, *mode, connector).await?;
            tracing::debug!("Connected");

            if let Err(e) = self.writer_tx.send(WriterCommand::Update(new_writer)) {
                tracing::error!("{e}");
            }

            // Delay before closing connection
            tokio::time::sleep(Duration::from_millis(100)).await;

            if !self.read_task.is_finished() {
                self.read_task.abort();
                tracing::debug!("Aborted task 'read'");
            }

            self.connection_mode
                .store(ConnectionMode::Active.as_u8(), Ordering::SeqCst);

            // Spawn new read task
            self.read_task = Arc::new(Self::spawn_read_task(
                self.connection_mode.clone(),
                reader,
                self.handler.clone(),
                py_handler.clone(),
                suffix.clone(),
            ));

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
        connection_state: Arc<AtomicU8>,
        mut reader: TcpReader,
        handler: Option<Arc<TcpMessageHandler>>,
        py_handler: Option<Arc<PyObject>>,
        suffix: Vec<u8>,
    ) -> tokio::task::JoinHandle<()> {
        tracing::debug!("Started task 'read'");

        // Interval between checking the connection mode
        let check_interval = Duration::from_millis(10);

        tokio::task::spawn(async move {
            let mut buf = Vec::new();

            loop {
                if !ConnectionMode::from_atomic(&connection_state).is_active() {
                    break;
                }

                match tokio::time::timeout(check_interval, reader.read_buf(&mut buf)).await {
                    // Connection has been terminated or vector buffer is complete
                    Ok(Ok(0)) => {
                        tracing::debug!("Connection closed by server");
                        break;
                    }
                    Ok(Err(e)) => {
                        tracing::debug!("Connection ended: {e}");
                        break;
                    }
                    // Received bytes of data
                    Ok(Ok(bytes)) => {
                        tracing::trace!("Received <binary> {bytes} bytes");

                        if let Some(handler) = &handler {
                            process_fix_buffer(&mut buf, handler);
                        } else {
                            while let Some((i, _)) = &buf
                                .windows(suffix.len())
                                .enumerate()
                                .find(|(_, pair)| pair.eq(&suffix))
                            {
                                let mut data: Vec<u8> = buf.drain(0..i + suffix.len()).collect();
                                data.truncate(data.len() - suffix.len());

                                if let Some(handler) = &handler {
                                    handler(&data);
                                }

                                if let Some(py_handler) = &py_handler {
                                    if let Err(e) = Python::with_gil(|py| {
                                        py_handler.call1(py, (data.as_slice(),))
                                    }) {
                                        tracing::error!("Call to handler failed: {e}");
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    Err(_) => {
                        // Timeout - continue loop and check connection mode
                        continue;
                    }
                }
            }

            tracing::debug!("Completed task 'read'");
        })
    }

    fn spawn_write_task(
        connection_state: Arc<AtomicU8>,
        writer: TcpWriter,
        mut writer_rx: tokio::sync::mpsc::UnboundedReceiver<WriterCommand>,
        suffix: Vec<u8>,
    ) -> tokio::task::JoinHandle<()> {
        tracing::debug!("Started task 'write'");

        // Interval between checking the connection mode
        let check_interval = Duration::from_millis(10);

        tokio::task::spawn(async move {
            let mut active_writer = writer;

            loop {
                if matches!(
                    ConnectionMode::from_atomic(&connection_state),
                    ConnectionMode::Disconnect | ConnectionMode::Closed
                ) {
                    break;
                }

                match tokio::time::timeout(check_interval, writer_rx.recv()).await {
                    Ok(Some(msg)) => {
                        // Re-check connection mode after receiving a message
                        let mode = ConnectionMode::from_atomic(&connection_state);
                        if matches!(mode, ConnectionMode::Disconnect | ConnectionMode::Closed) {
                            break;
                        }

                        match msg {
                            WriterCommand::Update(new_writer) => {
                                tracing::debug!("Received new writer");

                                // Delay before closing connection
                                tokio::time::sleep(Duration::from_millis(100)).await;

                                // Attempt to shutdown the writer gracefully before updating,
                                // we ignore any error as the writer may already be closed.
                                _ = active_writer.shutdown().await;

                                active_writer = new_writer;
                                tracing::debug!("Updated writer");
                            }
                            _ if mode.is_reconnect() => {
                                tracing::warn!("Skipping message while reconnecting, {msg:?}");
                                continue;
                            }
                            WriterCommand::Send(msg) => {
                                if let Err(e) = active_writer.write_all(&msg).await {
                                    tracing::error!("Failed to send message: {e}");
                                    // Mode is active so trigger reconnection
                                    tracing::warn!("Writer triggering reconnect");
                                    connection_state
                                        .store(ConnectionMode::Reconnect.as_u8(), Ordering::SeqCst);
                                    continue;
                                }
                                if let Err(e) = active_writer.write_all(&suffix).await {
                                    tracing::error!("Failed to send message: {e}");
                                }
                            }
                        }
                    }
                    Ok(None) => {
                        // Channel closed - writer task should terminate
                        tracing::debug!("Writer channel closed, terminating writer task");
                        break;
                    }
                    Err(_) => {
                        // Timeout - just continue the loop
                        continue;
                    }
                }
            }

            // Attempt to shutdown the writer gracefully before exiting,
            // we ignore any error as the writer may already be closed.
            _ = active_writer.shutdown().await;

            tracing::debug!("Completed task 'write'");
        })
    }

    fn spawn_heartbeat_task(
        connection_state: Arc<AtomicU8>,
        heartbeat: (u64, Vec<u8>),
        writer_tx: tokio::sync::mpsc::UnboundedSender<WriterCommand>,
    ) -> tokio::task::JoinHandle<()> {
        tracing::debug!("Started task 'heartbeat'");
        let (interval_secs, message) = heartbeat;

        tokio::task::spawn(async move {
            let interval = Duration::from_secs(interval_secs);

            loop {
                tokio::time::sleep(interval).await;

                match ConnectionMode::from_u8(connection_state.load(Ordering::SeqCst)) {
                    ConnectionMode::Active => {
                        let msg = WriterCommand::Send(message.clone().into());

                        match writer_tx.send(msg) {
                            Ok(()) => tracing::trace!("Sent heartbeat to writer task"),
                            Err(e) => {
                                tracing::error!("Failed to send heartbeat to writer task: {e}");
                            }
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

impl Drop for SocketClientInner {
    fn drop(&mut self) {
        if !self.read_task.is_finished() {
            self.read_task.abort();
            tracing::debug!("Aborted task 'read'");
        }

        if !self.write_task.is_finished() {
            self.write_task.abort();
            tracing::debug!("Aborted task 'write'");
        }

        if let Some(ref handle) = self.heartbeat_task.take() {
            if !handle.is_finished() {
                handle.abort();
                tracing::debug!("Aborted task 'heartbeat'");
            }
        }
    }
}

#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.network")
)]
pub struct SocketClient {
    pub(crate) controller_task: tokio::task::JoinHandle<()>,
    pub(crate) connection_mode: Arc<AtomicU8>,
    pub writer_tx: tokio::sync::mpsc::UnboundedSender<WriterCommand>,
}

impl SocketClient {
    /// Connect to the server.
    ///
    /// # Errors
    ///
    /// Returns any error connecting to the server.
    pub async fn connect(
        config: SocketConfig,
        handler: Option<Arc<TcpMessageHandler>>,
        post_connection: Option<PyObject>,
        post_reconnection: Option<PyObject>,
        post_disconnection: Option<PyObject>,
    ) -> anyhow::Result<Self> {
        let inner = SocketClientInner::connect_url(config, handler).await?;
        let writer_tx = inner.writer_tx.clone();
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
            controller_task,
            connection_mode,
            writer_tx,
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

    /// Sends a message of the given `data`.
    ///
    /// # Errors
    ///
    /// Returns any I/O error.
    pub async fn send_bytes(&self, data: Vec<u8>) -> Result<(), std::io::Error> {
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
                    tracing::error!(
                        "Failed to send data ({}): {e}",
                        String::from_utf8_lossy(&data)
                    );
                    return Ok(());
                }
                Err(_) => {
                    tracing::error!(
                        "Failed to send data ({}): timeout waiting to become ACTIVE",
                        String::from_utf8_lossy(&data)
                    );
                    return Ok(());
                }
            }
        }

        let msg = WriterCommand::Send(data.into());
        if let Err(e) = self.writer_tx.send(msg) {
            tracing::error!("{e}");
        }
        Ok(())
    }

    fn spawn_controller_task(
        mut inner: SocketClientInner,
        connection_mode: Arc<AtomicU8>,
        post_reconnection: Option<PyObject>,
        post_disconnection: Option<PyObject>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::task::spawn(async move {
            tracing::debug!("Started task 'controller'");

            let check_interval = Duration::from_millis(10);

            loop {
                tokio::time::sleep(check_interval).await;
                let mode = ConnectionMode::from_atomic(&connection_mode);

                if mode.is_disconnect() {
                    tracing::debug!("Disconnecting");

                    let timeout = Duration::from_secs(5);
                    if tokio::time::timeout(timeout, async {
                        // Delay awaiting graceful shutdown
                        tokio::time::sleep(Duration::from_millis(100)).await;

                        if !inner.read_task.is_finished() {
                            inner.read_task.abort();
                            tracing::debug!("Aborted task 'read'");
                        }

                        if let Some(task) = &inner.heartbeat_task {
                            if !task.is_finished() {
                                task.abort();
                                tracing::debug!("Aborted task 'heartbeat'");
                            }
                        }
                    })
                    .await
                    .is_err()
                    {
                        tracing::error!("Shutdown timed out after {}s", timeout.as_secs());
                    }

                    tracing::debug!("Closed");

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
                                    Ok(_) => {
                                        tracing::debug!("Called `post_reconnection` handler");
                                    }
                                    Err(e) => tracing::error!(
                                        "Error calling `post_reconnection` handler: {e}"
                                    ),
                                });
                            }
                        }
                        Err(e) => {
                            let duration = inner.backoff.next_duration();
                            tracing::warn!("Reconnect attempt failed: {e}");
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

            tracing::debug!("Completed task 'controller'");
        })
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
#[cfg(target_os = "linux")] // Only run network tests on Linux (CI stability)
mod tests {
    use std::ffi::CString;

    use nautilus_common::testing::wait_until_async;
    use nautilus_core::python::IntoPyObjectNautilusExt;
    use pyo3::prepare_freethreaded_python;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
        sync::Mutex,
        task,
        time::{Duration, sleep},
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

    async fn bind_test_server() -> (u16, TcpListener) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind ephemeral port");
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

        let (port, listener) = bind_test_server().await;
        let server_task = task::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            run_echo_server(socket).await;
        });

        let config = SocketConfig {
            url: format!("127.0.0.1:{port}"),
            mode: Mode::Plain,
            suffix: b"\r\n".to_vec(),
            py_handler: Some(Arc::new(create_handler())),
            heartbeat: None,
            reconnect_timeout_ms: None,
            reconnect_delay_initial_ms: None,
            reconnect_backoff_factor: None,
            reconnect_delay_max_ms: None,
            reconnect_jitter_ms: None,
            certs_dir: None,
        };

        let client = SocketClient::connect(config, None, None, None, None)
            .await
            .expect("Client connect failed unexpectedly");

        client.send_bytes(b"Hello".into()).await.unwrap();
        client.send_bytes(b"World".into()).await.unwrap();

        // Wait a bit for the server to echo them back
        sleep(Duration::from_millis(100)).await;

        client.send_bytes(b"close".into()).await.unwrap();
        server_task.await.unwrap();
        assert!(!client.is_closed());
    }

    #[tokio::test]
    async fn test_reconnect_fail_exhausted() {
        prepare_freethreaded_python();

        let (port, listener) = bind_test_server().await;
        drop(listener); // We drop it immediately -> no server is listening

        let config = SocketConfig {
            url: format!("127.0.0.1:{port}"),
            mode: Mode::Plain,
            suffix: b"\r\n".to_vec(),
            py_handler: Some(Arc::new(create_handler())),
            heartbeat: None,
            reconnect_timeout_ms: None,
            reconnect_delay_initial_ms: None,
            reconnect_backoff_factor: None,
            reconnect_delay_max_ms: None,
            reconnect_jitter_ms: None,
            certs_dir: None,
        };

        let client_res = SocketClient::connect(config, None, None, None, None).await;
        assert!(
            client_res.is_err(),
            "Should fail quickly with no server listening"
        );
    }

    #[tokio::test]
    async fn test_user_disconnect() {
        prepare_freethreaded_python();

        let (port, listener) = bind_test_server().await;
        let server_task = task::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
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
            py_handler: Some(Arc::new(create_handler())),
            heartbeat: None,
            reconnect_timeout_ms: None,
            reconnect_delay_initial_ms: None,
            reconnect_backoff_factor: None,
            reconnect_delay_max_ms: None,
            reconnect_jitter_ms: None,
            certs_dir: None,
        };

        let client = SocketClient::connect(config, None, None, None, None)
            .await
            .unwrap();

        client.close().await;
        assert!(client.is_closed());
        server_task.abort();
    }

    #[tokio::test]
    async fn test_heartbeat() {
        prepare_freethreaded_python();

        let (port, listener) = bind_test_server().await;
        let received = Arc::new(Mutex::new(Vec::new()));
        let received2 = received.clone();

        let server_task = task::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();

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
            py_handler: Some(Arc::new(create_handler())),
            heartbeat,
            reconnect_timeout_ms: None,
            reconnect_delay_initial_ms: None,
            reconnect_backoff_factor: None,
            reconnect_delay_max_ms: None,
            reconnect_jitter_ms: None,
            certs_dir: None,
        };

        let client = SocketClient::connect(config, None, None, None, None)
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

        let (port, listener) = bind_test_server().await;
        let server_task = task::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
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

        let py_handler = Some(Python::with_gil(|py| {
            let pymod = PyModule::from_code(py, &code, &filename, &module).unwrap();
            let func = pymod.getattr("handler").unwrap();
            Arc::new(func.into_py_any_unwrap(py))
        }));

        let config = SocketConfig {
            url: format!("127.0.0.1:{port}"),
            mode: Mode::Plain,
            suffix: b"\r\n".to_vec(),
            py_handler,
            heartbeat: None,
            reconnect_timeout_ms: None,
            reconnect_delay_initial_ms: None,
            reconnect_backoff_factor: None,
            reconnect_delay_max_ms: None,
            reconnect_jitter_ms: None,
            certs_dir: None,
        };

        let client = SocketClient::connect(config, None, None, None, None)
            .await
            .expect("Client connect failed unexpectedly");

        client.send_bytes(b"hello".into()).await.unwrap();
        sleep(Duration::from_millis(100)).await;

        client.send_bytes(b"ERR".into()).await.unwrap();
        sleep(Duration::from_secs(1)).await;

        assert!(client.is_active());

        client.close().await;

        assert!(client.is_closed());
        server_task.abort();
    }

    #[tokio::test]
    async fn test_reconnect_success() {
        prepare_freethreaded_python();

        let (port, listener) = bind_test_server().await;

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
            py_handler: Some(Arc::new(create_handler())),
            heartbeat: None,
            reconnect_timeout_ms: Some(5_000),
            reconnect_delay_initial_ms: Some(500),
            reconnect_delay_max_ms: Some(5_000),
            reconnect_backoff_factor: Some(2.0),
            reconnect_jitter_ms: Some(50),
            certs_dir: None,
        };

        let client = SocketClient::connect(config, None, None, None, None)
            .await
            .expect("Client connect failed unexpectedly");

        // Initially, the client should be active
        assert!(client.is_active(), "Client should start as active");

        // Wait until the client loses connection (i.e. not active),
        // then wait until it reconnects (active again).
        wait_until_async(|| async { client.is_active() }, Duration::from_secs(10)).await;

        client
            .send_bytes(b"TestReconnect".into())
            .await
            .expect("Send failed");

        client.close().await;
        server_task.abort();
    }
}
