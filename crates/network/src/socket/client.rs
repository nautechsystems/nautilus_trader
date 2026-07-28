// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
//!
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
//! - Event-driven state notification via `Notify` for immediate wakeup on transitions.

use std::{
    collections::VecDeque,
    fmt::Debug,
    path::Path,
    pin::pin,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::Duration,
};

use bytes::Bytes;
use nautilus_core::CleanDrop;
use nautilus_cryptography::providers::install_cryptographic_provider;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_tungstenite::tungstenite::{Error, client::IntoClientRequest, stream::Mode};

use super::{SocketConfig, TcpMessageHandler, TcpReader, TcpWriter, WriterCommand};
use crate::{
    backoff::{ExponentialBackoff, RECONNECT_STABILITY_THRESHOLD, wait_reconnect_delay},
    dst,
    error::{SendError, is_connection_drop_io_error},
    logging::{log_task_aborted, log_task_started, log_task_stopped},
    mode::{ConnectionMode, ReadSessionFence},
    net::TcpStream,
    tls::{Connector, create_tls_config_from_certs_dir, tcp_tls},
};

// Connection timing constants
const CONNECTION_STATE_CHECK_INTERVAL_MS: u64 = 10;
const GRACEFUL_SHUTDOWN_DELAY_MS: u64 = 100;
const GRACEFUL_SHUTDOWN_TIMEOUT_SECS: u64 = 5;
const WRITE_TIMEOUT_SECS: u64 = 5;

// Maximum buffer size for read operations (10 MB)
const MAX_READ_BUFFER_BYTES: usize = 10 * 1024 * 1024;

pub(crate) struct SocketTerminalFinalizer {
    connection_mode: Arc<AtomicU8>,
    post_disconnection: Option<Arc<dyn Fn() + Send + Sync>>,
    notification_started: AtomicBool,
    completion: Arc<SocketFinalizationState>,
}

struct SocketFinalizationState {
    notification_completed: AtomicBool,
    completion_notify: tokio::sync::Notify,
    state_notify: Arc<tokio::sync::Notify>,
}

struct SocketFinalizationCompletion {
    state: Arc<SocketFinalizationState>,
}

impl Drop for SocketFinalizationCompletion {
    fn drop(&mut self) {
        self.state
            .notification_completed
            .store(true, Ordering::Release);
        self.state.completion_notify.notify_waiters();
        self.state.state_notify.notify_waiters();
    }
}

impl SocketTerminalFinalizer {
    pub(crate) fn new(
        connection_mode: Arc<AtomicU8>,
        state_notify: Arc<tokio::sync::Notify>,
        post_disconnection: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Self {
        Self {
            connection_mode,
            post_disconnection,
            notification_started: AtomicBool::new(false),
            completion: Arc::new(SocketFinalizationState {
                notification_completed: AtomicBool::new(false),
                completion_notify: tokio::sync::Notify::new(),
                state_notify,
            }),
        }
    }

    pub(crate) fn transition_and_finalize(&self) {
        self.connection_mode
            .store(ConnectionMode::Closed.as_u8(), Ordering::SeqCst);
        self.finalize_closed();
    }

    fn finalize_closed(&self) {
        if self
            .notification_started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            if let Some(handler) = self.post_disconnection.clone() {
                let completion = SocketFinalizationCompletion {
                    state: Arc::clone(&self.completion),
                };
                let job = move || {
                    let _completion = completion;
                    handler();
                    log::debug!("Called `post_disconnection` handler");
                };

                if let Ok(handle) = tokio::runtime::Handle::try_current() {
                    handle.spawn_blocking(job);
                } else {
                    let _ = std::thread::Builder::new() // dst-ok: no runtime context
                        .name("socket-terminal-finalizer".to_string())
                        .spawn(job);
                }
            } else {
                drop(SocketFinalizationCompletion {
                    state: Arc::clone(&self.completion),
                });
            }
        }
    }

    pub(crate) async fn wait_for_completion(&self) {
        loop {
            let mut notified = pin!(self.completion.completion_notify.notified());
            notified.as_mut().enable();

            if self
                .completion
                .notification_completed
                .load(Ordering::Acquire)
            {
                return;
            }
            notified.await;
        }
    }
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
    read_fence: ReadSessionFence,
    write_task: tokio::task::JoinHandle<()>,
    writer_tx: tokio::sync::mpsc::UnboundedSender<WriterCommand>,
    heartbeat_task: Option<tokio::task::JoinHandle<()>>,
    connection_mode: Arc<AtomicU8>,
    state_notify: Arc<tokio::sync::Notify>,
    reconnect_timeout: Duration,
    backoff: ExponentialBackoff,
    handler: Option<TcpMessageHandler>,
    reconnect_max_attempts: Option<u32>,
    reconnect_attempt_count: u32,
}

impl SocketClientInner {
    /// Connect to a URL with the specified configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if connection fails or configuration is invalid.
    pub(crate) async fn connect_url(config: SocketConfig) -> anyhow::Result<Self> {
        const CONNECTION_TIMEOUT_SECS: u64 = 10;

        install_cryptographic_provider();

        // Validate suffix is non-empty to prevent panic in read loop (windows(0) panics)
        if config.suffix.is_empty() {
            anyhow::bail!("Socket suffix cannot be empty: suffix is required for message framing");
        }

        if let Some((interval_secs, _)) = &config.heartbeat
            && *interval_secs == 0
        {
            anyhow::bail!("Heartbeat interval cannot be zero");
        }

        if config.idle_timeout_ms == Some(0) {
            anyhow::bail!("Idle timeout cannot be zero");
        }

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
            connection_max_retries,
            reconnect_max_attempts,
            idle_timeout_ms,
            certs_dir,
        } = &config.clone();
        let reconnect_timeout = Duration::from_millis(reconnect_timeout_ms.unwrap_or(10_000));
        let reconnect_backoff = ExponentialBackoff::new(
            Duration::from_millis(reconnect_delay_initial_ms.unwrap_or(2_000)),
            Duration::from_millis(reconnect_delay_max_ms.unwrap_or(30_000)),
            reconnect_backoff_factor.unwrap_or(1.5),
            reconnect_jitter_ms.unwrap_or(100),
            true, // immediate-first
        )?;
        let connector = if let Some(dir) = certs_dir {
            let config = create_tls_config_from_certs_dir(Path::new(dir), false)?;
            Some(Connector::Rustls(Arc::new(config)))
        } else {
            None
        };

        // Retry initial connection with exponential backoff to handle transient DNS/network issues
        let max_retries = connection_max_retries.unwrap_or(5);

        let mut backoff = ExponentialBackoff::new(
            Duration::from_millis(500),
            Duration::from_secs(5),
            2.0,
            250,
            false,
        )?;

        #[allow(unused_assignments)]
        let mut last_error = String::new();
        let mut attempt = 0;
        let (reader, writer) = loop {
            attempt += 1;

            match dst::time::timeout(
                Duration::from_secs(CONNECTION_TIMEOUT_SECS),
                Self::tls_connect_with_server(url, *mode, connector.clone()),
            )
            .await
            {
                Ok(Ok(result)) => {
                    if attempt > 1 {
                        log::info!("Socket connection established after {attempt} attempts");
                    }
                    break result;
                }
                Ok(Err(e)) => {
                    last_error = e.to_string();
                    log::warn!(
                        "Socket connection attempt {attempt}/{max_retries} to {url} failed: {last_error}"
                    );
                }
                Err(_) => {
                    last_error = format!(
                        "Connection timeout after {CONNECTION_TIMEOUT_SECS}s (possible DNS resolution failure)"
                    );
                    log::warn!(
                        "Socket connection attempt {attempt}/{max_retries} to {url} timed out"
                    );
                }
            }

            if attempt >= max_retries {
                anyhow::bail!(
                    "Failed to connect to {} after {} attempts: {}. \
                    If this is a DNS error, check your network configuration and DNS settings.",
                    url,
                    max_retries,
                    if last_error.is_empty() {
                        "unknown error"
                    } else {
                        &last_error
                    }
                );
            }

            let delay = backoff.next_duration();
            log::debug!(
                "Retrying in {delay:?} (attempt {}/{})",
                attempt + 1,
                max_retries
            );
            dst::time::sleep(delay).await;
        };

        log::debug!("Connected");

        let connection_mode = Arc::new(AtomicU8::new(ConnectionMode::Active.as_u8()));
        let state_notify = Arc::new(tokio::sync::Notify::new());
        let read_fence = ReadSessionFence::new();

        let read_task = Arc::new(Self::spawn_read_task(
            connection_mode.clone(),
            read_fence.clone(),
            reader,
            message_handler.clone(),
            suffix.clone(),
            *idle_timeout_ms,
        ));

        let (writer_tx, writer_rx) = tokio::sync::mpsc::unbounded_channel::<WriterCommand>();

        let write_task = Self::spawn_write_task(
            connection_mode.clone(),
            state_notify.clone(),
            writer,
            writer_rx,
            suffix.clone(),
        );

        // Optionally spawn a heartbeat task to periodically ping server
        let heartbeat_task = heartbeat.as_ref().map(|heartbeat| {
            Self::spawn_heartbeat_task(
                connection_mode.clone(),
                heartbeat.clone(),
                writer_tx.clone(),
            )
        });

