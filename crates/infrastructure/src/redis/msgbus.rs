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

//! Redis-backed message bus backing for the system.
//!
//! # Architecture
//!
//! Runs background tasks on `get_runtime()` for publishing, stream reading,
//! and heartbeats. Messages are sent via an unbounded `tokio::sync::mpsc`
//! channel to the publish task, which buffers and writes them to Redis
//! streams. Each background task owns its own Redis connection created on
//! the Nautilus runtime.
//!
//! Handles are stored as `Option<JoinHandle>` for idempotent shutdown via
//! `close_async()`. The synchronous `close()` uses `block_in_place` to
//! bridge into the async shutdown path and must be called from outside any
//! `current_thread` Tokio runtime.

use std::{
    collections::{HashMap, VecDeque},
    fmt::Debug,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use bytes::Bytes;
use futures::stream::Stream;
use nautilus_common::{
    enums::SerializationEncoding,
    live::get_runtime,
    logging::{log_task_error, log_task_started, log_task_stopped},
    msgbus::{
        BusMessage, BusPayloadType, MessageBusBacking, MessageBusBackingFactory, MessageBusConfig,
        switchboard::CLOSE_TOPIC,
    },
};
use nautilus_core::{
    UUID4,
    time::{duration_since_unix_epoch, get_atomic_clock_realtime},
};
use nautilus_cryptography::providers::install_cryptographic_provider;
use nautilus_model::identifiers::TraderId;
use redis::{AsyncCommands, RetryMethod, aio::ConnectionManager, streams};
use serde::{Deserialize, Serialize};
use streams::StreamReadOptions;
use ustr::Ustr;

use super::{REDIS_MINID, REDIS_XTRIM, await_handle};
use crate::redis::{RedisConnectionConfig, create_redis_connection, get_stream_key};

const MSGBUS_PUBLISH: &str = "msgbus-publish";
const MSGBUS_STREAM: &str = "msgbus-stream";
const MSGBUS_HEARTBEAT: &str = "msgbus-heartbeat";
const HEARTBEAT_TOPIC: &str = "health:heartbeat";
const TRIM_BUFFER_SECS: u64 = 60;

type RedisStreamBulk = Vec<HashMap<String, Vec<HashMap<String, redis::Value>>>>;

/// Configuration for a Redis-backed message bus backing.
///
/// Redis 6.2 or higher is required for correct operation.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.infrastructure",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.infrastructure")
)]
pub struct RedisMessageBusConfig {
    /// The Redis host address. If `None`, `127.0.0.1` is used.
    pub host: Option<String>,
    /// The Redis port. If `None`, `6379` is used.
    pub port: Option<u16>,
    /// The Redis account username.
    pub username: Option<String>,
    /// The Redis account password.
    pub password: Option<String>,
    /// If Redis should use an SSL-enabled connection.
    pub ssl: bool,
    /// The timeout (in seconds) to wait for a new connection.
    pub connection_timeout: u16,
    /// The timeout (in seconds) to wait for a response.
    pub response_timeout: u16,
    /// The number of retry attempts with exponential backoff for connection attempts.
    pub number_of_retries: usize,
    /// The base value for exponential backoff calculation.
    pub exponent_base: u64,
    /// The maximum delay between retry attempts (in seconds).
    pub max_delay: u64,
    /// The multiplication factor for retry delay calculation.
    pub factor: u64,
}

impl Debug for RedisMessageBusConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let redacted = self.password.as_ref().map(|_| "***");
        f.debug_struct(stringify!(RedisMessageBusConfig))
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("password", &redacted)
            .field("ssl", &self.ssl)
            .field("connection_timeout", &self.connection_timeout)
            .field("response_timeout", &self.response_timeout)
            .field("number_of_retries", &self.number_of_retries)
            .field("exponent_base", &self.exponent_base)
            .field("max_delay", &self.max_delay)
            .field("factor", &self.factor)
            .finish()
    }
}

impl Default for RedisMessageBusConfig {
    fn default() -> Self {
        Self {
            host: None,
            port: None,
            username: None,
            password: None,
            ssl: false,
            connection_timeout: 20,
            response_timeout: 20,
            number_of_retries: 100,
            exponent_base: 2,
            max_delay: 1000,
            factor: 2,
        }
    }
}

impl RedisConnectionConfig for RedisMessageBusConfig {
    fn host(&self) -> Option<&str> {
        self.host.as_deref()
    }

    fn port(&self) -> Option<u16> {
        self.port
    }

    fn username(&self) -> Option<&str> {
        self.username.as_deref()
    }

    fn password(&self) -> Option<&str> {
        self.password.as_deref()
    }

    fn ssl(&self) -> bool {
        self.ssl
    }

    fn connection_timeout(&self) -> u16 {
        self.connection_timeout
    }

    fn response_timeout(&self) -> u16 {
        self.response_timeout
    }

    fn number_of_retries(&self) -> usize {
        self.number_of_retries
    }

    fn exponent_base(&self) -> u64 {
        self.exponent_base
    }

    fn max_delay(&self) -> u64 {
        self.max_delay
    }

    fn factor(&self) -> u64 {
        self.factor
    }
}

/// Factory for constructing Redis message bus backings.
#[derive(Debug, Clone)]
pub struct RedisMessageBusFactory {
    config: RedisMessageBusConfig,
}

impl RedisMessageBusFactory {
    /// Creates a new [`RedisMessageBusFactory`] from the given Redis configuration.
    #[must_use]
    pub const fn new(config: RedisMessageBusConfig) -> Self {
        Self { config }
    }
}

impl MessageBusBackingFactory for RedisMessageBusFactory {
    fn create(
        &self,
        trader_id: TraderId,
        instance_id: UUID4,
        config: MessageBusConfig,
    ) -> anyhow::Result<Box<dyn MessageBusBacking>> {
        Ok(Box::new(RedisMessageBusBacking::new(
            trader_id,
            instance_id,
            config,
            self.config.clone(),
        )?))
    }
}

pub struct RedisMessageBusBacking {
    /// The trader ID for this message bus backing.
    pub trader_id: TraderId,
    /// The instance ID for this message bus backing.
    pub instance_id: UUID4,
    pub_tx: tokio::sync::mpsc::UnboundedSender<BusMessage>,
    pub_handle: Option<tokio::task::JoinHandle<()>>,
    stream_rx: Option<tokio::sync::mpsc::Receiver<BusMessage>>,
    stream_handle: Option<tokio::task::JoinHandle<()>>,
    stream_signal: Arc<AtomicBool>,
    heartbeat_handle: Option<tokio::task::JoinHandle<()>>,
    heartbeat_signal: Arc<AtomicBool>,
}

impl Debug for RedisMessageBusBacking {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(RedisMessageBusBacking))
            .field("trader_id", &self.trader_id)
            .field("instance_id", &self.instance_id)
            .finish_non_exhaustive()
    }
}

