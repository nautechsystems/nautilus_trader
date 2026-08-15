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

//! Raw TCP client with optional TLS, suffix framing, heartbeats, and automatic reconnection.
//!
//! # State management
//!
//! The client tracks active, reconnecting, disconnecting, and closed states. State changes notify
//! waiting tasks immediately instead of relying only on periodic checks.
//!
//! # Connection ownership
//!
//! A controller owns the connection lifecycle. A dedicated reader task passes complete messages to
//! the configured callback, while a dedicated writer task serializes concurrent sends received over
//! a channel.
//!
//! # Framing and heartbeats
//!
//! The configured suffix frames the byte stream in both directions. The writer appends it to sent
//! messages and heartbeats, and the reader uses it to split incoming data into complete messages.
//! Heartbeats are optional and run in a separate task.
//!
//! # Reconnection
//!
//! The writer buffers messages while reconnecting. A successful reconnect installs the replacement
//! writer, drains that buffer, restarts the reader, and then invokes the configured
//! post‑reconnection callback.

use std::{
    collections::VecDeque,
    fmt::Debug,
    path::Path,
    pin::pin,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
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
    SocketStateSink,
    backoff::{ExponentialBackoff, RECONNECT_STABILITY_THRESHOLD, wait_reconnect_delay},
    dst,
    error::{SendError, is_connection_drop_io_error},
    logging::{log_task_aborted, log_task_started, log_task_stopped},
    mode::{
        ConnectionMode, ControllerLifecycle, ReadSessionFence, ReconnectOutcome,
        ReconnectRequestOutcome,
    },
    net::TcpStream,
    tls::{create_tls_config_from_certs_dir, tcp_tls},
};

// Connection timing constants
const CONNECTION_STATE_CHECK_INTERVAL_MS: u64 = 10;
const GRACEFUL_SHUTDOWN_DELAY_MS: u64 = 100;
const GRACEFUL_SHUTDOWN_TIMEOUT_SECS: u64 = 5;
const WRITE_TIMEOUT_SECS: u64 = 5;

// Maximum buffer size for read operations (10 MB)
const MAX_READ_BUFFER_BYTES: usize = 10 * 1024 * 1024;

/// Produces protocol messages that must precede buffered application writes after reconnect.
pub type SocketReconnectReplay = Arc<dyn Fn() -> Vec<Bytes> + Send + Sync>;

struct SocketClientInner {
    config: SocketConfig,
    connector: Option<Arc<rustls::ClientConfig>>,
    read_task: tokio::task::JoinHandle<()>,
    read_fence: ReadSessionFence,
    write_task: tokio::task::JoinHandle<()>,
    writer_tx: tokio::sync::mpsc::UnboundedSender<WriterCommand>,
    heartbeat_task: Option<tokio::task::JoinHandle<()>>,
    connection_mode: Arc<AtomicU8>,
    state_notify: Arc<tokio::sync::Notify>,
    reconnect_timeout: Duration,
    backoff: ExponentialBackoff,
    reconnect_max_attempts: Option<u32>,
    reconnect_attempt_count: u32,
    state_sink: Option<SocketStateSink>,
}

impl SocketClientInner {
    /// Connects to a URL with the specified configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if connection fails or configuration is invalid.
    async fn connect_url(
        config: SocketConfig,
        state_sink: Option<SocketStateSink>,
    ) -> anyhow::Result<Self> {
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

        let reconnect_timeout =
            Duration::from_millis(config.reconnect_timeout_ms.unwrap_or(10_000));
        let reconnect_backoff = ExponentialBackoff::new(
            Duration::from_millis(config.reconnect_delay_initial_ms.unwrap_or(2_000)),
            Duration::from_millis(config.reconnect_delay_max_ms.unwrap_or(30_000)),
            config.reconnect_backoff_factor.unwrap_or(1.5),
            config.reconnect_jitter_ms.unwrap_or(100),
            true, // immediate-first
        )?;
        let connector = if let Some(dir) = &config.certs_dir {
            let config = create_tls_config_from_certs_dir(Path::new(dir), false)?;
            Some(Arc::new(config))
        } else {
            None
        };

        // Retry initial connection with exponential backoff to handle transient DNS/network issues
        let max_retries = config.connection_max_retries.unwrap_or(5);

        let mut backoff = ExponentialBackoff::new(
            Duration::from_millis(500),
            Duration::from_secs(5),
            2.0,
            250,
            false,
        )?;

        let mut attempt = 0;
        let (reader, writer) = loop {
            attempt += 1;

            let last_error = match dst::time::timeout(
                Duration::from_secs(CONNECTION_TIMEOUT_SECS),
                Self::tls_connect_with_server(&config.url, config.mode, connector.clone()),
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
                    let error = e.to_string();
                    log::warn!(
                        "Socket connection attempt {attempt}/{max_retries} to {} failed: {error}",
                        config.url,
                    );
                    error
                }
                Err(_) => {
                    let error = format!(
                        "Connection timeout after {CONNECTION_TIMEOUT_SECS}s (possible DNS resolution failure)"
                    );
                    log::warn!(
                        "Socket connection attempt {attempt}/{max_retries} to {} timed out",
                        config.url,
                    );
                    error
                }
            };

            if attempt >= max_retries {
                anyhow::bail!(
                    "Failed to connect to {} after {} attempts: {}. \
                    If this is a DNS error, check your network configuration and DNS settings.",
                    config.url,
                    max_retries,
                    last_error,
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

        let connection_mode = Arc::new(AtomicU8::new(ConnectionMode::Reconnect.as_u8()));
        let state_notify = Arc::new(tokio::sync::Notify::new());
        let outcome =
            ConnectionMode::complete_reconnect_with_sink(&connection_mode, state_sink.as_ref());
        debug_assert_eq!(outcome, ReconnectOutcome::Reconnected);
        let read_fence = ReadSessionFence::new();

        let read_task = Self::spawn_read_task(
            connection_mode.clone(),
            read_fence.clone(),
            reader,
            config.message_handler.clone(),
            config.suffix.clone(),
            config.idle_timeout_ms,
        );

        let (writer_tx, writer_rx) = tokio::sync::mpsc::unbounded_channel::<WriterCommand>();

        let write_task = Self::spawn_write_task(
            connection_mode.clone(),
            state_notify.clone(),
            writer,
            writer_rx,
            config.suffix.clone(),
            state_sink.clone(),
        );

        // Optionally spawn a heartbeat task to periodically ping server
        let heartbeat_task = config.heartbeat.as_ref().map(|heartbeat| {
            Self::spawn_heartbeat_task(
                connection_mode.clone(),
                heartbeat.clone(),
                writer_tx.clone(),
            )
        });
        let reconnect_max_attempts = config.reconnect_max_attempts;

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
            reconnect_max_attempts,
            reconnect_attempt_count: 0,
            state_sink,
        })
    }

