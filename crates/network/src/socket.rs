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
//! - Connection state tracking (ACTIVE/RECONNECTING/DISCONNECTING/CLOSED).
//! - Synchronized reconnection with backoff.
//! - Split read/write architecture.
//! - Python callback integration.
//!
//! **Design**:
//! - Single reader, multiple writer model.
//! - Read half runs in dedicated task.
//! - Write half runs in dedicated task connected with channel.
//! - Controller task manages lifecycle.

use std::{
    fmt::Debug,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    time::Duration,
};

use bytes::Bytes;
use nautilus_core::CleanDrop;
use nautilus_cryptography::providers::install_cryptographic_provider;
use tokio::io::{AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio_tungstenite::{
    MaybeTlsStream,
    tungstenite::{Error, client::IntoClientRequest, stream::Mode},
};

use crate::{
    backoff::ExponentialBackoff,
    error::SendError,
    fix::process_fix_buffer,
    logging::{log_task_aborted, log_task_started, log_task_stopped},
    mode::ConnectionMode,
    net::TcpStream,
    tls::{Connector, create_tls_config_from_certs_dir, tcp_tls},
};

// Connection timing constants
const CONNECTION_STATE_CHECK_INTERVAL_MS: u64 = 10;
const GRACEFUL_SHUTDOWN_DELAY_MS: u64 = 100;
const GRACEFUL_SHUTDOWN_TIMEOUT_SECS: u64 = 5;
const SEND_OPERATION_CHECK_INTERVAL_MS: u64 = 1;

type TcpWriter = WriteHalf<MaybeTlsStream<TcpStream>>;
type TcpReader = ReadHalf<MaybeTlsStream<TcpStream>>;
pub type TcpMessageHandler = Arc<dyn Fn(&[u8]) + Send + Sync>;

/// Configuration for TCP socket connection.
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
    /// The optional function to handle incoming messages.
    pub message_handler: Option<TcpMessageHandler>,
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

impl Debug for SocketConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(SocketConfig))
            .field("url", &self.url)
            .field("mode", &self.mode)
            .field("suffix", &self.suffix)
            .field(
                "message_handler",
                &self.message_handler.as_ref().map(|_| "<function>"),
            )
            .field("heartbeat", &self.heartbeat)
            .field("reconnect_timeout_ms", &self.reconnect_timeout_ms)
            .field(
                "reconnect_delay_initial_ms",
                &self.reconnect_delay_initial_ms,
            )
            .field("reconnect_delay_max_ms", &self.reconnect_delay_max_ms)
            .field("reconnect_backoff_factor", &self.reconnect_backoff_factor)
            .field("reconnect_jitter_ms", &self.reconnect_jitter_ms)
            .field("certs_dir", &self.certs_dir)
            .finish()
    }
}

impl Clone for SocketConfig {
    fn clone(&self) -> Self {
        Self {
            url: self.url.clone(),
            mode: self.mode,
            suffix: self.suffix.clone(),
            message_handler: self.message_handler.clone(),
            heartbeat: self.heartbeat.clone(),
            reconnect_timeout_ms: self.reconnect_timeout_ms,
            reconnect_delay_initial_ms: self.reconnect_delay_initial_ms,
            reconnect_delay_max_ms: self.reconnect_delay_max_ms,
            reconnect_backoff_factor: self.reconnect_backoff_factor,
            reconnect_jitter_ms: self.reconnect_jitter_ms,
            certs_dir: self.certs_dir.clone(),
        }
    }
}

/// Represents a command for the writer task.
#[derive(Debug)]
pub enum WriterCommand {
    /// Update the writer reference with a new one after reconnection.
    Update(TcpWriter),
    /// Send data to the server.
    Send(Bytes),
}

/// Creates a `TcpStream` with the server.
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
    handler: Option<TcpMessageHandler>,
}

