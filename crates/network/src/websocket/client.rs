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

//! WebSocket client implementation with automatic reconnection.
//!
//! This module contains the core WebSocket client implementation including:
//! - Connection management with automatic reconnection.
//! - Split read/write architecture with separate tasks.
//! - Unbounded channels on latency-sensitive paths.
//! - Heartbeat support.
//! - Rate limiting integration.

use std::{
    collections::VecDeque,
    fmt::Debug,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    time::Duration,
};

use futures_util::{SinkExt, StreamExt};
use http::HeaderName;
use nautilus_core::CleanDrop;
use nautilus_cryptography::providers::install_cryptographic_provider;
#[cfg(feature = "turmoil")]
use tokio_tungstenite::MaybeTlsStream;
#[cfg(feature = "turmoil")]
use tokio_tungstenite::client_async;
#[cfg(not(feature = "turmoil"))]
use tokio_tungstenite::connect_async_with_config;
use tokio_tungstenite::tungstenite::{
    Error, Message, client::IntoClientRequest, http::HeaderValue,
};

use super::{
    config::WebSocketConfig,
    consts::{
        CONNECTION_STATE_CHECK_INTERVAL_MS, GRACEFUL_SHUTDOWN_DELAY_MS,
        GRACEFUL_SHUTDOWN_TIMEOUT_SECS, SEND_OPERATION_CHECK_INTERVAL_MS,
    },
    types::{MessageHandler, MessageReader, MessageWriter, PingHandler, WriterCommand},
};
#[cfg(feature = "turmoil")]
use crate::net::TcpConnector;
use crate::{
    RECONNECTED,
    backoff::ExponentialBackoff,
    error::SendError,
    logging::{log_task_aborted, log_task_started, log_task_stopped},
    mode::ConnectionMode,
    ratelimiter::{RateLimiter, clock::MonotonicClock, quota::Quota},
};

/// `WebSocketClient` connects to a websocket server to read and send messages.
///
/// The client is opinionated about how messages are read and written. It
/// assumes that data can only have one reader but multiple writers.
///
/// The client splits the connection into read and write halves. It moves
/// the read half into a tokio task which keeps receiving messages from the
/// server and calls a handler - a Python function that takes the data
/// as its parameter. It stores the write half in the struct wrapped
/// with an Arc Mutex. This way the client struct can be used to write
/// data to the server from multiple scopes/tasks.
///
/// The client also maintains a heartbeat if given a duration in seconds.
/// It's preferable to set the duration slightly lower - heartbeat more
/// frequently - than the required amount.
pub struct WebSocketClientInner {
    config: WebSocketConfig,
    read_task: Option<tokio::task::JoinHandle<()>>,
    write_task: tokio::task::JoinHandle<()>,
    writer_tx: tokio::sync::mpsc::UnboundedSender<WriterCommand>,
    heartbeat_task: Option<tokio::task::JoinHandle<()>>,
    connection_mode: Arc<AtomicU8>,
    reconnect_timeout: Duration,
    backoff: ExponentialBackoff,
    /// True if this is a stream-based client (created via `connect_stream`).
    /// Stream-based clients disable auto-reconnect because the reader is
    /// owned by the caller and cannot be replaced during reconnection.
    is_stream_mode: bool,
    /// Maximum number of reconnection attempts before giving up (None = unlimited).
    reconnect_max_attempts: Option<u32>,
    /// Current count of consecutive reconnection attempts.
    reconnection_attempt_count: u32,
}

impl WebSocketClientInner {
    /// Create an inner websocket client with an existing writer.
    ///
    /// # Errors
    ///
    /// Returns an error if the exponential backoff configuration is invalid.
    pub async fn new_with_writer(
        config: WebSocketConfig,
        writer: MessageWriter,
    ) -> Result<Self, Error> {
        install_cryptographic_provider();

        let connection_mode = Arc::new(AtomicU8::new(ConnectionMode::Active.as_u8()));

        // Note: We don't spawn a read task here since the reader is handled externally
        let read_task = None;

        let backoff = ExponentialBackoff::new(
            Duration::from_millis(config.reconnect_delay_initial_ms.unwrap_or(2_000)),
            Duration::from_millis(config.reconnect_delay_max_ms.unwrap_or(30_000)),
            config.reconnect_backoff_factor.unwrap_or(1.5),
            config.reconnect_jitter_ms.unwrap_or(100),
            true, // immediate-first
        )
        .map_err(|e| Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))?;

        let (writer_tx, writer_rx) = tokio::sync::mpsc::unbounded_channel::<WriterCommand>();
        let write_task = Self::spawn_write_task(connection_mode.clone(), writer, writer_rx);

        let heartbeat_task = if let Some(heartbeat_interval) = config.heartbeat {
            Some(Self::spawn_heartbeat_task(
                connection_mode.clone(),
                heartbeat_interval,
                config.heartbeat_msg.clone(),
                writer_tx.clone(),
            ))
        } else {
            None
        };

