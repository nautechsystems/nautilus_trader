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

//! High-performance WebSocket client implementation with automatic reconnection
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
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    time::Duration,
};

use futures_util::{
    SinkExt, StreamExt,
    stream::{SplitSink, SplitStream},
};
use http::HeaderName;
use nautilus_core::CleanDrop;
use nautilus_cryptography::providers::install_cryptographic_provider;
#[cfg(feature = "turmoil")]
use tokio_tungstenite::client_async;
#[cfg(not(feature = "turmoil"))]
use tokio_tungstenite::connect_async;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{Error, Message, client::IntoClientRequest, http::HeaderValue},
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

pub const TEXT_PING: &str = "ping";
pub const TEXT_PONG: &str = "pong";

// Connection timing constants
const CONNECTION_STATE_CHECK_INTERVAL_MS: u64 = 10;
const GRACEFUL_SHUTDOWN_DELAY_MS: u64 = 100;
const GRACEFUL_SHUTDOWN_TIMEOUT_SECS: u64 = 5;
const SEND_OPERATION_CHECK_INTERVAL_MS: u64 = 1;

#[cfg(not(feature = "turmoil"))]
type MessageWriter = SplitSink<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>, Message>;
#[cfg(not(feature = "turmoil"))]
pub type MessageReader = SplitStream<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>>;

#[cfg(feature = "turmoil")]
type MessageWriter = SplitSink<WebSocketStream<MaybeTlsStream<crate::net::TcpStream>>, Message>;
#[cfg(feature = "turmoil")]
pub type MessageReader = SplitStream<WebSocketStream<MaybeTlsStream<crate::net::TcpStream>>>;

/// Function type for handling WebSocket messages.
///
/// When provided, the client will spawn an internal task to read messages and pass them
/// to this handler. This enables automatic reconnection where the client can replace the
/// reader internally.
///
/// When `None`, the client returns a `MessageReader` stream (via `connect_stream`) that
/// the caller owns and reads from directly. This disables automatic reconnection because
/// the reader cannot be replaced - the caller must manually reconnect.
pub type MessageHandler = Arc<dyn Fn(Message) + Send + Sync>;

/// Function type for handling WebSocket ping messages.
pub type PingHandler = Arc<dyn Fn(Vec<u8>) + Send + Sync>;

/// Creates a channel-based message handler.
///
/// Returns a tuple containing the message handler and a receiver for messages.
#[must_use]
pub fn channel_message_handler() -> (
    MessageHandler,
    tokio::sync::mpsc::UnboundedReceiver<Message>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let handler = Arc::new(move |msg: Message| {
        if let Err(e) = tx.send(msg) {
            tracing::debug!("Failed to send message to channel: {e}");
        }
    });
    (handler, rx)
}

/// Configuration for WebSocket client connections.
///
/// # Connection Modes
///
/// The `message_handler` field determines the connection mode:
///
/// ## Handler Mode (`message_handler: Some(...)`)
/// - Use with [`WebSocketClient::connect`].
/// - Client spawns internal task to read messages and call handler.
/// - **Supports automatic reconnection** with exponential backoff.
/// - Reconnection config fields (`reconnect_*`) are active.
/// - Best for long-lived connections, Python bindings, callback-based APIs.
///
/// ## Stream Mode (`message_handler: None`)
/// - Use with [`WebSocketClient::connect_stream`].
/// - Returns a [`MessageReader`] stream for the caller to read from.
/// - **Does NOT support automatic reconnection** (reader owned by caller).
/// - Reconnection config fields are ignored.
/// - On disconnect, client transitions to CLOSED state and caller must manually reconnect.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.network")
)]
pub struct WebSocketConfig {
    /// The URL to connect to.
    pub url: String,
    /// The default headers.
    pub headers: Vec<(String, String)>,
    /// The function to handle incoming messages.
    ///
    /// - `Some(handler)`: Handler mode with automatic reconnection (use with `connect`).
    /// - `None`: Stream mode without automatic reconnection (use with `connect_stream`).
    ///
    /// See [`WebSocketConfig`] docs for detailed explanation of modes.
    pub message_handler: Option<MessageHandler>,
    /// The optional heartbeat interval (seconds).
    pub heartbeat: Option<u64>,
    /// The optional heartbeat message.
    pub heartbeat_msg: Option<String>,
    /// The handler for incoming pings.
    pub ping_handler: Option<PingHandler>,
    /// The timeout (milliseconds) for reconnection attempts.
    ///
    /// **Note**: Only applies to handler mode. Ignored in stream mode.
    pub reconnect_timeout_ms: Option<u64>,
    /// The initial reconnection delay (milliseconds) for reconnects.
    ///
    /// **Note**: Only applies to handler mode. Ignored in stream mode.
    pub reconnect_delay_initial_ms: Option<u64>,
    /// The maximum reconnect delay (milliseconds) for exponential backoff.
    ///
    /// **Note**: Only applies to handler mode. Ignored in stream mode.
    pub reconnect_delay_max_ms: Option<u64>,
    /// The exponential backoff factor for reconnection delays.
    ///
    /// **Note**: Only applies to handler mode. Ignored in stream mode.
    pub reconnect_backoff_factor: Option<f64>,
    /// The maximum jitter (milliseconds) added to reconnection delays.
    ///
    /// **Note**: Only applies to handler mode. Ignored in stream mode.
    pub reconnect_jitter_ms: Option<u64>,
}

