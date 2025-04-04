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
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use bytes::Bytes;
use futures::stream::Stream;
use nautilus_common::{
    msgbus::{
        CLOSE_TOPIC,
        database::{BusMessage, DatabaseConfig, MessageBusConfig, MessageBusDatabaseAdapter},
    },
    runtime::get_runtime,
};
use nautilus_core::{
    UUID4,
    time::{duration_since_unix_epoch, get_atomic_clock_realtime},
};
use nautilus_cryptography::providers::install_cryptographic_provider;
use nautilus_model::identifiers::TraderId;
use redis::*;
use streams::StreamReadOptions;

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

impl MessageBusDatabaseAdapter for RedisMessageBusDatabase {
    type DatabaseType = RedisMessageBusDatabase;

    /// Creates a new [`RedisMessageBusDatabase`] instance.
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
                log::error!("Error in task '{MSGBUS_PUBLISH}': {e}");
            };
        }));

        // Conditionally create stream task and channel if external streams configured
        let external_streams = config.external_streams.clone().unwrap_or_default();
        let stream_signal = Arc::new(AtomicBool::new(false));
        let (stream_rx, stream_handle) = if !external_streams.is_empty() {
            let stream_signal_clone = stream_signal.clone();
            let (stream_tx, stream_rx) = tokio::sync::mpsc::channel::<BusMessage>(100_000);
            (
                Some(stream_rx),
                Some(get_runtime().spawn(async move {
                    if let Err(e) =
                        stream_messages(stream_tx, db_config, external_streams, stream_signal_clone)
                            .await
                    {
                        log::error!("Error in task '{MSGBUS_STREAM}': {e}");
                    }
                })),
            )
        } else {
            (None, None)
        };

        // Create heartbeat task
        let heartbeat_signal = Arc::new(AtomicBool::new(false));
        let heartbeat_handle = if let Some(heartbeat_interval_secs) = config.heartbeat_interval_secs
        {
            let signal = heartbeat_signal.clone();
            let pub_tx_clone = pub_tx.clone();

            Some(get_runtime().spawn(async move {
                run_heartbeat(heartbeat_interval_secs, signal, pub_tx_clone).await
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
    fn publish(&self, topic: String, payload: Bytes) {
        let msg = BusMessage { topic, payload };
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
            let msg = BusMessage {
                topic: CLOSE_TOPIC.to_string(),
                payload: Bytes::new(), // Empty
            };
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
    /// Gets the stream receiver for this instance.
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

pub async fn publish_messages(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<BusMessage>,
    trader_id: TraderId,
    instance_id: UUID4,
    config: MessageBusConfig,
) -> anyhow::Result<()> {
    tracing::debug!("Starting message publishing");

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
        .map(|mins| Duration::from_secs(mins as u64 * 60));
    let mut last_trim_index: HashMap<String, usize> = HashMap::new();

    // Buffering
    let mut buffer: VecDeque<BusMessage> = VecDeque::new();
    let mut last_drain = Instant::now();
    let buffer_interval = Duration::from_millis(config.buffer_interval_ms.unwrap_or(0) as u64);

    loop {
        if last_drain.elapsed() >= buffer_interval && !buffer.is_empty() {
            drain_buffer(
                &mut con,
                &stream_key,
                config.stream_per_topic,
                autotrim_duration,
                &mut last_trim_index,
                &mut buffer,
            )
            .await?;
            last_drain = Instant::now();
        } else {
            match rx.recv().await {
                Some(msg) => {
                    if msg.topic == CLOSE_TOPIC {
                        tracing::debug!("Received close message");
                        drop(rx);
                        break;
                    }
                    buffer.push_back(msg);
                }
                None => {
                    tracing::debug!("Channel hung up");
                    break;
                }
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

    tracing::debug!("Stopped message publishing");
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
        let stream_key = match stream_per_topic {
            true => format!("{stream_key}:{}", &msg.topic),
            false => stream_key.to_string(),
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
                last_trim_index.insert(
                    stream_key.to_string(),
                    unix_duration_now.as_millis() as usize,
                );
            }
        }
    }

    pipe.query_async(conn).await.map_err(anyhow::Error::from)
}

pub async fn stream_messages(
    tx: tokio::sync::mpsc::Sender<BusMessage>,
    config: DatabaseConfig,
    stream_keys: Vec<String>,
    stream_signal: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    tracing::info!("Starting message streaming");
    let mut con = create_redis_connection(MSGBUS_STREAM, config).await?;

    let stream_keys = &stream_keys
        .iter()
        .map(String::as_str)
        .collect::<Vec<&str>>();

    tracing::debug!("Listening to streams: [{}]", stream_keys.join(", "));

    // Start streaming from current timestamp
    let clock = get_atomic_clock_realtime();
    let timestamp_ms = clock.get_time_ms();
    let mut last_id = timestamp_ms.to_string();

    let opts = StreamReadOptions::default().block(100);

    'outer: loop {
        if stream_signal.load(Ordering::Relaxed) {
            tracing::debug!("Received streaming terminate signal");
            break;
        }
        let result: Result<RedisStreamBulk, _> =
            con.xread_options(&[&stream_keys], &[&last_id], &opts).await;
        match result {
            Ok(stream_bulk) => {
                if stream_bulk.is_empty() {
                    // Timeout occurred: no messages received
                    continue;
                }
                for entry in stream_bulk.iter() {
                    for (_stream_key, stream_msgs) in entry.iter() {
                        for stream_msg in stream_msgs.iter() {
                            for (id, array) in stream_msg {
                                last_id.clear();
                                last_id.push_str(id);
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

    tracing::debug!("Stopped message streaming");
    Ok(())
}

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

        Ok(BusMessage { topic, payload })
    } else {
        anyhow::bail!("Invalid stream message format: {stream_msg:?}")
    }
}

async fn run_heartbeat(
    heartbeat_interval_secs: u16,
    signal: Arc<AtomicBool>,
    pub_tx: tokio::sync::mpsc::UnboundedSender<BusMessage>,
) {
    tracing::debug!("Starting heartbeat at {heartbeat_interval_secs} second intervals");

    let heartbeat_interval = Duration::from_secs(heartbeat_interval_secs as u64);
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

    tracing::debug!("Stopped heartbeat");
}

fn create_heartbeat_msg() -> BusMessage {
    BusMessage {
        topic: HEARTBEAT_TOPIC.to_string(),
        payload: Bytes::from(chrono::Utc::now().to_rfc3339().into_bytes()),
    }
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
        let mut config = MessageBusConfig::default();
        config.database = Some(DatabaseConfig::default());

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
        flush_redis(&mut con).await.unwrap()
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
        let mut config = MessageBusConfig::default();
        config.database = Some(DatabaseConfig::default());

        let stream_key = get_stream_key(trader_id, instance_id, &config);
        let external_streams = vec![stream_key.clone()];
        let stream_signal = Arc::new(AtomicBool::new(false));
        let stream_signal_clone = stream_signal.clone();

        // Use a message ID in the future, as streaming begins
        // around the timestamp the thread is spawned.
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
        flush_redis(&mut con).await.unwrap()
    }

    #[rstest]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_stream_messages(#[future] redis_connection: ConnectionManager) {
        let mut con = redis_connection.await;
        let (tx, mut rx) = tokio::sync::mpsc::channel::<BusMessage>(100);

        let trader_id = TraderId::from("tester-001");
        let instance_id = UUID4::new();
        let mut config = MessageBusConfig::default();
        config.database = Some(DatabaseConfig::default());

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
        flush_redis(&mut con).await.unwrap()
    }

    #[rstest]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_publish_messages(#[future] redis_connection: ConnectionManager) {
        let mut con = redis_connection.await;
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<BusMessage>();

        let trader_id = TraderId::from("tester-001");
        let instance_id = UUID4::new();
        let mut config = MessageBusConfig::default();
        config.database = Some(DatabaseConfig::default());
        config.stream_per_topic = false;
        let stream_key = get_stream_key(trader_id, instance_id, &config);

        // Start the publish_messages task
        let handle = tokio::spawn(async move {
            publish_messages(rx, trader_id, instance_id, config)
                .await
                .unwrap();
        });

        // Send a test message
        let msg = BusMessage {
            topic: "test_topic".to_string(),
            payload: Bytes::from("test_payload"),
        };
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
            Duration::from_secs(2),
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
        let msg = BusMessage {
            topic: CLOSE_TOPIC.to_string(),
            payload: Bytes::new(), // Empty
        };
        tx.send(msg).unwrap();

        // Shutdown and cleanup
        handle.await.unwrap();
        flush_redis(&mut con).await.unwrap();
    }

    #[rstest]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_close() {
        let trader_id = TraderId::from("tester-001");
        let instance_id = UUID4::new();
        let mut config = MessageBusConfig::default();
        config.database = Some(DatabaseConfig::default());

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