    /// Parses a URL into its socket address and request URL.
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
        connector: Option<Arc<rustls::ClientConfig>>,
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

    /// Reconnects to the server.
    ///
    /// Makes a new connection with server, uses the new read and write halves
    /// to update the reader and writer.
    ///
    /// The reconnect timeout bounds only connection establishment. Once the
    /// new writer is handed to the writer task the swap runs to completion,
    /// so buffered messages can never drain into a connection that lost its
    /// reader to a timeout; the writer task bounds both the old-writer
    /// shutdown and the buffer drain with its graceful-shutdown timeout.
    async fn reconnect(
        &mut self,
        reconnect_replay: Option<&SocketReconnectReplay>,
    ) -> Result<ReconnectOutcome, Error> {
        log::info!("Reconnecting");

        if ConnectionMode::from_atomic(&self.connection_mode).is_disconnect() {
            log::debug!("Reconnect aborted due to disconnect state");
            return Ok(ReconnectOutcome::Aborted);
        }

        // Bound only connection establishment; the swap below must run to completion
        let (reader, new_writer) = dst::time::timeout(
            self.reconnect_timeout,
            Self::tls_connect_with_server(
                &self.config.url,
                self.config.mode,
                self.connector.clone(),
            ),
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
            return Ok(ReconnectOutcome::Aborted);
        }
        log::debug!("Connected");

        // Use a oneshot channel to synchronize with the writer task.
        // We must verify that the buffer was successfully drained before transitioning to ACTIVE
        // to prevent silent message loss if the new connection drops immediately.
        let (tx, rx) = tokio::sync::oneshot::channel();
        let command = if let Some(reconnect_replay) = reconnect_replay {
            WriterCommand::UpdateWithReplay(new_writer, reconnect_replay(), tx)
        } else {
            WriterCommand::Update(new_writer, tx)
        };

        if let Err(e) = self.writer_tx.send(command) {
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
            return Ok(ReconnectOutcome::Aborted);
        }

        self.read_fence.invalidate();

        if !self.read_task.is_finished() {
            self.read_task.abort();
            log_task_aborted("read");
        }

        // Atomically transition from Reconnect to Active
        // This prevents race condition where disconnect could be requested between check and store
        if ConnectionMode::complete_reconnect_with_sink(
            &self.connection_mode,
            self.state_sink.as_ref(),
        ) == ReconnectOutcome::Aborted
        {
            log::debug!("Reconnect aborted (state changed during reconnect)");
            return Ok(ReconnectOutcome::Aborted);
        }

        // Spawn new read task
        self.read_fence = ReadSessionFence::new();
        self.read_task = Self::spawn_read_task(
            self.connection_mode.clone(),
            self.read_fence.clone(),
            reader,
            self.config.message_handler.clone(),
            self.config.suffix.clone(),
            self.config.idle_timeout_ms,
        );

        log::info!("Reconnect succeeded");
        Ok(ReconnectOutcome::Reconnected)
    }