impl SocketClientInner {
    /// Connect to a URL with the specified configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if connection fails or configuration is invalid.
    pub async fn connect_url(config: SocketConfig) -> anyhow::Result<Self> {
        install_cryptographic_provider();

        let SocketConfig {
            url,
            mode,
            heartbeat,
            suffix,
            message_handler,
            reconnect_timeout_ms,
            reconnect_delay_initial_ms,
            reconnect_delay_max_ms,
            reconnect_backoff_factor,
            reconnect_jitter_ms,
            certs_dir,
        } = &config.clone();
        let connector = if let Some(dir) = certs_dir {
            let config = create_tls_config_from_certs_dir(Path::new(dir), false)?;
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
            message_handler.clone(),
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
        )?;

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
            handler: message_handler.clone(),
        })
    }

    /// Parse URL and extract socket address and request URL.
    ///
    /// Accepts either:
    /// - Raw socket address: "host:port" → returns ("host:port", "scheme://host:port")
    /// - Full URL: "scheme://host:port/path" → returns ("host:port", original URL)
    ///
    /// # Errors
    ///
    /// Returns an error if the URL is invalid or missing required components.
    fn parse_socket_url(url: &str, mode: Mode) -> Result<(String, String), Error> {
        if url.contains("://") {
            // URL with scheme (e.g., "wss://host:port/path")
            let parsed = url.parse::<http::Uri>().map_err(|e| {
                Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("Invalid URL: {e}"),
                ))
            })?;

            let host = parsed.host().ok_or_else(|| {
                Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "URL missing host",
                ))
            })?;

            let port = parsed
                .port_u16()
                .unwrap_or_else(|| match parsed.scheme_str() {
                    Some("wss" | "https") => 443,
                    Some("ws" | "http") => 80,
                    _ => match mode {
                        Mode::Tls => 443,
                        Mode::Plain => 80,
                    },
                });

            Ok((format!("{host}:{port}"), url.to_string()))
        } else {
            // Raw socket address (e.g., "host:port")
            // Construct a proper URL for the request based on mode
            let scheme = match mode {
                Mode::Tls => "wss",
                Mode::Plain => "ws",
            };
            Ok((url.to_string(), format!("{scheme}://{url}")))
        }
    }

    /// Establish a TLS or plain TCP connection with the server.
    ///
    /// Accepts either a raw socket address (e.g., "host:port") or a full URL with scheme
    /// (e.g., "wss://host:port"). For FIX/raw socket connections, use the host:port format.
    /// For WebSocket-style connections, include the scheme.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection cannot be established.
    pub async fn tls_connect_with_server(
        url: &str,
        mode: Mode,
        connector: Option<Connector>,
    ) -> Result<(TcpReader, TcpWriter), Error> {
        tracing::debug!("Connecting to {url}");

        let (socket_addr, request_url) = Self::parse_socket_url(url, mode)?;
        let tcp_result = TcpStream::connect(&socket_addr).await;

        match tcp_result {
            Ok(stream) => {
                tracing::debug!("TCP connection established to {socket_addr}, proceeding with TLS");
                let request = request_url.into_client_request()?;
                tcp_tls(&request, mode, stream, connector)
                    .await
                    .map(tokio::io::split)
            }
            Err(e) => {
                tracing::error!("TCP connection failed to {socket_addr}: {e:?}");
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

        if ConnectionMode::from_atomic(&self.connection_mode).is_disconnect() {
            tracing::debug!("Reconnect aborted due to disconnect state");
            return Ok(());
        }

        tokio::time::timeout(self.reconnect_timeout, async {
            let SocketConfig {
                url,
                mode,
                heartbeat: _,
                suffix,
                message_handler: _,
                reconnect_timeout_ms: _,
                reconnect_delay_initial_ms: _,
                reconnect_backoff_factor: _,
                reconnect_delay_max_ms: _,
                reconnect_jitter_ms: _,
                certs_dir: _,
            } = &self.config;
            // Create a fresh connection
            let connector = self.connector.clone();
            // Attempt to connect; abort early if a disconnect was requested
            let (reader, new_writer) = Self::tls_connect_with_server(url, *mode, connector).await?;

            if ConnectionMode::from_atomic(&self.connection_mode).is_disconnect() {
                tracing::debug!("Reconnect aborted mid-flight (after connect)");
                return Ok(());
            }
            tracing::debug!("Connected");

            if let Err(e) = self.writer_tx.send(WriterCommand::Update(new_writer)) {
                tracing::error!("{e}");
            }

            // Delay before closing connection
            tokio::time::sleep(Duration::from_millis(GRACEFUL_SHUTDOWN_DELAY_MS)).await;

            if ConnectionMode::from_atomic(&self.connection_mode).is_disconnect() {
                tracing::debug!("Reconnect aborted mid-flight (after delay)");
                return Ok(());
            }

            if !self.read_task.is_finished() {
                self.read_task.abort();
                log_task_aborted("read");
            }

            // Atomically transition from Reconnect to Active
            // This prevents race condition where disconnect could be requested between check and store
            if self
                .connection_mode
                .compare_exchange(
                    ConnectionMode::Reconnect.as_u8(),
                    ConnectionMode::Active.as_u8(),
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                )
                .is_err()
            {
                tracing::debug!("Reconnect aborted (state changed during reconnect)");
                return Ok(());
            }

            // Spawn new read task
            self.read_task = Arc::new(Self::spawn_read_task(
                self.connection_mode.clone(),
                reader,
                self.handler.clone(),
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
        handler: Option<TcpMessageHandler>,
        suffix: Vec<u8>,
    ) -> tokio::task::JoinHandle<()> {
        log_task_started("read");

        // Interval between checking the connection mode
        let check_interval = Duration::from_millis(CONNECTION_STATE_CHECK_INTERVAL_MS);

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

                        // Check if buffer contains FIX protocol messages (starts with "8=FIX")
                        let is_fix = buf.len() >= 5 && buf.starts_with(b"8=FIX");

                        if is_fix && handler.is_some() {
                            // FIX protocol processing
                            if let Some(ref handler) = handler {
                                process_fix_buffer(&mut buf, handler);
                            }
                        } else {
                            // Regular suffix-based message processing
                            while let Some((i, _)) = &buf
                                .windows(suffix.len())
                                .enumerate()
                                .find(|(_, pair)| pair.eq(&suffix))
                            {
                                let mut data: Vec<u8> = buf.drain(0..i + suffix.len()).collect();
                                data.truncate(data.len() - suffix.len());

                                if let Some(ref handler) = handler {
                                    handler(&data);
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

            log_task_stopped("read");
        })
    }

    fn spawn_write_task(
        connection_state: Arc<AtomicU8>,
        writer: TcpWriter,
        mut writer_rx: tokio::sync::mpsc::UnboundedReceiver<WriterCommand>,
        suffix: Vec<u8>,
    ) -> tokio::task::JoinHandle<()> {
        log_task_started("write");

        // Interval between checking the connection mode
        let check_interval = Duration::from_millis(CONNECTION_STATE_CHECK_INTERVAL_MS);

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
                                _ = tokio::time::timeout(
                                    Duration::from_secs(GRACEFUL_SHUTDOWN_TIMEOUT_SECS),
                                    active_writer.shutdown(),
                                )
                                .await;

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
                                    tracing::error!("Failed to send suffix: {e}");
                                    // Mode is active so trigger reconnection
                                    tracing::warn!("Writer triggering reconnect");
                                    connection_state
                                        .store(ConnectionMode::Reconnect.as_u8(), Ordering::SeqCst);
                                    continue;
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
            _ = tokio::time::timeout(
                Duration::from_secs(GRACEFUL_SHUTDOWN_TIMEOUT_SECS),
                active_writer.shutdown(),
            )
            .await;

            log_task_stopped("write");
        })
    }

    fn spawn_heartbeat_task(
        connection_state: Arc<AtomicU8>,
        heartbeat: (u64, Vec<u8>),
        writer_tx: tokio::sync::mpsc::UnboundedSender<WriterCommand>,
    ) -> tokio::task::JoinHandle<()> {
        log_task_started("heartbeat");
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

            log_task_stopped("heartbeat");
        })
    }
}

impl Drop for SocketClientInner {
    fn drop(&mut self) {
        // Delegate to explicit cleanup handler
        self.clean_drop();
    }
}

/// Cleanup on drop: aborts background tasks and clears handlers to break reference cycles.
impl CleanDrop for SocketClientInner {
    fn clean_drop(&mut self) {
        if !self.read_task.is_finished() {
            self.read_task.abort();
            log_task_aborted("read");
        }

        if !self.write_task.is_finished() {
            self.write_task.abort();
            log_task_aborted("write");
        }

        if let Some(ref handle) = self.heartbeat_task.take()
            && !handle.is_finished()
        {
            handle.abort();
            log_task_aborted("heartbeat");
        }

        #[cfg(feature = "python")]
        {
            // Remove stored handler to break ref cycle
            self.config.message_handler = None;
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
    pub(crate) reconnect_timeout: Duration,
    pub writer_tx: tokio::sync::mpsc::UnboundedSender<WriterCommand>,
}

impl Debug for SocketClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(SocketClient)).finish()
    }
}

impl SocketClient {
    /// Connect to the server.
    ///
    /// # Errors
    ///
    /// Returns any error connecting to the server.
    pub async fn connect(
        config: SocketConfig,
        post_connection: Option<Arc<dyn Fn() + Send + Sync>>,
        post_reconnection: Option<Arc<dyn Fn() + Send + Sync>>,
        post_disconnection: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> anyhow::Result<Self> {
        let inner = SocketClientInner::connect_url(config).await?;
        let writer_tx = inner.writer_tx.clone();
        let connection_mode = inner.connection_mode.clone();
        let reconnect_timeout = inner.reconnect_timeout;

        let controller_task = Self::spawn_controller_task(
            inner,
            connection_mode.clone(),
            post_reconnection,
            post_disconnection,
        );

        if let Some(handler) = post_connection {
            handler();
            tracing::debug!("Called `post_connection` handler");
        }

        Ok(Self {
            controller_task,
            connection_mode,
            reconnect_timeout,
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

        if let Ok(()) =
            tokio::time::timeout(Duration::from_secs(GRACEFUL_SHUTDOWN_TIMEOUT_SECS), async {
                while !self.is_closed() {
                    tokio::time::sleep(Duration::from_millis(CONNECTION_STATE_CHECK_INTERVAL_MS))
                        .await;
                }

                if !self.controller_task.is_finished() {
                    self.controller_task.abort();
                    log_task_aborted("controller");
                }
            })
            .await
        {
            log_task_stopped("controller");
        } else {
            tracing::error!("Timeout waiting for controller task to finish");
            if !self.controller_task.is_finished() {
                self.controller_task.abort();
                log_task_aborted("controller");
            }
        }
    }

    /// Sends a message of the given `data`.
    ///
    /// # Errors
    ///
    /// Returns an error if sending fails.
    pub async fn send_bytes(&self, data: Vec<u8>) -> Result<(), SendError> {
        if self.is_closed() {
            return Err(SendError::Closed);
        }

        let timeout = self.reconnect_timeout;
        let check_interval = Duration::from_millis(SEND_OPERATION_CHECK_INTERVAL_MS);

        if !self.is_active() {
            tracing::debug!("Waiting for client to become ACTIVE before sending...");

            let inner = tokio::time::timeout(timeout, async {
                loop {
                    if self.is_active() {
                        return Ok(());
                    }
                    if matches!(
                        self.connection_mode(),
                        ConnectionMode::Disconnect | ConnectionMode::Closed
                    ) {
                        return Err(());
                    }
                    tokio::time::sleep(check_interval).await;
                }
            })
            .await
            .map_err(|_| SendError::Timeout)?;
            inner.map_err(|()| SendError::Closed)?;
        }

        let msg = WriterCommand::Send(data.into());
        self.writer_tx
            .send(msg)
            .map_err(|e| SendError::BrokenPipe(e.to_string()))
    }

    fn spawn_controller_task(
        mut inner: SocketClientInner,
        connection_mode: Arc<AtomicU8>,
        post_reconnection: Option<Arc<dyn Fn() + Send + Sync>>,
        post_disconnection: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::task::spawn(async move {
            log_task_started("controller");

            let check_interval = Duration::from_millis(CONNECTION_STATE_CHECK_INTERVAL_MS);

            loop {
                tokio::time::sleep(check_interval).await;
                let mut mode = ConnectionMode::from_atomic(&connection_mode);

                if mode.is_disconnect() {
                    tracing::debug!("Disconnecting");

                    let timeout = Duration::from_secs(GRACEFUL_SHUTDOWN_TIMEOUT_SECS);
                    if tokio::time::timeout(timeout, async {
                        // Delay awaiting graceful shutdown
                        tokio::time::sleep(Duration::from_millis(GRACEFUL_SHUTDOWN_DELAY_MS)).await;

                        if !inner.read_task.is_finished() {
                            inner.read_task.abort();
                            log_task_aborted("read");
                        }

                        if let Some(task) = &inner.heartbeat_task
                            && !task.is_finished()
                        {
                            task.abort();
                            log_task_aborted("heartbeat");
                        }
                    })
                    .await
                    .is_err()
                    {
                        tracing::error!("Shutdown timed out after {}s", timeout.as_secs());
                    }

                    tracing::debug!("Closed");

                    if let Some(ref handler) = post_disconnection {
                        handler();
                        tracing::debug!("Called `post_disconnection` handler");
                    }
                    break; // Controller finished
                }

                if mode.is_active() && !inner.is_alive() {
                    if connection_mode
                        .compare_exchange(
                            ConnectionMode::Active.as_u8(),
                            ConnectionMode::Reconnect.as_u8(),
                            Ordering::SeqCst,
                            Ordering::SeqCst,
                        )
                        .is_ok()
                    {
                        tracing::debug!("Detected dead read task, transitioning to RECONNECT");
                    }
                    mode = ConnectionMode::from_atomic(&connection_mode);
                }

                if mode.is_reconnect() {
                    match inner.reconnect().await {
                        Ok(()) => {
                            tracing::debug!("Reconnected successfully");
                            inner.backoff.reset();
                            // Only invoke reconnect handler if still active
                            if ConnectionMode::from_atomic(&connection_mode).is_active() {
                                if let Some(ref handler) = post_reconnection {
                                    handler();
                                    tracing::debug!("Called `post_reconnection` handler");
                                }
                            } else {
                                tracing::debug!(
                                    "Skipping post_reconnection handlers due to disconnect state"
                                );
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

            log_task_stopped("controller");
        })
    }
}

// Abort controller task on drop to clean up background tasks
impl Drop for SocketClient {
    fn drop(&mut self) {
        if !self.controller_task.is_finished() {
            self.controller_task.abort();
            log_task_aborted("controller");
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
#[cfg(feature = "python")]
#[cfg(target_os = "linux")] // Only run network tests on Linux (CI stability)
mod tests {
    use nautilus_common::testing::wait_until_async;
    use pyo3::Python;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
        sync::Mutex,
        task,
        time::{Duration, sleep},
    };

    use super::*;

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
        Python::initialize();

        let (port, listener) = bind_test_server().await;
        let server_task = task::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            run_echo_server(socket).await;
        });

        let config = SocketConfig {
            url: format!("127.0.0.1:{port}"),
            mode: Mode::Plain,
            suffix: b"\r\n".to_vec(),
            message_handler: None,
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
        Python::initialize();

        let (port, listener) = bind_test_server().await;
        drop(listener); // We drop it immediately -> no server is listening

        let config = SocketConfig {
            url: format!("127.0.0.1:{port}"),
            mode: Mode::Plain,
            suffix: b"\r\n".to_vec(),
            message_handler: None,
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
        Python::initialize();

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
            message_handler: None,
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
        Python::initialize();

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
            message_handler: None,
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
    async fn test_reconnect_success() {
        Python::initialize();

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
            message_handler: None,
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
            .send_bytes(b"TestReconnect".into())
            .await
            .expect("Send failed");

        client.close().await;
        server_task.abort();
    }
}

#[cfg(test)]
#[cfg(not(feature = "turmoil"))]
mod rust_tests {
    use rstest::rstest;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        task,
        time::{Duration, sleep},
    };

    use super::*;

    #[rstest]
    #[tokio::test]
    async fn test_reconnect_then_close() {
        // Bind an ephemeral port
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        // Server task: accept one connection and then drop it
        let server = task::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                drop(sock.shutdown());
            }
            // Keep listener alive briefly to avoid premature exit
            sleep(Duration::from_secs(1)).await;
        });

        // Configure client with a short reconnect backoff
        let config = SocketConfig {
            url: format!("127.0.0.1:{port}"),
            mode: Mode::Plain,
            suffix: b"\r\n".to_vec(),
            message_handler: None,
            heartbeat: None,
            reconnect_timeout_ms: Some(1_000),
            reconnect_delay_initial_ms: Some(50),
            reconnect_delay_max_ms: Some(100),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            certs_dir: None,
        };

        // Connect client (handler=None)
        let client = SocketClient::connect(config.clone(), None, None, None)
            .await
            .unwrap();

        // Allow server to drop connection and client to notice
        sleep(Duration::from_millis(100)).await;

        // Now close the client
        client.close().await;
        assert!(client.is_closed());
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_reconnect_state_flips_when_reader_stops() {
        // Bind an ephemeral port and accept a single connection which we immediately close.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = task::spawn(async move {
            if let Ok((sock, _)) = listener.accept().await {
                drop(sock);
            }
            // Give the client a moment to observe the closed connection.
            sleep(Duration::from_millis(50)).await;
        });

        let config = SocketConfig {
            url: format!("127.0.0.1:{port}"),
            mode: Mode::Plain,
            suffix: b"\r\n".to_vec(),
            message_handler: None,
            heartbeat: None,
            reconnect_timeout_ms: Some(1_000),
            reconnect_delay_initial_ms: Some(50),
            reconnect_delay_max_ms: Some(100),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            certs_dir: None,
        };

        let client = SocketClient::connect(config, None, None, None)
            .await
            .unwrap();

        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if client.is_reconnecting() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("client did not enter RECONNECT state");

        client.close().await;
        server.abort();
    }

    #[rstest]
    fn test_parse_socket_url_raw_address() {
        // Raw socket address with TLS mode
        let (socket_addr, request_url) =
            SocketClientInner::parse_socket_url("example.com:6130", Mode::Tls).unwrap();
        assert_eq!(socket_addr, "example.com:6130");
        assert_eq!(request_url, "wss://example.com:6130");

        // Raw socket address with Plain mode
        let (socket_addr, request_url) =
            SocketClientInner::parse_socket_url("localhost:8080", Mode::Plain).unwrap();
        assert_eq!(socket_addr, "localhost:8080");
        assert_eq!(request_url, "ws://localhost:8080");
    }

    #[rstest]
    fn test_parse_socket_url_with_scheme() {
        // Full URL with wss scheme
        let (socket_addr, request_url) =
            SocketClientInner::parse_socket_url("wss://example.com:443/path", Mode::Tls).unwrap();
        assert_eq!(socket_addr, "example.com:443");
        assert_eq!(request_url, "wss://example.com:443/path");

        // Full URL with ws scheme
        let (socket_addr, request_url) =
            SocketClientInner::parse_socket_url("ws://localhost:8080", Mode::Plain).unwrap();
        assert_eq!(socket_addr, "localhost:8080");
        assert_eq!(request_url, "ws://localhost:8080");
    }

    #[rstest]
    fn test_parse_socket_url_default_ports() {
        // wss without explicit port defaults to 443
        let (socket_addr, _) =
            SocketClientInner::parse_socket_url("wss://example.com", Mode::Tls).unwrap();
        assert_eq!(socket_addr, "example.com:443");

        // ws without explicit port defaults to 80
        let (socket_addr, _) =
            SocketClientInner::parse_socket_url("ws://example.com", Mode::Plain).unwrap();
        assert_eq!(socket_addr, "example.com:80");

        // https defaults to 443
        let (socket_addr, _) =
            SocketClientInner::parse_socket_url("https://example.com", Mode::Tls).unwrap();
        assert_eq!(socket_addr, "example.com:443");

        // http defaults to 80
        let (socket_addr, _) =
            SocketClientInner::parse_socket_url("http://example.com", Mode::Plain).unwrap();
        assert_eq!(socket_addr, "example.com:80");
    }

    #[rstest]
    fn test_parse_socket_url_unknown_scheme_uses_mode() {
        // Unknown scheme defaults to mode-based port
        let (socket_addr, _) =
            SocketClientInner::parse_socket_url("custom://example.com", Mode::Tls).unwrap();
        assert_eq!(socket_addr, "example.com:443");

        let (socket_addr, _) =
            SocketClientInner::parse_socket_url("custom://example.com", Mode::Plain).unwrap();
        assert_eq!(socket_addr, "example.com:80");
    }

    #[rstest]
    fn test_parse_socket_url_ipv6() {
        // IPv6 address with port
        let (socket_addr, request_url) =
            SocketClientInner::parse_socket_url("[::1]:8080", Mode::Plain).unwrap();
        assert_eq!(socket_addr, "[::1]:8080");
        assert_eq!(request_url, "ws://[::1]:8080");

        // IPv6 in URL
        let (socket_addr, _) =
            SocketClientInner::parse_socket_url("ws://[::1]:8080", Mode::Plain).unwrap();
        assert_eq!(socket_addr, "[::1]:8080");
    }

    #[rstest]
    #[tokio::test]
    async fn test_url_parsing_raw_socket_address() {
        // Test that raw socket addresses (host:port) work correctly
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = task::spawn(async move {
            if let Ok((sock, _)) = listener.accept().await {
                drop(sock);
            }
            sleep(Duration::from_millis(50)).await;
        });

        let config = SocketConfig {
            url: format!("127.0.0.1:{port}"), // Raw socket address format
            mode: Mode::Plain,
            suffix: b"\r\n".to_vec(),
            message_handler: None,
            heartbeat: None,
            reconnect_timeout_ms: Some(1_000),
            reconnect_delay_initial_ms: Some(50),
            reconnect_delay_max_ms: Some(100),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            certs_dir: None,
        };

        // Should successfully connect with raw socket address
        let client = SocketClient::connect(config, None, None, None).await;
        assert!(
            client.is_ok(),
            "Client should connect with raw socket address format"
        );

        if let Ok(client) = client {
            client.close().await;
        }
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_url_parsing_with_scheme() {
        // Test that URLs with schemes also work
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = task::spawn(async move {
            if let Ok((sock, _)) = listener.accept().await {
                drop(sock);
            }
            sleep(Duration::from_millis(50)).await;
        });

        let config = SocketConfig {
            url: format!("ws://127.0.0.1:{port}"), // URL with scheme
            mode: Mode::Plain,
            suffix: b"\r\n".to_vec(),
            message_handler: None,
            heartbeat: None,
            reconnect_timeout_ms: Some(1_000),
            reconnect_delay_initial_ms: Some(50),
            reconnect_delay_max_ms: Some(100),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            certs_dir: None,
        };

        // Should successfully connect with URL format
        let client = SocketClient::connect(config, None, None, None).await;
        assert!(
            client.is_ok(),
            "Client should connect with URL scheme format"
        );

        if let Ok(client) = client {
            client.close().await;
        }
        server.abort();
    }

    #[rstest]
    fn test_parse_socket_url_ipv6_with_zone() {
        // IPv6 with zone ID (link-local address)
        let (socket_addr, request_url) =
            SocketClientInner::parse_socket_url("[fe80::1%eth0]:8080", Mode::Plain).unwrap();
        assert_eq!(socket_addr, "[fe80::1%eth0]:8080");
        assert_eq!(request_url, "ws://[fe80::1%eth0]:8080");

        // Verify zone is preserved in URL format too
        let (socket_addr, request_url) =
            SocketClientInner::parse_socket_url("ws://[fe80::1%lo]:9090", Mode::Plain).unwrap();
        assert_eq!(socket_addr, "[fe80::1%lo]:9090");
        assert_eq!(request_url, "ws://[fe80::1%lo]:9090");
    }

    #[rstest]
    #[tokio::test]
    async fn test_ipv6_loopback_connection() {
        // Test IPv6 loopback address connection
        // Skip if IPv6 is not available on the system
        if TcpListener::bind("[::1]:0").await.is_err() {
            eprintln!("IPv6 not available, skipping test");
            return;
        }

        let listener = TcpListener::bind("[::1]:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = task::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                let mut buf = vec![0u8; 1024];
                if let Ok(n) = sock.read(&mut buf).await {
                    // Echo back
                    let _ = sock.write_all(&buf[..n]).await;
                }
            }
            sleep(Duration::from_millis(50)).await;
        });

        let config = SocketConfig {
            url: format!("[::1]:{port}"), // IPv6 loopback
            mode: Mode::Plain,
            suffix: b"\r\n".to_vec(),
            message_handler: None,
            heartbeat: None,
            reconnect_timeout_ms: Some(1_000),
            reconnect_delay_initial_ms: Some(50),
            reconnect_delay_max_ms: Some(100),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            certs_dir: None,
        };

        let client = SocketClient::connect(config, None, None, None).await;
        assert!(
            client.is_ok(),
            "Client should connect to IPv6 loopback address"
        );

        if let Ok(client) = client {
            client.close().await;
        }
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_send_waits_during_reconnection() {
        // Test that send operations wait for reconnection to complete (up to configured timeout)
        use nautilus_common::testing::wait_until_async;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = task::spawn(async move {
            // First connection - accept and immediately close
            if let Ok((sock, _)) = listener.accept().await {
                drop(sock);
            }

            // Wait before accepting second connection
            sleep(Duration::from_millis(500)).await;

            // Second connection - accept and keep alive
            if let Ok((mut sock, _)) = listener.accept().await {
                // Echo messages
                let mut buf = vec![0u8; 1024];
                while let Ok(n) = sock.read(&mut buf).await {
                    if n == 0 {
                        break;
                    }
                    if sock.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
            }
        });

        let config = SocketConfig {
            url: format!("127.0.0.1:{port}"),
            mode: Mode::Plain,
            suffix: b"\r\n".to_vec(),
            message_handler: None,
            heartbeat: None,
            reconnect_timeout_ms: Some(5_000), // 5s timeout - enough for reconnect
            reconnect_delay_initial_ms: Some(100),
            reconnect_delay_max_ms: Some(200),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            certs_dir: None,
        };

        let client = SocketClient::connect(config, None, None, None)
            .await
            .unwrap();

        // Wait for reconnection to trigger
        wait_until_async(
            || async { client.is_reconnecting() },
            Duration::from_secs(2),
        )
        .await;

        // Try to send while reconnecting - should wait and succeed after reconnect
        let send_result = tokio::time::timeout(
            Duration::from_secs(3),
            client.send_bytes(b"test_message".to_vec()),
        )
        .await;

        assert!(
            send_result.is_ok() && send_result.unwrap().is_ok(),
            "Send should succeed after waiting for reconnection"
        );

        client.close().await;
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_send_bytes_timeout_uses_configured_reconnect_timeout() {
        // Test that send_bytes operations respect the configured reconnect_timeout.
        // When a client is stuck in RECONNECT longer than the timeout, sends should fail with Timeout.
        use nautilus_common::testing::wait_until_async;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = task::spawn(async move {
            // Accept first connection and immediately close it
            if let Ok((sock, _)) = listener.accept().await {
                drop(sock);
            }
            // Drop listener entirely so reconnection fails completely
            drop(listener);
            sleep(Duration::from_secs(60)).await;
        });

        let config = SocketConfig {
            url: format!("127.0.0.1:{port}"),
            mode: Mode::Plain,
            suffix: b"\r\n".to_vec(),
            message_handler: None,
            heartbeat: None,
            reconnect_timeout_ms: Some(1_000), // 1s timeout for faster test
            reconnect_delay_initial_ms: Some(5_000), // Long backoff to keep client in RECONNECT
            reconnect_delay_max_ms: Some(5_000),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            certs_dir: None,
        };

        let client = SocketClient::connect(config, None, None, None)
            .await
            .unwrap();

        // Wait for client to enter RECONNECT state
        wait_until_async(
            || async { client.is_reconnecting() },
            Duration::from_secs(3),
        )
        .await;

        // Attempt send while stuck in RECONNECT - should timeout after 1s (configured timeout)
        // The client will try to reconnect for 1s, fail, then wait 5s backoff before next attempt
        let start = std::time::Instant::now();
        let send_result = client.send_bytes(b"test".to_vec()).await;
        let elapsed = start.elapsed();

        assert!(
            send_result.is_err(),
            "Send should fail when client stuck in RECONNECT, got: {:?}",
            send_result
        );
        assert!(
            matches!(send_result, Err(crate::error::SendError::Timeout)),
            "Send should return Timeout error, got: {:?}",
            send_result
        );
        // Verify timeout respects configured value (1s), but don't check upper bound
        // as CI scheduler jitter can cause legitimate delays beyond the timeout
        assert!(
            elapsed >= Duration::from_millis(900),
            "Send should timeout after at least 1s (configured timeout), took {:?}",
            elapsed
        );

        client.close().await;
        server.abort();
    }
}