impl RedisMessageBusBacking {
    /// Creates a new [`RedisMessageBusBacking`] instance for the given `trader_id`, `instance_id`, and `config`.
    ///
    /// # Errors
    ///
    /// Returns an error if the heartbeat interval is configured as zero seconds.
    pub fn new(
        trader_id: TraderId,
        instance_id: UUID4,
        config: MessageBusConfig,
        backing: RedisMessageBusConfig,
    ) -> anyhow::Result<Self> {
        install_cryptographic_provider();

        if config.heartbeat_interval_secs == Some(0) {
            anyhow::bail!("heartbeat_interval_secs must be greater than 0");
        }

        let external_streams = config.external_streams.clone().unwrap_or_default();
        let heartbeat_interval_secs = config.heartbeat_interval_secs;
        let publish = backing.clone();

        let (pub_tx, pub_rx) = tokio::sync::mpsc::unbounded_channel::<BusMessage>();

        // Create publish task (start the runtime here for now)
        let pub_handle = Some(get_runtime().spawn(async move {
            if let Err(e) = publish_messages(pub_rx, trader_id, instance_id, config, publish).await
            {
                log_task_error(MSGBUS_PUBLISH, &e);
            }
        }));

        // Conditionally create stream task and channel if external streams configured
        let stream_signal = Arc::new(AtomicBool::new(false));
        let (stream_rx, stream_handle) = if external_streams.is_empty() {
            (None, None)
        } else {
            let stream_signal_clone = stream_signal.clone();
            let (stream_tx, stream_rx) = tokio::sync::mpsc::channel::<BusMessage>(100_000);
            (
                Some(stream_rx),
                Some(get_runtime().spawn(async move {
                    if let Err(e) = stream_messages(
                        stream_tx,
                        backing.clone(),
                        external_streams,
                        stream_signal_clone,
                    )
                    .await
                    {
                        log_task_error(MSGBUS_STREAM, &e);
                    }
                })),
            )
        };

        // Create heartbeat task
        let heartbeat_signal = Arc::new(AtomicBool::new(false));
        let heartbeat_handle = if let Some(heartbeat_interval_secs) = heartbeat_interval_secs {
            let signal = heartbeat_signal.clone();
            let pub_tx_clone = pub_tx.clone();

            Some(get_runtime().spawn(async move {
                run_heartbeat(heartbeat_interval_secs, signal, pub_tx_clone).await;
            }))
        } else {
            None
        };

        Ok(Self {
            trader_id,
            instance_id,
            pub_tx,
            pub_handle,
            stream_rx,
            stream_handle,
            stream_signal,
            heartbeat_handle,
            heartbeat_signal,
        })
    }
}

impl MessageBusBacking for RedisMessageBusBacking {
    /// Returns whether the message bus backing publishing channel is closed.
    fn is_closed(&self) -> bool {
        self.pub_tx.is_closed()
    }

    /// Queues a serialized bus message for external publication.
    fn publish(&self, message: BusMessage) {
        if let Err(e) = self.pub_tx.send(message) {
            log::error!("Failed to send message: {e}");
        }
    }

    fn take_receiver(&mut self) -> anyhow::Result<tokio::sync::mpsc::Receiver<BusMessage>> {
        self.get_stream_receiver()
    }

    /// Closes the message bus backing.
    fn close(&mut self) {
        log::debug!("Closing");

        self.stream_signal.store(true, Ordering::Relaxed);
        self.heartbeat_signal.store(true, Ordering::Relaxed);

        if !self.pub_tx.is_closed() {
            let msg = BusMessage::new_close();

            if let Err(e) = self.pub_tx.send(msg) {
                log::warn!("Failed to send close message: {e:?}");
            }
        }

        // Keep close sync for now to avoid async trait method
        tokio::task::block_in_place(|| {
            get_runtime().block_on(async {
                self.close_async().await;
            });
        });

        log::debug!("Closed");
    }
}

impl RedisMessageBusBacking {
    /// Retrieves the Redis stream receiver for this message bus instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the stream receiver has already been taken.
    pub fn get_stream_receiver(
        &mut self,
    ) -> anyhow::Result<tokio::sync::mpsc::Receiver<BusMessage>> {
        self.stream_rx
            .take()
            .ok_or_else(|| anyhow::anyhow!("Stream receiver already taken"))
    }

    /// Streams messages arriving on the stream receiver channel.
    pub fn stream(
        mut stream_rx: tokio::sync::mpsc::Receiver<BusMessage>,
    ) -> impl Stream<Item = BusMessage> + 'static {
        async_stream::stream! {
            while let Some(msg) = stream_rx.recv().await {
                yield msg;
            }
        }
    }

    pub async fn close_async(&mut self) {
        await_handle(self.pub_handle.take(), MSGBUS_PUBLISH).await;
        await_handle(self.stream_handle.take(), MSGBUS_STREAM).await;
        await_handle(self.heartbeat_handle.take(), MSGBUS_HEARTBEAT).await;
    }
}

/// Publishes messages received on `rx` to Redis streams for the given `trader_id` and `instance_id`, using `config`.
///
/// # Errors
///
/// Returns an error if:
/// - The backing configuration is missing in `config`.
/// - Establishing the Redis connection fails.
/// - Any Redis command fails during publishing.
pub async fn publish_messages(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<BusMessage>,
    trader_id: TraderId,
    instance_id: UUID4,
    config: MessageBusConfig,
    backing: RedisMessageBusConfig,
) -> anyhow::Result<()> {
    log_task_started(MSGBUS_PUBLISH);

    let mut con = create_redis_connection(MSGBUS_PUBLISH, &backing).await?;
    let stream_key = get_stream_key(trader_id, instance_id, &config);

    // Auto-trimming
    let autotrim_duration = config
        .autotrim_mins
        .filter(|&mins| mins > 0)
        .map(|mins| Duration::from_secs(u64::from(mins) * 60));
    let mut last_trim_index: HashMap<String, usize> = HashMap::new();

    // Buffering
    let mut buffer: VecDeque<BusMessage> = VecDeque::new();
    let buffer_interval = Duration::from_millis(u64::from(config.buffer_interval_ms.unwrap_or(0)));

    // A sleep used to trigger periodic flushing of the buffer.
    // When `buffer_interval` is zero we skip using the timer and flush immediately
    // after every message.
    let flush_timer = tokio::time::sleep(buffer_interval);
    tokio::pin!(flush_timer);

    loop {
        tokio::select! {
            maybe_msg = rx.recv() => {
                if let Some(msg) = maybe_msg {
                    if msg.topic == CLOSE_TOPIC {
                        log::debug!("Received close message");
                        // Ensure we exit the loop after flushing any remaining messages.
                        if !buffer.is_empty() {
                            drain_buffer(
                                &mut con,
                                &stream_key,
                                config.stream_per_topic,
                                autotrim_duration,
                                &mut last_trim_index,
                                &mut buffer,
                            ).await?;
                        }
                        break;
                    }

                    buffer.push_back(msg);

                    if buffer_interval.is_zero() {
                        // Immediate flush mode
                        drain_buffer(
                            &mut con,
                            &stream_key,
                            config.stream_per_topic,
                            autotrim_duration,
                            &mut last_trim_index,
                            &mut buffer,
                        ).await?;
                    }
                } else {
                    log::debug!("Channel hung up");
                    break;
                }
            }
            // Only poll the timer when the interval is non-zero. This avoids
            // unnecessarily waking the task when immediate flushing is enabled.
            () = &mut flush_timer, if !buffer_interval.is_zero() => {
                if !buffer.is_empty() {
                    drain_buffer(
                        &mut con,
                        &stream_key,
                        config.stream_per_topic,
                        autotrim_duration,
                        &mut last_trim_index,
                        &mut buffer,
                    ).await?;
                }

                // Schedule the next tick
                flush_timer.as_mut().reset(tokio::time::Instant::now() + buffer_interval);
            }
        }
    }

    // Drain any remaining messages
    if !buffer.is_empty() {
        drain_buffer(
            &mut con,
            &stream_key,
            config.stream_per_topic,
            autotrim_duration,
            &mut last_trim_index,
            &mut buffer,
        )
        .await?;
    }

    log_task_stopped(MSGBUS_PUBLISH);
    Ok(())
}