    /// Returns whether the read and write tasks are still running.
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
                return true;
            }

            buffer.pop_front();
        }

        log::info!("Successfully sent all {initial_buffer_len} buffered messages");

        false
    }

    async fn replace_writer<W>(
        active_writer: &mut W,
        new_writer: W,
        replay: Vec<Bytes>,
        reconnect_buffer: &mut VecDeque<Bytes>,
        suffix: &[u8],
    ) -> bool
    where
        W: AsyncWrite + Unpin,
    {
        log::debug!("Received new writer");
        dst::time::sleep(Duration::from_millis(GRACEFUL_SHUTDOWN_DELAY_MS)).await;

        _ = dst::time::timeout(
            Duration::from_secs(GRACEFUL_SHUTDOWN_TIMEOUT_SECS),
            active_writer.shutdown(),
        )
        .await;

        *active_writer = new_writer;
        log::debug!("Updated writer");

        let drain_result =
            dst::time::timeout(Duration::from_secs(GRACEFUL_SHUTDOWN_TIMEOUT_SECS), async {
                for replay_msg in replay {
                    let mut framed = Vec::with_capacity(replay_msg.len() + suffix.len());
                    framed.extend_from_slice(&replay_msg);
                    framed.extend_from_slice(suffix);
                    if let Err(e) = active_writer.write_all(&framed).await {
                        log::warn!("Failed to send reconnect replay: {e}");
                        return true;
                    }
                }

                Self::drain_reconnect_buffer(reconnect_buffer, active_writer, suffix).await
            })
            .await;

        let send_error = drain_result.unwrap_or_else(|_| {
            log::warn!(
                "Timed out sending reconnect replay and buffered messages, {} buffered messages remain",
                reconnect_buffer.len()
            );
            true
        });
        !send_error
    }

    fn spawn_write_task<W>(
        connection_state: Arc<AtomicU8>,
        state_notify: Arc<tokio::sync::Notify>,
        writer: W,
        mut writer_rx: tokio::sync::mpsc::UnboundedReceiver<WriterCommand<W>>,
        suffix: Vec<u8>,
        state_sink: Option<SocketStateSink>,
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
                                let sent = Self::replace_writer(
                                    &mut active_writer,
                                    new_writer,
                                    Vec::new(),
                                    &mut reconnect_buffer,
                                    &suffix,
                                )
                                .await;

                                if let Err(e) = tx.send(sent) {
                                    log::error!(
                                        "Failed to report drain status to controller: {e:?}"
                                    );
                                }
                            }
                            WriterCommand::UpdateWithReplay(new_writer, replay, tx) => {
                                let sent = Self::replace_writer(
                                    &mut active_writer,
                                    new_writer,
                                    replay,
                                    &mut reconnect_buffer,
                                    &suffix,
                                )
                                .await;

                                if let Err(e) = tx.send(sent) {
                                    log::error!(
                                        "Failed to report drain status to controller: {e:?}"
                                    );
                                }
                            }
                            WriterCommand::Send(data) if mode.is_reconnect() => {
                                log::debug!(
                                    "Buffering message while reconnecting ({} bytes)",
                                    data.len()
                                );
                                reconnect_buffer.push_back(data);
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
                                    if ConnectionMode::request_reconnect_with_sink(
                                        &connection_state,
                                        state_sink.as_ref(),
                                    ) {
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

/// Cleanup on drop: invalidates the read session and aborts background tasks.
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
    }
}

/// A suffix‑framed TCP client with optional TLS and automatic reconnection.
///
/// The internal writer task serializes concurrent calls to [`Self::send_bytes`]. The configured
/// suffix frames all sent and received messages, and an optional heartbeat task sends its payload
/// at the configured interval. See [`SocketConfig`] for framing and reconnect policy.
pub struct SocketClient {
    pub(crate) controller_task: tokio::task::JoinHandle<()>,
    pub(crate) connection_mode: Arc<AtomicU8>,
    pub(crate) state_notify: Arc<tokio::sync::Notify>,
    pub(crate) reconnect_timeout: Duration,
    pub writer_tx: tokio::sync::mpsc::UnboundedSender<WriterCommand>,
    state_sink: Option<SocketStateSink>,
    controller_lifecycle: Arc<ControllerLifecycle>,
    controller_notify: Arc<tokio::sync::Notify>,
}

/// Cloneable controller handle for requesting one raw socket reconnect.
#[derive(Clone)]
pub struct SocketReconnectHandle {
    connection_mode: Arc<AtomicU8>,
    state_sink: Option<SocketStateSink>,
    controller_lifecycle: Arc<ControllerLifecycle>,
    controller_notify: Arc<tokio::sync::Notify>,
}

impl Debug for SocketReconnectHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(SocketReconnectHandle))
            .field(
                "connection_mode",
                &ConnectionMode::from_atomic(&self.connection_mode),
            )
            .finish_non_exhaustive()
    }
}

impl SocketReconnectHandle {
    /// Requests that the controller replace the active transport.
    ///
    /// An accepted request reports the transport unavailable and wakes the controller. Rejected
    /// requests leave the transport state unchanged.
    #[must_use]
    pub fn request_reconnect(&self) -> ReconnectRequestOutcome {
        let Some(_request) = self.controller_lifecycle.enter_request() else {
            return ReconnectRequestOutcome::Closed;
        };

        let outcome = ConnectionMode::request_reconnect_outcome_with_sink(
            &self.connection_mode,
            self.state_sink.as_ref(),
        );

        if outcome == ReconnectRequestOutcome::Accepted {
            self.controller_notify.notify_one();
        }
        outcome
    }
}

impl Debug for SocketClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(SocketClient)).finish()
    }
}

