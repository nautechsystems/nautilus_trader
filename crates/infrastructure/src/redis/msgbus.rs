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
    logging::{log_task_error, log_task_started, log_task_stopped},
    msgbus::{
        BusMessage,
        database::{DatabaseConfig, MessageBusConfig, MessageBusDatabaseAdapter},
        switchboard::CLOSE_TOPIC,
    },
    runtime::get_runtime,
};
use nautilus_core::{
    UUID4,
    time::{duration_since_unix_epoch, get_atomic_clock_realtime},
};
use nautilus_cryptography::providers::install_cryptographic_provider;
use nautilus_model::identifiers::TraderId;
use redis::{AsyncCommands, streams};
use streams::StreamReadOptions;
use tokio::time::Instant;
use ustr::Ustr;

use super::{REDIS_MINID, REDIS_XTRIM, await_handle};
use crate::redis::{create_redis_connection, get_stream_key};

const MSGBUS_PUBLISH: &str = "msgbus-publish";
const MSGBUS_STREAM: &str = "msgbus-stream";
const MSGBUS_HEARTBEAT: &str = "msgbus-heartbeat";
const HEARTBEAT_TOPIC: &str = "health:heartbeat";
const TRIM_BUFFER_SECS: u64 = 60;

type RedisStreamBulk = Vec<HashMap<String, Vec<HashMap<String, redis::Value>>>>;

#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.infrastructure")
)]
pub struct RedisMessageBusDatabase {
    /// The trader ID for this message bus database.
    pub trader_id: TraderId,
    /// The instance ID for this message bus database.
    pub instance_id: UUID4,
    pub_tx: tokio::sync::mpsc::UnboundedSender<BusMessage>,
    pub_handle: Option<tokio::task::JoinHandle<()>>,
    stream_rx: Option<tokio::sync::mpsc::Receiver<BusMessage>>,
    stream_handle: Option<tokio::task::JoinHandle<()>>,
    stream_signal: Arc<AtomicBool>,
    heartbeat_handle: Option<tokio::task::JoinHandle<()>>,
    heartbeat_signal: Arc<AtomicBool>,
}

impl Debug for RedisMessageBusDatabase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(RedisMessageBusDatabase))
            .field("trader_id", &self.trader_id)
            .field("instance_id", &self.instance_id)
            .finish()
    }
}

impl MessageBusDatabaseAdapter for RedisMessageBusDatabase {
    type DatabaseType = Self;