async fn drain_buffer(
    conn: &mut redis::aio::ConnectionManager,
    stream_key: &str,
    stream_per_topic: bool,
    autotrim_duration: Option<Duration>,
    last_trim_index: &mut HashMap<String, usize>,
    buffer: &mut VecDeque<BusMessage>,
) -> anyhow::Result<()> {
    let mut pipe = redis::pipe();
    pipe.atomic();

    for msg in buffer.drain(..) {
        let encoding = msg.encoding.to_string();
        let items: Vec<(&str, &[u8])> = vec![
            ("topic", msg.topic.as_ref()),
            ("type", msg.payload_type.as_str().as_bytes()),
            ("payload", msg.payload.as_ref()),
            ("encoding", encoding.as_bytes()),
        ];
        let stream_key = if stream_per_topic {
            format!("{stream_key}:{}", msg.topic)
        } else {
            stream_key.to_string()
        };
        pipe.xadd(&stream_key, "*", &items);

        if autotrim_duration.is_none() {
            continue; // Nothing else to do
        }

        // Autotrim stream
        let last_trim_ms = last_trim_index.entry(stream_key.clone()).or_insert(0); // Remove clone
        let unix_duration_now = duration_since_unix_epoch();
        let trim_buffer = Duration::from_secs(TRIM_BUFFER_SECS);

        // Improve efficiency of this by batching
        if *last_trim_ms < unix_duration_now.saturating_sub(trim_buffer).as_millis() as usize {
            let min_timestamp_ms = unix_duration_now
                .saturating_sub(autotrim_duration.unwrap())
                .as_millis() as usize;
            let result: Result<(), redis::RedisError> = redis::cmd(REDIS_XTRIM)
                .arg(stream_key.clone())
                .arg(REDIS_MINID)
                .arg(min_timestamp_ms)
                .query_async(conn)
                .await;

            if let Err(e) = result {
                log::error!("Error trimming stream '{stream_key}': {e}");
            } else {
                last_trim_index.insert(stream_key.clone(), unix_duration_now.as_millis() as usize);
            }
        }
    }

    pipe.query_async(conn).await.map_err(anyhow::Error::from)
}