impl SocketClient {
    /// Connects to the server.
    ///
    /// After a successful reconnect, `post_reconnection` runs after the replacement writer is
    /// installed, buffered messages are drained, and the replacement reader is started. The
    /// callback does not run after the initial connection.
    ///
    /// # Errors
    ///
    /// Returns any error connecting to the server.
    pub async fn connect(
        config: SocketConfig,
        post_reconnection: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> anyhow::Result<Self> {
        Self::connect_with_options(config, post_reconnection, None, None).await
    }

    /// Connects and sends protocol replay before messages buffered during each reconnect.
    ///
    /// # Errors
    ///
    /// Returns any error connecting to the server.
    pub async fn connect_with_reconnect_replay(
        config: SocketConfig,
        reconnect_replay: SocketReconnectReplay,
    ) -> anyhow::Result<Self> {
        Self::connect_with_options(config, None, None, Some(reconnect_replay)).await
    }

    /// Connects to the server and reports semantic transport availability changes.
    ///
    /// # Errors
    ///
    /// Returns any error connecting to the server.
    pub async fn connect_with_state_sink(
        config: SocketConfig,
        post_reconnection: Option<Arc<dyn Fn() + Send + Sync>>,
        state_sink: Option<SocketStateSink>,
    ) -> anyhow::Result<Self> {
        Self::connect_with_options(config, post_reconnection, state_sink, None).await
    }

    async fn connect_with_options(
        config: SocketConfig,
        post_reconnection: Option<Arc<dyn Fn() + Send + Sync>>,
        state_sink: Option<SocketStateSink>,
        reconnect_replay: Option<SocketReconnectReplay>,
    ) -> anyhow::Result<Self> {
        let inner = SocketClientInner::connect_url(config, state_sink).await?;
        let writer_tx = inner.writer_tx.clone();
        let connection_mode = inner.connection_mode.clone();
        let state_notify = inner.state_notify.clone();
        let reconnect_timeout = inner.reconnect_timeout;
        let state_sink = inner.state_sink.clone();
        let controller_lifecycle = Arc::new(ControllerLifecycle::new());
        let controller_notify = Arc::new(tokio::sync::Notify::new());
        let controller_task = Self::spawn_controller_task(
            inner,
            connection_mode.clone(),
            state_notify.clone(),
            Arc::clone(&controller_lifecycle),
            Arc::clone(&controller_notify),
            post_reconnection,
            reconnect_replay,
        );
        controller_lifecycle.set_abort_handle(controller_task.abort_handle());

        Ok(Self {
            controller_task,
            connection_mode,
            state_notify,
            reconnect_timeout,
            writer_tx,
            state_sink,
            controller_lifecycle,
            controller_notify,
        })
    }

    /// Returns a cloneable handle to this client's reconnect controller.
    #[must_use]
    pub fn reconnect_handle(&self) -> SocketReconnectHandle {
        SocketReconnectHandle {
            connection_mode: Arc::clone(&self.connection_mode),
            state_sink: self.state_sink.clone(),
            controller_lifecycle: Arc::clone(&self.controller_lifecycle),
            controller_notify: Arc::clone(&self.controller_notify),
        }
    }

    /// Requests that the controller replace the active transport.
    ///
    /// Returns `true` only when this call transitions the client from active to reconnecting.
    /// Duplicate, disconnecting, and closed requests return `false`.
    #[must_use]
    pub fn request_reconnect(&self) -> bool {
        self.reconnect_handle().request_reconnect() == ReconnectRequestOutcome::Accepted
    }

    /// Returns the current connection mode.
    #[must_use]
    pub fn connection_mode(&self) -> ConnectionMode {
        ConnectionMode::from_atomic(&self.connection_mode)
    }

    /// Returns whether the client connection is active.
    ///
    /// Returns `true` if the client is connected and has not been signalled to disconnect.
    /// The client will automatically retry connection based on its configuration.
    #[inline]
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.connection_mode().is_active()
    }

    /// Returns whether the client is reconnecting.
    ///
    /// Returns `true` if the client lost connection and is attempting to reestablish it.
    /// The client will automatically retry connection based on its configuration.
    #[inline]
    #[must_use]
    pub fn is_reconnecting(&self) -> bool {
        self.connection_mode().is_reconnect()
    }

    /// Returns whether the client is disconnecting.
    ///
    /// Returns `true` if the client is in disconnect mode.
    #[inline]
    #[must_use]
    pub fn is_disconnecting(&self) -> bool {
        self.connection_mode().is_disconnect()
    }

    /// Returns whether the client is closed.
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

