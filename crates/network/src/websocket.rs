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
use nautilus_cryptography::providers::install_cryptographic_provider;
use pyo3::{prelude::*, types::PyBytes};
use tokio::{
    net::TcpStream,
    sync::mpsc::{self, Receiver, Sender},
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Error, Message, client::IntoClientRequest, http::HeaderValue},
};

use crate::{
    backoff::ExponentialBackoff,
    mode::ConnectionMode,
    ratelimiter::{RateLimiter, clock::MonotonicClock, quota::Quota},
};
type MessageWriter = SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>;
pub type MessageReader = SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;

#[derive(Debug, Clone)]
pub enum Consumer {
    Python(Option<Arc<PyObject>>),
    Rust(Sender<Message>),
}

impl Consumer {
    #[must_use]
    pub fn rust_consumer() -> (Self, Receiver<Message>) {
        let (tx, rx) = mpsc::channel(100);
        (Self::Rust(tx), rx)
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.network")
)]
pub struct WebSocketConfig {
    /// The URL to connect to.
    pub url: String,
    /// The default headers.
    pub headers: Vec<(String, String)>,
    /// The Python function to handle incoming messages.
    pub handler: Consumer,
    /// The optional heartbeat interval (seconds).
    pub heartbeat: Option<u64>,
    /// The optional heartbeat message.
    pub heartbeat_msg: Option<String>,
    /// The handler for incoming pings.
    pub ping_handler: Option<Arc<PyObject>>,
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
    /// Create an inner websocket client.
    pub async fn connect_url(config: WebSocketConfig) -> Result<Self, Error> {
        install_cryptographic_provider();

        #[allow(unused_variables)]
        let WebSocketConfig {
            url,
            handler,
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

        let read_task = match &handler {
            Consumer::Python(handler) => handler.as_ref().map(|handler| {
                Self::spawn_python_callback_task(
                    connection_mode.clone(),
                    reader,
                    handler.clone(),
                    ping_handler.clone(),
                )
            }),
            Consumer::Rust(sender) => Some(Self::spawn_rust_streaming_task(
                connection_mode.clone(),
                reader,
                sender.clone(),
            )),
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
        );

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

        tokio::time::timeout(self.reconnect_timeout, async {
            let (new_writer, reader) =
                Self::connect_with_server(&self.config.url, self.config.headers.clone()).await?;

            if let Err(e) = self.writer_tx.send(WriterCommand::Update(new_writer)) {
                tracing::error!("{e}");
            }

            // Delay before closing connection
            tokio::time::sleep(Duration::from_millis(100)).await;

            if let Some(ref read_task) = self.read_task.take() {
                if !read_task.is_finished() {
                    read_task.abort();
                    tracing::debug!("Aborted task 'read'");
                }
            }

            self.connection_mode
                .store(ConnectionMode::Active.as_u8(), Ordering::SeqCst);

            self.read_task = match &self.config.handler {
                Consumer::Python(handler) => handler.as_ref().map(|handler| {
                    Self::spawn_python_callback_task(
                        self.connection_mode.clone(),
                        reader,
                        handler.clone(),
                        self.config.ping_handler.clone(),
                    )
                }),
                Consumer::Rust(sender) => Some(Self::spawn_rust_streaming_task(
                    self.connection_mode.clone(),
                    reader,
                    sender.clone(),
                )),
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

    fn spawn_rust_streaming_task(
        connection_state: Arc<AtomicU8>,
        mut reader: MessageReader,
        sender: Sender<Message>,
    ) -> tokio::task::JoinHandle<()> {
        tracing::debug!("Started streaming task 'read'");

        let check_interval = Duration::from_millis(10);

        tokio::task::spawn(async move {
            loop {
                if !ConnectionMode::from_atomic(&connection_state).is_active() {
                    break;
                }

                match tokio::time::timeout(check_interval, reader.next()).await {
                    Ok(Some(Ok(message))) => {
                        if let Err(e) = sender.send(message).await {
                            tracing::error!("Failed to send message: {e}");
                        }
                    }
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

    fn spawn_python_callback_task(
        connection_state: Arc<AtomicU8>,
        mut reader: MessageReader,
        handler: Arc<PyObject>,
        ping_handler: Option<Arc<PyObject>>,
    ) -> tokio::task::JoinHandle<()> {
        tracing::debug!("Started task 'read'");

        // Interval between checking the connection mode
        let check_interval = Duration::from_millis(10);

        tokio::task::spawn(async move {
            loop {
                if !ConnectionMode::from_atomic(&connection_state).is_active() {
                    break;
                }

                match tokio::time::timeout(check_interval, reader.next()).await {
                    Ok(Some(Ok(Message::Binary(data)))) => {
                        tracing::trace!("Received message <binary> {} bytes", data.len());
                        if let Err(e) =
                            Python::with_gil(|py| handler.call1(py, (PyBytes::new(py, &data),)))
                        {
                            tracing::error!("Error calling handler: {e}");
                            break;
                        }
                        continue;
                    }
                    Ok(Some(Ok(Message::Text(data)))) => {
                        tracing::trace!("Received message: {data}");
                        if let Err(e) = Python::with_gil(|py| {
                            handler.call1(py, (PyBytes::new(py, data.as_bytes()),))
                        }) {
                            tracing::error!("Error calling handler: {e}");
                            break;
                        }
                        continue;
                    }
                    Ok(Some(Ok(Message::Ping(ping)))) => {
                        tracing::trace!("Received ping: {ping:?}");
                        if let Some(ref handler) = ping_handler {
                            if let Err(e) =
                                Python::with_gil(|py| handler.call1(py, (PyBytes::new(py, &ping),)))
                            {
                                tracing::error!("Error calling handler: {e}");
                                break;
                            }
                        }
                        continue;
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
                    // Internally tungstenite considers the connection closed when polling
                    // for the next message in the stream returns None.
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
        tracing::debug!("Started task 'write'");

        // Interval between checking the connection mode
        let check_interval = Duration::from_millis(10);

        tokio::task::spawn(async move {
            let mut active_writer = writer;

            loop {
                match ConnectionMode::from_atomic(&connection_state) {
                    ConnectionMode::Disconnect => {
                        // Attempt to close the writer gracefully before exiting,
                        // we ignore any error as the writer may already be closed.
                        _ = active_writer.close().await;
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
                                _ = active_writer.close().await;

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
            _ = active_writer.close().await;

            tracing::debug!("Completed task 'write'");
        })
    }

    fn spawn_heartbeat_task(
        connection_state: Arc<AtomicU8>,
        heartbeat_secs: u64,
        message: Option<String>,
        writer_tx: tokio::sync::mpsc::UnboundedSender<WriterCommand>,
    ) -> tokio::task::JoinHandle<()> {
        tracing::debug!("Started task 'heartbeat'");

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

            tracing::debug!("Completed task 'heartbeat'");
        })
    }
}

impl Drop for WebSocketClientInner {
    fn drop(&mut self) {
        if let Some(ref read_task) = self.read_task.take() {
            if !read_task.is_finished() {
                read_task.abort();
                tracing::debug!("Aborted task 'read'");
            }
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

/// WebSocket client with automatic reconnection.
///
/// Handles connection state, Python callbacks, and rate limiting.
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
        let (ws_stream, _) = connect_async(config.url.clone().into_client_request()?).await?;
        let (writer, reader) = ws_stream.split();
        let inner = WebSocketClientInner::connect_url(config).await?;

        let connection_mode = inner.connection_mode.clone();
        let rate_limiter = Arc::new(RateLimiter::new_with_quota(default_quota, keyed_quotas));
        let writer_tx = inner.writer_tx.clone();
        if let Err(e) = writer_tx.send(WriterCommand::Update(writer)) {
            tracing::error!("{e}");
        }

        let controller_task = Self::spawn_controller_task(
            inner,
            connection_mode.clone(),
            post_reconnect,
            None, // no post_reconnection
            None, // no post_disconnection
        );

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
        post_connection: Option<PyObject>,
        post_reconnection: Option<PyObject>,
        post_disconnection: Option<PyObject>,
        keyed_quotas: Vec<(String, Quota)>,
        default_quota: Option<Quota>,
    ) -> Result<Self, Error> {
        tracing::debug!("Connecting");
        let inner = WebSocketClientInner::connect_url(config.clone()).await?;
        let connection_mode = inner.connection_mode.clone();
        let writer_tx = inner.writer_tx.clone();

        let controller_task = Self::spawn_controller_task(
            inner,
            connection_mode.clone(),
            None,               // Rust handler
            post_reconnection,  // TODO: Deprecated
            post_disconnection, // TODO: Deprecated
        );
        let rate_limiter = Arc::new(RateLimiter::new_with_quota(default_quota, keyed_quotas));

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

        match tokio::time::timeout(Duration::from_secs(5), async {
            while !self.is_disconnected() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }

            if !self.controller_task.is_finished() {
                self.controller_task.abort();
                tracing::debug!("Aborted task 'controller'");
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
    pub async fn send_text(&self, data: String, keys: Option<Vec<String>>) {
        self.rate_limiter.await_keys_ready(keys).await;

        if !self.is_active() {
            tracing::error!("Cannot send data - connection not active");
            return;
        }

        tracing::trace!("Sending text: {data:?}");

        let msg = Message::Text(data.into());
        if let Err(e) = self.writer_tx.send(WriterCommand::Send(msg)) {
            tracing::error!("Error sending message: {e}");
        }
    }

    /// Sends the given bytes `data` to the server.
    pub async fn send_bytes(&self, data: Vec<u8>, keys: Option<Vec<String>>) {
        self.rate_limiter.await_keys_ready(keys).await;

        if !self.is_active() {
            tracing::error!("Cannot send data - connection not active");
            return;
        }

        tracing::trace!("Sending bytes: {data:?}");

        let msg = Message::Binary(data.into());
        if let Err(e) = self.writer_tx.send(WriterCommand::Send(msg)) {
            tracing::error!("Error sending message: {e}");
        }
    }

    /// Sends a close message to the server.
    pub async fn send_close_message(&self) {
        if !self.is_active() {
            tracing::error!("Cannot send close message - connection not active");
            return;
        }

        let msg = Message::Close(None);
        if let Err(e) = self.writer_tx.send(WriterCommand::Send(msg)) {
            tracing::error!("Error sending close message: {e}");
        }
    }

    fn spawn_controller_task(
        mut inner: WebSocketClientInner,
        connection_mode: Arc<AtomicU8>,
        post_reconnection: Option<Arc<dyn Fn() + Send + Sync>>,
        py_post_reconnection: Option<PyObject>, // TODO: Deprecated
        py_post_disconnection: Option<PyObject>, // TODO: Deprecated
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

                        if let Some(task) = &inner.read_task {
                            if !task.is_finished() {
                                task.abort();
                                tracing::debug!("Aborted task 'read'");
                            }
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

                    if let Some(ref handler) = py_post_disconnection {
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

                            if let Some(ref callback) = post_reconnection {
                                callback();
                            }

                            // TODO: Python based websocket handlers deprecated (will be removed)
                            if let Some(ref handler) = py_post_reconnection {
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
        websocket::{Consumer, WebSocketClient, WebSocketConfig},
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
            handler: Consumer::Python(None),
            heartbeat: None,
            heartbeat_msg: None,
            ping_handler: None,
            reconnect_timeout_ms: None,
            reconnect_delay_initial_ms: None,
            reconnect_backoff_factor: None,
            reconnect_delay_max_ms: None,
            reconnect_jitter_ms: None,
        };
        WebSocketClient::connect(config, None, None, None, vec![], None)
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
            handler: Consumer::Python(None),
            heartbeat: None,
            heartbeat_msg: None,
            ping_handler: None,
            reconnect_timeout_ms: None,
            reconnect_delay_initial_ms: None,
            reconnect_backoff_factor: None,
            reconnect_delay_max_ms: None,
            reconnect_jitter_ms: None,
        };
        let res = WebSocketClient::connect(config, None, None, None, vec![], None).await;
        assert!(res.is_err(), "Should fail quickly with no server");
    }

    #[tokio::test]
    async fn test_websocket_forced_close_reconnect() {
        let server = TestServer::setup().await;
        let client = setup_test_client(server.port).await;

        // 1) Send normal message
        client.send_text("Hello".into(), None).await;

        // 2) Trigger forced close from server
        client.send_text("close-now".into(), None).await;

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
            handler: Consumer::Python(None),
            heartbeat: None,
            heartbeat_msg: None,
            ping_handler: None,
            reconnect_timeout_ms: None,
            reconnect_delay_initial_ms: None,
            reconnect_backoff_factor: None,
            reconnect_delay_max_ms: None,
            reconnect_jitter_ms: None,
        };

        let client = WebSocketClient::connect(
            config,
            None,
            None,
            None,
            vec![("default".into(), quota)],
            None,
        )
        .await
        .unwrap();

        // First 2 should succeed
        client.send_text("test1".into(), None).await;
        client.send_text("test2".into(), None).await;

        // Third should error
        client.send_text("test3".into(), None).await;

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
                client.send_text(format!("test{i}"), None).await;
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