    /// Creates a new [`RedisMessageBusDatabase`] instance for the given `trader_id`, `instance_id`, and `config`.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The database configuration is missing in `config`.
    /// - Establishing the Redis connection for publishing fails.
    fn new(
        trader_id: TraderId,
        instance_id: UUID4,
        config: MessageBusConfig,
    ) -> anyhow::Result<Self> {
        install_cryptographic_provider();

        let config_clone = config.clone();
        let db_config = config
            .database
            .clone()
            .ok_or_else(|| anyhow::anyhow!("No database config"))?;

        let (pub_tx, pub_rx) = tokio::sync::mpsc::unbounded_channel::<BusMessage>();

        // Create publish task (start the runtime here for now)
        let pub_handle = Some(get_runtime().spawn(async move {
            if let Err(e) = publish_messages(pub_rx, trader_id, instance_id, config_clone).await {
                log_task_error(MSGBUS_PUBLISH, &e);
            }
        }));

        // Conditionally create stream task and channel if external streams configured
        let external_streams = config.external_streams.clone().unwrap_or_default();
        let stream_signal = Arc::new(AtomicBool::new(false));
        let (stream_rx, stream_handle) = if external_streams.is_empty() {
            (None, None)
        } else {
            let stream_signal_clone = stream_signal.clone();
            let (stream_tx, stream_rx) = tokio::sync::mpsc::channel::<BusMessage>(100_000);
            (
                Some(stream_rx),
                Some(get_runtime().spawn(async move {
                    if let Err(e) =
                        stream_messages(stream_tx, db_config, external_streams, stream_signal_clone)
                            .await
                    {
                        log_task_error(MSGBUS_STREAM, &e);
                    }
                })),
            )
        };

        // Create heartbeat task
        let heartbeat_signal = Arc::new(AtomicBool::new(false));
        let heartbeat_handle = if let Some(heartbeat_interval_secs) = config.heartbeat_interval_secs
        {
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

    /// Returns whether the message bus database adapter publishing channel is closed.
    fn is_closed(&self) -> bool {
        self.pub_tx.is_closed()
    }

    /// Publishes a message with the given `topic` and `payload`.
    fn publish(&self, topic: Ustr, payload: Bytes) {
        let msg = BusMessage::new(topic, payload);
        if let Err(e) = self.pub_tx.send(msg) {
            log::error!("Failed to send message: {e}");
        }
    }

    /// Closes the message bus database adapter.
    fn close(&mut self) {
        log::debug!("Closing");

        self.stream_signal.store(true, Ordering::Relaxed);
        self.heartbeat_signal.store(true, Ordering::Relaxed);

        if !self.pub_tx.is_closed() {
            let msg = BusMessage::new_close();

            if let Err(e) = self.pub_tx.send(msg) {
                log::error!("Failed to send close message: {e:?}");
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

impl RedisMessageBusDatabase {
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
/// - The database configuration is missing in `config`.
/// - Establishing the Redis connection fails.
/// - Any Redis command fails during publishing.
pub async fn publish_messages(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<BusMessage>,
    trader_id: TraderId,
    instance_id: UUID4,
    config: MessageBusConfig,
) -> anyhow::Result<()> {
    log_task_started(MSGBUS_PUBLISH);

    let db_config = config
        .database
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("No database config"))?;
    let mut con = create_redis_connection(MSGBUS_PUBLISH, db_config.clone()).await?;
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
                        tracing::debug!("Received close message");
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
                    tracing::debug!("Channel hung up");
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
                flush_timer.as_mut().reset(Instant::now() + buffer_interval);
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
        let items: Vec<(&str, &[u8])> = vec![
            ("topic", msg.topic.as_ref()),
            ("payload", msg.payload.as_ref()),
        ];
        let stream_key = if stream_per_topic {
            format!("{stream_key}:{}", &msg.topic)
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
        if *last_trim_ms < (unix_duration_now - trim_buffer).as_millis() as usize {
            let min_timestamp_ms =
                (unix_duration_now - autotrim_duration.unwrap()).as_millis() as usize;
            let result: Result<(), redis::RedisError> = redis::cmd(REDIS_XTRIM)
                .arg(stream_key.clone())
                .arg(REDIS_MINID)
                .arg(min_timestamp_ms)
                .query_async(conn)
                .await;

            if let Err(e) = result {
                tracing::error!("Error trimming stream '{stream_key}': {e}");
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
/// - Establishing the Redis connection fails.
/// - Any Redis read operation fails.
pub async fn stream_messages(
    tx: tokio::sync::mpsc::Sender<BusMessage>,
    config: DatabaseConfig,
    stream_keys: Vec<String>,
    stream_signal: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    log_task_started(MSGBUS_STREAM);

    let mut con = create_redis_connection(MSGBUS_STREAM, config).await?;

    let stream_keys = &stream_keys
        .iter()
        .map(String::as_str)
        .collect::<Vec<&str>>();

    tracing::debug!("Listening to streams: [{}]", stream_keys.join(", "));

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
            tracing::debug!("Received streaming terminate signal");
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
                                            tracing::debug!("Channel closed: {e:?}");
                                            break 'outer; // End streaming
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!("{e:?}");
                                        continue;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                anyhow::bail!("Error reading from stream: {e:?}");
            }
        }
    }

    log_task_stopped(MSGBUS_STREAM);
    Ok(())
}

/// Decodes a Redis stream message value into a `BusMessage`.
///
/// # Errors
///
/// Returns an error if:
/// - The incoming `stream_msg` is not an array.
/// - The array has fewer than four elements (invalid format).
/// - Parsing the topic or payload fails.
fn decode_bus_message(stream_msg: &redis::Value) -> anyhow::Result<BusMessage> {
    if let redis::Value::Array(stream_msg) = stream_msg {
        if stream_msg.len() < 4 {
            anyhow::bail!("Invalid stream message format: {stream_msg:?}");
        }

        let topic = match &stream_msg[1] {
            redis::Value::BulkString(bytes) => match String::from_utf8(bytes.clone()) {
                Ok(topic) => topic,
                Err(e) => anyhow::bail!("Error parsing topic: {e}"),
            },
            _ => {
                anyhow::bail!("Invalid topic format: {stream_msg:?}");
            }
        };

        let payload = match &stream_msg[3] {
            redis::Value::BulkString(bytes) => Bytes::copy_from_slice(bytes),
            _ => {
                anyhow::bail!("Invalid payload format: {stream_msg:?}");
            }
        };

        Ok(BusMessage::with_str_topic(topic, payload))
    } else {
        anyhow::bail!("Invalid stream message format: {stream_msg:?}")
    }
}

async fn run_heartbeat(
    heartbeat_interval_secs: u16,
    signal: Arc<AtomicBool>,
    pub_tx: tokio::sync::mpsc::UnboundedSender<BusMessage>,
) {
    log_task_started("heartbeat");
    tracing::debug!("Heartbeat at {heartbeat_interval_secs} second intervals");

    let heartbeat_interval = Duration::from_secs(u64::from(heartbeat_interval_secs));
    let heartbeat_timer = tokio::time::interval(heartbeat_interval);

    let check_interval = Duration::from_millis(100);
    let check_timer = tokio::time::interval(check_interval);

    tokio::pin!(heartbeat_timer);
    tokio::pin!(check_timer);

    loop {
        if signal.load(Ordering::Relaxed) {
            tracing::debug!("Received heartbeat terminate signal");
            break;
        }

        tokio::select! {
            _ = heartbeat_timer.tick() => {
                let heartbeat = create_heartbeat_msg();
                if let Err(e) = pub_tx.send(heartbeat) {
                    // We expect an error if the channel is closed during shutdown
                    tracing::debug!("Error sending heartbeat: {e}");
                }
            },
            _ = check_timer.tick() => {}
        }
    }

    log_task_stopped("heartbeat");
}

fn create_heartbeat_msg() -> BusMessage {
    let payload = Bytes::from(chrono::Utc::now().to_rfc3339().into_bytes());
    BusMessage::with_str_topic(HEARTBEAT_TOPIC, payload)
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use redis::Value;
    use rstest::*;

    use super::*;

    #[rstest]
    fn test_decode_bus_message_valid() {
        let stream_msg = Value::Array(vec![
            Value::BulkString(b"0".to_vec()),
            Value::BulkString(b"topic1".to_vec()),
            Value::BulkString(b"unused".to_vec()),
            Value::BulkString(b"data1".to_vec()),
        ]);

        let result = decode_bus_message(&stream_msg);
        assert!(result.is_ok());
        let msg = result.unwrap();
        assert_eq!(msg.topic, "topic1");
        assert_eq!(msg.payload, Bytes::from("data1"));
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
            "Invalid stream message format: [bulk-string('\"0\"'), bulk-string('\"topic1\"')]"
        );
    }

    #[rstest]
    fn test_decode_bus_message_invalid_topic_format() {
        let stream_msg = Value::Array(vec![
            Value::BulkString(b"0".to_vec()),
            Value::Int(42), // Invalid topic format
            Value::BulkString(b"unused".to_vec()),
            Value::BulkString(b"data1".to_vec()),
        ]);

        let result = decode_bus_message(&stream_msg);
        assert!(result.is_err());
        assert_eq!(
            format!("{}", result.unwrap_err()),
            "Invalid topic format: [bulk-string('\"0\"'), int(42), bulk-string('\"unused\"'), bulk-string('\"data1\"')]"
        );
    }

    #[rstest]
    fn test_decode_bus_message_invalid_payload_format() {
        let stream_msg = Value::Array(vec![
            Value::BulkString(b"0".to_vec()),
            Value::BulkString(b"topic1".to_vec()),
            Value::BulkString(b"unused".to_vec()),
            Value::Int(42), // Invalid payload format
        ]);

        let result = decode_bus_message(&stream_msg);
        assert!(result.is_err());
        assert_eq!(
            format!("{}", result.unwrap_err()),
            "Invalid payload format: [bulk-string('\"0\"'), bulk-string('\"topic1\"'), bulk-string('\"unused\"'), int(42)]"
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
}

#[cfg(target_os = "linux")] // Run Redis tests on Linux platforms only
#[cfg(test)]
mod serial_tests {
    use nautilus_common::testing::wait_until_async;
    use redis::aio::ConnectionManager;
    use rstest::*;

    use super::*;
    use crate::redis::flush_redis;

    #[fixture]
    async fn redis_connection() -> ConnectionManager {
        let config = DatabaseConfig::default();
        let mut con = create_redis_connection(MSGBUS_STREAM, config)
            .await
            .unwrap();
        flush_redis(&mut con).await.unwrap();
        con
    }

    #[rstest]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_stream_messages_terminate_signal(#[future] redis_connection: ConnectionManager) {
        let mut con = redis_connection.await;
        let (tx, mut rx) = tokio::sync::mpsc::channel::<BusMessage>(100);

        let trader_id = TraderId::from("tester-001");
        let instance_id = UUID4::new();
        let config = MessageBusConfig {
            database: Some(DatabaseConfig::default()),
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
                DatabaseConfig::default(),
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
        flush_redis(&mut con).await.unwrap();
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
            database: Some(DatabaseConfig::default()),
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
                DatabaseConfig::default(),
                external_streams,
                stream_signal_clone,
            )
            .await
            .unwrap();
        });

        // Shutdown and cleanup
        handle.await.unwrap();
        flush_redis(&mut con).await.unwrap();
    }

    #[rstest]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_stream_messages(#[future] redis_connection: ConnectionManager) {
        let mut con = redis_connection.await;
        let (tx, mut rx) = tokio::sync::mpsc::channel::<BusMessage>(100);

        let trader_id = TraderId::from("tester-001");
        let instance_id = UUID4::new();
        let config = MessageBusConfig {
            database: Some(DatabaseConfig::default()),
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
                DatabaseConfig::default(),
                external_streams,
                stream_signal_clone,
            )
            .await
            .unwrap();
        });

        // Receive and verify the message
        let msg = rx.recv().await.unwrap();
        assert_eq!(msg.topic, "topic1");
        assert_eq!(msg.payload, Bytes::from("data1"));

        // Shutdown and cleanup
        rx.close();
        stream_signal.store(true, Ordering::Relaxed);
        handle.await.unwrap();
        flush_redis(&mut con).await.unwrap();
    }

    #[rstest]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_publish_messages(#[future] redis_connection: ConnectionManager) {
        let mut con = redis_connection.await;
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<BusMessage>();

        let trader_id = TraderId::from("tester-001");
        let instance_id = UUID4::new();
        let config = MessageBusConfig {
            database: Some(DatabaseConfig::default()),
            stream_per_topic: false,
            ..Default::default()
        };
        let stream_key = get_stream_key(trader_id, instance_id, &config);

        // Start the publish_messages task
        let handle = tokio::spawn(async move {
            publish_messages(rx, trader_id, instance_id, config)
                .await
                .unwrap();
        });

        // Send a test message
        let msg = BusMessage::with_str_topic("test_topic", Bytes::from("test_payload"));
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
        assert_eq!(decoded_message.payload, Bytes::from("test_payload"));

        // Stop publishing task
        let msg = BusMessage::new_close();
        tx.send(msg).unwrap();

        // Shutdown and cleanup
        handle.await.unwrap();
        flush_redis(&mut con).await.unwrap();
    }

    #[rstest]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_stream_messages_multiple_streams(#[future] redis_connection: ConnectionManager) {
        let mut con = redis_connection.await;
        let (tx, mut rx) = tokio::sync::mpsc::channel::<BusMessage>(100);

        // Setup multiple stream keys
        let stream_key1 = "test:stream:1".to_string();
        let stream_key2 = "test:stream:2".to_string();
        let external_streams = vec![stream_key1.clone(), stream_key2.clone()];
        let stream_signal = Arc::new(AtomicBool::new(false));
        let stream_signal_clone = stream_signal.clone();

        let clock = get_atomic_clock_realtime();
        let base_id = clock.get_time_ms() + 1_000_000;

        // Start streaming task
        let handle = tokio::spawn(async move {
            stream_messages(
                tx,
                DatabaseConfig::default(),
                external_streams,
                stream_signal_clone,
            )
            .await
            .unwrap();
        });

        tokio::time::sleep(Duration::from_millis(200)).await;

        // Publish to stream 1 at higher ID
        let _: () = con
            .xadd(
                &stream_key1,
                format!("{}", base_id + 100),
                &[("topic", "stream1-first"), ("payload", "data")],
            )
            .await
            .unwrap();

        let msg = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("Stream 1 message should be received")
            .unwrap();
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

        let msg = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("Stream 2 message should be received")
            .unwrap();
        assert_eq!(msg.topic, "stream2-second");

        // Shutdown and cleanup
        rx.close();
        stream_signal.store(true, Ordering::Relaxed);
        handle.await.unwrap();
        flush_redis(&mut con).await.unwrap();
    }