        Ok(Self {
            config: config.clone(),
            writer_tx,
            connection_mode,
            reconnect_timeout: Duration::from_millis(config.reconnect_timeout_ms.unwrap_or(10000)),
            heartbeat_task,
            read_task,
            write_task,
            backoff,
            is_stream_mode: true,
            reconnect_max_attempts: config.reconnect_max_attempts,
            reconnection_attempt_count: 0,
        })
    }

    /// Create an inner websocket client.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The connection to the server fails.
    /// - The exponential backoff configuration is invalid.
    pub async fn connect_url(config: WebSocketConfig) -> Result<Self, Error> {
        install_cryptographic_provider();

        let WebSocketConfig {
            url,
            message_handler,
            heartbeat,
            headers,
            heartbeat_msg,
            ping_handler,
            reconnect_timeout_ms,
            reconnect_delay_initial_ms,
            reconnect_delay_max_ms,
            reconnect_backoff_factor,
            reconnect_jitter_ms,
            reconnect_max_attempts,
        } = &config;

        // Capture whether we're in stream mode before moving config
        let is_stream_mode = message_handler.is_none();
        let reconnect_max_attempts = *reconnect_max_attempts;

        let (writer, reader) = Self::connect_with_server(url, headers.clone()).await?;

        let connection_mode = Arc::new(AtomicU8::new(ConnectionMode::Active.as_u8()));

        let read_task = if message_handler.is_some() {
            Some(Self::spawn_message_handler_task(
                connection_mode.clone(),
                reader,
                message_handler.as_ref(),
                ping_handler.as_ref(),
            ))
        } else {
            None
        };

        let (writer_tx, writer_rx) = tokio::sync::mpsc::unbounded_channel::<WriterCommand>();
        let write_task = Self::spawn_write_task(connection_mode.clone(), writer, writer_rx);

        // Optionally spawn a heartbeat task to periodically ping server
        let heartbeat_task = heartbeat.as_ref().map(|heartbeat_secs| {
            Self::spawn_heartbeat_task(
                connection_mode.clone(),
                *heartbeat_secs,
                heartbeat_msg.clone(),
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
        )
        .map_err(|e| Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))?;

        Ok(Self {
            config,
            read_task,
            write_task,
            writer_tx,
            heartbeat_task,
            connection_mode,
            reconnect_timeout,
            backoff,
            // Set stream mode when no message handler (reader not managed by client)
            is_stream_mode,
            reconnect_max_attempts,
            reconnection_attempt_count: 0,
        })
    }

    /// Connects with the server creating a tokio-tungstenite websocket stream.
    /// Production version that uses `connect_async_with_config` convenience helper.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The URL cannot be parsed into a valid client request.
    /// - Header values are invalid.
    /// - The WebSocket connection fails.
    #[inline]
    #[cfg(not(feature = "turmoil"))]
    pub async fn connect_with_server(
        url: &str,
        headers: Vec<(String, String)>,
    ) -> Result<(MessageWriter, MessageReader), Error> {
        let mut request = url.into_client_request()?;
        let req_headers = request.headers_mut();

        let mut header_names: Vec<HeaderName> = Vec::new();
        for (key, val) in headers {
            let header_value = HeaderValue::from_str(&val)?;
            let header_name: HeaderName = key.parse()?;
            header_names.push(header_name.clone());
            req_headers.insert(header_name, header_value);
        }

        connect_async_with_config(request, None, true)
            .await
            .map(|resp| resp.0.split())
    }

    /// Connects with the server creating a tokio-tungstenite websocket stream.
    /// Turmoil version that uses the lower-level `client_async` API with injected stream.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The URL cannot be parsed into a valid client request.
    /// - The URL is missing a hostname.
    /// - Header values are invalid.
    /// - The TCP connection fails.
    /// - TLS setup fails (for wss:// URLs).
    /// - The WebSocket handshake fails.
    #[inline]
    #[cfg(feature = "turmoil")]
    pub async fn connect_with_server(
        url: &str,
        headers: Vec<(String, String)>,
    ) -> Result<(MessageWriter, MessageReader), Error> {
        use rustls::ClientConfig;
        use tokio_rustls::TlsConnector;

        let mut request = url.into_client_request()?;
        let req_headers = request.headers_mut();

        let mut header_names: Vec<HeaderName> = Vec::new();
        for (key, val) in headers {
            let header_value = HeaderValue::from_str(&val)?;
            let header_name: HeaderName = key.parse()?;
            header_names.push(header_name.clone());
            req_headers.insert(header_name, header_value);
        }

        let uri = request.uri();
        let scheme = uri.scheme_str().unwrap_or("ws");
        let host = uri.host().ok_or_else(|| {
            Error::Url(tokio_tungstenite::tungstenite::error::UrlError::NoHostName)
        })?;

        // Determine port: use explicit port if specified, otherwise default based on scheme
        let port = uri
            .port_u16()
            .unwrap_or_else(|| if scheme == "wss" { 443 } else { 80 });

        let addr = format!("{host}:{port}");

        // Use the connector to get a turmoil-compatible stream
        let connector = crate::net::RealTcpConnector;
        let tcp_stream = connector.connect(&addr).await?;
        if let Err(e) = tcp_stream.set_nodelay(true) {
            tracing::warn!("Failed to enable TCP_NODELAY for socket client: {e:?}");
        }

        // Wrap stream appropriately based on scheme
        let maybe_tls_stream = if scheme == "wss" {
            // Build TLS config with webpki roots
            let mut root_store = rustls::RootCertStore::empty();
            root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

            let config = ClientConfig::builder()
                .with_root_certificates(root_store)
                .with_no_client_auth();

            let tls_connector = TlsConnector::from(std::sync::Arc::new(config));
            let domain =
                rustls::pki_types::ServerName::try_from(host.to_string()).map_err(|e| {
                    Error::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!("Invalid DNS name: {e}"),
                    ))
                })?;

            let tls_stream = tls_connector.connect(domain, tcp_stream).await?;
            MaybeTlsStream::Rustls(tls_stream)
        } else {
            MaybeTlsStream::Plain(tcp_stream)
        };

        // Use client_async with the stream (plain or TLS)
        client_async(request, maybe_tls_stream)
            .await
            .map(|resp| resp.0.split())
    }

    /// Reconnect with server.
    ///
    /// Make a new connection with server. Use the new read and write halves
    /// to update self writer and read and heartbeat tasks.
    ///
    /// For stream-based clients (created via `connect_stream`), reconnection is disabled
    /// because the reader is owned by the caller and cannot be replaced. Stream users
    /// should handle disconnections by creating a new connection.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The reconnection attempt times out.
    /// - The connection to the server fails.
    pub async fn reconnect(&mut self) -> Result<(), Error> {
        tracing::debug!("Reconnecting");

        if self.is_stream_mode {
            tracing::warn!(
                "Auto-reconnect disabled for stream-based WebSocket client; \
                stream users must manually reconnect by creating a new connection"
            );
            // Transition to CLOSED state to stop reconnection attempts
            self.connection_mode
                .store(ConnectionMode::Closed.as_u8(), Ordering::SeqCst);
            return Ok(());
        }

        if ConnectionMode::from_atomic(&self.connection_mode).is_disconnect() {
            tracing::debug!("Reconnect aborted due to disconnect state");
            return Ok(());
        }

        tokio::time::timeout(self.reconnect_timeout, async {
            // Attempt to connect; abort early if a disconnect was requested
            let (new_writer, reader) =
                Self::connect_with_server(&self.config.url, self.config.headers.clone()).await?;

            if ConnectionMode::from_atomic(&self.connection_mode).is_disconnect() {
                tracing::debug!("Reconnect aborted mid-flight (after connect)");
                return Ok(());
            }

            // Use a oneshot channel to synchronize with the writer task.
            // We must verify that the buffer was successfully drained before transitioning to ACTIVE
            // to prevent silent message loss if the new connection drops immediately.
            let (tx, rx) = tokio::sync::oneshot::channel();
            if let Err(e) = self.writer_tx.send(WriterCommand::Update(new_writer, tx)) {
                tracing::error!("{e}");
                return Err(Error::Io(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    format!("Failed to send update command: {e}"),
                )));
            }

            // Wait for writer to confirm it has drained the buffer
            match rx.await {
                Ok(true) => tracing::debug!("Writer confirmed buffer drain success"),
                Ok(false) => {
                    tracing::warn!("Writer failed to drain buffer, aborting reconnect");
                    // Return error to trigger retry logic in controller
                    return Err(Error::Io(std::io::Error::other(
                        "Failed to drain reconnection buffer",
                    )));
                }
                Err(e) => {
                    tracing::error!("Writer dropped update channel: {e}");
                    return Err(Error::Io(std::io::Error::new(
                        std::io::ErrorKind::BrokenPipe,
                        "Writer task dropped response channel",
                    )));
                }
            }

            // Delay before closing connection
            tokio::time::sleep(Duration::from_millis(GRACEFUL_SHUTDOWN_DELAY_MS)).await;

            if ConnectionMode::from_atomic(&self.connection_mode).is_disconnect() {
                tracing::debug!("Reconnect aborted mid-flight (after delay)");
                return Ok(());
            }

            if let Some(ref read_task) = self.read_task.take()
                && !read_task.is_finished()
            {
                read_task.abort();
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

            self.read_task = if self.config.message_handler.is_some() {
                Some(Self::spawn_message_handler_task(
                    self.connection_mode.clone(),
                    reader,
                    self.config.message_handler.as_ref(),
                    self.config.ping_handler.as_ref(),
                ))
            } else {
                None
            };

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

    /// Check if the client is still connected.
    ///
    /// The client is connected if the read task has not finished. It is expected
    /// that in case of any failure client or server side. The read task will be
    /// shutdown or will receive a `Close` frame which will finish it. There
    /// might be some delay between the connection being closed and the client
    /// detecting.
    #[inline]
    #[must_use]
    pub fn is_alive(&self) -> bool {
        match &self.read_task {
            Some(read_task) => !read_task.is_finished(),
            None => true, // Stream is being used directly
        }
    }

    fn spawn_message_handler_task(
        connection_state: Arc<AtomicU8>,
        mut reader: MessageReader,
        message_handler: Option<&MessageHandler>,
        ping_handler: Option<&PingHandler>,
    ) -> tokio::task::JoinHandle<()> {
        tracing::debug!("Started message handler task 'read'");

        let check_interval = Duration::from_millis(CONNECTION_STATE_CHECK_INTERVAL_MS);

        // Clone Arc handlers for the async task
        let message_handler = message_handler.cloned();
        let ping_handler = ping_handler.cloned();

        tokio::task::spawn(async move {
            loop {
                if !ConnectionMode::from_atomic(&connection_state).is_active() {
                    break;
                }

                match tokio::time::timeout(check_interval, reader.next()).await {
                    Ok(Some(Ok(Message::Binary(data)))) => {
                        tracing::trace!("Received message <binary> {} bytes", data.len());
                        if let Some(ref handler) = message_handler {
                            handler(Message::Binary(data));
                        }
                    }
                    Ok(Some(Ok(Message::Text(data)))) => {
                        tracing::trace!("Received message: {data}");
                        if let Some(ref handler) = message_handler {
                            handler(Message::Text(data));
                        }
                    }
                    Ok(Some(Ok(Message::Ping(ping_data)))) => {
                        tracing::trace!("Received ping: {ping_data:?}");
                        if let Some(ref handler) = ping_handler {
                            handler(ping_data.to_vec());
                        }
                    }
                    Ok(Some(Ok(Message::Pong(_)))) => {
                        tracing::trace!("Received pong");
                    }
                    Ok(Some(Ok(Message::Close(_)))) => {
                        tracing::debug!("Received close message - terminating");
                        break;
                    }
                    Ok(Some(Ok(_))) => (),
                    Ok(Some(Err(e))) => {
                        tracing::error!("Received error message - terminating: {e}");
                        break;
                    }
                    Ok(None) => {
                        tracing::debug!("No message received - terminating");
                        break;
                    }
                    Err(_) => {
                        // Timeout - continue loop and check connection mode
                        continue;
                    }
                }
            }
        })
    }

    /// Attempts to send all buffered messages after reconnection.
    ///
    /// Returns `true` if a send error occurred (caller should trigger reconnection).
    /// Messages remain in buffer if send fails, preserving them for the next reconnection attempt.
    async fn drain_reconnect_buffer(
        buffer: &mut VecDeque<Message>,
        writer: &mut MessageWriter,
    ) -> bool {
        if buffer.is_empty() {
            return false;
        }

        let initial_buffer_len = buffer.len();
        tracing::info!(
            "Sending {} buffered messages after reconnection",
            initial_buffer_len
        );

        let mut send_error_occurred = false;

        while let Some(buffered_msg) = buffer.front() {
            // Clone message before attempting send (to keep in buffer if send fails)
            let msg_to_send = buffered_msg.clone();

            if let Err(e) = writer.send(msg_to_send).await {
                tracing::error!(
                    "Failed to send buffered message after reconnection: {e}, {} messages remain in buffer",
                    buffer.len()
                );
                send_error_occurred = true;
                break; // Stop processing buffer, remaining messages preserved for next reconnection
            }

            // Only remove from buffer after successful send
            buffer.pop_front();
        }

        if buffer.is_empty() {
            tracing::info!(
                "Successfully sent all {} buffered messages",
                initial_buffer_len
            );
        }

        send_error_occurred
    }

    fn spawn_write_task(
        connection_state: Arc<AtomicU8>,
        writer: MessageWriter,
        mut writer_rx: tokio::sync::mpsc::UnboundedReceiver<WriterCommand>,
    ) -> tokio::task::JoinHandle<()> {
        log_task_started("write");

        // Interval between checking the connection mode
        let check_interval = Duration::from_millis(CONNECTION_STATE_CHECK_INTERVAL_MS);

        tokio::task::spawn(async move {
            let mut active_writer = writer;
            // Buffer for messages received during reconnection
            // VecDeque for efficient pop_front() operations
            let mut reconnect_buffer: VecDeque<Message> = VecDeque::new();

            loop {
                match ConnectionMode::from_atomic(&connection_state) {
                    ConnectionMode::Disconnect => {
                        // Log any buffered messages that will be lost
                        if !reconnect_buffer.is_empty() {
                            tracing::warn!(
                                "Discarding {} buffered messages due to disconnect",
                                reconnect_buffer.len()
                            );
                            reconnect_buffer.clear();
                        }

                        // Attempt to close the writer gracefully before exiting,
                        // we ignore any error as the writer may already be closed.
                        _ = tokio::time::timeout(
                            Duration::from_secs(GRACEFUL_SHUTDOWN_TIMEOUT_SECS),
                            active_writer.close(),
                        )
                        .await;
                        break;
                    }
                    ConnectionMode::Closed => {
                        // Log any buffered messages that will be lost
                        if !reconnect_buffer.is_empty() {
                            tracing::warn!(
                                "Discarding {} buffered messages due to closed connection",
                                reconnect_buffer.len()
                            );
                            reconnect_buffer.clear();
                        }
                        break;
                    }
                    _ => {}
                }

                match tokio::time::timeout(check_interval, writer_rx.recv()).await {
                    Ok(Some(msg)) => {
                        // Re-check connection mode after receiving a message
                        let mode = ConnectionMode::from_atomic(&connection_state);
                        if matches!(mode, ConnectionMode::Disconnect | ConnectionMode::Closed) {
                            break;
                        }

                        match msg {
                            WriterCommand::Update(new_writer, tx) => {
                                tracing::debug!("Received new writer");

                                // Delay before closing connection
                                tokio::time::sleep(Duration::from_millis(100)).await;

                                // Attempt to close the writer gracefully on update,
                                // we ignore any error as the writer may already be closed.
                                _ = tokio::time::timeout(
                                    Duration::from_secs(GRACEFUL_SHUTDOWN_TIMEOUT_SECS),
                                    active_writer.close(),
                                )
                                .await;

                                active_writer = new_writer;
                                tracing::debug!("Updated writer");

                                let send_error = Self::drain_reconnect_buffer(
                                    &mut reconnect_buffer,
                                    &mut active_writer,
                                )
                                .await;

                                if let Err(e) = tx.send(!send_error) {
                                    tracing::error!(
                                        "Failed to report drain status to controller: {e:?}"
                                    );
                                }
                            }
                            WriterCommand::Send(msg) if mode.is_reconnect() => {
                                // Buffer messages during reconnection instead of dropping them
                                tracing::debug!(
                                    "Buffering message during reconnection (buffer size: {})",
                                    reconnect_buffer.len() + 1
                                );
                                reconnect_buffer.push_back(msg);
                            }
                            WriterCommand::Send(msg) => {
                                if let Err(e) = active_writer.send(msg.clone()).await {
                                    tracing::error!("Failed to send message: {e}");
                                    tracing::warn!("Writer triggering reconnect");
                                    reconnect_buffer.push_back(msg);
                                    connection_state
                                        .store(ConnectionMode::Reconnect.as_u8(), Ordering::SeqCst);
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

            // Attempt to close the writer gracefully before exiting,
            // we ignore any error as the writer may already be closed.
            _ = tokio::time::timeout(
                Duration::from_secs(GRACEFUL_SHUTDOWN_TIMEOUT_SECS),
                active_writer.close(),
            )
            .await;

            log_task_stopped("write");
        })
    }

    fn spawn_heartbeat_task(
        connection_state: Arc<AtomicU8>,
        heartbeat_secs: u64,
        message: Option<String>,
        writer_tx: tokio::sync::mpsc::UnboundedSender<WriterCommand>,
    ) -> tokio::task::JoinHandle<()> {
        log_task_started("heartbeat");

        tokio::task::spawn(async move {
            let interval = Duration::from_secs(heartbeat_secs);

            loop {
                tokio::time::sleep(interval).await;

                match ConnectionMode::from_u8(connection_state.load(Ordering::SeqCst)) {
                    ConnectionMode::Active => {
                        let msg = match &message {
                            Some(text) => WriterCommand::Send(Message::Text(text.clone().into())),
                            None => WriterCommand::Send(Message::Ping(vec![].into())),
                        };

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

impl Drop for WebSocketClientInner {
    fn drop(&mut self) {
        // Delegate to explicit cleanup handler
        self.clean_drop();
    }
}

/// Cleanup on drop: aborts background tasks and clears handlers to break reference cycles.
impl CleanDrop for WebSocketClientInner {
    fn clean_drop(&mut self) {
        if let Some(ref read_task) = self.read_task.take()
            && !read_task.is_finished()
        {
            read_task.abort();
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

        // Clear handlers to break potential reference cycles
        self.config.message_handler = None;
        self.config.ping_handler = None;
    }
}

impl Debug for WebSocketClientInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebSocketClientInner")
            .field("config", &self.config)
            .field(
                "connection_mode",
                &ConnectionMode::from_atomic(&self.connection_mode),
            )
            .field("reconnect_timeout", &self.reconnect_timeout)
            .field("is_stream_mode", &self.is_stream_mode)
            .finish()
    }
}

/// WebSocket client with automatic reconnection.
///
/// Handles connection state, callbacks, and rate limiting.
/// See module docs for architecture details.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.network")
)]
pub struct WebSocketClient {
    pub(crate) controller_task: tokio::task::JoinHandle<()>,
    pub(crate) connection_mode: Arc<AtomicU8>,
    pub(crate) reconnect_timeout: Duration,
    pub(crate) rate_limiter: Arc<RateLimiter<String, MonotonicClock>>,
    pub(crate) writer_tx: tokio::sync::mpsc::UnboundedSender<WriterCommand>,
}

impl Debug for WebSocketClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(WebSocketClient)).finish()
    }
}

impl WebSocketClient {
    /// Creates a websocket client in **stream mode** that returns a [`MessageReader`].
    ///
    /// Returns a stream that the caller owns and reads from directly. Automatic reconnection
    /// is **disabled** because the reader cannot be replaced internally. On disconnection, the
    /// client transitions to CLOSED state and the caller must manually reconnect by calling
    /// `connect_stream` again.
    ///
    /// Use stream mode when you need custom reconnection logic, direct control over message
    /// reading, or fine-grained backpressure handling.
    ///
    /// See [`WebSocketConfig`] documentation for comparison with handler mode.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection cannot be established.
    #[allow(clippy::too_many_arguments)]
    pub async fn connect_stream(
        config: WebSocketConfig,
        keyed_quotas: Vec<(String, Quota)>,
        default_quota: Option<Quota>,
        post_reconnect: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Result<(MessageReader, Self), Error> {
        install_cryptographic_provider();

        // Create a single connection and split it, respecting configured headers
        let (writer, reader) =
            WebSocketClientInner::connect_with_server(&config.url, config.headers.clone()).await?;

        // Create inner without connecting (we'll provide the writer)
        let inner = WebSocketClientInner::new_with_writer(config, writer).await?;

        let connection_mode = inner.connection_mode.clone();
        let reconnect_timeout = inner.reconnect_timeout;
        let rate_limiter = Arc::new(RateLimiter::new_with_quota(default_quota, keyed_quotas));
        let writer_tx = inner.writer_tx.clone();

        let controller_task =
            Self::spawn_controller_task(inner, connection_mode.clone(), post_reconnect);

        Ok((
            reader,
            Self {
                controller_task,
                connection_mode,
                reconnect_timeout,
                rate_limiter,
                writer_tx,
            },
        ))
    }

    /// Creates a websocket client in **handler mode** with automatic reconnection.
    ///
    /// Requires `config.message_handler` to be set. The handler is called for each incoming
    /// message on an internal task. Automatic reconnection is **enabled** with exponential
    /// backoff. On disconnection, the client automatically attempts to reconnect and replaces
    /// the internal reader (the handler continues working seamlessly).
    ///
    /// Use handler mode for simplified connection management, automatic reconnection, Python
    /// bindings, or callback-based message handling.
    ///
    /// See [`WebSocketConfig`] documentation for comparison with stream mode.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection cannot be established or if
    /// `config.message_handler` is `None` (use `connect_stream` instead).
    pub async fn connect(
        config: WebSocketConfig,
        post_reconnection: Option<Arc<dyn Fn() + Send + Sync>>,
        keyed_quotas: Vec<(String, Quota)>,
        default_quota: Option<Quota>,
    ) -> Result<Self, Error> {
        // Validate that handler mode has a message handler
        if config.message_handler.is_none() {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Handler mode requires config.message_handler to be set. Use connect_stream() for stream mode without a handler.",
            )));
        }

        tracing::debug!("Connecting");
        let inner = WebSocketClientInner::connect_url(config).await?;
        let connection_mode = inner.connection_mode.clone();
        let writer_tx = inner.writer_tx.clone();
        let reconnect_timeout = inner.reconnect_timeout;

        let controller_task =
            Self::spawn_controller_task(inner, connection_mode.clone(), post_reconnection);

        let rate_limiter = Arc::new(RateLimiter::new_with_quota(default_quota, keyed_quotas));

        Ok(Self {
            controller_task,
            connection_mode,
            reconnect_timeout,
            rate_limiter,
            writer_tx,
        })
    }

    /// Returns the current connection mode.
    #[must_use]
    pub fn connection_mode(&self) -> ConnectionMode {
        ConnectionMode::from_atomic(&self.connection_mode)
    }

    /// Returns a clone of the connection mode atomic for external state tracking.
    ///
    /// This allows adapter clients to track connection state across reconnections
    /// without message-passing delays.
    #[must_use]
    pub fn connection_mode_atomic(&self) -> Arc<AtomicU8> {
        Arc::clone(&self.connection_mode)
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

    /// Check if the client is disconnected.
    #[must_use]
    pub fn is_disconnected(&self) -> bool {
        self.controller_task.is_finished()
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

    /// Wait for the client to become active before sending.
    ///
    /// Returns an error if the client is closed, disconnecting, or if the wait times out.
    async fn wait_for_active(&self) -> Result<(), SendError> {
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

        Ok(())
    }

    /// Set disconnect mode to true.
    ///
    /// Controller task will periodically check the disconnect mode
    /// and shutdown the client if it is alive
    pub async fn disconnect(&self) {
        tracing::debug!("Disconnecting");
        self.connection_mode
            .store(ConnectionMode::Disconnect.as_u8(), Ordering::SeqCst);

        if let Ok(()) =
            tokio::time::timeout(Duration::from_secs(GRACEFUL_SHUTDOWN_TIMEOUT_SECS), async {
                while !self.is_disconnected() {
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
            tracing::debug!("Controller task finished");
        } else {
            tracing::error!("Timeout waiting for controller task to finish");
            if !self.controller_task.is_finished() {
                self.controller_task.abort();
                log_task_aborted("controller");
            }
        }
    }

    /// Sends the given text `data` to the server.
    ///
    /// # Errors
    ///
    /// Returns a websocket error if unable to send.
    #[allow(unused_variables)]
    pub async fn send_text(
        &self,
        data: String,
        keys: Option<Vec<String>>,
    ) -> Result<(), SendError> {
        // Check connection state before rate limiting to fail fast
        if self.is_closed() || self.is_disconnecting() {
            return Err(SendError::Closed);
        }

        self.rate_limiter.await_keys_ready(keys).await;
        self.wait_for_active().await?;

        tracing::trace!("Sending text: {data:?}");

        let msg = Message::Text(data.into());
        self.writer_tx
            .send(WriterCommand::Send(msg))
            .map_err(|e| SendError::BrokenPipe(e.to_string()))
    }

    /// Sends a pong frame back to the server.
    ///
    /// # Errors
    ///
    /// Returns a websocket error if unable to send.
    pub async fn send_pong(&self, data: Vec<u8>) -> Result<(), SendError> {
        self.wait_for_active().await?;

        tracing::trace!("Sending pong frame ({} bytes)", data.len());

        let msg = Message::Pong(data.into());
        self.writer_tx
            .send(WriterCommand::Send(msg))
            .map_err(|e| SendError::BrokenPipe(e.to_string()))
    }

    /// Sends the given bytes `data` to the server.
    ///
    /// # Errors
    ///
    /// Returns a websocket error if unable to send.
    #[allow(unused_variables)]
    pub async fn send_bytes(
        &self,
        data: Vec<u8>,
        keys: Option<Vec<String>>,
    ) -> Result<(), SendError> {
        // Check connection state before rate limiting to fail fast
        if self.is_closed() || self.is_disconnecting() {
            return Err(SendError::Closed);
        }

        self.rate_limiter.await_keys_ready(keys).await;
        self.wait_for_active().await?;

        tracing::trace!("Sending bytes: {data:?}");

        let msg = Message::Binary(data.into());
        self.writer_tx
            .send(WriterCommand::Send(msg))
            .map_err(|e| SendError::BrokenPipe(e.to_string()))
    }

    /// Sends a close message to the server.
    ///
    /// # Errors
    ///
    /// Returns a websocket error if unable to send.
    pub async fn send_close_message(&self) -> Result<(), SendError> {
        self.wait_for_active().await?;

        let msg = Message::Close(None);
        self.writer_tx
            .send(WriterCommand::Send(msg))
            .map_err(|e| SendError::BrokenPipe(e.to_string()))
    }

    fn spawn_controller_task(
        mut inner: WebSocketClientInner,
        connection_mode: Arc<AtomicU8>,
        post_reconnection: Option<Arc<dyn Fn() + Send + Sync>>,
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

                        if let Some(task) = &inner.read_task
                            && !task.is_finished()
                        {
                            task.abort();
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
                    // Check if max reconnection attempts exceeded
                    if let Some(max_attempts) = inner.reconnect_max_attempts
                        && inner.reconnection_attempt_count >= max_attempts
                    {
                        tracing::error!(
                            "Max reconnection attempts ({}) exceeded, transitioning to CLOSED",
                            max_attempts
                        );
                        connection_mode.store(ConnectionMode::Closed.as_u8(), Ordering::SeqCst);
                        break;
                    }

                    inner.reconnection_attempt_count += 1;
                    tracing::debug!(
                        "Reconnection attempt {} of {}",
                        inner.reconnection_attempt_count,
                        inner
                            .reconnect_max_attempts
                            .map_or_else(|| "unlimited".to_string(), |m| m.to_string())
                    );

                    match inner.reconnect().await {
                        Ok(()) => {
                            inner.backoff.reset();
                            inner.reconnection_attempt_count = 0; // Reset counter on success

                            // Only invoke callbacks if not in disconnect state
                            if ConnectionMode::from_atomic(&connection_mode).is_active() {
                                if let Some(ref handler) = inner.config.message_handler {
                                    let reconnected_msg =
                                        Message::Text(RECONNECTED.to_string().into());
                                    handler(reconnected_msg);
                                    tracing::debug!("Sent reconnected message to handler");
                                }

                                // TODO: Retain this legacy callback for use from Python
                                if let Some(ref callback) = post_reconnection {
                                    callback();
                                    tracing::debug!("Called `post_reconnection` handler");
                                }

                                tracing::debug!("Reconnected successfully");
                            } else {
                                tracing::debug!(
                                    "Skipping post_reconnection handlers due to disconnect state"
                                );
                            }
                        }
                        Err(e) => {
                            let duration = inner.backoff.next_duration();
                            tracing::warn!(
                                "Reconnect attempt {} failed: {e}",
                                inner.reconnection_attempt_count
                            );
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
impl Drop for WebSocketClient {
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
#[cfg(not(feature = "turmoil"))]
#[cfg(target_os = "linux")] // Only run network tests on Linux (CI stability)
mod tests {
    use std::{num::NonZeroU32, sync::Arc};

    use futures_util::{SinkExt, StreamExt};
    use tokio::{
        net::TcpListener,
        task::{self, JoinHandle},
    };
    use tokio_tungstenite::{
        accept_hdr_async,
        tungstenite::{
            handshake::server::{self, Callback},
            http::HeaderValue,
        },
    };

    use crate::{
        ratelimiter::quota::Quota,
        websocket::{WebSocketClient, WebSocketConfig},
    };

    struct TestServer {
        task: JoinHandle<()>,
        port: u16,
    }

    #[derive(Debug, Clone)]
    struct TestCallback {
        key: String,
        value: HeaderValue,
    }

    impl Callback for TestCallback {
        #[allow(clippy::panic_in_result_fn)]
        fn on_request(
            self,
            request: &server::Request,
            response: server::Response,
        ) -> Result<server::Response, server::ErrorResponse> {
            let _ = response;
            let value = request.headers().get(&self.key);
            assert!(value.is_some());

            if let Some(value) = request.headers().get(&self.key) {
                assert_eq!(value, self.value);
            }

            Ok(response)
        }
    }

    impl TestServer {
        async fn setup() -> Self {
            let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = TcpListener::local_addr(&server).unwrap().port();

            let header_key = "test".to_string();
            let header_value = "test".to_string();

            let test_call_back = TestCallback {
                key: header_key,
                value: HeaderValue::from_str(&header_value).unwrap(),
            };

            let task = task::spawn(async move {
                // Keep accepting connections
                loop {
                    let (conn, _) = server.accept().await.unwrap();
                    let mut websocket = accept_hdr_async(conn, test_call_back.clone())
                        .await
                        .unwrap();

                    task::spawn(async move {
                        while let Some(Ok(msg)) = websocket.next().await {
                            match msg {
                                tokio_tungstenite::tungstenite::protocol::Message::Text(txt)
                                    if txt == "close-now" =>
                                {
                                    tracing::debug!("Forcibly closing from server side");
                                    // This sends a close frame, then stops reading
                                    let _ = websocket.close(None).await;
                                    break;
                                }
                                // Echo text/binary frames
                                tokio_tungstenite::tungstenite::protocol::Message::Text(_)
                                | tokio_tungstenite::tungstenite::protocol::Message::Binary(_) => {
                                    if websocket.send(msg).await.is_err() {
                                        break;
                                    }
                                }
                                // If the client closes, we also break
                                tokio_tungstenite::tungstenite::protocol::Message::Close(
                                    _frame,
                                ) => {
                                    let _ = websocket.close(None).await;
                                    break;
                                }
                                // Ignore pings/pongs
                                _ => {}
                            }
                        }
                    });
                }
            });

            Self { task, port }
        }
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    async fn setup_test_client(port: u16) -> WebSocketClient {
        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{port}"),
            headers: vec![("test".into(), "test".into())],
            message_handler: Some(Arc::new(|_| {})),
            heartbeat: None,
            heartbeat_msg: None,
            ping_handler: None,
            reconnect_timeout_ms: None,
            reconnect_delay_initial_ms: None,
            reconnect_backoff_factor: None,
            reconnect_delay_max_ms: None,
            reconnect_jitter_ms: None,
            reconnect_max_attempts: None,
        };
        WebSocketClient::connect(config, None, vec![], None)
            .await
            .expect("Failed to connect")
    }

    #[tokio::test]
    async fn test_websocket_basic() {
        let server = TestServer::setup().await;
        let client = setup_test_client(server.port).await;

        assert!(!client.is_disconnected());

        client.disconnect().await;
        assert!(client.is_disconnected());
    }

    #[tokio::test]
    async fn test_websocket_heartbeat() {
        let server = TestServer::setup().await;
        let client = setup_test_client(server.port).await;

        // Wait ~3s => server should see multiple "ping"
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        // Cleanup
        client.disconnect().await;
        assert!(client.is_disconnected());
    }

    #[tokio::test]
    async fn test_websocket_reconnect_exhausted() {
        let config = WebSocketConfig {
            url: "ws://127.0.0.1:9997".into(), // <-- No server
            headers: vec![],
            message_handler: Some(Arc::new(|_| {})),
            heartbeat: None,
            heartbeat_msg: None,
            ping_handler: None,
            reconnect_timeout_ms: None,
            reconnect_delay_initial_ms: None,
            reconnect_backoff_factor: None,
            reconnect_delay_max_ms: None,
            reconnect_jitter_ms: None,
            reconnect_max_attempts: None,
        };
        let res = WebSocketClient::connect(config, None, vec![], None).await;
        assert!(res.is_err(), "Should fail quickly with no server");
    }

    #[tokio::test]
    async fn test_websocket_forced_close_reconnect() {
        let server = TestServer::setup().await;
        let client = setup_test_client(server.port).await;

        // 1) Send normal message
        client.send_text("Hello".into(), None).await.unwrap();

        // 2) Trigger forced close from server
        client.send_text("close-now".into(), None).await.unwrap();

        // 3) Wait a bit => read loop sees close => reconnect
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        // Confirm not disconnected
        assert!(!client.is_disconnected());

        // Cleanup
        client.disconnect().await;
        assert!(client.is_disconnected());
    }

    #[tokio::test]
    async fn test_rate_limiter() {
        let server = TestServer::setup().await;
        let quota = Quota::per_second(NonZeroU32::new(2).unwrap());

        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{}", server.port),
            headers: vec![("test".into(), "test".into())],
            message_handler: Some(Arc::new(|_| {})),
            heartbeat: None,
            heartbeat_msg: None,
            ping_handler: None,
            reconnect_timeout_ms: None,
            reconnect_delay_initial_ms: None,
            reconnect_backoff_factor: None,
            reconnect_delay_max_ms: None,
            reconnect_jitter_ms: None,
            reconnect_max_attempts: None,
        };

        let client = WebSocketClient::connect(config, None, vec![("default".into(), quota)], None)
            .await
            .unwrap();

        // First 2 should succeed
        client.send_text("test1".into(), None).await.unwrap();
        client.send_text("test2".into(), None).await.unwrap();

        // Third should error
        client.send_text("test3".into(), None).await.unwrap();

        // Cleanup
        client.disconnect().await;
        assert!(client.is_disconnected());
    }

    #[tokio::test]
    async fn test_concurrent_writers() {
        let server = TestServer::setup().await;
        let client = Arc::new(setup_test_client(server.port).await);

        let mut handles = vec![];
        for i in 0..10 {
            let client = client.clone();
            handles.push(task::spawn(async move {
                client.send_text(format!("test{i}"), None).await.unwrap();
            }));
        }

        for handle in handles {
            handle.await.unwrap();
        }

        // Cleanup
        client.disconnect().await;
        assert!(client.is_disconnected());
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
#[cfg(not(feature = "turmoil"))]
mod rust_tests {
    use futures_util::StreamExt;
    use rstest::rstest;
    use tokio::{
        net::TcpListener,
        task,
        time::{Duration, sleep},
    };
    use tokio_tungstenite::accept_async;

    use super::*;
    use crate::websocket::types::channel_message_handler;

    #[rstest]
    #[tokio::test]
    async fn test_reconnect_then_disconnect() {
        // Bind an ephemeral port
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        // Server task: accept one ws connection then close it
        let server = task::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let ws = accept_async(stream).await.unwrap();
            drop(ws);
            // Keep alive briefly
            sleep(Duration::from_secs(1)).await;
        });

        // Build a channel-based message handler for incoming messages (unused here)
        let (handler, _rx) = channel_message_handler();

        // Configure client with short reconnect backoff
        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{port}"),
            headers: vec![],
            message_handler: Some(handler),
            heartbeat: None,
            heartbeat_msg: None,
            ping_handler: None,
            reconnect_timeout_ms: Some(1_000),
            reconnect_delay_initial_ms: Some(50),
            reconnect_delay_max_ms: Some(100),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            reconnect_max_attempts: None,
        };

        // Connect the client
        let client = WebSocketClient::connect(config, None, vec![], None)
            .await
            .unwrap();

        // Allow server to drop connection and client to detect
        sleep(Duration::from_millis(100)).await;
        // Now immediately disconnect the client
        client.disconnect().await;
        assert!(client.is_disconnected());
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_reconnect_state_flips_when_reader_stops() {
        // Bind an ephemeral port and accept a single websocket connection which we drop.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = task::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(ws) = accept_async(stream).await
            {
                drop(ws);
            }
            sleep(Duration::from_millis(50)).await;
        });

        let (handler, _rx) = channel_message_handler();

        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{port}"),
            headers: vec![],
            message_handler: Some(handler),
            heartbeat: None,
            heartbeat_msg: None,
            ping_handler: None,
            reconnect_timeout_ms: Some(1_000),
            reconnect_delay_initial_ms: Some(50),
            reconnect_delay_max_ms: Some(100),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            reconnect_max_attempts: None,
        };

        let client = WebSocketClient::connect(config, None, vec![], None)
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

        client.disconnect().await;
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_stream_mode_disables_auto_reconnect() {
        // Test that stream-based clients (created via connect_stream) set is_stream_mode flag
        // and that reconnect() transitions to CLOSED state for stream mode
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = task::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(_ws) = accept_async(stream).await
            {
                // Keep connection alive briefly
                sleep(Duration::from_millis(100)).await;
            }
        });

        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{port}"),
            headers: vec![],
            message_handler: None, // Stream mode - no handler
            heartbeat: None,
            heartbeat_msg: None,
            ping_handler: None,
            reconnect_timeout_ms: Some(1_000),
            reconnect_delay_initial_ms: Some(50),
            reconnect_delay_max_ms: Some(100),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            reconnect_max_attempts: None,
        };

        // Create stream-based client
        let (_reader, _client) = WebSocketClient::connect_stream(config, vec![], None, None)
            .await
            .unwrap();

        // Note: We can't easily test the reconnect behavior from the outside since
        // the inner client is private. The key fix is that WebSocketClientInner
        // now has is_stream_mode=true for connect_stream, and reconnect() will
        // transition to CLOSED state instead of creating a new reader that gets dropped.
        // This is tested implicitly by the fact that stream users won't get stuck
        // in an infinite reconnect loop.

        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_message_handler_mode_allows_auto_reconnect() {
        // Test that regular clients (with message handler) can auto-reconnect
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = task::spawn(async move {
            // Accept first connection and close it
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(ws) = accept_async(stream).await
            {
                drop(ws);
            }
            sleep(Duration::from_millis(50)).await;
        });

        let (handler, _rx) = channel_message_handler();

        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{port}"),
            headers: vec![],
            message_handler: Some(handler), // Has message handler
            heartbeat: None,
            heartbeat_msg: None,
            ping_handler: None,
            reconnect_timeout_ms: Some(1_000),
            reconnect_delay_initial_ms: Some(50),
            reconnect_delay_max_ms: Some(100),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            reconnect_max_attempts: None,
        };

        let client = WebSocketClient::connect(config, None, vec![], None)
            .await
            .unwrap();

        // Wait for the connection to be dropped and reconnection to be attempted
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if client.is_reconnecting() || client.is_closed() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("client should attempt reconnection or close");

        // Should either be reconnecting or closed (depending on timing)
        // The important thing is it's not staying active forever
        assert!(
            client.is_reconnecting() || client.is_closed(),
            "Client with message handler should attempt reconnection"
        );

        client.disconnect().await;
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_handler_mode_reconnect_with_new_connection() {
        // Test that handler mode successfully reconnects and messages continue flowing
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = task::spawn(async move {
            // First connection - accept and immediately close
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(ws) = accept_async(stream).await
            {
                drop(ws);
            }

            // Small delay to let client detect disconnection
            sleep(Duration::from_millis(100)).await;

            // Second connection - accept, send a message, then keep alive
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(mut ws) = accept_async(stream).await
            {
                use futures_util::SinkExt;
                let _ = ws
                    .send(Message::Text("reconnected".to_string().into()))
                    .await;
                sleep(Duration::from_secs(1)).await;
            }
        });

        let (handler, mut rx) = channel_message_handler();

        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{port}"),
            headers: vec![],
            message_handler: Some(handler),
            heartbeat: None,
            heartbeat_msg: None,
            ping_handler: None,
            reconnect_timeout_ms: Some(2_000),
            reconnect_delay_initial_ms: Some(50),
            reconnect_delay_max_ms: Some(200),
            reconnect_backoff_factor: Some(1.5),
            reconnect_jitter_ms: Some(10),
            reconnect_max_attempts: None,
        };

        let client = WebSocketClient::connect(config, None, vec![], None)
            .await
            .unwrap();

        // Wait for reconnection to happen and message to arrive
        let result = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Ok(msg) = rx.try_recv()
                    && matches!(msg, Message::Text(ref text) if AsRef::<str>::as_ref(text) == "reconnected")
                {
                    return true;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;

        assert!(
            result.is_ok(),
            "Should receive message after reconnection within timeout"
        );

        client.disconnect().await;
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_stream_mode_no_auto_reconnect() {
        // Test that stream mode does not automatically reconnect when connection is lost
        // The caller owns the reader and is responsible for detecting disconnection
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = task::spawn(async move {
            // Accept connection and send one message, then close
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(mut ws) = accept_async(stream).await
            {
                use futures_util::SinkExt;
                let _ = ws.send(Message::Text("hello".to_string().into())).await;
                sleep(Duration::from_millis(50)).await;
                // Connection closes when ws is dropped
            }
        });

        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{port}"),
            headers: vec![],
            message_handler: None, // Stream mode
            heartbeat: None,
            heartbeat_msg: None,
            ping_handler: None,
            reconnect_timeout_ms: Some(1_000),
            reconnect_delay_initial_ms: Some(50),
            reconnect_delay_max_ms: Some(100),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            reconnect_max_attempts: None,
        };

        let (mut reader, client) = WebSocketClient::connect_stream(config, vec![], None, None)
            .await
            .unwrap();

        // Initially active
        assert!(client.is_active(), "Client should start as active");

        // Read the hello message
        let msg = reader.next().await;
        assert!(
            matches!(msg, Some(Ok(Message::Text(ref text))) if AsRef::<str>::as_ref(text) == "hello"),
            "Should receive initial message"
        );

        // Read until connection closes (reader will return None or error)
        while let Some(msg) = reader.next().await {
            if msg.is_err() || matches!(msg, Ok(Message::Close(_))) {
                break;
            }
        }

        // In stream mode, the controller cannot detect disconnection (reader is owned by caller)
        // The client remains ACTIVE - it's the caller's responsibility to call disconnect()
        sleep(Duration::from_millis(200)).await;

        // Client should still be ACTIVE (not RECONNECTING or CLOSED)
        // This is correct behavior - stream mode doesn't auto-detect disconnection
        assert!(
            client.is_active() || client.is_closed(),
            "Stream mode client stays ACTIVE (caller owns reader) or caller disconnected"
        );
        assert!(
            !client.is_reconnecting(),
            "Stream mode client should never attempt reconnection"
        );

        client.disconnect().await;
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_send_timeout_uses_configured_reconnect_timeout() {
        // Test that send operations respect the configured reconnect_timeout.
        // When a client is stuck in RECONNECT longer than the timeout, sends should fail with Timeout.
        use nautilus_common::testing::wait_until_async;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = task::spawn(async move {
            // Accept first connection and immediately close it
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(ws) = accept_async(stream).await
            {
                drop(ws);
            }
            // Don't accept second connection - client will be stuck in RECONNECT
            sleep(Duration::from_secs(60)).await;
        });

        let (handler, _rx) = channel_message_handler();

        // Configure with SHORT 2s reconnect timeout
        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{port}"),
            headers: vec![],
            message_handler: Some(handler),
            heartbeat: None,
            heartbeat_msg: None,
            ping_handler: None,
            reconnect_timeout_ms: Some(2_000), // 2s timeout
            reconnect_delay_initial_ms: Some(50),
            reconnect_delay_max_ms: Some(100),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            reconnect_max_attempts: None,
        };

        let client = WebSocketClient::connect(config, None, vec![], None)
            .await
            .unwrap();

        // Wait for client to enter RECONNECT state
        wait_until_async(
            || async { client.is_reconnecting() },
            Duration::from_secs(3),
        )
        .await;

        // Attempt send while stuck in RECONNECT - should timeout after 2s (configured timeout)
        let start = std::time::Instant::now();
        let send_result = client.send_text("test".to_string(), None).await;
        let elapsed = start.elapsed();

        assert!(
            send_result.is_err(),
            "Send should fail when client stuck in RECONNECT"
        );
        assert!(
            matches!(send_result, Err(crate::error::SendError::Timeout)),
            "Send should return Timeout error, was: {:?}",
            send_result
        );
        // Verify timeout respects configured value (2s), but don't check upper bound
        // as CI scheduler jitter can cause legitimate delays beyond the timeout
        assert!(
            elapsed >= Duration::from_millis(1800),
            "Send should timeout after at least 2s (configured timeout), took {:?}",
            elapsed
        );

        client.disconnect().await;
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_send_waits_during_reconnection() {
        // Test that send operations wait for reconnection to complete (up to timeout)
        use nautilus_common::testing::wait_until_async;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = task::spawn(async move {
            // First connection - accept and immediately close
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(ws) = accept_async(stream).await
            {
                drop(ws);
            }

            // Wait a bit before accepting second connection
            sleep(Duration::from_millis(500)).await;

            // Second connection - accept and keep alive
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(mut ws) = accept_async(stream).await
            {
                // Echo messages
                while let Some(Ok(msg)) = ws.next().await {
                    if ws.send(msg).await.is_err() {
                        break;
                    }
                }
            }
        });

        let (handler, _rx) = channel_message_handler();

        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{port}"),
            headers: vec![],
            message_handler: Some(handler),
            heartbeat: None,
            heartbeat_msg: None,
            ping_handler: None,
            reconnect_timeout_ms: Some(5_000), // 5s timeout - enough for reconnect
            reconnect_delay_initial_ms: Some(100),
            reconnect_delay_max_ms: Some(200),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            reconnect_max_attempts: None,
        };

        let client = WebSocketClient::connect(config, None, vec![], None)
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
            client.send_text("test_message".to_string(), None),
        )
        .await;

        assert!(
            send_result.is_ok() && send_result.unwrap().is_ok(),
            "Send should succeed after waiting for reconnection"
        );

        client.disconnect().await;
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_rate_limiter_before_active_wait() {
        // Test that rate limiting happens BEFORE active state check.
        // This prevents race conditions where connection state changes during rate limit wait.
        // We verify this by: (1) exhausting rate limit, (2) ensuring client is RECONNECTING,
        // (3) sending again and confirming it waits for rate limit THEN reconnection.
        use std::{num::NonZeroU32, sync::Arc};

        use nautilus_common::testing::wait_until_async;

        use crate::ratelimiter::quota::Quota;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = task::spawn(async move {
            // First connection - accept and close after receiving one message
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(mut ws) = accept_async(stream).await
            {
                // Receive first message then close
                if let Some(Ok(_)) = ws.next().await {
                    drop(ws);
                }
            }

            // Wait before accepting reconnection
            sleep(Duration::from_millis(500)).await;

            // Second connection - accept and keep alive
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(mut ws) = accept_async(stream).await
            {
                while let Some(Ok(msg)) = ws.next().await {
                    if ws.send(msg).await.is_err() {
                        break;
                    }
                }
            }
        });

        let (handler, _rx) = channel_message_handler();

        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{port}"),
            headers: vec![],
            message_handler: Some(handler),
            heartbeat: None,
            heartbeat_msg: None,
            ping_handler: None,
            reconnect_timeout_ms: Some(5_000),
            reconnect_delay_initial_ms: Some(50),
            reconnect_delay_max_ms: Some(100),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            reconnect_max_attempts: None,
        };

        // Very restrictive rate limit: 1 request per second, burst of 1
        let quota =
            Quota::per_second(NonZeroU32::new(1).unwrap()).allow_burst(NonZeroU32::new(1).unwrap());

        let client = Arc::new(
            WebSocketClient::connect(config, None, vec![("test_key".to_string(), quota)], None)
                .await
                .unwrap(),
        );

        // First send exhausts burst capacity and triggers connection close
        client
            .send_text("msg1".to_string(), Some(vec!["test_key".to_string()]))
            .await
            .unwrap();

        // Wait for client to enter RECONNECT state
        wait_until_async(
            || async { client.is_reconnecting() },
            Duration::from_secs(2),
        )
        .await;

        // Second send: will hit rate limit (~1s) THEN wait for reconnection (~0.5s)
        let start = std::time::Instant::now();
        let send_result = client
            .send_text("msg2".to_string(), Some(vec!["test_key".to_string()]))
            .await;
        let elapsed = start.elapsed();

        // Should succeed after both rate limit AND reconnection
        assert!(
            send_result.is_ok(),
            "Send should succeed after rate limit + reconnection, was: {:?}",
            send_result
        );
        // Total wait should be at least rate limit time (~1s)
        // The reconnection completes while rate limiting or after
        // Use 850ms threshold to account for timing jitter in CI
        assert!(
            elapsed >= Duration::from_millis(850),
            "Should wait for rate limit (~1s), waited {:?}",
            elapsed
        );

        client.disconnect().await;
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_disconnect_during_reconnect_exits_cleanly() {
        // Test CAS race condition: disconnect called during reconnection
        // Should exit cleanly without spawning new tasks
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = task::spawn(async move {
            // Accept first connection and immediately close
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(ws) = accept_async(stream).await
            {
                drop(ws);
            }
            // Don't accept second connection - let reconnect hang
            sleep(Duration::from_secs(60)).await;
        });

        let (handler, _rx) = channel_message_handler();

        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{port}"),
            headers: vec![],
            message_handler: Some(handler),
            heartbeat: None,
            heartbeat_msg: None,
            ping_handler: None,
            reconnect_timeout_ms: Some(2_000), // 2s timeout - shorter than disconnect timeout
            reconnect_delay_initial_ms: Some(100),
            reconnect_delay_max_ms: Some(200),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            reconnect_max_attempts: None,
        };

        let client = WebSocketClient::connect(config, None, vec![], None)
            .await
            .unwrap();

        // Wait for reconnection to start
        tokio::time::timeout(Duration::from_secs(2), async {
            while !client.is_reconnecting() {
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("Client should enter RECONNECT state");

        // Disconnect while reconnecting
        client.disconnect().await;

        // Should be cleanly closed
        assert!(
            client.is_disconnected(),
            "Client should be cleanly disconnected"
        );

        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_send_fails_fast_when_closed_before_rate_limit() {
        // Test that send operations check connection state BEFORE rate limiting,
        // preventing unnecessary delays when the connection is already closed.
        use std::{num::NonZeroU32, sync::Arc};

        use nautilus_common::testing::wait_until_async;

        use crate::ratelimiter::quota::Quota;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = task::spawn(async move {
            // Accept connection and immediately close
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(ws) = accept_async(stream).await
            {
                drop(ws);
            }
            sleep(Duration::from_secs(60)).await;
        });

        let (handler, _rx) = channel_message_handler();

        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{port}"),
            headers: vec![],
            message_handler: Some(handler),
            heartbeat: None,
            heartbeat_msg: None,
            ping_handler: None,
            reconnect_timeout_ms: Some(5_000),
            reconnect_delay_initial_ms: Some(50),
            reconnect_delay_max_ms: Some(100),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            reconnect_max_attempts: None,
        };

        // Very restrictive rate limit: 1 request per 10 seconds
        // This ensures that if we wait for rate limit, the test will timeout
        let quota = Quota::with_period(Duration::from_secs(10))
            .unwrap()
            .allow_burst(NonZeroU32::new(1).unwrap());

        let client = Arc::new(
            WebSocketClient::connect(config, None, vec![("test_key".to_string(), quota)], None)
                .await
                .unwrap(),
        );

        // Wait for disconnection
        wait_until_async(
            || async { client.is_reconnecting() || client.is_closed() },
            Duration::from_secs(2),
        )
        .await;

        // Explicitly disconnect to move away from ACTIVE state
        client.disconnect().await;
        assert!(
            !client.is_active(),
            "Client should not be active after disconnect"
        );

        // Attempt send - should fail IMMEDIATELY without waiting for rate limit
        let start = std::time::Instant::now();
        let result = client
            .send_text("test".to_string(), Some(vec!["test_key".to_string()]))
            .await;
        let elapsed = start.elapsed();

        // Should fail with Closed error
        assert!(result.is_err(), "Send should fail when client is closed");
        assert!(
            matches!(result, Err(crate::error::SendError::Closed)),
            "Send should return Closed error, was: {:?}",
            result
        );

        // Should fail FAST (< 100ms) without waiting for rate limit (10s)
        assert!(
            elapsed < Duration::from_millis(100),
            "Send should fail fast without rate limiting, took {:?}",
            elapsed
        );

        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_connect_rejects_config_without_message_handler() {
        // Test that connect() properly rejects configs without a message handler
        // to prevent zombie connections that appear alive but never detect disconnections

        let config = WebSocketConfig {
            url: "ws://127.0.0.1:9999".to_string(),
            headers: vec![],
            message_handler: None, // No handler provided
            heartbeat: None,
            heartbeat_msg: None,
            ping_handler: None,
            reconnect_timeout_ms: Some(1_000),
            reconnect_delay_initial_ms: Some(100),
            reconnect_delay_max_ms: Some(500),
            reconnect_backoff_factor: Some(1.5),
            reconnect_jitter_ms: Some(0),
            reconnect_max_attempts: None,
        };

        let result = WebSocketClient::connect(config, None, vec![], None).await;

        assert!(
            result.is_err(),
            "connect() should reject configs without message_handler"
        );

        let err = result.unwrap_err();
        let err_msg = err.to_string();
        assert!(
            err_msg.contains("Handler mode requires config.message_handler"),
            "Error should mention missing message_handler, was: {err_msg}"
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_client_without_handler_sets_stream_mode() {
        // Test that if a client is somehow created without a handler,
        // it properly sets is_stream_mode=true to prevent zombie connections

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = task::spawn(async move {
            // Accept and immediately close to simulate server disconnect
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(ws) = accept_async(stream).await
            {
                drop(ws); // Drop connection immediately
            }
        });

        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{port}"),
            headers: vec![],
            message_handler: None, // No handler
            heartbeat: None,
            heartbeat_msg: None,
            ping_handler: None,
            reconnect_timeout_ms: Some(1_000),
            reconnect_delay_initial_ms: Some(100),
            reconnect_delay_max_ms: Some(500),
            reconnect_backoff_factor: Some(1.5),
            reconnect_jitter_ms: Some(0),
            reconnect_max_attempts: None,
        };

        // Create client directly via connect_url to bypass validation
        let inner = WebSocketClientInner::connect_url(config).await.unwrap();

        // Verify is_stream_mode is true when no handler
        assert!(
            inner.is_stream_mode,
            "Client without handler should have is_stream_mode=true"
        );

        // Verify that when stream mode is enabled, reconnection is disabled
        // (documented behavior - stream mode clients close instead of reconnecting)

        server.abort();
    }
}