        Ok(Self {
            config,
            connector,
            read_task,
            read_fence,
            write_task,
            writer_tx,
            heartbeat_task,
            connection_mode,
            state_notify,
            reconnect_timeout,
            backoff: reconnect_backoff,
            handler: message_handler.clone(),
            reconnect_max_attempts: *reconnect_max_attempts,
            reconnect_attempt_count: 0,
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
    pub(crate) async fn tls_connect_with_server(
        url: &str,
        mode: Mode,
        connector: Option<Connector>,
    ) -> Result<(TcpReader, TcpWriter), Error> {
        log::debug!("Connecting to {url}");

        let (socket_addr, request_url) = Self::parse_socket_url(url, mode)?;
        let tcp_result = TcpStream::connect(&socket_addr).await;

        match tcp_result {
            Ok(stream) => {
                log::debug!("TCP connection established to {socket_addr}, proceeding with TLS");

                if let Err(e) = stream.set_nodelay(true) {
                    log::warn!("Failed to enable TCP_NODELAY for socket client: {e:?}");
                }
                let request = request_url.into_client_request()?;
                tcp_tls(&request, mode, stream, connector)
                    .await
                    .map(tokio::io::split)
            }
            Err(e) => {
                log::warn!("TCP connection failed to {socket_addr}: {e:?}");
                Err(Error::Io(e))
            }
        }
    }

    /// Reconnect with server.
    ///
    /// Makes a new connection with server, uses the new read and write halves
    /// to update the reader and writer.
    ///
    /// The reconnect timeout bounds only connection establishment. Once the
    /// new writer is handed to the writer task the swap runs to completion,
    /// so buffered messages can never drain into a connection that lost its
    /// reader to a timeout; the writer task bounds both the old-writer
    /// shutdown and the buffer drain with its graceful-shutdown timeout.
    async fn reconnect(&mut self) -> Result<(), Error> {
        log::debug!("Reconnecting");

        if ConnectionMode::from_atomic(&self.connection_mode).is_disconnect() {
            log::debug!("Reconnect aborted due to disconnect state");
            return Ok(());
        }

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
            connection_max_retries: _,
            reconnect_max_attempts: _,
            idle_timeout_ms,
            certs_dir: _,
        } = &self.config;
        // Create a fresh connection
        let connector = self.connector.clone();

        // Bound only connection establishment; the swap below must run to completion
        let (reader, new_writer) = dst::time::timeout(
            self.reconnect_timeout,
            Self::tls_connect_with_server(url, *mode, connector),
        )
        .await
        .map_err(|_| {
            Error::Io(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!(
                    "reconnection timed out after {}s",
                    self.reconnect_timeout.as_secs_f64()
                ),
            ))
        })??;

        if ConnectionMode::from_atomic(&self.connection_mode).is_disconnect() {
            log::debug!("Reconnect aborted mid-flight (after connect)");
            return Ok(());
        }
        log::debug!("Connected");

        // Use a oneshot channel to synchronize with the writer task.
        // We must verify that the buffer was successfully drained before transitioning to ACTIVE
        // to prevent silent message loss if the new connection drops immediately.
        let (tx, rx) = tokio::sync::oneshot::channel();
        if let Err(e) = self.writer_tx.send(WriterCommand::Update(new_writer, tx)) {
            log::error!("{e}");
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                format!("Failed to send update command: {e}"),
            )));
        }

        // Wait for writer to confirm it has drained the buffer
        match rx.await {
            Ok(true) => log::debug!("Writer confirmed buffer drain success"),
            Ok(false) => {
                log::warn!("Writer failed to drain buffer, aborting reconnect");
                // Return error to trigger retry logic in controller
                return Err(Error::Io(std::io::Error::other(
                    "Failed to drain reconnection buffer",
                )));
            }
            Err(e) => {
                log::error!("Writer dropped update channel: {e}");
                return Err(Error::Io(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "Writer task dropped response channel",
                )));
            }
        }

        // Delay before closing connection
        dst::time::sleep(Duration::from_millis(GRACEFUL_SHUTDOWN_DELAY_MS)).await;

        if ConnectionMode::from_atomic(&self.connection_mode).is_disconnect() {
            log::debug!("Reconnect aborted mid-flight (after delay)");
            return Ok(());
        }

        self.read_fence.invalidate();

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
            log::debug!("Reconnect aborted (state changed during reconnect)");
            return Ok(());
        }

        // Spawn new read task
        self.read_fence = ReadSessionFence::new();
        self.read_task = Arc::new(Self::spawn_read_task(
            self.connection_mode.clone(),
            self.read_fence.clone(),
            reader,
            self.handler.clone(),
            suffix.clone(),
            *idle_timeout_ms,
        ));

        log::debug!("Reconnect succeeded");
        Ok(())
    }

    /// Check if the client is still alive.
    ///
    /// Returns `true` if both the read and write tasks are still running.
    /// There may be some delay between the connection closing and the
    /// client detecting it.
    #[inline]
    #[must_use]
    pub(crate) fn is_alive(&self) -> bool {
        !self.read_task.is_finished() && !self.write_task.is_finished()
    }

    #[must_use]
    fn spawn_read_task<R>(
        connection_state: Arc<AtomicU8>,
        read_fence: ReadSessionFence,
        reader: R,
        handler: Option<TcpMessageHandler>,
        suffix: Vec<u8>,
        idle_timeout_ms: Option<u64>,
    ) -> tokio::task::JoinHandle<()>
    where
        R: AsyncRead + Unpin + Send + 'static,
    {
        log_task_started("read");

        // Interval between checking the connection mode
        let check_interval = Duration::from_millis(CONNECTION_STATE_CHECK_INTERVAL_MS);
        let idle_timeout = idle_timeout_ms.map(Duration::from_millis);

        tokio::task::spawn(Self::run_read_loop(
            connection_state,
            read_fence,
            reader,
            handler,
            suffix,
            idle_timeout,
            check_interval,
        ))
    }

    async fn run_read_loop<R>(
        connection_state: Arc<AtomicU8>,
        read_fence: ReadSessionFence,
        mut reader: R,
        handler: Option<TcpMessageHandler>,
        suffix: Vec<u8>,
        idle_timeout: Option<Duration>,
        check_interval: Duration,
    ) where
        R: AsyncRead + Unpin,
    {
        let mut buf = Vec::new();
        let mut last_data_time = dst::time::Instant::now();

        'read: loop {
            if !ConnectionMode::from_atomic(&connection_state).is_active() || !read_fence.is_valid()
            {
                if !buf.is_empty() {
                    log::debug!(
                        "Dropping {} buffered bytes after socket session ended",
                        buf.len()
                    );
                    buf.clear();
                }
                break;
            }

            match dst::time::timeout(check_interval, reader.read_buf(&mut buf)).await {
                // Connection has been terminated or vector buffer is complete
                Ok(Ok(0)) => {
                    log::debug!("Connection closed by server");
                    break;
                }
                Ok(Err(e)) => {
                    log::debug!("Connection ended: {e}");
                    break;
                }
                // Received bytes of data
                Ok(Ok(bytes)) => {
                    log::trace!("Received <binary> {bytes} bytes");
                    last_data_time = dst::time::Instant::now();

                    if !ConnectionMode::from_atomic(&connection_state).is_active()
                        || !read_fence.is_valid()
                    {
                        log::debug!(
                            "Dropping {} buffered bytes after socket session ended",
                            buf.len()
                        );
                        buf.clear();
                        break;
                    }

                    while let Some((i, _)) = &buf
                        .windows(suffix.len())
                        .enumerate()
                        .find(|(_, pair)| pair.eq(&suffix))
                    {
                        let mut data: Vec<u8> = buf.drain(0..i + suffix.len()).collect();
                        data.truncate(data.len() - suffix.len());

                        if let Some(ref handler) = handler {
                            if !ConnectionMode::from_atomic(&connection_state).is_active()
                                || !read_fence.is_valid()
                            {
                                log::debug!(
                                    "Dropping {} buffered bytes after socket session ended",
                                    data.len() + buf.len()
                                );
                                buf.clear();
                                break 'read;
                            }
                            handler(&data);
                        }
                    }

                    if buf.len() > MAX_READ_BUFFER_BYTES {
                        log::error!(
                            "Read buffer exceeded maximum size ({MAX_READ_BUFFER_BYTES} bytes), closing connection"
                        );
                        break;
                    }
                }
                Err(_) => {
                    if let Some(timeout) = idle_timeout {
                        let idle_duration = last_data_time.elapsed();
                        if idle_duration >= timeout {
                            log::warn!(
                                "Read idle timeout: no data received for {:.1}s",
                                idle_duration.as_secs_f64()
                            );
                            break;
                        }
                    }
                }
            }
        }

        log_task_stopped("read");
    }

    /// Drains buffered messages after reconnection completes.
    ///
    /// Attempts to send all buffered messages that were queued during reconnection.
    /// Uses a peek-and-pop pattern to preserve messages if sending fails midway through the buffer.
    ///
    /// # Returns
    ///
    /// Returns `true` if a send error occurred (buffer may still contain unsent messages),
    /// `false` if all messages were sent successfully (buffer is empty).
    async fn drain_reconnect_buffer<W>(
        buffer: &mut VecDeque<Bytes>,
        writer: &mut W,
        suffix: &[u8],
    ) -> bool
    where
        W: AsyncWrite + Unpin,
    {
        if buffer.is_empty() {
            return false;
        }

        let initial_buffer_len = buffer.len();
        log::info!("Sending {initial_buffer_len} buffered messages after reconnection");

        let mut send_error_occurred = false;

        while let Some(buffered_msg) = buffer.front() {
            let mut combined_msg = Vec::with_capacity(buffered_msg.len() + suffix.len());
            combined_msg.extend_from_slice(buffered_msg);
            combined_msg.extend_from_slice(suffix);

            if let Err(e) = writer.write_all(&combined_msg).await {
                if is_connection_drop_io_error(&e) {
                    log::warn!(
                        "Failed to send buffered message with suffix after reconnection: {e}, {} messages remain in buffer",
                        buffer.len()
                    );
                } else {
                    log::error!(
                        "Failed to send buffered message with suffix after reconnection: {e}, {} messages remain in buffer",
                        buffer.len()
                    );
                }
                send_error_occurred = true;
                break;
            }

            buffer.pop_front();
        }

        if buffer.is_empty() {
            log::info!("Successfully sent all {initial_buffer_len} buffered messages");
        }

        send_error_occurred
    }

    fn spawn_write_task<W>(
        connection_state: Arc<AtomicU8>,
        state_notify: Arc<tokio::sync::Notify>,
        writer: W,
        mut writer_rx: tokio::sync::mpsc::UnboundedReceiver<WriterCommand<W>>,
        suffix: Vec<u8>,
    ) -> tokio::task::JoinHandle<()>
    where
        W: AsyncWrite + Unpin + Send + 'static,
    {
        log_task_started("write");

        // Interval between checking the connection mode
        let check_interval = Duration::from_millis(CONNECTION_STATE_CHECK_INTERVAL_MS);

        tokio::task::spawn(async move {
            let mut active_writer = writer;
            let mut reconnect_buffer: VecDeque<Bytes> = VecDeque::new();
            let mut write_buf: Vec<u8> = Vec::new();

            loop {
                if matches!(
                    ConnectionMode::from_atomic(&connection_state),
                    ConnectionMode::Disconnect | ConnectionMode::Closed
                ) {
                    break;
                }

                match dst::time::timeout(check_interval, writer_rx.recv()).await {
                    Ok(Some(msg)) => {
                        // Re-check connection mode after receiving a message
                        let mode = ConnectionMode::from_atomic(&connection_state);
                        if matches!(mode, ConnectionMode::Disconnect | ConnectionMode::Closed) {
                            break;
                        }

                        match msg {
                            WriterCommand::Update(new_writer, tx) => {
                                log::debug!("Received new writer");

                                // Delay before closing connection
                                dst::time::sleep(Duration::from_millis(100)).await;

                                // Attempt to shutdown the writer gracefully before updating,
                                // we ignore any error as the writer may already be closed.
                                _ = dst::time::timeout(
                                    Duration::from_secs(GRACEFUL_SHUTDOWN_TIMEOUT_SECS),
                                    active_writer.shutdown(),
                                )
                                .await;

                                active_writer = new_writer;
                                log::debug!("Updated writer");

                                // Bound the drain: a peer that accepts the connection but stops
                                // reading must not wedge the writer task.
                                let drain_result = dst::time::timeout(
                                    Duration::from_secs(GRACEFUL_SHUTDOWN_TIMEOUT_SECS),
                                    Self::drain_reconnect_buffer(
                                        &mut reconnect_buffer,
                                        &mut active_writer,
                                        &suffix,
                                    ),
                                )
                                .await;
                                let send_error = drain_result.unwrap_or_else(|_| {
                                    log::warn!(
                                        "Timed out draining reconnect buffer, {} messages remain",
                                        reconnect_buffer.len()
                                    );
                                    true
                                });

                                if let Err(e) = tx.send(!send_error) {
                                    log::error!(
                                        "Failed to report drain status to controller: {e:?}"
                                    );
                                }
                            }
                            _ if mode.is_reconnect() => {
                                if let WriterCommand::Send(data) = msg {
                                    log::debug!(
                                        "Buffering message while reconnecting ({} bytes)",
                                        data.len()
                                    );
                                    reconnect_buffer.push_back(data);
                                }
                            }
                            WriterCommand::Send(msg) => {
                                write_buf.clear();
                                write_buf.extend_from_slice(&msg);
                                write_buf.extend_from_slice(&suffix);

                                let write_result = dst::time::timeout(
                                    Duration::from_secs(WRITE_TIMEOUT_SECS),
                                    active_writer.write_all(&write_buf),
                                )
                                .await;
                                let write_failed = match write_result {
                                    Ok(Ok(())) => false,
                                    Ok(Err(e)) => {
                                        if is_connection_drop_io_error(&e) {
                                            log::warn!("Failed to send message: {e}");
                                        } else {
                                            log::error!("Failed to send message: {e}");
                                        }
                                        true
                                    }
                                    Err(_) => {
                                        log::warn!(
                                            "Timed out sending message after {WRITE_TIMEOUT_SECS}s"
                                        );
                                        true
                                    }
                                };

                                if write_failed {
                                    reconnect_buffer.push_back(msg);

                                    // CAS: a disconnect landing mid-write must not be overwritten
                                    if ConnectionMode::request_reconnect(&connection_state) {
                                        log::warn!("Writer triggering reconnect");
                                        state_notify.notify_one();
                                    }
                                }
                            }
                        }
                    }
                    Ok(None) => {
                        // Channel closed - writer task should terminate
                        log::debug!("Writer channel closed, terminating writer task");
                        break;
                    }
                    Err(_) => {
                        // Timeout - just continue the loop
                    }
                }
            }

            // Attempt to shutdown the writer gracefully before exiting,
            // we ignore any error as the writer may already be closed.
            _ = dst::time::timeout(
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
                dst::time::sleep(interval).await;

                match ConnectionMode::from_u8(connection_state.load(Ordering::SeqCst)) {
                    ConnectionMode::Active => {
                        let msg = WriterCommand::Send(message.clone().into());

                        match writer_tx.send(msg) {
                            Ok(()) => log::trace!("Sent heartbeat to writer task"),
                            Err(e) => {
                                log::error!("Failed to send heartbeat to writer task: {e}");
                            }
                        }
                    }
                    ConnectionMode::Reconnect => {}
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
        self.read_fence.invalidate();

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
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.network")
)]
pub struct SocketClient {
    pub(crate) controller_task: tokio::task::JoinHandle<()>,
    pub(crate) connection_mode: Arc<AtomicU8>,
    pub(crate) state_notify: Arc<tokio::sync::Notify>,
    pub(crate) reconnect_timeout: Duration,
    pub writer_tx: tokio::sync::mpsc::UnboundedSender<WriterCommand>,
    pub(crate) terminal_finalizer: Arc<SocketTerminalFinalizer>,
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
        let state_notify = inner.state_notify.clone();
        let reconnect_timeout = inner.reconnect_timeout;
        let terminal_finalizer = Arc::new(SocketTerminalFinalizer::new(
            connection_mode.clone(),
            state_notify.clone(),
            post_disconnection,
        ));

        let controller_task = Self::spawn_controller_task(
            inner,
            connection_mode.clone(),
            state_notify.clone(),
            post_reconnection,
            Arc::clone(&terminal_finalizer),
        );

        if let Some(handler) = post_connection {
            handler();
            log::debug!("Called `post_connection` handler");
        }

        Ok(Self {
            controller_task,
            connection_mode,
            state_notify,
            reconnect_timeout,
            writer_tx,
            terminal_finalizer,
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
        self.close_with_timeout(Duration::from_secs(GRACEFUL_SHUTDOWN_TIMEOUT_SECS))
            .await;
    }

    async fn close_with_timeout(&self, finalization_timeout: Duration) {
        // Preserve a CLOSED terminal state; the controller has already exited
        ConnectionMode::request_disconnect(&self.connection_mode);
        self.state_notify.notify_waiters();

        if dst::time::timeout(
            finalization_timeout,
            self.terminal_finalizer.wait_for_completion(),
        )
        .await
            == Ok(())
        {
            if !self.controller_task.is_finished() {
                self.controller_task.abort();
                log_task_aborted("controller");
            }
            log_task_stopped("controller");
        } else {
            log::warn!("Timeout waiting for controller task to finish");

            if !self.controller_task.is_finished() {
                self.controller_task.abort();
                log_task_aborted("controller");
            }
            self.terminal_finalizer.transition_and_finalize();
        }
    }

    /// Checks whether the connection is in a terminal state (disconnecting or closed).
    ///
    /// Single atomic load to fail fast before waiting.
    #[inline]
    fn check_not_terminal(&self) -> Result<(), SendError> {
        match self.connection_mode() {
            ConnectionMode::Disconnect | ConnectionMode::Closed => Err(SendError::Closed),
            _ => Ok(()),
        }
    }

    /// Waits for the client to become active before sending.
    ///
    /// Uses `state_notify` for event-driven wakeup so sends resume immediately
    /// after reconnection completes. A fallback interval guards against missed
    /// notifications.
    async fn wait_for_active(&self) -> Result<(), SendError> {
        const FALLBACK_INTERVAL_MS: u64 = 100;

        let mode = self.connection_mode();
        if mode.is_active() {
            return Ok(());
        }

        if matches!(mode, ConnectionMode::Disconnect | ConnectionMode::Closed) {
            return Err(SendError::Closed);
        }

        log::debug!("Waiting for client to become ACTIVE before sending...");

        let fallback_interval = Duration::from_millis(FALLBACK_INTERVAL_MS);

        dst::time::timeout(self.reconnect_timeout, async {
            loop {
                // Enable before the state check: an unpolled Notified is unregistered and misses notifies
                let mut notified = pin!(self.state_notify.notified());
                notified.as_mut().enable();

                let mode = self.connection_mode();
                if mode.is_active() {
                    return Ok(());
                }

                if matches!(mode, ConnectionMode::Disconnect | ConnectionMode::Closed) {
                    return Err(());
                }

                tokio::select! {
                    biased;
                    () = notified => {}
                    () = dst::time::sleep(fallback_interval) => {}
                }
            }
        })
        .await
        .map_err(|_| SendError::Timeout)?
        .map_err(|()| SendError::Closed)
    }

    /// Sends a message of the given `data`.
    ///
    /// Returns `Ok(())` when the message is enqueued to the writer channel. This does NOT
    /// guarantee delivery: if a disconnect occurs concurrently, the writer task may drop the
    /// message. During reconnection, messages are buffered and replayed on the new connection.
    ///
    /// # Errors
    ///
    /// Returns an error if sending fails.
    pub async fn send_bytes(&self, data: Vec<u8>) -> Result<(), SendError> {
        self.check_not_terminal()?;
        self.wait_for_active().await?;

        let msg = WriterCommand::Send(data.into());
        self.writer_tx
            .send(msg)
            .map_err(|e| SendError::BrokenPipe(e.to_string()))
    }

    fn spawn_controller_task(
        mut inner: SocketClientInner,
        connection_mode: Arc<AtomicU8>,
        state_notify: Arc<tokio::sync::Notify>,
        post_reconnection: Option<Arc<dyn Fn() + Send + Sync>>,
        terminal_finalizer: Arc<SocketTerminalFinalizer>,
    ) -> tokio::task::JoinHandle<()> {
        const CONTROLLER_FALLBACK_INTERVAL_MS: u64 = 100;

        tokio::task::spawn(async move {
            log_task_started("controller");

            let fallback_interval = Duration::from_millis(CONTROLLER_FALLBACK_INTERVAL_MS);
            let mut reconnected_at = None;

            loop {
                tokio::select! {
                    biased;
                    () = state_notify.notified() => {}
                    () = dst::time::sleep(fallback_interval) => {}
                }

                let mut mode = ConnectionMode::from_atomic(&connection_mode);

                if mode.is_disconnect() {
                    log::debug!("Disconnecting");

                    let timeout = Duration::from_secs(GRACEFUL_SHUTDOWN_TIMEOUT_SECS);
                    if dst::time::timeout(timeout, async {
                        // Delay awaiting graceful shutdown
                        dst::time::sleep(Duration::from_millis(GRACEFUL_SHUTDOWN_DELAY_MS)).await;

                        inner.read_fence.invalidate();
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
                        log::warn!("Shutdown timed out after {}s", timeout.as_secs());
                    }

                    log::debug!("Closed");
                    terminal_finalizer.transition_and_finalize();
                    break; // Controller finished
                }

                if mode.is_closed() {
                    log::debug!("Connection closed");

                    inner.read_fence.invalidate();
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

                    terminal_finalizer.finalize_closed();
                    break;
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
                        log::debug!("Detected dead read task, transitioning to RECONNECT");
                    }
                    mode = ConnectionMode::from_atomic(&connection_mode);
                }

                if mode.is_reconnect() {
                    let reconnect_uptime = reconnected_at
                        .take()
                        .map(|started: dst::time::Instant| started.elapsed());
                    let previous_reconnect_stable = reconnect_uptime
                        .is_some_and(|uptime| uptime >= RECONNECT_STABILITY_THRESHOLD);

                    if previous_reconnect_stable {
                        inner.backoff.reset();
                        inner.reconnect_attempt_count = 0;
                        log::debug!(
                            "Socket remained active for at least {}s, resetting reconnect cycle",
                            RECONNECT_STABILITY_THRESHOLD.as_secs()
                        );
                    }

                    // Check max reconnection attempts before attempting reconnect
                    if let Some(max_attempts) = inner.reconnect_max_attempts
                        && inner.reconnect_attempt_count >= max_attempts
                    {
                        log::error!(
                            "Max reconnection attempts ({max_attempts}) exceeded, transitioning to CLOSED"
                        );

                        if connection_mode
                            .compare_exchange(
                                ConnectionMode::Reconnect.as_u8(),
                                ConnectionMode::Closed.as_u8(),
                                Ordering::SeqCst,
                                Ordering::SeqCst,
                            )
                            .is_ok()
                        {
                            terminal_finalizer.finalize_closed();
                            break;
                        }
                        continue;
                    }

                    if reconnect_uptime.is_some() && !previous_reconnect_stable {
                        let duration = inner.backoff.next_duration();
                        if !duration.is_zero() {
                            log::warn!("Backing off for {}s...", duration.as_secs_f64());
                        }

                        if !wait_reconnect_delay(
                            duration,
                            connection_mode.as_ref(),
                            state_notify.as_ref(),
                        )
                        .await
                        {
                            log::debug!("Backoff interrupted by terminal state");
                            continue;
                        }
                    }

                    inner.reconnect_attempt_count += 1;

                    // Race reconnect against disconnect notification
                    let reconnect_result = tokio::select! {
                        biased;
                        result = inner.reconnect() => Some(result),
                        () = async {
                            loop {
                                // Enable before the check so a disconnect notify between iterations is not missed
                                let mut notified = pin!(state_notify.notified());
                                notified.as_mut().enable();

                                if ConnectionMode::from_atomic(&connection_mode).is_disconnect() {
                                    break;
                                }
                                notified.await;
                            }
                        } => None,
                    };

                    match reconnect_result {
                        None => {
                            log::debug!("Reconnect interrupted by disconnect");
                        }
                        Some(Ok(())) => {
                            log::debug!("Reconnected successfully");
                            reconnected_at = Some(dst::time::Instant::now());

                            state_notify.notify_waiters();

                            if ConnectionMode::from_atomic(&connection_mode).is_active() {
                                if let Some(ref handler) = post_reconnection {
                                    handler();
                                    log::debug!("Called `post_reconnection` handler");
                                }
                            } else {
                                log::debug!(
                                    "Skipping post_reconnection handlers due to disconnect state"
                                );
                            }
                        }
                        Some(Err(e)) => {
                            let duration = inner.backoff.next_duration();
                            log::warn!(
                                "Reconnect attempt {} failed: {e}",
                                inner.reconnect_attempt_count
                            );

                            if !duration.is_zero() {
                                log::warn!("Backing off for {}s...", duration.as_secs_f64());
                                if !wait_reconnect_delay(
                                    duration,
                                    connection_mode.as_ref(),
                                    state_notify.as_ref(),
                                )
                                .await
                                {
                                    log::debug!("Backoff interrupted by terminal state");
                                }
                            }
                        }
                    }
                }
            }
            terminal_finalizer.finalize_closed();

            log_task_stopped("controller");
        })
    }
}

// Dropping cancels background work without reporting a terminal lifecycle transition.
impl Drop for SocketClient {
    fn drop(&mut self) {
        if !self.controller_task.is_finished() {
            self.controller_task.abort();
            log_task_aborted("controller");
        }
    }
}

#[cfg(test)]
#[cfg(feature = "python")]
#[cfg(not(feature = "turmoil"))]
#[cfg(not(all(feature = "simulation", madsim)))] // transport-layer I/O not simulated
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
                    while let Some(idx) = buf.array_windows().position(|w| w == b"\r\n") {
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
            reconnect_max_attempts: None,
            connection_max_retries: None,
            idle_timeout_ms: None,
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

        // Wait until port is truly unavailable (OS has released it)
        wait_until_async(
            || async {
                TcpStream::connect(format!("127.0.0.1:{port}"))
                    .await
                    .is_err()
            },
            Duration::from_secs(2),
        )
        .await;

        let config = SocketConfig {
            url: format!("127.0.0.1:{port}"),
            mode: Mode::Plain,
            suffix: b"\r\n".to_vec(),
            message_handler: None,
            heartbeat: None,
            reconnect_timeout_ms: Some(100),
            reconnect_delay_initial_ms: Some(50),
            reconnect_backoff_factor: Some(1.0),
            reconnect_delay_max_ms: Some(50),
            reconnect_jitter_ms: Some(0),
            connection_max_retries: Some(1),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
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
            reconnect_max_attempts: None,
            connection_max_retries: None,
            idle_timeout_ms: None,
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
    async fn test_close_after_closed_returns_fast_and_preserves_state() {
        Python::initialize();

        let (port, listener) = bind_test_server().await;

        let server_task = task::spawn(async move {
            // Accept the first connection then drop it; never accept again so
            // the client exhausts its reconnect attempts and transitions to CLOSED
            let (socket, _) = listener.accept().await.unwrap();
            drop(socket);
            drop(listener);
            sleep(Duration::from_secs(5)).await;
        });

        let config = SocketConfig {
            url: format!("127.0.0.1:{port}"),
            mode: Mode::Plain,
            suffix: b"\r\n".to_vec(),
            message_handler: None,
            heartbeat: None,
            reconnect_timeout_ms: Some(200),
            reconnect_delay_initial_ms: Some(50),
            reconnect_backoff_factor: Some(1.0),
            reconnect_delay_max_ms: Some(50),
            reconnect_jitter_ms: Some(0),
            connection_max_retries: None,
            reconnect_max_attempts: Some(1),
            idle_timeout_ms: None,
            certs_dir: None,
        };

        let client = SocketClient::connect(config, None, None, None)
            .await
            .unwrap();

        wait_until_async(|| async { client.is_closed() }, Duration::from_secs(5)).await;

        // Closing an already CLOSED client must return promptly (no 5s spin
        // waiting for a controller that has already exited) and must not
        // regress the terminal state to DISCONNECT
        let start = std::time::Instant::now();
        client.close().await;
        let elapsed = start.elapsed();

        assert!(client.is_closed(), "Client should remain CLOSED");
        assert!(
            !client.is_disconnecting(),
            "Closed client should not report DISCONNECT after close()"
        );
        assert!(
            elapsed < Duration::from_secs(2),
            "close() on a closed client should return fast, took {elapsed:?}"
        );

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
                        while let Some(idx) = buf.array_windows().position(|w| w == b"\r\n") {
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
            reconnect_max_attempts: None,
            connection_max_retries: None,
            idle_timeout_ms: None,
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
            reconnect_max_attempts: None,
            connection_max_retries: None,
            idle_timeout_ms: None,
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
#[cfg(not(all(feature = "simulation", madsim)))] // transport-layer I/O not simulated
mod rust_tests {
    use std::{
        pin::Pin,
        sync::{
            Arc, Condvar, Mutex as StdMutex,
            atomic::{AtomicUsize, Ordering as AtomicOrdering},
        },
        task::{Context, Poll, Waker},
    };

    use nautilus_common::testing::wait_until_async;
    use rstest::rstest;
    use tokio::{
        io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream, ReadBuf},
        net::TcpListener,
        sync::oneshot,
        task::{self, yield_now},
        time::{Duration, sleep},
    };

    use super::*;

    const TEST_TIMEOUT: Duration = Duration::from_secs(10);

    struct CondvarReleaseGuard {
        release: Arc<(StdMutex<bool>, Condvar)>,
    }

    impl CondvarReleaseGuard {
        fn new(release: Arc<(StdMutex<bool>, Condvar)>) -> Self {
            Self { release }
        }

        fn release(&self) {
            let (lock, condvar) = self.release.as_ref();
            let mut released = lock
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *released = true;
            condvar.notify_all();
        }
    }

    impl Drop for CondvarReleaseGuard {
        fn drop(&mut self) {
            self.release();
        }
    }

    async fn recv_rendezvous<T: Send + 'static>(
        receiver: std::sync::mpsc::Receiver<T>,
        name: &'static str,
    ) -> T {
        let receive_task = tokio::task::spawn_blocking(move || receiver.recv_timeout(TEST_TIMEOUT));

        match tokio::time::timeout(TEST_TIMEOUT * 2, receive_task).await {
            Ok(Ok(Ok(value))) => value,
            Ok(Ok(Err(e))) => {
                panic!("{name} did not arrive within the test timeout: {e}")
            }
            Ok(Err(e)) => panic!("{name} receive task failed: {e}"),
            Err(e) => panic!("{name} receive task did not finish: {e}"),
        }
    }

    async fn await_task_termination(
        task: tokio::task::JoinHandle<()>,
        name: &'static str,
        cancellation_ok: bool,
    ) {
        match tokio::time::timeout(TEST_TIMEOUT, task).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) if cancellation_ok && e.is_cancelled() => {}
            Ok(Err(e)) => panic!("{name} failed: {e}"),
            Err(e) => panic!("{name} did not terminate within the test timeout: {e}"),
        }
    }

    async fn wait_for_finalizer(finalizer: &SocketTerminalFinalizer, name: &'static str) {
        tokio::time::timeout(TEST_TIMEOUT, finalizer.wait_for_completion())
            .await
            .unwrap_or_else(|_| panic!("{name} completion was not published"));
    }

    struct ScriptedReader {
        first: Option<Vec<u8>>,
        remainder: Arc<StdMutex<Option<Vec<u8>>>>,
        pending_tx: Option<oneshot::Sender<()>>,
        waker: Arc<StdMutex<Option<Waker>>>,
    }

    impl AsyncRead for ScriptedReader {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            if let Some(first) = self.first.take() {
                buf.put_slice(&first);
                return Poll::Ready(Ok(()));
            }

            if let Some(remainder) = self.remainder.lock().unwrap().take() {
                buf.put_slice(&remainder);
                return Poll::Ready(Ok(()));
            }

            if let Some(tx) = self.pending_tx.take() {
                let _ = tx.send(());
            }
            *self.waker.lock().unwrap() = Some(cx.waker().clone());
            Poll::Pending
        }
    }

    struct BackpressuredWriter {
        stream: DuplexStream,
        pending_tx: Option<oneshot::Sender<()>>,
    }

    impl AsyncWrite for BackpressuredWriter {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            let result = Pin::new(&mut self.stream).poll_write(cx, buf);
            if result.is_pending()
                && let Some(tx) = self.pending_tx.take()
            {
                let _ = tx.send(());
            }
            result
        }

        fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Pin::new(&mut self.stream).poll_flush(cx)
        }

        fn poll_shutdown(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<std::io::Result<()>> {
            Pin::new(&mut self.stream).poll_shutdown(cx)
        }
    }

    struct RecordingWriter {
        bytes: Arc<StdMutex<Vec<u8>>>,
    }

    impl AsyncWrite for RecordingWriter {
        fn poll_write(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            self.bytes.lock().unwrap().extend_from_slice(buf);
            Poll::Ready(Ok(buf.len()))
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    fn test_socket_client(
        connection_state: Arc<AtomicU8>,
        state_notify: Arc<tokio::sync::Notify>,
        terminal_finalizer: Arc<SocketTerminalFinalizer>,
        controller_task: tokio::task::JoinHandle<()>,
    ) -> SocketClient {
        let (writer_tx, _writer_rx) = tokio::sync::mpsc::unbounded_channel();

        SocketClient {
            controller_task,
            connection_mode: connection_state,
            state_notify,
            reconnect_timeout: Duration::from_secs(1),
            writer_tx,
            terminal_finalizer,
        }
    }

    fn wait_for_finalizer_sync(finalizer: &SocketTerminalFinalizer) {
        let deadline = std::time::Instant::now() + TEST_TIMEOUT * 2;

        while !finalizer
            .completion
            .notification_completed
            .load(Ordering::Acquire)
        {
            assert!(
                std::time::Instant::now() < deadline,
                "terminal callback did not complete"
            );
            std::thread::yield_now();
        }
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn test_close_finalizes_after_controller_was_aborted() {
        let connection_state = Arc::new(AtomicU8::new(ConnectionMode::Active.as_u8()));
        let state_notify = Arc::new(tokio::sync::Notify::new());
        let callback_count = Arc::new(AtomicUsize::new(0));
        let callback_count_clone = Arc::clone(&callback_count);
        let terminal_finalizer = Arc::new(SocketTerminalFinalizer::new(
            Arc::clone(&connection_state),
            Arc::clone(&state_notify),
            Some(Arc::new(move || {
                callback_count_clone.fetch_add(1, AtomicOrdering::SeqCst);
            })),
        ));

        let controller_task = tokio::spawn(std::future::pending::<()>());
        controller_task.abort();
        let client = test_socket_client(
            connection_state,
            state_notify,
            terminal_finalizer,
            controller_task,
        );

        client.close_with_timeout(Duration::from_millis(1)).await;
        wait_for_finalizer_sync(&client.terminal_finalizer);

        assert!(client.is_closed());
        assert_eq!(callback_count.load(AtomicOrdering::SeqCst), 1);
    }

    #[rstest]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_close_does_not_wait_for_aborted_controller_to_finish() {
        let connection_state = Arc::new(AtomicU8::new(ConnectionMode::Active.as_u8()));
        let state_notify = Arc::new(tokio::sync::Notify::new());
        let callback_release = Arc::new((StdMutex::new(false), Condvar::new()));
        let callback_release_guard = CondvarReleaseGuard::new(Arc::clone(&callback_release));
        let callback_release_clone = Arc::clone(&callback_release);
        let (callback_entered_tx, callback_entered_rx) = std::sync::mpsc::channel();
        let terminal_finalizer = Arc::new(SocketTerminalFinalizer::new(
            Arc::clone(&connection_state),
            Arc::clone(&state_notify),
            Some(Arc::new(move || {
                callback_entered_tx.send(()).unwrap();
                let (lock, condvar) = callback_release_clone.as_ref();
                let mut released = lock.lock().unwrap();

                while !*released {
                    released = condvar.wait(released).unwrap();
                }
            })),
        ));
        let controller_finalizer = Arc::clone(&terminal_finalizer);
        let controller_task = tokio::spawn(async move {
            controller_finalizer.transition_and_finalize();
        });
        recv_rendezvous(
            callback_entered_rx,
            "controller-claimed terminal callback entry",
        )
        .await;
        let client = test_socket_client(
            connection_state,
            state_notify,
            terminal_finalizer,
            controller_task,
        );

        let close_result = tokio::time::timeout(
            TEST_TIMEOUT,
            client.close_with_timeout(Duration::from_millis(1)),
        )
        .await;

        callback_release_guard.release();

        assert!(
            close_result.is_ok(),
            "close should return after aborting a controller blocked in a callback"
        );
    }

    #[rstest]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_close_dispatches_unclaimed_blocking_callback() {
        let connection_state = Arc::new(AtomicU8::new(ConnectionMode::Active.as_u8()));
        let state_notify = Arc::new(tokio::sync::Notify::new());
        let callback_count = Arc::new(AtomicUsize::new(0));
        let callback_count_clone = Arc::clone(&callback_count);
        let callback_release = Arc::new((StdMutex::new(false), Condvar::new()));
        let callback_release_guard = CondvarReleaseGuard::new(Arc::clone(&callback_release));
        let callback_release_clone = Arc::clone(&callback_release);
        let (callback_entered_tx, callback_entered_rx) = std::sync::mpsc::channel();
        let terminal_finalizer = Arc::new(SocketTerminalFinalizer::new(
            Arc::clone(&connection_state),
            Arc::clone(&state_notify),
            Some(Arc::new(move || {
                callback_count_clone.fetch_add(1, AtomicOrdering::SeqCst);
                callback_entered_tx.send(()).unwrap();
                let (lock, condvar) = callback_release_clone.as_ref();
                let mut released = lock.lock().unwrap();

                while !*released {
                    released = condvar.wait(released).unwrap();
                }
            })),
        ));

        let controller_task = tokio::spawn(std::future::pending::<()>());
        let client = Arc::new(test_socket_client(
            connection_state,
            state_notify,
            Arc::clone(&terminal_finalizer),
            controller_task,
        ));
        let close_client = Arc::clone(&client);
        let (close_finished_tx, close_finished_rx) = std::sync::mpsc::channel();

        let close_task = tokio::spawn(async move {
            close_client
                .close_with_timeout(Duration::from_millis(1))
                .await;
            let _ = close_finished_tx.send(());
        });

        recv_rendezvous(callback_entered_rx, "close-claimed terminal callback entry").await;
        recv_rendezvous(
            close_finished_rx,
            "close task completion while terminal callback was blocked",
        )
        .await;
        let completion_while_blocked = terminal_finalizer
            .completion
            .notification_completed
            .load(Ordering::Acquire);

        callback_release_guard.release();

        await_task_termination(close_task, "close task", false).await;
        wait_for_finalizer(&terminal_finalizer, "close-claimed terminal callback").await;

        assert!(!completion_while_blocked);
        assert_eq!(callback_count.load(AtomicOrdering::SeqCst), 1);

        tokio::time::timeout(
            TEST_TIMEOUT,
            client.close_with_timeout(Duration::from_millis(1)),
        )
        .await
        .expect("second close did not return within the test timeout");
        terminal_finalizer.transition_and_finalize();
        assert_eq!(callback_count.load(AtomicOrdering::SeqCst), 1);
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn test_panicking_terminal_callback_still_completes_notification() {
        let connection_state = Arc::new(AtomicU8::new(ConnectionMode::Closed.as_u8()));
        let state_notify = Arc::new(tokio::sync::Notify::new());
        let finalizer = SocketTerminalFinalizer::new(
            connection_state,
            state_notify,
            Some(Arc::new(|| panic!("terminal callback panic"))),
        );

        finalizer.finalize_closed();
        wait_for_finalizer_sync(&finalizer);
        assert!(
            finalizer
                .completion
                .notification_completed
                .load(Ordering::Acquire)
        );
    }

    #[rstest]
    fn test_concurrent_terminal_finalizers_notify_once() {
        let connection_state = Arc::new(AtomicU8::new(ConnectionMode::Closed.as_u8()));
        let state_notify = Arc::new(tokio::sync::Notify::new());
        let callback_count = Arc::new(AtomicUsize::new(0));
        let callback_count_clone = Arc::clone(&callback_count);
        let (callback_entered_tx, callback_entered_rx) = std::sync::mpsc::channel();
        let (callback_release_tx, callback_release_rx) = std::sync::mpsc::channel();
        let callback_release_rx = Arc::new(StdMutex::new(callback_release_rx));
        let callback_release_rx_clone = Arc::clone(&callback_release_rx);
        let finalizer = Arc::new(SocketTerminalFinalizer::new(
            connection_state,
            state_notify,
            Some(Arc::new(move || {
                callback_entered_tx.send(()).unwrap();
                callback_release_rx_clone
                    .lock()
                    .unwrap()
                    .recv_timeout(TEST_TIMEOUT)
                    .expect("terminal callback release did not arrive within the test timeout");
                callback_count_clone.fetch_add(1, AtomicOrdering::SeqCst);
            })),
        ));
        let first_finalizer = Arc::clone(&finalizer);
        let first = std::thread::spawn(move || first_finalizer.finalize_closed());
        callback_entered_rx
            .recv_timeout(TEST_TIMEOUT)
            .expect("terminal callback entry did not arrive within the test timeout");
        let second_finalizer = Arc::clone(&finalizer);
        let second = std::thread::spawn(move || second_finalizer.finalize_closed());

        second.join().unwrap();
        callback_release_tx.send(()).unwrap();
        first.join().unwrap();
        wait_for_finalizer_sync(&finalizer);

        assert_eq!(callback_count.load(AtomicOrdering::SeqCst), 1);
        assert!(
            finalizer
                .completion
                .notification_completed
                .load(Ordering::Acquire)
        );
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn test_terminal_finalizer_cas_interleavings_notify_once(#[case] exhaustion_wins: bool) {
        let connection_state = Arc::new(AtomicU8::new(ConnectionMode::Reconnect.as_u8()));
        let state_notify = Arc::new(tokio::sync::Notify::new());
        let callback_count = Arc::new(AtomicUsize::new(0));
        let callback_count_clone = Arc::clone(&callback_count);
        let callback_state = Arc::clone(&connection_state);
        let finalizer = SocketTerminalFinalizer::new(
            Arc::clone(&connection_state),
            state_notify,
            Some(Arc::new(move || {
                assert_eq!(
                    ConnectionMode::from_atomic(&callback_state),
                    ConnectionMode::Closed
                );
                callback_count_clone.fetch_add(1, AtomicOrdering::SeqCst);
            })),
        );

        if exhaustion_wins {
            assert!(
                connection_state
                    .compare_exchange(
                        ConnectionMode::Reconnect.as_u8(),
                        ConnectionMode::Closed.as_u8(),
                        Ordering::SeqCst,
                        Ordering::SeqCst,
                    )
                    .is_ok()
            );
            finalizer.finalize_closed();
            assert!(!ConnectionMode::request_disconnect(&connection_state));
        } else {
            assert!(ConnectionMode::request_disconnect(&connection_state));
            assert!(
                connection_state
                    .compare_exchange(
                        ConnectionMode::Reconnect.as_u8(),
                        ConnectionMode::Closed.as_u8(),
                        Ordering::SeqCst,
                        Ordering::SeqCst,
                    )
                    .is_err()
            );
            finalizer.transition_and_finalize();
        }

        finalizer.transition_and_finalize();
        wait_for_finalizer_sync(&finalizer);
        assert_eq!(
            ConnectionMode::from_atomic(&connection_state),
            ConnectionMode::Closed
        );
        assert_eq!(callback_count.load(AtomicOrdering::SeqCst), 1);
        assert!(
            finalizer
                .completion
                .notification_completed
                .load(Ordering::Acquire)
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_reconnect_exhaustion_notifies_disconnection_once() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (accepted_tx, accepted_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();

        let server = task::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let _ = accepted_tx.send(());
            let _ = release_rx.await;
            drop(socket);
            drop(listener);
        });

        let config = SocketConfig {
            url: format!("127.0.0.1:{port}"),
            mode: Mode::Plain,
            suffix: b"\r\n".to_vec(),
            message_handler: None,
            heartbeat: None,
            reconnect_timeout_ms: Some(100),
            reconnect_delay_initial_ms: Some(1),
            reconnect_delay_max_ms: Some(1),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            connection_max_retries: Some(1),
            reconnect_max_attempts: Some(1),
            idle_timeout_ms: None,
            certs_dir: None,
        };

        let callback_count = Arc::new(AtomicUsize::new(0));
        let callback_count_clone = Arc::clone(&callback_count);
        let (callback_tx, callback_rx) = oneshot::channel();
        let callback_tx = Arc::new(StdMutex::new(Some(callback_tx)));
        let post_disconnection = Arc::new(move || {
            callback_count_clone.fetch_add(1, AtomicOrdering::SeqCst);

            if let Some(tx) = callback_tx.lock().unwrap().take() {
                let _ = tx.send(());
            }
        });

        let client = SocketClient::connect(config, None, None, Some(post_disconnection))
            .await
            .unwrap();
        accepted_rx.await.unwrap();
        release_tx.send(()).unwrap();

        tokio::time::timeout(Duration::from_secs(5), callback_rx)
            .await
            .expect("reconnect exhaustion should invoke post_disconnection")
            .unwrap();
        assert!(client.is_closed());
        assert_eq!(callback_count.load(AtomicOrdering::SeqCst), 1);

        client.close().await;
        assert_eq!(callback_count.load(AtomicOrdering::SeqCst), 1);
        server.await.unwrap();
    }

    #[rstest]
    #[tokio::test]
    async fn test_graceful_close_notifies_disconnection_once() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (accepted_tx, accepted_rx) = oneshot::channel();

        let server = task::spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            let _ = accepted_tx.send(());
            std::future::pending::<()>().await;
        });
        let config = SocketConfig {
            url: format!("127.0.0.1:{port}"),
            mode: Mode::Plain,
            suffix: b"\r\n".to_vec(),
            message_handler: None,
            heartbeat: None,
            reconnect_timeout_ms: None,
            reconnect_delay_initial_ms: None,
            reconnect_delay_max_ms: None,
            reconnect_backoff_factor: None,
            reconnect_jitter_ms: None,
            connection_max_retries: None,
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            certs_dir: None,
        };
        let callback_count = Arc::new(AtomicUsize::new(0));
        let callback_count_clone = Arc::clone(&callback_count);
        let post_disconnection = Arc::new(move || {
            callback_count_clone.fetch_add(1, AtomicOrdering::SeqCst);
        });
        let client = SocketClient::connect(config, None, None, Some(post_disconnection))
            .await
            .unwrap();
        accepted_rx.await.unwrap();

        client.close().await;
        client.close().await;

        assert!(client.is_closed());
        assert_eq!(callback_count.load(AtomicOrdering::SeqCst), 1);
        server.abort();
    }

    #[rstest]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_read_loop_drops_remaining_frames_after_session_replaced() {
        let connection_state = Arc::new(AtomicU8::new(ConnectionMode::Active.as_u8()));
        let read_fence = ReadSessionFence::new();
        let received = Arc::new(StdMutex::new(Vec::<Vec<u8>>::new()));
        let received_clone = Arc::clone(&received);
        let handler_release = Arc::new((StdMutex::new(false), Condvar::new()));
        let handler_release_guard = CondvarReleaseGuard::new(Arc::clone(&handler_release));
        let handler_release_clone = Arc::clone(&handler_release);
        let (handler_entered_tx, handler_entered_rx) = std::sync::mpsc::channel();
        let handler: TcpMessageHandler = Arc::new(move |data| {
            received_clone.lock().unwrap().push(data.to_vec());
            handler_entered_tx.send(()).unwrap();
            let (lock, condvar) = handler_release_clone.as_ref();
            let mut released = lock.lock().unwrap();

            while !*released {
                released = condvar.wait(released).unwrap();
            }
        });
        let reader = ScriptedReader {
            first: Some(b"first\r\nsecond\r\n".to_vec()),
            remainder: Arc::new(StdMutex::new(None)),
            pending_tx: None,
            waker: Arc::new(StdMutex::new(None)),
        };
        let read_task = SocketClientInner::spawn_read_task(
            Arc::clone(&connection_state),
            read_fence.clone(),
            reader,
            Some(handler),
            b"\r\n".to_vec(),
            None,
        );

        recv_rendezvous(handler_entered_rx, "socket first-handler entry").await;
        connection_state.store(ConnectionMode::Reconnect.as_u8(), Ordering::SeqCst);
        read_fence.invalidate();
        read_task.abort();
        connection_state.store(ConnectionMode::Active.as_u8(), Ordering::SeqCst);

        handler_release_guard.release();
        await_task_termination(read_task, "old socket read task", true).await;

        assert_eq!(received.lock().unwrap().as_slice(), &[b"first".to_vec()]);
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn test_read_loop_drops_partial_old_session_frame() {
        let connection_state = Arc::new(AtomicU8::new(ConnectionMode::Active.as_u8()));
        let old_read_fence = ReadSessionFence::new();
        let received = Arc::new(StdMutex::new(Vec::<Vec<u8>>::new()));
        let received_clone = Arc::clone(&received);
        let handler: TcpMessageHandler =
            Arc::new(move |data| received_clone.lock().unwrap().push(data.to_vec()));
        let (pending_tx, pending_rx) = oneshot::channel();
        let remainder = Arc::new(StdMutex::new(None));
        let waker = Arc::new(StdMutex::new(None));
        let reader = ScriptedReader {
            first: Some(b"old".to_vec()),
            remainder: Arc::clone(&remainder),
            pending_tx: Some(pending_tx),
            waker: Arc::clone(&waker),
        };

        let read_task = SocketClientInner::spawn_read_task(
            Arc::clone(&connection_state),
            old_read_fence.clone(),
            reader,
            Some(handler),
            b"\r\n".to_vec(),
            None,
        );

        pending_rx.await.unwrap();
        connection_state.store(ConnectionMode::Reconnect.as_u8(), Ordering::SeqCst);
        old_read_fence.invalidate();
        connection_state.store(ConnectionMode::Active.as_u8(), Ordering::SeqCst);
        *remainder.lock().unwrap() = Some(b"\r\nnew\r\n".to_vec());
        waker.lock().unwrap().take().unwrap().wake();
        read_task.await.unwrap();

        let fresh_reader = ScriptedReader {
            first: Some(b"new\r\n".to_vec()),
            remainder: Arc::new(StdMutex::new(None)),
            pending_tx: None,
            waker: Arc::new(StdMutex::new(None)),
        };
        let fresh_connection_state = Arc::clone(&connection_state);
        let fresh_received = Arc::clone(&received);
        let fresh_handler: TcpMessageHandler = Arc::new(move |data| {
            fresh_received.lock().unwrap().push(data.to_vec());
            fresh_connection_state.store(ConnectionMode::Reconnect.as_u8(), Ordering::SeqCst);
        });
        SocketClientInner::spawn_read_task(
            Arc::clone(&connection_state),
            ReadSessionFence::new(),
            fresh_reader,
            Some(fresh_handler),
            b"\r\n".to_vec(),
            None,
        )
        .await
        .unwrap();

        assert_eq!(received.lock().unwrap().as_slice(), &[b"new".to_vec()]);
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn test_read_loop_stops_when_first_handler_ends_session() {
        let connection_state = Arc::new(AtomicU8::new(ConnectionMode::Active.as_u8()));
        let handler_state = Arc::clone(&connection_state);
        let received = Arc::new(StdMutex::new(Vec::<Vec<u8>>::new()));
        let received_clone = Arc::clone(&received);
        let handler: TcpMessageHandler = Arc::new(move |data| {
            received_clone.lock().unwrap().push(data.to_vec());
            handler_state.store(ConnectionMode::Reconnect.as_u8(), Ordering::SeqCst);
        });
        let (pending_tx, _pending_rx) = oneshot::channel();
        let reader = ScriptedReader {
            first: Some(b"first\r\nsecond\r\n".to_vec()),
            remainder: Arc::new(StdMutex::new(None)),
            pending_tx: Some(pending_tx),
            waker: Arc::new(StdMutex::new(None)),
        };

        SocketClientInner::spawn_read_task(
            connection_state,
            ReadSessionFence::new(),
            reader,
            Some(handler),
            b"\r\n".to_vec(),
            None,
        )
        .await
        .unwrap();

        assert_eq!(received.lock().unwrap().as_slice(), &[b"first".to_vec()]);
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn test_stalled_socket_write_reconnects_and_replays_complete_message() {
        type TestWriter = Pin<Box<dyn AsyncWrite + Send>>;

        let connection_state = Arc::new(AtomicU8::new(ConnectionMode::Active.as_u8()));
        let state_notify = Arc::new(tokio::sync::Notify::new());
        let (stream, _non_reading_peer) = tokio::io::duplex(1);
        let (pending_tx, pending_rx) = oneshot::channel();
        let writer: TestWriter = Box::pin(BackpressuredWriter {
            stream,
            pending_tx: Some(pending_tx),
        });
        let (writer_tx, writer_rx) =
            tokio::sync::mpsc::unbounded_channel::<WriterCommand<TestWriter>>();
        let write_task = SocketClientInner::spawn_write_task(
            Arc::clone(&connection_state),
            Arc::clone(&state_notify),
            writer,
            writer_rx,
            b"\r\n".to_vec(),
        );

        writer_tx
            .send(WriterCommand::Send(Bytes::from_static(b"complete-message")))
            .unwrap();
        pending_rx.await.unwrap();

        let recorded = Arc::new(StdMutex::new(Vec::new()));
        let new_writer: TestWriter = Box::pin(RecordingWriter {
            bytes: Arc::clone(&recorded),
        });
        let (update_tx, update_rx) = oneshot::channel();
        writer_tx
            .send(WriterCommand::Update(new_writer, update_tx))
            .unwrap();

        tokio::time::advance(Duration::from_secs(GRACEFUL_SHUTDOWN_TIMEOUT_SECS)).await;
        yield_now().await;

        assert!(
            tokio::time::timeout(Duration::from_secs(10), update_rx)
                .await
                .expect("writer update should not remain queued behind a stalled write")
                .unwrap(),
            "writer should report successful replay"
        );
        assert_eq!(
            ConnectionMode::from_atomic(&connection_state),
            ConnectionMode::Reconnect
        );
        assert_eq!(recorded.lock().unwrap().as_slice(), b"complete-message\r\n");

        connection_state.store(ConnectionMode::Closed.as_u8(), Ordering::SeqCst);
        state_notify.notify_waiters();
        drop(writer_tx);
        write_task.await.unwrap();
    }

    #[rstest]
    #[tokio::test]
    async fn test_connect_url_rejects_invalid_reconnect_backoff_before_dial() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let config = SocketConfig {
            url: format!("127.0.0.1:{port}"),
            mode: Mode::Plain,
            suffix: b"\r\n".to_vec(),
            message_handler: None,
            heartbeat: None,
            reconnect_timeout_ms: Some(1_000),
            reconnect_delay_initial_ms: Some(50),
            reconnect_delay_max_ms: Some(100),
            reconnect_backoff_factor: Some(100.1),
            reconnect_jitter_ms: Some(0),
            connection_max_retries: Some(1),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            certs_dir: None,
        };

        let error = match SocketClientInner::connect_url(config).await {
            Ok(_) => panic!("invalid reconnect backoff should be rejected"),
            Err(e) => e,
        };

        assert!(
            error.to_string().contains("factor"),
            "error should mention the invalid factor, was: {error}"
        );
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock,
            "invalid reconnect backoff must be rejected before dialing"
        );
    }

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
            connection_max_retries: Some(1),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            certs_dir: None,
        };

        // Connect client (handler=None)
        let client = SocketClient::connect(config.clone(), None, None, None)
            .await
            .unwrap();

        // Wait for client to detect dropped connection and enter reconnect state
        wait_until_async(
            || async { client.is_reconnecting() },
            Duration::from_secs(2),
        )
        .await;

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
            connection_max_retries: Some(1),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            certs_dir: None,
        };

        let client = SocketClient::connect(config, None, None, None)
            .await
            .unwrap();

        wait_until_async(
            || async { client.is_reconnecting() },
            Duration::from_secs(2),
        )
        .await;

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
            connection_max_retries: Some(1),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
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
            connection_max_retries: Some(1),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
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
            connection_max_retries: Some(1),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
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
            connection_max_retries: Some(1),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
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
            sleep(Duration::from_mins(1)).await;
        });

        let config = SocketConfig {
            url: format!("127.0.0.1:{port}"),
            mode: Mode::Plain,
            suffix: b"\r\n".to_vec(),
            message_handler: None,
            heartbeat: None,
            reconnect_timeout_ms: Some(1_000), // 1s timeout for faster test
            reconnect_delay_initial_ms: Some(200), // Short backoff (but > timeout) to keep client in RECONNECT
            reconnect_delay_max_ms: Some(200),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            connection_max_retries: Some(1),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
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
            "Send should fail when client stuck in RECONNECT, was: {send_result:?}"
        );
        assert!(
            matches!(send_result, Err(crate::error::SendError::Timeout)),
            "Send should return Timeout error, was: {send_result:?}"
        );
        // Verify timeout respects configured value (1s), but don't check upper bound
        // as CI scheduler jitter can cause legitimate delays beyond the timeout
        assert!(
            elapsed >= Duration::from_millis(900),
            "Send should timeout after at least 1s (configured timeout), took {elapsed:?}"
        );

        client.close().await;
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_idle_timeout_triggers_reconnect() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        // Server accepts connection but sends nothing (simulates silent death)
        let server = task::spawn(async move {
            let (_sock1, _) = listener.accept().await.unwrap();
            // Hold connection open but send nothing, wait for reconnect attempt
            sleep(Duration::from_secs(5)).await;
        });

        let config = SocketConfig {
            url: format!("127.0.0.1:{port}"),
            mode: Mode::Plain,
            suffix: b"\r\n".to_vec(),
            message_handler: None,
            heartbeat: None,
            reconnect_timeout_ms: Some(2_000),
            reconnect_delay_initial_ms: Some(50),
            reconnect_delay_max_ms: Some(100),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            connection_max_retries: Some(1),
            reconnect_max_attempts: Some(1),
            idle_timeout_ms: Some(500),
            certs_dir: None,
        };

        let client = SocketClient::connect(config, None, None, None)
            .await
            .unwrap();

        assert!(client.is_active());

        // Wait for idle timeout to fire and client to enter reconnect
        wait_until_async(
            || async { client.is_reconnecting() || client.is_closed() },
            Duration::from_secs(3),
        )
        .await;

        assert!(
            !client.is_active(),
            "Client should not be active after idle timeout"
        );

        client.close().await;
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_idle_timeout_resets_on_data() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        // Server sends data every 200ms (well within the 1s idle timeout)
        let server = task::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            for _ in 0..10 {
                sleep(Duration::from_millis(200)).await;

                if sock.write_all(b"ping\r\n").await.is_err() {
                    break;
                }
            }
        });

        let config = SocketConfig {
            url: format!("127.0.0.1:{port}"),
            mode: Mode::Plain,
            suffix: b"\r\n".to_vec(),
            message_handler: None,
            heartbeat: None,
            reconnect_timeout_ms: Some(2_000),
            reconnect_delay_initial_ms: Some(50),
            reconnect_delay_max_ms: Some(100),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            connection_max_retries: Some(1),
            reconnect_max_attempts: Some(1),
            idle_timeout_ms: Some(1_000),
            certs_dir: None,
        };

        let client = SocketClient::connect(config, None, None, None)
            .await
            .unwrap();

        assert!(client.is_active());

        // Wait 1.5s - data arrives every 200ms so idle timeout (1s) should NOT fire
        sleep(Duration::from_millis(1_500)).await;

        assert!(
            client.is_active(),
            "Client should remain active when data is flowing"
        );

        client.close().await;
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_close_during_backoff_exits_promptly() {
        // Verify that close() interrupts backoff sleep (Finding 1).
        // Server accepts then drops, no second listener -> reconnect fails -> enters backoff.
        // We close while backing off and assert the client shuts down quickly.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = task::spawn(async move {
            // Accept first connection, close immediately
            if let Ok((mut sock, _)) = listener.accept().await {
                drop(sock.shutdown());
            }
            // Don't accept again so reconnect fails and enters backoff
            sleep(Duration::from_mins(1)).await;
        });

        let config = SocketConfig {
            url: format!("127.0.0.1:{port}"),
            mode: Mode::Plain,
            suffix: b"\r\n".to_vec(),
            message_handler: None,
            heartbeat: None,
            reconnect_timeout_ms: Some(1_000),
            reconnect_delay_initial_ms: Some(10_000), // 10s backoff to ensure we're sleeping
            reconnect_delay_max_ms: Some(10_000),
            reconnect_backoff_factor: Some(1.0),
            reconnect_jitter_ms: Some(0),
            connection_max_retries: None,
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            certs_dir: None,
        };

        let client = SocketClient::connect(config, None, None, None)
            .await
            .unwrap();

        // Wait for client to enter reconnect
        wait_until_async(
            || async { client.is_reconnecting() },
            Duration::from_secs(3),
        )
        .await;

        // Wait for the reconnect attempt to fail and enter backoff sleep
        sleep(Duration::from_millis(1_500)).await;

        // Close while backing off
        let start = std::time::Instant::now();
        client.close().await;
        let elapsed = start.elapsed();

        assert!(client.is_closed(), "Client should be closed");
        // Should exit well before the 10s backoff sleep completes
        assert!(
            elapsed < Duration::from_secs(2),
            "Close should interrupt backoff sleep, took {elapsed:?}"
        );

        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_zero_idle_timeout_rejected() {
        let config = SocketConfig {
            url: "127.0.0.1:9999".to_string(),
            mode: Mode::Plain,
            suffix: b"\r\n".to_vec(),
            message_handler: None,
            heartbeat: None,
            reconnect_timeout_ms: None,
            reconnect_delay_initial_ms: None,
            reconnect_delay_max_ms: None,
            reconnect_backoff_factor: None,
            reconnect_jitter_ms: None,
            reconnect_max_attempts: None,
            connection_max_retries: Some(1),
            idle_timeout_ms: Some(0),
            certs_dir: None,
        };

        let result = SocketClient::connect(config, None, None, None).await;

        assert!(result.is_err(), "Zero idle timeout should be rejected");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("Idle timeout cannot be zero"),
            "Error should mention zero idle timeout, was: {err_msg}"
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_empty_suffix_rejected() {
        let config = SocketConfig {
            url: "127.0.0.1:9999".to_string(),
            mode: Mode::Plain,
            suffix: vec![],
            message_handler: None,
            heartbeat: None,
            reconnect_timeout_ms: None,
            reconnect_delay_initial_ms: None,
            reconnect_delay_max_ms: None,
            reconnect_backoff_factor: None,
            reconnect_jitter_ms: None,
            reconnect_max_attempts: None,
            connection_max_retries: Some(1),
            idle_timeout_ms: None,
            certs_dir: None,
        };

        let result = SocketClient::connect(config, None, None, None).await;

        assert!(
            result.is_err(),
            "Empty suffix should cause connection to fail"
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("suffix cannot be empty"),
            "Error should mention empty suffix, was: {err_msg}"
        );
    }
}