impl Debug for WebSocketConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(WebSocketConfig))
            .field("url", &self.url)
            .field("headers", &self.headers)
            .field(
                "message_handler",
                &self.message_handler.as_ref().map(|_| "<function>"),
            )
            .field("heartbeat", &self.heartbeat)
            .field("heartbeat_msg", &self.heartbeat_msg)
            .field(
                "ping_handler",
                &self.ping_handler.as_ref().map(|_| "<function>"),
            )
            .field("reconnect_timeout_ms", &self.reconnect_timeout_ms)
            .field(
                "reconnect_delay_initial_ms",
                &self.reconnect_delay_initial_ms,
            )
            .field("reconnect_delay_max_ms", &self.reconnect_delay_max_ms)
            .field("reconnect_backoff_factor", &self.reconnect_backoff_factor)
            .field("reconnect_jitter_ms", &self.reconnect_jitter_ms)
            .finish()
    }
}

impl Clone for WebSocketConfig {
    fn clone(&self) -> Self {
        Self {
            url: self.url.clone(),
            headers: self.headers.clone(),
            message_handler: self.message_handler.clone(),
            heartbeat: self.heartbeat,
            heartbeat_msg: self.heartbeat_msg.clone(),
            ping_handler: self.ping_handler.clone(),
            reconnect_timeout_ms: self.reconnect_timeout_ms,
            reconnect_delay_initial_ms: self.reconnect_delay_initial_ms,
            reconnect_delay_max_ms: self.reconnect_delay_max_ms,
            reconnect_backoff_factor: self.reconnect_backoff_factor,
            reconnect_jitter_ms: self.reconnect_jitter_ms,
        }
    }
}

/// Represents a command for the writer task.
#[derive(Debug)]
pub(crate) enum WriterCommand {
    /// Update the writer reference with a new one after reconnection.
    Update(MessageWriter),
    /// Send message to the server.
    Send(Message),
}

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
struct WebSocketClientInner {
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
}

impl WebSocketClientInner {
    /// Create an inner websocket client with an existing writer.
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
        })
    }

    /// Create an inner websocket client.
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
        } = &config;
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
            is_stream_mode: false,
        })
    }

    /// Connects with the server creating a tokio-tungstenite websocket stream.
    /// Production version that uses `connect_async` convenience helper.
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

        connect_async(request).await.map(|resp| resp.0.split())
    }

    /// Connects with the server creating a tokio-tungstenite websocket stream.
    /// Turmoil version that uses the lower-level `client_async` API with injected stream.
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

            if let Err(e) = self.writer_tx.send(WriterCommand::Update(new_writer)) {
                tracing::error!("{e}");
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

            loop {
                match ConnectionMode::from_atomic(&connection_state) {
                    ConnectionMode::Disconnect => {
                        // Attempt to close the writer gracefully before exiting,
                        // we ignore any error as the writer may already be closed.
                        _ = tokio::time::timeout(
                            Duration::from_secs(GRACEFUL_SHUTDOWN_TIMEOUT_SECS),
                            active_writer.close(),
                        )
                        .await;
                        break;
                    }
                    ConnectionMode::Closed => break,
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
                            WriterCommand::Update(new_writer) => {
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
                            }
                            _ if mode.is_reconnect() => {
                                tracing::warn!("Skipping message while reconnecting, {msg:?}");
                                continue;
                            }
                            WriterCommand::Send(msg) => {
                                if let Err(e) = active_writer.send(msg).await {
                                    tracing::error!("Failed to send message: {e}");
                                    // Mode is active so trigger reconnection
                                    tracing::warn!("Writer triggering reconnect");
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
                    match inner.reconnect().await {
                        Ok(()) => {
                            inner.backoff.reset();

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
            message_handler: None,
            heartbeat: None,
            heartbeat_msg: None,
            ping_handler: None,
            reconnect_timeout_ms: None,
            reconnect_delay_initial_ms: None,
            reconnect_backoff_factor: None,
            reconnect_delay_max_ms: None,
            reconnect_jitter_ms: None,
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
            message_handler: None,
            heartbeat: None,
            heartbeat_msg: None,
            ping_handler: None,
            reconnect_timeout_ms: None,
            reconnect_delay_initial_ms: None,
            reconnect_backoff_factor: None,
            reconnect_delay_max_ms: None,
            reconnect_jitter_ms: None,
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
            message_handler: None,
            heartbeat: None,
            heartbeat_msg: None,
            ping_handler: None,
            reconnect_timeout_ms: None,
            reconnect_delay_initial_ms: None,
            reconnect_backoff_factor: None,
            reconnect_delay_max_ms: None,
            reconnect_jitter_ms: None,
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
            "Send should return Timeout error, got: {:?}",
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
            "Send should succeed after rate limit + reconnection, got: {:?}",
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
}
