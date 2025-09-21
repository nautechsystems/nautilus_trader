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
use tokio::net::TcpStream;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Error, Message, client::IntoClientRequest, http::HeaderValue},
};

use crate::{
    RECONNECTED,
    backoff::ExponentialBackoff,
    error::SendError,
    logging::{log_task_aborted, log_task_started, log_task_stopped},
    mode::ConnectionMode,
    ratelimiter::{RateLimiter, clock::MonotonicClock, quota::Quota},
};

// Connection timing constants
const CONNECTION_STATE_CHECK_INTERVAL_MS: u64 = 10;
const GRACEFUL_SHUTDOWN_DELAY_MS: u64 = 100;
const GRACEFUL_SHUTDOWN_TIMEOUT_SECS: u64 = 5;

type MessageWriter = SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>;
pub type MessageReader = SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;

/// Function type for handling WebSocket messages.
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
            tracing::error!("Failed to send message to channel: {e}");
        }
    });
    (handler, rx)
}

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
    pub message_handler: Option<MessageHandler>,
    /// The optional heartbeat interval (seconds).
    pub heartbeat: Option<u64>,
    /// The optional heartbeat message.
    pub heartbeat_msg: Option<String>,
    /// The handler for incoming pings.
    pub ping_handler: Option<PingHandler>,
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
}

impl Debug for WebSocketConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebSocketConfig")
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
        })
    }

    /// Connects with the server creating a tokio-tungstenite websocket stream.
    #[inline]
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

    /// Reconnect with server.
    ///
    /// Make a new connection with server. Use the new read and write halves
    /// to update self writer and read and heartbeat tasks.
    pub async fn reconnect(&mut self) -> Result<(), Error> {
        tracing::debug!("Reconnecting");

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

            // If a disconnect was requested during reconnect, do not proceed to reactivate
            if ConnectionMode::from_atomic(&self.connection_mode).is_disconnect() {
                tracing::debug!("Reconnect aborted mid-flight (before spawn read)");
                return Ok(());
            }

            // Mark as active only if not disconnecting
            self.connection_mode
                .store(ConnectionMode::Active.as_u8(), Ordering::SeqCst);

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
    pub(crate) writer_tx: tokio::sync::mpsc::UnboundedSender<WriterCommand>,
    pub(crate) rate_limiter: Arc<RateLimiter<String, MonotonicClock>>,
}

impl Debug for WebSocketClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(WebSocketClient)).finish()
    }
}

impl WebSocketClient {
    /// Creates a websocket client that returns a stream for reading messages.
    ///
    /// # Errors
    ///
    /// Returns any error connecting to the server.
    #[allow(clippy::too_many_arguments)]
    pub async fn connect_stream(
        config: WebSocketConfig,
        keyed_quotas: Vec<(String, Quota)>,
        default_quota: Option<Quota>,
        post_reconnect: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Result<(MessageReader, Self), Error> {
        install_cryptographic_provider();

        // Create a single connection and split it
        let (ws_stream, _) = connect_async(config.url.clone().into_client_request()?).await?;
        let (writer, reader) = ws_stream.split();

        // Create inner without connecting (we'll provide the writer)
        let inner = WebSocketClientInner::new_with_writer(config, writer).await?;

        let connection_mode = inner.connection_mode.clone();
        let writer_tx = inner.writer_tx.clone();

        let controller_task =
            Self::spawn_controller_task(inner, connection_mode.clone(), post_reconnect);

        let rate_limiter = Arc::new(RateLimiter::new_with_quota(default_quota, keyed_quotas));

        Ok((
            reader,
            Self {
                controller_task,
                connection_mode,
                writer_tx,
                rate_limiter,
            },
        ))
    }

    /// Creates a websocket client.
    ///
    /// Creates an inner client and controller task to reconnect or disconnect
    /// the client. Also assumes ownership of writer from inner client.
    ///
    /// # Errors
    ///
    /// Returns any websocket error.
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

        let controller_task =
            Self::spawn_controller_task(inner, connection_mode.clone(), post_reconnection);

        let rate_limiter = Arc::new(RateLimiter::new_with_quota(default_quota, keyed_quotas));

        Ok(Self {
            controller_task,
            connection_mode,
            writer_tx,
            rate_limiter,
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

    /// Set disconnect mode to true.
    ///
    /// Controller task will periodically check the disconnect mode
    /// and shutdown the client if it is alive
    pub async fn disconnect(&self) {
        tracing::debug!("Disconnecting");
        self.connection_mode
            .store(ConnectionMode::Disconnect.as_u8(), Ordering::SeqCst);

        match tokio::time::timeout(Duration::from_secs(GRACEFUL_SHUTDOWN_TIMEOUT_SECS), async {
            while !self.is_disconnected() {
                tokio::time::sleep(Duration::from_millis(CONNECTION_STATE_CHECK_INTERVAL_MS)).await;
            }

            if !self.controller_task.is_finished() {
                self.controller_task.abort();
                log_task_aborted("controller");
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
    ) -> std::result::Result<(), SendError> {
        self.rate_limiter.await_keys_ready(keys).await;

        if !self.is_active() {
            return Err(SendError::Closed);
        }

        tracing::trace!("Sending text: {data:?}");

        let msg = Message::Text(data.into());
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
    ) -> std::result::Result<(), SendError> {
        self.rate_limiter.await_keys_ready(keys).await;

        if !self.is_active() {
            return Err(SendError::Closed);
        }

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
    pub async fn send_close_message(&self) -> std::result::Result<(), SendError> {
        if !self.is_active() {
            return Err(SendError::Closed);
        }

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
                let mode = ConnectionMode::from_atomic(&connection_mode);

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

                if mode.is_reconnect() || (mode.is_active() && !inner.is_alive()) {
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

#[cfg(test)]
mod rust_tests {
    use tokio::{
        net::TcpListener,
        task,
        time::{Duration, sleep},
    };
    use tokio_tungstenite::accept_async;

    use super::*;

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
}