    async fn close_with_timeout(&self, shutdown_timeout: Duration) {
        ConnectionMode::request_disconnect(&self.connection_mode);
        self.state_notify.notify_waiters();

        if dst::time::timeout(shutdown_timeout, async {
            while !self.controller_task.is_finished() {
                dst::time::sleep(Duration::from_millis(CONNECTION_STATE_CHECK_INTERVAL_MS)).await;
            }
        })
        .await
        .is_err()
        {
            log::warn!("Timeout waiting for controller task to finish");
        }

        if !self.controller_task.is_finished() {
            self.controller_task.abort();
            log_task_aborted("controller");
        }

        self.connection_mode
            .store(ConnectionMode::Closed.as_u8(), Ordering::SeqCst);
        self.state_notify.notify_waiters();
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
    /// Returns `Ok(())` when the message is enqueued to the writer channel. This does not
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
        controller_lifecycle: Arc<ControllerLifecycle>,
        controller_notify: Arc<tokio::sync::Notify>,
        post_reconnection: Option<Arc<dyn Fn() + Send + Sync>>,
        reconnect_replay: Option<SocketReconnectReplay>,
    ) -> tokio::task::JoinHandle<()> {
        const CONTROLLER_FALLBACK_INTERVAL_MS: u64 = 100;

        tokio::task::spawn(async move {
            let _activity = controller_lifecycle.activity();
            log_task_started("controller");

            let fallback_interval = Duration::from_millis(CONTROLLER_FALLBACK_INTERVAL_MS);
            let mut reconnected_at = None;

            loop {
                tokio::select! {
                    biased;
                    () = controller_notify.notified() => {}
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
                    connection_mode.store(ConnectionMode::Closed.as_u8(), Ordering::SeqCst);
                    state_notify.notify_waiters();
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

                    state_notify.notify_waiters();
                    break;
                }

                if mode.is_active() && !inner.is_alive() {
                    if ConnectionMode::request_reconnect_with_sink(
                        &connection_mode,
                        inner.state_sink.as_ref(),
                    ) {
                        log::info!("Detected dead read task, transitioning to RECONNECT");
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
                            state_notify.notify_waiters();
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
                        result = inner.reconnect(reconnect_replay.as_ref()) => Some(result),
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
                        Some(Ok(ReconnectOutcome::Reconnected)) => {
                            log::debug!("Reconnected successfully");
                            reconnected_at = Some(dst::time::Instant::now());

                            state_notify.notify_waiters();

                            // The outcome records a completed reconnection; emit recovery
                            // callbacks only while the replacement is still `Active`, not
                            // after a teardown or another drop.
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
                        Some(Ok(ReconnectOutcome::Aborted)) => {
                            log::debug!("Reconnect aborted");
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
            log_task_stopped("controller");
        })
    }
}

// Dropping cancels background work without reporting a terminal socket state transition.
impl Drop for SocketClient {
    fn drop(&mut self) {
        let controller_running = !self.controller_task.is_finished();
        self.controller_lifecycle.close_and_abort();

        if controller_running {
            log_task_aborted("controller");
        }
    }
}

#[cfg(test)]
#[cfg(not(feature = "turmoil"))]
#[cfg(not(all(feature = "simulation", madsim)))] // transport-layer I/O not simulated
#[cfg(target_os = "linux")] // Only run network tests on Linux (CI stability)
mod tests {
    use std::sync::Mutex as StdMutex;

    use nautilus_common::testing::wait_until_async;
    use rstest::rstest;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
        sync::Mutex,
        task,
        time::{Duration, sleep},
    };

    use super::*;
    use crate::SocketState;

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

        let client = SocketClient::connect(config, None)
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

        let client_res = SocketClient::connect(config, None).await;
        assert!(
            client_res.is_err(),
            "Should fail quickly with no server listening"
        );
    }

    #[tokio::test]
    async fn test_user_disconnect() {
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

        let client = SocketClient::connect(config, None).await.unwrap();

        client.close().await;
        assert!(client.is_closed());
        server_task.abort();
    }

    #[tokio::test]
    async fn test_close_after_closed_returns_fast_and_preserves_state() {
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

        let client = SocketClient::connect(config, None).await.unwrap();

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

        let client = SocketClient::connect(config, None).await.unwrap();

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

        let client = SocketClient::connect(config, None)
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

    #[rstest]
    #[tokio::test]
    async fn test_state_sink_reports_initial_loss_and_recovery() {
        let (port, listener) = bind_test_server().await;
        let server_task = task::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            sleep(Duration::from_millis(100)).await;
            socket.shutdown().await.unwrap();

            let (socket, _) = listener.accept().await.unwrap();
            run_echo_server(socket).await;
        });
        let config = SocketConfig {
            url: format!("127.0.0.1:{port}"),
            mode: Mode::Plain,
            suffix: b"\r\n".to_vec(),
            message_handler: None,
            heartbeat: None,
            reconnect_timeout_ms: Some(1_000),
            reconnect_delay_initial_ms: Some(10),
            reconnect_backoff_factor: Some(1.0),
            reconnect_delay_max_ms: Some(10),
            reconnect_jitter_ms: Some(0),
            reconnect_max_attempts: Some(3),
            connection_max_retries: Some(1),
            idle_timeout_ms: None,
            certs_dir: None,
        };
        let states = Arc::new(StdMutex::new(Vec::new()));
        let states_callback = Arc::clone(&states);
        let sink = SocketStateSink::new(move |state| {
            states_callback.lock().unwrap().push(state);
        });

        let client = SocketClient::connect_with_state_sink(config, None, Some(sink))
            .await
            .unwrap();

        assert_eq!(*states.lock().unwrap(), vec![SocketState::Connected]);

        wait_until_async(
            || {
                let states = Arc::clone(&states);
                async move { states.lock().unwrap().len() == 3 }
            },
            Duration::from_secs(5),
        )
        .await;
        assert_eq!(
            *states.lock().unwrap(),
            vec![
                SocketState::Connected,
                SocketState::Disconnected,
                SocketState::Connected,
            ]
        );

        client.close().await;
        assert_eq!(states.lock().unwrap().len(), 3);
        server_task.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_state_sink_ignores_initial_connection_failure() {
        let (port, listener) = bind_test_server().await;
        drop(listener);
        let config = SocketConfig {
            url: format!("127.0.0.1:{port}"),
            mode: Mode::Plain,
            suffix: b"\r\n".to_vec(),
            message_handler: None,
            heartbeat: None,
            reconnect_timeout_ms: Some(100),
            reconnect_delay_initial_ms: Some(1),
            reconnect_backoff_factor: Some(1.0),
            reconnect_delay_max_ms: Some(1),
            reconnect_jitter_ms: Some(0),
            reconnect_max_attempts: Some(1),
            connection_max_retries: Some(1),
            idle_timeout_ms: None,
            certs_dir: None,
        };
        let states = Arc::new(StdMutex::new(Vec::new()));
        let states_callback = Arc::clone(&states);
        let sink = SocketStateSink::new(move |state| {
            states_callback.lock().unwrap().push(state);
        });

        let result = SocketClient::connect_with_state_sink(config, None, Some(sink)).await;

        assert!(result.is_err());
        assert_eq!(*states.lock().unwrap(), Vec::new());
    }
}

#[cfg(test)]
#[cfg(not(feature = "turmoil"))]
#[cfg(not(all(feature = "simulation", madsim)))] // transport-layer I/O not simulated
mod rust_tests {
    use std::{
        pin::Pin,
        sync::{Arc, Condvar, Mutex as StdMutex, atomic::AtomicUsize},
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
    use crate::SocketState;

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

    fn reconnect_test_config(port: u16) -> SocketConfig {
        SocketConfig {
            url: format!("127.0.0.1:{port}"),
            mode: Mode::Plain,
            suffix: b"\r\n".to_vec(),
            message_handler: None,
            heartbeat: None,
            reconnect_timeout_ms: Some(1_000),
            reconnect_delay_initial_ms: None,
            reconnect_backoff_factor: None,
            reconnect_delay_max_ms: None,
            reconnect_jitter_ms: None,
            connection_max_retries: Some(1),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            certs_dir: None,
        }
    }

    #[rstest]
    #[tokio::test]
    async fn test_reconnect_outcome_is_aborted_before_dial() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            std::future::pending::<()>().await;
        });
        let mut inner = SocketClientInner::connect_url(reconnect_test_config(port), None)
            .await
            .unwrap();
        inner
            .connection_mode
            .store(ConnectionMode::Disconnect.as_u8(), Ordering::SeqCst);

        let outcome = inner.reconnect(None).await.unwrap();

        assert_eq!(outcome, ReconnectOutcome::Aborted);
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_reconnect_outcome_is_reconnected() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (_first, _) = listener.accept().await.unwrap();
            let (_second, _) = listener.accept().await.unwrap();
            std::future::pending::<()>().await;
        });
        let mut inner = SocketClientInner::connect_url(reconnect_test_config(port), None)
            .await
            .unwrap();
        inner
            .connection_mode
            .store(ConnectionMode::Reconnect.as_u8(), Ordering::SeqCst);