/// Streams messages from Redis streams and sends them over the provided `tx` channel.
///
/// # Errors
///
/// Returns an error if:
/// - Establishing the Redis connection fails before the terminate signal is received.
/// - A Redis read operation returns a non-retryable error.
pub async fn stream_messages(
    tx: tokio::sync::mpsc::Sender<BusMessage>,
    config: RedisMessageBusConfig,
    stream_keys: Vec<String>,
    stream_signal: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    log_task_started(MSGBUS_STREAM);

    let Some(mut con) = connect_stream_connection(&config, &stream_signal).await? else {
        log_task_stopped(MSGBUS_STREAM);
        return Ok(());
    };

    let mut read_error_count = 0;

    let stream_keys = &stream_keys
        .iter()
        .map(String::as_str)
        .collect::<Vec<&str>>();

    log::debug!("Listening to streams: [{}]", stream_keys.join(", "));

    // Start streaming from current timestamp
    let clock = get_atomic_clock_realtime();
    let timestamp_ms = clock.get_time_ms();
    let initial_id = timestamp_ms.to_string();

    let mut last_ids: HashMap<String, String> = stream_keys
        .iter()
        .map(|&key| (key.to_string(), initial_id.clone()))
        .collect();

    let opts = StreamReadOptions::default().block(100);

    'outer: loop {
        if stream_signal.load(Ordering::Relaxed) {
            log::debug!("Received streaming terminate signal");
            break;
        }

        let ids: Vec<String> = stream_keys
            .iter()
            .map(|&key| last_ids[key].clone())
            .collect();
        let id_refs: Vec<&str> = ids.iter().map(String::as_str).collect();

        let result: Result<RedisStreamBulk, _> =
            con.xread_options(&[&stream_keys], &[&id_refs], &opts).await;

        match result {
            Ok(stream_bulk) => {
                read_error_count = 0;

                if stream_bulk.is_empty() {
                    // Timeout occurred: no messages received
                    continue;
                }

                for entry in &stream_bulk {
                    for (stream_key, stream_msgs) in entry {
                        for stream_msg in stream_msgs {
                            for (id, array) in stream_msg {
                                last_ids.insert(stream_key.clone(), id.clone());

                                match decode_bus_message(array) {
                                    Ok(msg) => {
                                        if let Err(e) = tx.send(msg).await {
                                            log::debug!("Channel closed: {e:?}");
                                            break 'outer; // End streaming
                                        }
                                    }
                                    Err(e) => {
                                        log::error!("{e:?}");
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                if !is_retryable_stream_error(&e) {
                    anyhow::bail!("Error reading from stream: {e:?}");
                }

                log::error!("Error reading from stream: {e:?}");

                let Some(reconnected) =
                    reconnect_stream_connection(&config, &stream_signal, &mut read_error_count)
                        .await?
                else {
                    break;
                };
                con = reconnected;
            }
        }
    }

    log_task_stopped(MSGBUS_STREAM);
    Ok(())
}

async fn connect_stream_connection(
    config: &RedisMessageBusConfig,
    stream_signal: &Arc<AtomicBool>,
) -> anyhow::Result<Option<ConnectionManager>> {
    let connect = create_redis_connection(MSGBUS_STREAM, config);
    let terminate = wait_for_stream_signal(stream_signal);

    tokio::pin!(connect);
    tokio::pin!(terminate);

    tokio::select! {
        result = &mut connect => result.map(Some),
        () = &mut terminate => Ok(None),
    }
}

async fn reconnect_stream_connection(
    config: &RedisMessageBusConfig,
    stream_signal: &Arc<AtomicBool>,
    read_error_count: &mut usize,
) -> anyhow::Result<Option<ConnectionManager>> {
    loop {
        let retry_delay = stream_retry_delay(config, *read_error_count);
        *read_error_count = (*read_error_count).saturating_add(1);

        if !wait_for_retry_delay(retry_delay, stream_signal).await {
            return Ok(None);
        }

        match connect_stream_connection(config, stream_signal).await {
            Ok(Some(con)) => return Ok(Some(con)),
            Ok(None) => return Ok(None),
            Err(e) => {
                log::error!("Error reconnecting to stream: {e:?}");
            }
        }
    }
}

fn stream_retry_delay(config: &RedisMessageBusConfig, attempt: usize) -> Duration {
    let exponent = u32::try_from(attempt.min(32)).unwrap_or(32);
    let delay_ms = config
        .factor
        .saturating_mul(config.exponent_base.saturating_pow(exponent));
    let max_delay = Duration::from_secs(config.max_delay);

    Duration::from_millis(delay_ms)
        .min(max_delay)
        .max(Duration::from_millis(1))
}

fn is_retryable_stream_error(error: &redis::RedisError) -> bool {
    matches!(
        error.retry_method(),
        RetryMethod::Reconnect
            | RetryMethod::ReconnectFromInitialConnections
            | RetryMethod::RetryImmediately
            | RetryMethod::WaitAndRetry
    )
}

async fn wait_for_retry_delay(retry_delay: Duration, stream_signal: &Arc<AtomicBool>) -> bool {
    let retry_timer = tokio::time::sleep(retry_delay);
    let terminate = wait_for_stream_signal(stream_signal);

    tokio::pin!(retry_timer);
    tokio::pin!(terminate);

    tokio::select! {
        () = &mut retry_timer => true,
        () = &mut terminate => false,
    }
}

async fn wait_for_stream_signal(stream_signal: &Arc<AtomicBool>) {
    let check_timer = tokio::time::interval(Duration::from_millis(100));

    tokio::pin!(check_timer);

    while !stream_signal.load(Ordering::Relaxed) {
        check_timer.tick().await;
    }
}

// Redis fields are unordered, and older streams may omit type or encoding headers
fn decode_bus_message(stream_msg: &redis::Value) -> anyhow::Result<BusMessage> {
    let redis::Value::Array(fields) = stream_msg else {
        anyhow::bail!("Invalid stream message format: {stream_msg:?}");
    };

    if fields.len() < 4 || fields.len() % 2 != 0 {
        anyhow::bail!("Invalid stream message format: {stream_msg:?}");
    }

    let mut topic: Option<String> = None;
    let mut payload_type = BusPayloadType::Custom(Ustr::default());
    let mut encoding = SerializationEncoding::default();
    let mut payload: Option<Bytes> = None;

    for pair in fields.chunks_exact(2) {
        let redis::Value::BulkString(key) = &pair[0] else {
            anyhow::bail!("Invalid stream field key: {stream_msg:?}");
        };

        match key.as_slice() {
            b"topic" => {
                let redis::Value::BulkString(bytes) = &pair[1] else {
                    anyhow::bail!("Invalid topic format: {stream_msg:?}");
                };
                topic = Some(
                    String::from_utf8(bytes.clone())
                        .map_err(|e| anyhow::anyhow!("Error parsing topic: {e}"))?,
                );
            }
            b"type" => {
                let redis::Value::BulkString(bytes) = &pair[1] else {
                    anyhow::bail!("Invalid type format: {stream_msg:?}");
                };
                let type_name = std::str::from_utf8(bytes)
                    .map_err(|e| anyhow::anyhow!("Error parsing type: {e}"))?;
                payload_type = BusPayloadType::from_name(type_name);
            }
            b"encoding" => {
                let redis::Value::BulkString(bytes) = &pair[1] else {
                    anyhow::bail!("Invalid encoding format: {stream_msg:?}");
                };
                let value = std::str::from_utf8(bytes)
                    .map_err(|e| anyhow::anyhow!("Error parsing encoding: {e}"))?;
                encoding = value
                    .parse()
                    .map_err(|e| anyhow::anyhow!("Error parsing encoding: {e}"))?;
            }
            b"payload" => {
                let redis::Value::BulkString(bytes) = &pair[1] else {
                    anyhow::bail!("Invalid payload format: {stream_msg:?}");
                };
                payload = Some(Bytes::copy_from_slice(bytes));
            }
            _ => {}
        }
    }

    let Some(topic) = topic else {
        anyhow::bail!("Stream message missing topic: {stream_msg:?}");
    };
    let Some(payload) = payload else {
        anyhow::bail!("Stream message missing payload: {stream_msg:?}");
    };

    Ok(BusMessage::with_str_topic(
        topic,
        payload_type,
        payload,
        encoding,
    ))
}

async fn run_heartbeat(
    heartbeat_interval_secs: u16,
    signal: Arc<AtomicBool>,
    pub_tx: tokio::sync::mpsc::UnboundedSender<BusMessage>,
) {
    log_task_started("heartbeat");
    log::debug!("Heartbeat at {heartbeat_interval_secs} second intervals");

    let heartbeat_interval = Duration::from_secs(u64::from(heartbeat_interval_secs));
    let heartbeat_timer = tokio::time::interval(heartbeat_interval);

    let check_interval = Duration::from_millis(100);
    let check_timer = tokio::time::interval(check_interval);

    tokio::pin!(heartbeat_timer);
    tokio::pin!(check_timer);

    loop {
        if signal.load(Ordering::Relaxed) {
            log::debug!("Received heartbeat terminate signal");
            break;
        }

        tokio::select! {
            _ = heartbeat_timer.tick() => {
                let heartbeat = create_heartbeat_msg();
                if let Err(e) = pub_tx.send(heartbeat) {
                    // We expect an error if the channel is closed during shutdown
                    log::debug!("Error sending heartbeat: {e}");
                }
            },
            _ = check_timer.tick() => {}
        }
    }

    log_task_stopped("heartbeat");
}

fn create_heartbeat_msg() -> BusMessage {
    let payload = Bytes::from(chrono::Utc::now().to_rfc3339().into_bytes());
    BusMessage::with_str_topic(
        HEARTBEAT_TOPIC,
        BusPayloadType::Custom(Ustr::default()),
        payload,
        SerializationEncoding::default(),
    )
}

#[cfg(test)]
mod tests {
    use nautilus_common::{msgbus::external_io_from_backing, testing::wait_until_async};
    use redis::Value;
    use rstest::*;
    use serde_json::json;

    use super::*;

    #[rstest]
    fn test_default_redis_message_bus_config() {
        let config = RedisMessageBusConfig::default();

        assert_eq!(config.host, None);
        assert_eq!(config.port, None);
        assert_eq!(config.username, None);
        assert_eq!(config.password, None);
        assert!(!config.ssl);
        assert_eq!(config.connection_timeout, 20);
        assert_eq!(config.response_timeout, 20);
        assert_eq!(config.number_of_retries, 100);
        assert_eq!(config.exponent_base, 2);
        assert_eq!(config.max_delay, 1000);
        assert_eq!(config.factor, 2);
    }

    #[rstest]
    fn test_deserialize_redis_message_bus_config() {
        let config_json = json!({
            "host": "localhost",
            "port": 6379,
            "username": "user",
            "password": "pass",
            "ssl": true,
            "connection_timeout": 30,
            "response_timeout": 10,
            "number_of_retries": 3,
            "exponent_base": 2,
            "max_delay": 10,
            "factor": 2
        });

        let config: RedisMessageBusConfig = serde_json::from_value(config_json).unwrap();

        assert_eq!(config.host, Some("localhost".to_string()));
        assert_eq!(config.port, Some(6379));
        assert_eq!(config.username, Some("user".to_string()));
        assert_eq!(config.password, Some("pass".to_string()));
        assert!(config.ssl);
        assert_eq!(config.connection_timeout, 30);
        assert_eq!(config.response_timeout, 10);
        assert_eq!(config.number_of_retries, 3);
        assert_eq!(config.exponent_base, 2);
        assert_eq!(config.max_delay, 10);
        assert_eq!(config.factor, 2);
    }

    #[rstest]
    fn test_deserialize_redis_message_bus_config_rejects_type_selector() {
        let config_json = json!({
            "type": "redis",
        });

        let error = serde_json::from_value::<RedisMessageBusConfig>(config_json).unwrap_err();

        assert!(error.to_string().contains("unknown field `type`"));
    }

    #[rstest]
    fn test_decode_bus_message_valid() {
        let stream_msg = Value::Array(vec![
            Value::BulkString(b"topic".to_vec()),
            Value::BulkString(b"topic1".to_vec()),
            Value::BulkString(b"type".to_vec()),
            Value::BulkString(b"QuoteTick".to_vec()),
            Value::BulkString(b"payload".to_vec()),
            Value::BulkString(b"data1".to_vec()),
            Value::BulkString(b"encoding".to_vec()),
            Value::BulkString(b"msgpack".to_vec()),
        ]);

        let result = decode_bus_message(&stream_msg);
        assert!(result.is_ok());
        let msg = result.unwrap();
        assert_eq!(msg.topic, "topic1");
        assert_eq!(msg.payload_type, BusPayloadType::QuoteTick);
        assert_eq!(msg.encoding, SerializationEncoding::MsgPack);
        assert_eq!(msg.payload, Bytes::from("data1"));
    }

    #[rstest]
    fn test_decode_bus_message_defaults_legacy_headers() {
        let stream_msg = Value::Array(vec![
            Value::BulkString(b"topic".to_vec()),
            Value::BulkString(b"topic1".to_vec()),
            Value::BulkString(b"payload".to_vec()),
            Value::BulkString(b"data1".to_vec()),
        ]);

        let result = decode_bus_message(&stream_msg);
        assert!(result.is_ok());
        let msg = result.unwrap();
        assert_eq!(msg.topic, "topic1");
        assert_eq!(msg.payload_type, BusPayloadType::Custom(Ustr::default()));
        assert_eq!(msg.encoding, SerializationEncoding::Json);
        assert_eq!(msg.payload, Bytes::from("data1"));
    }

    #[rstest]
    fn test_decode_bus_message_unknown_type_is_custom() {
        let stream_msg = Value::Array(vec![
            Value::BulkString(b"topic".to_vec()),
            Value::BulkString(b"topic1".to_vec()),
            Value::BulkString(b"type".to_vec()),
            Value::BulkString(b"UnknownPayload".to_vec()),
            Value::BulkString(b"payload".to_vec()),
            Value::BulkString(b"data1".to_vec()),
        ]);

        let result = decode_bus_message(&stream_msg);
        assert!(result.is_ok());
        let msg = result.unwrap();
        assert_eq!(
            msg.payload_type,
            BusPayloadType::Custom(Ustr::from("UnknownPayload"))
        );
        assert_eq!(msg.encoding, SerializationEncoding::Json);
    }

    #[rstest]
    fn test_decode_bus_message_accepts_unordered_metadata_fields() {
        let stream_msg = Value::Array(vec![
            Value::BulkString(b"payload".to_vec()),
            Value::BulkString(b"data1".to_vec()),
            Value::BulkString(b"encoding".to_vec()),
            Value::BulkString(b"msgpack".to_vec()),
            Value::BulkString(b"type".to_vec()),
            Value::BulkString(b"TradeTick".to_vec()),
            Value::BulkString(b"topic".to_vec()),
            Value::BulkString(b"topic1".to_vec()),
        ]);

        let msg = decode_bus_message(&stream_msg).unwrap();

        assert_eq!(msg.topic, "topic1");
        assert_eq!(msg.payload_type, BusPayloadType::TradeTick);
        assert_eq!(msg.encoding, SerializationEncoding::MsgPack);
        assert_eq!(msg.payload, Bytes::from("data1"));
    }

    #[rstest]
    fn test_decode_bus_message_rejects_invalid_encoding_header() {
        let stream_msg = Value::Array(vec![
            Value::BulkString(b"topic".to_vec()),
            Value::BulkString(b"topic1".to_vec()),
            Value::BulkString(b"encoding".to_vec()),
            Value::BulkString(b"invalid".to_vec()),
            Value::BulkString(b"payload".to_vec()),
            Value::BulkString(b"data1".to_vec()),
        ]);

        let error = decode_bus_message(&stream_msg).unwrap_err();

        assert!(
            error.to_string().contains("Error parsing encoding"),
            "{error:?}"
        );
    }

    #[rstest]
    fn test_decode_bus_message_missing_fields() {
        let stream_msg = Value::Array(vec![
            Value::BulkString(b"0".to_vec()),
            Value::BulkString(b"topic1".to_vec()),
        ]);

        let result = decode_bus_message(&stream_msg);
        assert!(result.is_err());
        assert_eq!(
            format!("{}", result.unwrap_err()),
            "Invalid stream message format: array([bulk-string('\"0\"'), bulk-string('\"topic1\"')])"
        );
    }

    #[rstest]
    fn test_decode_bus_message_invalid_topic_format() {
        let stream_msg = Value::Array(vec![
            Value::BulkString(b"topic".to_vec()),
            Value::Int(42),
            Value::BulkString(b"payload".to_vec()),
            Value::BulkString(b"data1".to_vec()),
        ]);

        let result = decode_bus_message(&stream_msg);
        assert!(result.is_err());
        assert_eq!(
            format!("{}", result.unwrap_err()),
            "Invalid topic format: array([bulk-string('\"topic\"'), int(42), bulk-string('\"payload\"'), bulk-string('\"data1\"')])"
        );
    }

    #[rstest]
    fn test_decode_bus_message_invalid_type_format() {
        let stream_msg = Value::Array(vec![
            Value::BulkString(b"topic".to_vec()),
            Value::BulkString(b"topic1".to_vec()),
            Value::BulkString(b"type".to_vec()),
            Value::Int(42),
            Value::BulkString(b"payload".to_vec()),
            Value::BulkString(b"data1".to_vec()),
        ]);

        let result = decode_bus_message(&stream_msg);
        assert!(result.is_err());
        assert_eq!(
            format!("{}", result.unwrap_err()),
            "Invalid type format: array([bulk-string('\"topic\"'), bulk-string('\"topic1\"'), bulk-string('\"type\"'), int(42), bulk-string('\"payload\"'), bulk-string('\"data1\"')])"
        );
    }

    #[rstest]
    fn test_decode_bus_message_invalid_encoding_format() {
        let stream_msg = Value::Array(vec![
            Value::BulkString(b"topic".to_vec()),
            Value::BulkString(b"topic1".to_vec()),
            Value::BulkString(b"encoding".to_vec()),
            Value::Int(42),
            Value::BulkString(b"payload".to_vec()),
            Value::BulkString(b"data1".to_vec()),
        ]);

        let result = decode_bus_message(&stream_msg);
        assert!(result.is_err());
        assert_eq!(
            format!("{}", result.unwrap_err()),
            "Invalid encoding format: array([bulk-string('\"topic\"'), bulk-string('\"topic1\"'), bulk-string('\"encoding\"'), int(42), bulk-string('\"payload\"'), bulk-string('\"data1\"')])"
        );
    }

    #[rstest]
    fn test_decode_bus_message_invalid_payload_format() {
        let stream_msg = Value::Array(vec![
            Value::BulkString(b"topic".to_vec()),
            Value::BulkString(b"topic1".to_vec()),
            Value::BulkString(b"payload".to_vec()),
            Value::Int(42),
        ]);

        let result = decode_bus_message(&stream_msg);
        assert!(result.is_err());
        assert_eq!(
            format!("{}", result.unwrap_err()),
            "Invalid payload format: array([bulk-string('\"topic\"'), bulk-string('\"topic1\"'), bulk-string('\"payload\"'), int(42)])"
        );
    }

    #[rstest]
    fn test_decode_bus_message_invalid_stream_msg_format() {
        let stream_msg = Value::BulkString(b"not an array".to_vec());

        let result = decode_bus_message(&stream_msg);
        assert!(result.is_err());
        assert_eq!(
            format!("{}", result.unwrap_err()),
            "Invalid stream message format: bulk-string('\"not an array\"')"
        );
    }

    #[rstest]
    fn test_new_rejects_zero_heartbeat_interval() {
        let trader_id = TraderId::from("tester-001");
        let instance_id = UUID4::new();
        let config = MessageBusConfig {
            heartbeat_interval_secs: Some(0),
            ..Default::default()
        };

        let result = RedisMessageBusBacking::new(
            trader_id,
            instance_id,
            config,
            RedisMessageBusConfig::default(),
        );

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "heartbeat_interval_secs must be greater than 0"
        );
    }

    #[rstest]
    fn test_stream_retry_delay_uses_config_bounds() {
        let config = RedisMessageBusConfig {
            factor: 10,
            exponent_base: 2,
            max_delay: 1,
            ..Default::default()
        };

        assert_eq!(stream_retry_delay(&config, 0), Duration::from_millis(10));
        assert_eq!(stream_retry_delay(&config, 1), Duration::from_millis(20));
        assert_eq!(stream_retry_delay(&config, 10), Duration::from_secs(1));
    }

    #[rstest]
    fn test_stream_error_retry_classification() {
        let dropped =
            redis::RedisError::from(std::io::Error::from(std::io::ErrorKind::ConnectionReset));
        let client: redis::RedisError = (redis::ErrorKind::Client, "client error").into();

        assert!(is_retryable_stream_error(&dropped));
        assert!(!is_retryable_stream_error(&client));
    }

    #[tokio::test]
    async fn test_wait_for_retry_delay_returns_false_when_signaled() {
        let stream_signal = Arc::new(AtomicBool::new(true));
        let signal = stream_signal.clone();
        let fut = async move { wait_for_retry_delay(Duration::from_secs(30), &signal).await };

        let handle = tokio::spawn(fut);

        wait_until_async(|| async { handle.is_finished() }, Duration::from_secs(1)).await;

        assert!(!handle.await.unwrap());
    }

    #[rstest]
    fn test_external_io_from_backing_takes_stream_receiver() {
        let (stream_tx, stream_rx) = tokio::sync::mpsc::channel::<BusMessage>(1);
        let backing = backing_with_stream_receiver(stream_rx);
        let message = BusMessage::with_str_topic(
            "events/data",
            BusPayloadType::QuoteTick,
            Bytes::from_static(b"payload"),
            SerializationEncoding::Json,
        );

        let (_egress, mut ingress) = external_io_from_backing(Box::new(backing));
        stream_tx.try_send(message.clone()).unwrap();
        let mut receiver = ingress.take_receiver().unwrap();
        let received = receiver.try_recv().unwrap();

        assert_eq!(received.topic, message.topic);
        assert_eq!(received.payload, message.payload);
        assert!(ingress.take_receiver().is_err());
    }

    fn backing_with_stream_receiver(
        stream_rx: tokio::sync::mpsc::Receiver<BusMessage>,
    ) -> RedisMessageBusBacking {
        let (pub_tx, _pub_rx) = tokio::sync::mpsc::unbounded_channel::<BusMessage>();
        RedisMessageBusBacking {
            trader_id: TraderId::from("tester-001"),
            instance_id: UUID4::new(),
            pub_tx,
            pub_handle: None,
            stream_rx: Some(stream_rx),
            stream_handle: None,
            stream_signal: Arc::new(AtomicBool::new(false)),
            heartbeat_handle: None,
            heartbeat_signal: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[cfg(target_os = "linux")] // Run Redis tests on Linux platforms only
#[cfg(test)]
mod serial_tests {
    use std::{sync::mpsc, thread};

    use nautilus_common::{
        enums::Environment,
        msgbus::{self, TypedHandler},
        testing::wait_until_async,
    };
    use nautilus_live::{
        builder::LiveNodeBuilder,
        config::{LiveExecEngineConfig, LiveNodeConfig},
    };
    use nautilus_model::data::{QuoteTick, TradeTick};
    use redis::aio::ConnectionManager;
    use rstest::*;

    use super::*;

    #[fixture]
    async fn redis_connection() -> ConnectionManager {
        let config = RedisMessageBusConfig::default();
        create_redis_connection(MSGBUS_STREAM, &config)
            .await
            .unwrap()
    }

    #[rstest]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_stream_messages_terminate_signal(#[future] redis_connection: ConnectionManager) {
        let _con = redis_connection.await;
        let (tx, mut rx) = tokio::sync::mpsc::channel::<BusMessage>(100);

        let trader_id = TraderId::from("tester-001");
        let instance_id = UUID4::new();
        let config = MessageBusConfig {
            use_instance_id: true,
            ..Default::default()
        };

        let stream_key = get_stream_key(trader_id, instance_id, &config);
        let external_streams = vec![stream_key.clone()];
        let stream_signal = Arc::new(AtomicBool::new(false));
        let stream_signal_clone = stream_signal.clone();

        // Start the message streaming task
        let handle = tokio::spawn(async move {
            stream_messages(
                tx,
                RedisMessageBusConfig::default(),
                external_streams,
                stream_signal_clone,
            )
            .await
            .unwrap();
        });

        stream_signal.store(true, Ordering::Relaxed);
        let _ = rx.recv().await; // Wait for the tx to close

        // Shutdown and cleanup
        rx.close();
        handle.await.unwrap();
    }

    #[rstest]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_stream_messages_when_receiver_closed(
        #[future] redis_connection: ConnectionManager,
    ) {
        let mut con = redis_connection.await;
        let (tx, mut rx) = tokio::sync::mpsc::channel::<BusMessage>(100);

        let trader_id = TraderId::from("tester-001");
        let instance_id = UUID4::new();
        let config = MessageBusConfig {
            use_instance_id: true,
            ..Default::default()
        };

        let stream_key = get_stream_key(trader_id, instance_id, &config);
        let external_streams = vec![stream_key.clone()];
        let stream_signal = Arc::new(AtomicBool::new(false));
        let stream_signal_clone = stream_signal.clone();

        // Use a message ID in the future, as streaming begins
        // around the timestamp the task is spawned.
        let clock = get_atomic_clock_realtime();
        let future_id = (clock.get_time_ms() + 1_000_000).to_string();

        // Publish test message
        let _: () = con
            .xadd(
                stream_key,
                future_id,
                &[("topic", "topic1"), ("payload", "data1")],
            )
            .await
            .unwrap();

        // Immediately close channel
        rx.close();

        // Start the message streaming task
        let handle = tokio::spawn(async move {
            stream_messages(
                tx,
                RedisMessageBusConfig::default(),
                external_streams,
                stream_signal_clone,
            )
            .await
            .unwrap();
        });

        // Shutdown and cleanup
        handle.await.unwrap();
    }

    #[rstest]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_stream_messages(#[future] redis_connection: ConnectionManager) {
        let mut con = redis_connection.await;
        let (tx, mut rx) = tokio::sync::mpsc::channel::<BusMessage>(100);

        let trader_id = TraderId::from("tester-001");
        let instance_id = UUID4::new();
        let config = MessageBusConfig {
            use_instance_id: true,
            ..Default::default()
        };

        let stream_key = get_stream_key(trader_id, instance_id, &config);
        let external_streams = vec![stream_key.clone()];
        let stream_signal = Arc::new(AtomicBool::new(false));
        let stream_signal_clone = stream_signal.clone();

        // Use a message ID in the future, as streaming begins
        // around the timestamp the task is spawned.
        let clock = get_atomic_clock_realtime();
        let future_id = (clock.get_time_ms() + 1_000_000).to_string();

        // Publish test message
        let _: () = con
            .xadd(
                stream_key,
                future_id,
                &[("topic", "topic1"), ("payload", "data1")],
            )
            .await
            .unwrap();

        // Start the message streaming task
        let handle = tokio::spawn(async move {
            stream_messages(
                tx,
                RedisMessageBusConfig::default(),
                external_streams,
                stream_signal_clone,
            )
            .await
            .unwrap();
        });

        // Receive and verify the message
        let msg = receive_bus_message(&mut rx, Duration::from_secs(2)).await;
        assert_eq!(msg.topic, "topic1");
        assert_eq!(msg.payload, Bytes::from("data1"));

        // Shutdown and cleanup
        rx.close();
        stream_signal.store(true, Ordering::Relaxed);
        handle.await.unwrap();
    }

    #[rstest]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_stream_messages_skips_malformed_entry(
        #[future] redis_connection: ConnectionManager,
    ) {
        let mut con = redis_connection.await;
        let (tx, mut rx) = tokio::sync::mpsc::channel::<BusMessage>(100);

        let suffix = UUID4::new();
        let stream_key = format!("test:stream:malformed:{suffix}");
        let external_streams = vec![stream_key.clone()];
        let stream_signal = Arc::new(AtomicBool::new(false));
        let stream_signal_clone = stream_signal.clone();

        let clock = get_atomic_clock_realtime();
        let base_id = clock.get_time_ms() + 1_000_000;

        let _: () = con
            .xadd(
                &stream_key,
                format!("{}", base_id + 1),
                &[("topic", "missing-payload")],
            )
            .await
            .unwrap();
        let _: () = con
            .xadd(
                &stream_key,
                format!("{}", base_id + 2),
                &[("topic", "valid"), ("payload", "data")],
            )
            .await
            .unwrap();

        let handle = tokio::spawn(async move {
            stream_messages(
                tx,
                RedisMessageBusConfig::default(),
                external_streams,
                stream_signal_clone,
            )
            .await
            .unwrap();
        });

        let msg = receive_bus_message(&mut rx, Duration::from_secs(2)).await;

        rx.close();
        stream_signal.store(true, Ordering::Relaxed);
        handle.await.unwrap();

        assert_eq!(msg.topic, "valid");
        assert_eq!(msg.payload, Bytes::from("data"));
    }

    #[rstest]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_stream_messages_returns_unrecoverable_read_error(
        #[future] redis_connection: ConnectionManager,
    ) {
        let mut con = redis_connection.await;
        let (tx, _rx) = tokio::sync::mpsc::channel::<BusMessage>(100);

        let suffix = UUID4::new();
        let stream_key = format!("test:stream:wrong-type:{suffix}");
        let external_streams = vec![stream_key.clone()];
        let stream_signal = Arc::new(AtomicBool::new(false));

        let _: () = con.set(&stream_key, "not-a-stream").await.unwrap();

        let result = stream_messages(
            tx,
            RedisMessageBusConfig::default(),
            external_streams,
            stream_signal,
        )
        .await;

        let _: () = con.del(&stream_key).await.unwrap();

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Error reading from stream")
        );
    }

    #[rstest]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_stream_connection_returns_none_when_signaled() {
        let config = RedisMessageBusConfig {
            port: Some(1),
            connection_timeout: 20,
            ..Default::default()
        };
        let stream_signal = Arc::new(AtomicBool::new(true));
        let signal = stream_signal.clone();
        let handle = tokio::spawn(async move { connect_stream_connection(&config, &signal).await });

        wait_until_async(|| async { handle.is_finished() }, Duration::from_secs(1)).await;

        assert!(handle.await.unwrap().unwrap().is_none());
    }

    #[rstest]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_publish_messages(#[future] redis_connection: ConnectionManager) {
        let mut con = redis_connection.await;
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<BusMessage>();

        let trader_id = TraderId::from("tester-001");
        let instance_id = UUID4::new();
        let config = MessageBusConfig {
            use_instance_id: true,
            stream_per_topic: false,
            ..Default::default()
        };
        let stream_key = get_stream_key(trader_id, instance_id, &config);

        // Start the publish_messages task
        let handle = tokio::spawn(async move {
            publish_messages(
                rx,
                trader_id,
                instance_id,
                config,
                RedisMessageBusConfig::default(),
            )
            .await
            .unwrap();
        });

        // Send a test message
        let msg = BusMessage::with_str_topic(
            "test_topic",
            BusPayloadType::QuoteTick,
            Bytes::from("test_payload"),
            SerializationEncoding::Json,
        );
        tx.send(msg).unwrap();

        // Wait until the message is published to Redis
        wait_until_async(
            || {
                let mut con = con.clone();
                let stream_key = stream_key.clone();
                async move {
                    let messages: RedisStreamBulk =
                        con.xread(&[&stream_key], &["0"]).await.unwrap();
                    !messages.is_empty()
                }
            },
            Duration::from_secs(3),
        )
        .await;

        // Verify the message was published to Redis
        let messages: RedisStreamBulk = con.xread(&[&stream_key], &["0"]).await.unwrap();
        assert_eq!(messages.len(), 1);
        let stream_msgs = messages[0].get(&stream_key).unwrap();
        let stream_msg_array = &stream_msgs[0].values().next().unwrap();
        let decoded_message = decode_bus_message(stream_msg_array).unwrap();
        assert_eq!(decoded_message.topic, "test_topic");
        assert_eq!(decoded_message.payload_type, BusPayloadType::QuoteTick);
        assert_eq!(decoded_message.encoding, SerializationEncoding::Json);
        assert_eq!(decoded_message.payload, Bytes::from("test_payload"));

        // Stop publishing task
        let msg = BusMessage::new_close();
        tx.send(msg).unwrap();

        // Shutdown and cleanup
        handle.await.unwrap();
    }

    #[rstest]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_two_live_nodes_publish_and_ingest_external_redis_stream(
        #[future] redis_connection: ConnectionManager,
    ) {
        let _con = redis_connection.await;
        let redis_config = RedisMessageBusConfig::default();
        let trader_a = TraderId::from("NODEA-001");
        let instance_a = UUID4::new();
        let node_a_msgbus = MessageBusConfig {
            use_instance_id: true,
            stream_per_topic: false,
            ..Default::default()
        };
        let stream_key = get_stream_key(trader_a, instance_a, &node_a_msgbus);
        let node_b_msgbus = MessageBusConfig {
            external_streams: Some(vec![stream_key]),
            stream_per_topic: false,
            ..Default::default()
        };
        let quote = QuoteTick::default();
        let trade = TradeTick::default();
        let (ready_tx, ready_rx) = mpsc::channel::<()>();
        let (quote_tx, quote_rx) = mpsc::channel::<QuoteTick>();
        let (trade_tx, trade_rx) = mpsc::channel::<TradeTick>();

        let node_b = thread::spawn({
            let redis_config = redis_config.clone();
            move || -> anyhow::Result<()> {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()?;

                runtime.block_on(async move {
                    let config = LiveNodeConfig {
                        environment: Environment::Sandbox,
                        trader_id: TraderId::from("NODEB-001"),
                        msgbus: Some(node_b_msgbus),
                        exec_engine: LiveExecEngineConfig {
                            reconciliation: false,
                            ..Default::default()
                        },
                        delay_post_stop: Duration::ZERO,
                        timeout_connection: Duration::from_millis(500),
                        timeout_disconnection: Duration::from_millis(500),
                        ..Default::default()
                    };
                    let mut node = LiveNodeBuilder::from_config(config)?
                        .with_external_msgbus_factory(Box::new(RedisMessageBusFactory::new(
                            redis_config,
                        )))
                        .build()?;
                    let handle = node.handle();
                    let quote_handler = TypedHandler::from({
                        let quote_tx = quote_tx.clone();
                        let handle = handle.clone();
                        move |quote: &QuoteTick| {
                            let _ = quote_tx.send(*quote);
                            handle.stop();
                        }
                    });
                    let trade_handler = TypedHandler::from(move |trade: &TradeTick| {
                        let _ = trade_tx.send(*trade);
                    });

                    msgbus::subscribe_quotes("data.quotes.*".into(), quote_handler, None);
                    msgbus::subscribe_trades("data.trades.*".into(), trade_handler, None);
                    msgbus::get_message_bus()
                        .borrow_mut()
                        .add_streaming_type(BusPayloadType::QuoteTick);
                    let result = tokio::time::timeout(Duration::from_secs(10), async {
                        let run = node.run();
                        tokio::pin!(run);

                        let announce_ready = async {
                            for _ in 0..100 {
                                if handle.is_running() {
                                    ready_tx.send(())?;
                                    return Ok(());
                                }
                                tokio::time::sleep(Duration::from_millis(10)).await;
                            }

                            anyhow::bail!("node B did not reach running state")
                        };

                        tokio::select! {
                            result = &mut run => result,
                            ready = announce_ready => {
                                ready?;
                                run.await
                            }
                        }
                    })
                    .await;
                    msgbus::get_message_bus().borrow_mut().dispose();

                    match result {
                        Ok(Ok(())) => Ok(()),
                        Ok(Err(e)) => Err(e),
                        Err(e) => anyhow::bail!("node B timed out: {e}"),
                    }
                })
            }
        });

        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("node B should start Redis ingress");

        let node_a = thread::spawn(move || -> anyhow::Result<()> {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()?;

            runtime.block_on(async move {
                let config = LiveNodeConfig {
                    environment: Environment::Sandbox,
                    trader_id: trader_a,
                    instance_id: Some(instance_a),
                    msgbus: Some(node_a_msgbus),
                    exec_engine: LiveExecEngineConfig {
                        reconciliation: false,
                        ..Default::default()
                    },
                    delay_post_stop: Duration::ZERO,
                    timeout_connection: Duration::from_millis(500),
                    timeout_disconnection: Duration::from_millis(500),
                    ..Default::default()
                };
                let _node = LiveNodeBuilder::from_config(config)?
                    .with_external_msgbus_factory(Box::new(RedisMessageBusFactory::new(
                        redis_config,
                    )))
                    .build()?;

                msgbus::publish_trade("data.trades.TEST".into(), &trade);
                msgbus::publish_quote("data.quotes.TEST".into(), &quote);
                msgbus::get_message_bus().borrow_mut().dispose();

                Ok(())
            })
        });

        node_a
            .join()
            .expect("node A thread should not panic")
            .expect("node A should publish externally");
        let received_quote = quote_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("node B should republish the registered quote type");
        node_b
            .join()
            .expect("node B thread should not panic")
            .expect("node B should ingest and stop cleanly");

        assert_eq!(received_quote, quote);
        assert!(
            trade_rx.try_recv().is_err(),
            "unregistered trade type should not republish internally"
        );
    }

    #[rstest]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_stream_messages_multiple_streams(#[future] redis_connection: ConnectionManager) {
        let mut con = redis_connection.await;
        let (tx, mut rx) = tokio::sync::mpsc::channel::<BusMessage>(100);

        // Setup multiple stream keys
        let suffix = UUID4::new();
        let stream_key1 = format!("test:stream:{suffix}:1");
        let stream_key2 = format!("test:stream:{suffix}:2");
        let external_streams = vec![stream_key1.clone(), stream_key2.clone()];
        let stream_signal = Arc::new(AtomicBool::new(false));
        let stream_signal_clone = stream_signal.clone();

        let clock = get_atomic_clock_realtime();
        let base_id = clock.get_time_ms() + 1_000_000;

        // Start streaming task
        let handle = tokio::spawn(async move {
            stream_messages(
                tx,
                RedisMessageBusConfig::default(),
                external_streams,
                stream_signal_clone,
            )
            .await
            .unwrap();
        });

        // Publish to stream 1 at higher ID
        let _: () = con
            .xadd(
                &stream_key1,
                format!("{}", base_id + 100),
                &[("topic", "stream1-first"), ("payload", "data")],
            )
            .await
            .unwrap();

        let msg = receive_bus_message(&mut rx, Duration::from_secs(2)).await;
        assert_eq!(msg.topic, "stream1-first");

        // Publish to stream 2 at lower ID (tests independent cursor tracking)
        let _: () = con
            .xadd(
                &stream_key2,
                format!("{}", base_id + 50),
                &[("topic", "stream2-second"), ("payload", "data")],
            )
            .await
            .unwrap();

        let msg = receive_bus_message(&mut rx, Duration::from_secs(2)).await;
        assert_eq!(msg.topic, "stream2-second");

        // Shutdown and cleanup
        rx.close();
        stream_signal.store(true, Ordering::Relaxed);
        handle.await.unwrap();
    }

    #[rstest]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_stream_messages_interleaved_at_different_rates(
        #[future] redis_connection: ConnectionManager,
    ) {
        let mut con = redis_connection.await;
        let (tx, mut rx) = tokio::sync::mpsc::channel::<BusMessage>(100);

        // Setup multiple stream keys
        let suffix = UUID4::new();
        let stream_key1 = format!("test:stream:interleaved:{suffix}:1");
        let stream_key2 = format!("test:stream:interleaved:{suffix}:2");
        let stream_key3 = format!("test:stream:interleaved:{suffix}:3");
        let external_streams = vec![
            stream_key1.clone(),
            stream_key2.clone(),
            stream_key3.clone(),
        ];
        let stream_signal = Arc::new(AtomicBool::new(false));
        let stream_signal_clone = stream_signal.clone();

        let clock = get_atomic_clock_realtime();
        let base_id = clock.get_time_ms() + 1_000_000;

        let handle = tokio::spawn(async move {
            stream_messages(
                tx,
                RedisMessageBusConfig::default(),
                external_streams,
                stream_signal_clone,
            )
            .await
            .unwrap();
        });

        // Stream 1 advances with high ID
        let _: () = con
            .xadd(
                &stream_key1,
                format!("{}", base_id + 100),
                &[("topic", "s1m1"), ("payload", "data")],
            )
            .await
            .unwrap();
        let msg = receive_bus_message(&mut rx, Duration::from_secs(2)).await;
        assert_eq!(msg.topic, "s1m1");

        // Stream 2 gets message at lower ID - would be skipped with global cursor
        let _: () = con
            .xadd(
                &stream_key2,
                format!("{}", base_id + 50),
                &[("topic", "s2m1"), ("payload", "data")],
            )
            .await
            .unwrap();
        let msg = receive_bus_message(&mut rx, Duration::from_secs(2)).await;
        assert_eq!(msg.topic, "s2m1");

        // Stream 3 gets message at even lower ID
        let _: () = con
            .xadd(
                &stream_key3,
                format!("{}", base_id + 25),
                &[("topic", "s3m1"), ("payload", "data")],
            )
            .await
            .unwrap();
        let msg = receive_bus_message(&mut rx, Duration::from_secs(2)).await;
        assert_eq!(msg.topic, "s3m1");

        // Shutdown and cleanup
        rx.close();
        stream_signal.store(true, Ordering::Relaxed);
        handle.await.unwrap();
    }

    #[rstest]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_close() {
        let trader_id = TraderId::from("tester-001");
        let instance_id = UUID4::new();
        let config = MessageBusConfig {
            use_instance_id: true,
            ..Default::default()
        };

        let mut db = RedisMessageBusBacking::new(
            trader_id,
            instance_id,
            config,
            RedisMessageBusConfig::default(),
        )
        .unwrap();

        // Close the message bus backing (test should not hang)
        MessageBusBacking::close(&mut db);
    }

    #[rstest]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_heartbeat_task() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<BusMessage>();
        let signal = Arc::new(AtomicBool::new(false));

        // Start the heartbeat task with a short interval
        let handle = tokio::spawn(run_heartbeat(1, signal.clone(), tx));

        let heartbeat = receive_unbounded_bus_message(&mut rx, Duration::from_secs(2)).await;

        // Stop the heartbeat task
        signal.store(true, Ordering::Relaxed);
        handle.await.unwrap();

        // Ensure heartbeats were sent
        assert_eq!(heartbeat.topic, HEARTBEAT_TOPIC);
    }

    async fn receive_bus_message(
        rx: &mut tokio::sync::mpsc::Receiver<BusMessage>,
        timeout: Duration,
    ) -> BusMessage {
        let mut received = None;

        wait_until_async(
            || {
                if received.is_none() {
                    received = rx.try_recv().ok();
                }

                let has_received = received.is_some();
                async move { has_received }
            },
            timeout,
        )
        .await;

        received.unwrap()
    }

    async fn receive_unbounded_bus_message(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<BusMessage>,
        timeout: Duration,
    ) -> BusMessage {
        let mut received = None;

        wait_until_async(
            || {
                if received.is_none() {
                    received = rx.try_recv().ok();
                }

                let has_received = received.is_some();
                async move { has_received }
            },
            timeout,
        )
        .await;

        received.unwrap()
    }
}