    #[rstest]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_stream_messages_interleaved_at_different_rates(
        #[future] redis_connection: ConnectionManager,
    ) {
        let mut con = redis_connection.await;
        let (tx, mut rx) = tokio::sync::mpsc::channel::<BusMessage>(100);

        // Setup multiple stream keys
        let stream_key1 = "test:stream:interleaved:1".to_string();
        let stream_key2 = "test:stream:interleaved:2".to_string();
        let stream_key3 = "test:stream:interleaved:3".to_string();
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
                DatabaseConfig::default(),
                external_streams,
                stream_signal_clone,
            )
            .await
            .unwrap();
        });

        tokio::time::sleep(Duration::from_millis(200)).await;

        // Stream 1 advances with high ID
        let _: () = con
            .xadd(
                &stream_key1,
                format!("{}", base_id + 100),
                &[("topic", "s1m1"), ("payload", "data")],
            )
            .await
            .unwrap();
        let msg = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("Stream 1 message should be received")
            .unwrap();
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
        let msg = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("Stream 2 message should be received")
            .unwrap();
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
        let msg = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("Stream 3 message should be received")
            .unwrap();
        assert_eq!(msg.topic, "s3m1");

        // Shutdown and cleanup
        rx.close();
        stream_signal.store(true, Ordering::Relaxed);
        handle.await.unwrap();
        flush_redis(&mut con).await.unwrap();
    }

    #[rstest]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_close() {
        let trader_id = TraderId::from("tester-001");
        let instance_id = UUID4::new();
        let config = MessageBusConfig {
            database: Some(DatabaseConfig::default()),
            ..Default::default()
        };

        let mut db = RedisMessageBusDatabase::new(trader_id, instance_id, config).unwrap();

        // Close the message bus database (test should not hang)
        db.close();
    }

    #[rstest]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_heartbeat_task() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<BusMessage>();
        let signal = Arc::new(AtomicBool::new(false));

        // Start the heartbeat task with a short interval
        let handle = tokio::spawn(run_heartbeat(1, signal.clone(), tx));

        // Wait for a couple of heartbeats
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Stop the heartbeat task
        signal.store(true, Ordering::Relaxed);
        handle.await.unwrap();

        // Ensure heartbeats were sent
        let mut heartbeats: Vec<BusMessage> = Vec::new();
        while let Ok(hb) = rx.try_recv() {
            heartbeats.push(hb);
        }

        assert!(!heartbeats.is_empty());

        for hb in heartbeats {
            assert_eq!(hb.topic, HEARTBEAT_TOPIC);
        }
    }
}