        let outcome = inner.reconnect(None).await.unwrap();

        assert_eq!(outcome, ReconnectOutcome::Reconnected);
        assert_eq!(
            ConnectionMode::from_atomic(&inner.connection_mode),
            ConnectionMode::Active
        );
        server.abort();
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
        controller_task: tokio::task::JoinHandle<()>,
    ) -> SocketClient {
        let (writer_tx, _writer_rx) = tokio::sync::mpsc::unbounded_channel();
        let controller_lifecycle = Arc::new(ControllerLifecycle::new());
        controller_lifecycle.set_abort_handle(controller_task.abort_handle());

        SocketClient {
            controller_task,
            connection_mode: connection_state,
            state_notify,
            reconnect_timeout: Duration::from_secs(1),
            writer_tx,
            controller_lifecycle,
            controller_notify: Arc::new(tokio::sync::Notify::new()),
            state_sink: None,
        }
    }

    #[rstest]
    #[tokio::test]
    async fn test_reconnect_handle_is_closed_after_client_drop() {
        let connection_state = Arc::new(AtomicU8::new(ConnectionMode::Active.as_u8()));
        let state_notify = Arc::new(tokio::sync::Notify::new());
        let controller_task = tokio::spawn(std::future::pending::<()>());
        let client = test_socket_client(connection_state, state_notify, controller_task);
        let handle = client.reconnect_handle();

        drop(client);

        assert_eq!(handle.request_reconnect(), ReconnectRequestOutcome::Closed);
    }

    #[rstest]
    #[tokio::test]
    async fn test_concurrent_drop_defers_controller_abort_until_request_completes() {
        let connection_state = Arc::new(AtomicU8::new(ConnectionMode::Active.as_u8()));
        let state_notify = Arc::new(tokio::sync::Notify::new());

        let controller_task = tokio::spawn(std::future::pending::<()>());
        let controller_abort = controller_task.abort_handle();
        let mut client = test_socket_client(connection_state, state_notify, controller_task);
        let callback_release = Arc::new((StdMutex::new(false), Condvar::new()));
        let callback_release_guard = CondvarReleaseGuard::new(Arc::clone(&callback_release));
        let callback_release_clone = Arc::clone(&callback_release);
        let (callback_entered_tx, callback_entered_rx) = std::sync::mpsc::channel();
        let states = Arc::new(StdMutex::new(Vec::new()));
        let states_clone = Arc::clone(&states);
        client.state_sink = Some(SocketStateSink::new(move |state| {
            states_clone.lock().unwrap().push(state);
            callback_entered_tx.send(()).unwrap();
            let (lock, condvar) = callback_release_clone.as_ref();
            let mut released = lock.lock().unwrap();

            while !*released {
                released = condvar.wait(released).unwrap();
            }
        }));
        let handle = client.reconnect_handle();
        let surviving_handle = handle.clone();
        let controller_notify = Arc::clone(&handle.controller_notify);
        let request_thread = std::thread::spawn(move || handle.request_reconnect());

        recv_rendezvous(callback_entered_rx, "socket state callback entry").await;
        let (drop_finished_tx, drop_finished_rx) = std::sync::mpsc::channel();
        let drop_thread = std::thread::spawn(move || {
            drop(client);
            drop_finished_tx.send(()).unwrap();
        });

        recv_rendezvous(drop_finished_rx, "client drop").await;
        drop_thread.join().unwrap();
        assert!(!controller_abort.is_finished());

        callback_release_guard.release();
        assert_eq!(
            request_thread.join().unwrap(),
            ReconnectRequestOutcome::Accepted
        );
        tokio::time::timeout(Duration::from_millis(10), controller_notify.notified())
            .await
            .expect("accepted request should notify before deferred controller abort");
        wait_until_async(|| async { controller_abort.is_finished() }, TEST_TIMEOUT).await;

        assert_eq!(
            surviving_handle.request_reconnect(),
            ReconnectRequestOutcome::Closed
        );
        assert_eq!(*states.lock().unwrap(), vec![SocketState::Disconnected]);
        assert!(
            tokio::time::timeout(Duration::from_millis(10), controller_notify.notified())
                .await
                .is_err(),
            "closed request should not notify controller",
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_reconnect_state_callback_can_drop_client() {
        let connection_state = Arc::new(AtomicU8::new(ConnectionMode::Active.as_u8()));
        let state_notify = Arc::new(tokio::sync::Notify::new());

        let controller_task = tokio::spawn(std::future::pending::<()>());
        let controller_abort = controller_task.abort_handle();
        let mut client = test_socket_client(connection_state, state_notify, controller_task);
        let client_slot = Arc::new(StdMutex::new(None));
        let client_slot_callback = Arc::clone(&client_slot);
        let states = Arc::new(StdMutex::new(Vec::new()));
        let states_callback = Arc::clone(&states);
        client.state_sink = Some(SocketStateSink::new(move |state| {
            states_callback.lock().unwrap().push(state);
            drop(client_slot_callback.lock().unwrap().take());
        }));
        let handle = client.reconnect_handle();
        let controller_notify = Arc::clone(&handle.controller_notify);
        *client_slot.lock().unwrap() = Some(client);
        let (result_tx, result_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || result_tx.send(handle.request_reconnect()).unwrap());

        assert_eq!(
            recv_rendezvous(result_rx, "reconnect request").await,
            ReconnectRequestOutcome::Accepted
        );
        tokio::time::timeout(Duration::from_millis(10), controller_notify.notified())
            .await
            .expect("accepted request should notify before deferred controller abort");
        wait_until_async(|| async { controller_abort.is_finished() }, TEST_TIMEOUT).await;

        assert!(client_slot.lock().unwrap().is_none());
        assert_eq!(*states.lock().unwrap(), vec![SocketState::Disconnected]);
    }

    #[rstest]
    #[tokio::test]
    async fn test_manual_reconnect_uses_controller_path() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (first_accepted_tx, first_accepted_rx) = oneshot::channel();
        let (second_accepted_tx, second_accepted_rx) = oneshot::channel();
        let (payload_tx, payload_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();

        let server = task::spawn(async move {
            let (_first, _) = listener.accept().await.unwrap();
            first_accepted_tx.send(()).unwrap();

            let (mut second, _) = listener.accept().await.unwrap();
            second_accepted_tx.send(()).unwrap();
            let mut payload = [0_u8; 8];
            second.read_exact(&mut payload).await.unwrap();
            payload_tx.send(payload).unwrap();
            let _ = release_rx.await;
        });
        let states = Arc::new(StdMutex::new(Vec::new()));
        let states_callback = Arc::clone(&states);
        let sink = SocketStateSink::new(move |state| {
            states_callback.lock().unwrap().push(state);
        });
        let callback_count = Arc::new(AtomicUsize::new(0));
        let callback_count_clone = Arc::clone(&callback_count);
        let post_reconnection = Arc::new(move || {
            callback_count_clone.fetch_add(1, Ordering::SeqCst);
        });
        let client = SocketClient::connect_with_state_sink(
            reconnect_test_config(port),
            Some(post_reconnection),
            Some(sink),
        )
        .await
        .unwrap();
        first_accepted_rx.await.unwrap();
        let handle = client.reconnect_handle();

        assert!(client.request_reconnect());
        assert_eq!(
            handle.request_reconnect(),
            ReconnectRequestOutcome::AlreadyReconnecting
        );
        assert!(!client.request_reconnect());
        assert_eq!(
            *states.lock().unwrap(),
            vec![SocketState::Connected, SocketState::Disconnected]
        );

        tokio::time::timeout(TEST_TIMEOUT, second_accepted_rx)
            .await
            .expect("controller should establish a replacement connection")
            .unwrap();
        wait_until_async(
            || async { client.is_active() && callback_count.load(Ordering::SeqCst) == 1 },
            TEST_TIMEOUT,
        )
        .await;
        client.send_bytes(b"manual".to_vec()).await.unwrap();

        assert_eq!(
            tokio::time::timeout(TEST_TIMEOUT, payload_rx)
                .await
                .expect("replacement connection should receive the framed payload")
                .unwrap(),
            *b"manual\r\n"
        );
        assert_eq!(callback_count.load(Ordering::SeqCst), 1);
        assert_eq!(
            *states.lock().unwrap(),
            vec![
                SocketState::Connected,
                SocketState::Disconnected,
                SocketState::Connected,
            ]
        );

        client.close().await;
        release_tx.send(()).unwrap();
        server.await.unwrap();
        assert_eq!(callback_count.load(Ordering::SeqCst), 1);
        assert_eq!(states.lock().unwrap().len(), 3);
    }

    #[rstest]
    #[case(ConnectionMode::Disconnect)]
    #[case(ConnectionMode::Closed)]
    #[tokio::test]
    async fn test_send_bytes_rejects_terminal_state(#[case] mode: ConnectionMode) {
        let connection_state = Arc::new(AtomicU8::new(mode.as_u8()));
        let state_notify = Arc::new(tokio::sync::Notify::new());
        let controller_task = tokio::spawn(std::future::pending::<()>());
        let client = test_socket_client(connection_state, state_notify, controller_task);

        let result = client.send_bytes(b"terminal".to_vec()).await;

        assert!(matches!(result, Err(SendError::Closed)));
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn test_close_sets_closed_after_controller_was_aborted() {
        let connection_state = Arc::new(AtomicU8::new(ConnectionMode::Active.as_u8()));
        let state_notify = Arc::new(tokio::sync::Notify::new());

        let controller_task = tokio::spawn(std::future::pending::<()>());
        controller_task.abort();
        let client = test_socket_client(connection_state, state_notify, controller_task);

        client.close_with_timeout(Duration::from_millis(1)).await;

        assert!(client.is_closed());
    }

    #[rstest]
    #[tokio::test]
    async fn test_reconnect_exhaustion_closes_client() {
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

        let states = Arc::new(StdMutex::new(Vec::new()));
        let states_callback = Arc::clone(&states);
        let sink = SocketStateSink::new(move |state| {
            states_callback.lock().unwrap().push(state);
        });
        let client = SocketClient::connect_with_state_sink(config, None, Some(sink))
            .await
            .unwrap();
        accepted_rx.await.unwrap();
        release_tx.send(()).unwrap();

        wait_until_async(|| async { client.is_closed() }, Duration::from_secs(5)).await;
        assert!(client.is_closed());
        assert_eq!(
            *states.lock().unwrap(),
            vec![SocketState::Connected, SocketState::Disconnected]
        );

        client.close().await;
        assert!(client.is_closed());
        assert_eq!(states.lock().unwrap().len(), 2);
        server.await.unwrap();
    }

    #[rstest]
    #[tokio::test]
    async fn test_drop_suppresses_socket_state_event() {
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
        let states = Arc::new(StdMutex::new(Vec::new()));
        let states_callback = Arc::clone(&states);
        let sink = SocketStateSink::new(move |state| {
            states_callback.lock().unwrap().push(state);
        });
        let client = SocketClient::connect_with_state_sink(config, None, Some(sink))
            .await
            .unwrap();
        accepted_rx.await.unwrap();

        drop(client);
        sleep(Duration::from_millis(25)).await;

        assert_eq!(*states.lock().unwrap(), vec![SocketState::Connected]);
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_graceful_close_is_idempotent() {
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
        let client = SocketClient::connect(config, None).await.unwrap();
        accepted_rx.await.unwrap();

        client.close().await;
        client.close().await;

        assert!(client.is_closed());
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
    async fn test_stalled_socket_write_sends_reconnect_replay_before_buffer() {
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
        let states = Arc::new(StdMutex::new(Vec::new()));
        let states_callback = Arc::clone(&states);
        let sink = SocketStateSink::new(move |state| {
            states_callback.lock().unwrap().push(state);
        });
        let write_task = SocketClientInner::spawn_write_task(
            Arc::clone(&connection_state),
            Arc::clone(&state_notify),
            writer,
            writer_rx,
            b"\r\n".to_vec(),
            Some(sink),
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
            .send(WriterCommand::UpdateWithReplay(
                new_writer,
                vec![Bytes::from_static(b"authentication")],
                update_tx,
            ))
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
        assert_eq!(
            recorded.lock().unwrap().as_slice(),
            b"authentication\r\ncomplete-message\r\n"
        );

        connection_state.store(ConnectionMode::Closed.as_u8(), Ordering::SeqCst);
        state_notify.notify_waiters();
        drop(writer_tx);
        write_task.await.unwrap();

        assert_eq!(*states.lock().unwrap(), vec![SocketState::Disconnected]);
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

        let error = match SocketClientInner::connect_url(config, None).await {
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
        let client = SocketClient::connect(config.clone(), None).await.unwrap();

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

        let client = SocketClient::connect(config, None).await.unwrap();

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
        let client = SocketClient::connect(config, None).await;
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
        let client = SocketClient::connect(config, None).await;
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

        let client = SocketClient::connect(config, None).await;
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

        let client = SocketClient::connect(config, None).await.unwrap();

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

        let client = SocketClient::connect(config, None).await.unwrap();

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

        let client = SocketClient::connect(config, None).await.unwrap();

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

        let client = SocketClient::connect(config, None).await.unwrap();

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

        let client = SocketClient::connect(config, None).await.unwrap();

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

        let result = SocketClient::connect(config, None).await;

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

        let result = SocketClient::connect(config, None).await;

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

#[cfg(test)]
mod reconnect_request_tests {
    use std::sync::{Arc, Mutex, atomic::AtomicU8};

    use rstest::rstest;

    use super::*;
    use crate::SocketState;

    fn handle(
        mode: ConnectionMode,
    ) -> (
        SocketReconnectHandle,
        Arc<tokio::sync::Notify>,
        Arc<Mutex<Vec<SocketState>>>,
    ) {
        let controller_notify = Arc::new(tokio::sync::Notify::new());
        let states = Arc::new(Mutex::new(Vec::new()));
        let states_callback = Arc::clone(&states);
        let state_sink = SocketStateSink::new(move |state| {
            states_callback.lock().unwrap().push(state);
        });
        let handle = SocketReconnectHandle {
            connection_mode: Arc::new(AtomicU8::new(mode.as_u8())),
            state_sink: Some(state_sink),
            controller_lifecycle: Arc::new(ControllerLifecycle::new()),
            controller_notify: Arc::clone(&controller_notify),
        };
        (handle, controller_notify, states)
    }

    #[rstest]
    #[tokio::test]
    async fn accepted_request_reports_loss_and_wakes_controller_once() {
        let (handle, controller_notify, states) = handle(ConnectionMode::Active);

        assert_eq!(
            handle.request_reconnect(),
            ReconnectRequestOutcome::Accepted
        );
        assert_eq!(*states.lock().unwrap(), vec![SocketState::Disconnected]);
        tokio::time::timeout(Duration::from_millis(10), controller_notify.notified())
            .await
            .expect("accepted request should notify controller");

        assert_eq!(
            handle.request_reconnect(),
            ReconnectRequestOutcome::AlreadyReconnecting
        );
        assert_eq!(*states.lock().unwrap(), vec![SocketState::Disconnected]);
        assert!(
            tokio::time::timeout(Duration::from_millis(10), controller_notify.notified())
                .await
                .is_err(),
            "duplicate request should not notify controller",
        );
    }

    #[rstest]
    #[case(
        ConnectionMode::Reconnect,
        ReconnectRequestOutcome::AlreadyReconnecting
    )]
    #[case(ConnectionMode::Disconnect, ReconnectRequestOutcome::Disconnected)]
    #[case(ConnectionMode::Closed, ReconnectRequestOutcome::Closed)]
    #[tokio::test]
    async fn rejected_request_preserves_state_and_does_not_wake_controller(
        #[case] mode: ConnectionMode,
        #[case] expected: ReconnectRequestOutcome,
    ) {
        let (handle, controller_notify, states) = handle(mode);

        assert_eq!(handle.request_reconnect(), expected);
        assert!(states.lock().unwrap().is_empty());
        assert!(
            tokio::time::timeout(Duration::from_millis(10), controller_notify.notified())
                .await
                .is_err(),
            "rejected request should not notify controller",
        );
    }
}
