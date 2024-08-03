// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
        atomic::{AtomicBool, Ordering},
        mpsc::TryRecvError,
        Arc,
    },
    thread::{self},
    time::{Duration, Instant},
};

use bytes::Bytes;
use futures::stream::Stream;
use nautilus_common::msgbus::{
    database::{BusMessage, DatabaseConfig, MessageBusConfig, MessageBusDatabaseAdapter},
    CLOSE_TOPIC,
};
use nautilus_core::{
    time::{duration_since_unix_epoch, get_atomic_clock_realtime},
    uuid::UUID4,
};
use nautilus_model::identifiers::TraderId;
use redis::*;
use streams::StreamReadOptions;

use super::{REDIS_MINID, REDIS_XTRIM};
use crate::redis::{create_redis_connection, get_stream_key};

const MSGBUS_PUBLISH: &str = "msgbus-publish";
const MSGBUS_STREAM: &str = "msgbus-stream";
const TRIM_BUFFER_SECONDS: u64 = 60;

type RedisStreamBulk = Vec<HashMap<String, Vec<HashMap<String, redis::Value>>>>;

#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.infrastructure")
)]
pub struct RedisMessageBusDatabase {
    pub trader_id: TraderId,
    pub instance_id: UUID4,
    pub_tx: std::sync::mpsc::Sender<BusMessage>,
    pub_handle: Option<std::thread::JoinHandle<anyhow::Result<()>>>,
    stream_rx: Option<tokio::sync::mpsc::Receiver<BusMessage>>,
    stream_handle: Option<std::thread::JoinHandle<anyhow::Result<()>>>,
    stream_signal: Arc<AtomicBool>,
}

impl MessageBusDatabaseAdapter for RedisMessageBusDatabase {
    type DatabaseType = RedisMessageBusDatabase;

    fn new(
        trader_id: TraderId,
        instance_id: UUID4,
        config: MessageBusConfig,
    ) -> anyhow::Result<Self> {
        let config_clone = config.clone();
        let db_config = config
            .database
            .clone()
            .ok_or_else(|| anyhow::anyhow!("No database config"))?;

        let (pub_tx, pub_rx) = std::sync::mpsc::channel::<BusMessage>();

        // Create publish thread and channel
        let pub_handle = Some(
            std::thread::Builder::new()
                .name(MSGBUS_PUBLISH.to_string())
                .spawn(move || publish_messages(pub_rx, trader_id, instance_id, config_clone))
                .expect("Error spawning '{MSGBUS_PUBLISH}' thread"),
        );

        // Conditionally create stream thread and channel if external streams configured
        let external_streams = config.external_streams.clone().unwrap_or_default();
        let stream_signal = Arc::new(AtomicBool::new(false));
        let (stream_rx, stream_handle) = if !external_streams.is_empty() {
            let stream_signal_clone = stream_signal.clone();
            let (stream_tx, stream_rx) = tokio::sync::mpsc::channel::<BusMessage>(100_000);
            (
                Some(stream_rx),
                Some(
                    std::thread::Builder::new()
                        .name(MSGBUS_STREAM.to_string())
                        .spawn(move || {
                            stream_messages(
                                stream_tx,
                                db_config,
                                external_streams,
                                stream_signal_clone,
                            )
                        })
                        .expect("Error spawning '{MSGBUS_STREAM}' thread"),
                ),
            )
        } else {
            (None, None)
        };

        Ok(Self {
            trader_id,
            instance_id,
            pub_tx,
            pub_handle,
            stream_rx,
            stream_handle,
            stream_signal,
        })
    }

    fn publish(&self, topic: String, payload: Bytes) -> anyhow::Result<()> {
        let msg = BusMessage { topic, payload };
        if let Err(e) = self.pub_tx.send(msg) {
            // This will occur for now when the Python task
            // blindly attempts to publish to a closed channel.
            tracing::debug!("Failed to send message: {}", e);
        }
        Ok(())
    }

    fn close(&mut self) -> anyhow::Result<()> {
        tracing::debug!("Closing message bus database adapter");

        self.stream_signal.store(true, Ordering::Relaxed);

        let msg = BusMessage {
            topic: CLOSE_TOPIC.to_string(),
            payload: Bytes::new(), // Empty
        };
        if let Err(e) = self.pub_tx.send(msg) {
            tracing::error!("Failed to send close message: {:?}", e);
        }

        if let Some(handle) = self.pub_handle.take() {
            tracing::debug!("Joining '{MSGBUS_PUBLISH}' thread");
            if let Err(e) = handle.join().map_err(|e| anyhow::anyhow!("{:?}", e)) {
                tracing::error!("Error joining '{MSGBUS_PUBLISH}' thread: {:?}", e);
            }
        }

        if let Some(handle) = self.stream_handle.take() {
            tracing::debug!("Joining '{MSGBUS_STREAM}' thread");
            if let Err(e) = handle.join().map_err(|e| anyhow::anyhow!("{:?}", e)) {
                tracing::error!("Error joining '{MSGBUS_STREAM}' thread: {:?}", e);
            }
        }
        Ok(())
    }
}

impl RedisMessageBusDatabase {
    pub fn get_stream_receiver(
        &mut self,
    ) -> anyhow::Result<tokio::sync::mpsc::Receiver<BusMessage>> {
        self.stream_rx
            .take()
            .ok_or_else(|| anyhow::anyhow!("Stream receiver already taken"))
    }

    pub fn stream(
        mut stream_rx: tokio::sync::mpsc::Receiver<BusMessage>,
    ) -> impl Stream<Item = BusMessage> + 'static {
        async_stream::stream! {
            while let Some(msg) = stream_rx.recv().await {
                yield msg;
            }
        }
    }
}

pub fn publish_messages(
    rx: std::sync::mpsc::Receiver<BusMessage>,
    trader_id: TraderId,
    instance_id: UUID4,
    config: MessageBusConfig,
) -> anyhow::Result<()> {
    tracing::debug!("Starting message publishing");
    let db_config = config
        .database
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("No database config"))?;
    let mut con = create_redis_connection(MSGBUS_PUBLISH, db_config.clone())?;
    let stream_key = get_stream_key(trader_id, instance_id, &config);

    // Autotrimming
    let autotrim_duration = config
        .autotrim_mins
        .filter(|&mins| mins > 0)
        .map(|mins| Duration::from_secs(mins as u64 * 60));
    let mut last_trim_index: HashMap<String, usize> = HashMap::new();

    // Buffering
    let mut buffer: VecDeque<BusMessage> = VecDeque::new();
    let mut last_drain = Instant::now();
    let recv_interval = Duration::from_millis(1);
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
            )?;
            last_drain = Instant::now();
        } else {
            // Continue to receive and handle messages until channel is hung up
            // or the close topic is received.
            match rx.try_recv() {
                Ok(msg) => {
                    if msg.topic == CLOSE_TOPIC {
                        drop(rx);
                        break;
                    }
                    buffer.push_back(msg);
                }
                Err(TryRecvError::Empty) => thread::sleep(recv_interval),
                Err(TryRecvError::Disconnected) => break, // Channel hung up
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
        )?;
    }

    Ok(())
}

fn drain_buffer(
    conn: &mut Connection,
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
        let trim_buffer = Duration::from_secs(TRIM_BUFFER_SECONDS);

        // Improve efficiency of this by batching
        if *last_trim_ms < (unix_duration_now - trim_buffer).as_millis() as usize {
            let min_timestamp_ms =
                (unix_duration_now - autotrim_duration.unwrap()).as_millis() as usize;
            let result: Result<(), redis::RedisError> = redis::cmd(REDIS_XTRIM)
                .arg(stream_key.clone())
                .arg(REDIS_MINID)
                .arg(min_timestamp_ms)
                .query(conn);

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

    pipe.query::<()>(conn).map_err(anyhow::Error::from)
}

pub fn stream_messages(
    tx: tokio::sync::mpsc::Sender<BusMessage>,
    config: DatabaseConfig,
    stream_keys: Vec<String>,
    stream_signal: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    tracing::debug!("Starting message streaming");
    let mut con = create_redis_connection(MSGBUS_STREAM, config)?;

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
            tracing::debug!("Received terminate signal");
            break;
        }
        let result: Result<RedisStreamBulk, _> =
            con.xread_options(&[&stream_keys], &[&last_id], &opts);
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
                                        if tx.blocking_send(msg).is_err() {
                                            tracing::debug!("Channel closed");
                                            break 'outer; // End streaming
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!("{:?}", e);
                                        continue;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                return Err(anyhow::anyhow!("Error reading from stream: {:?}", e));
            }
        }
    }
    tracing::debug!("Completed message streaming");
    Ok(())
}

fn decode_bus_message(stream_msg: &redis::Value) -> anyhow::Result<BusMessage> {
    if let redis::Value::Array(stream_msg) = stream_msg {
        if stream_msg.len() < 4 {
            anyhow::bail!("Invalid stream message format: {:?}", stream_msg);
        }

        let topic = match &stream_msg[1] {
            redis::Value::BulkString(bytes) => {
                String::from_utf8(bytes.clone()).expect("Error parsing topic")
            }
            _ => {
                anyhow::bail!("Invalid topic format: {:?}", stream_msg);
            }
        };

        let payload = match &stream_msg[3] {
            redis::Value::BulkString(bytes) => Bytes::copy_from_slice(bytes),
            _ => {
                anyhow::bail!("Invalid payload format: {:?}", stream_msg);
            }
        };

        Ok(BusMessage { topic, payload })
    } else {
        anyhow::bail!("Invalid stream message format: {:?}", stream_msg)
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
    use std::thread;

    use nautilus_common::testing::wait_until;
    use redis::Commands;
    use rstest::*;

    use super::*;
    use crate::redis::flush_redis;

    #[fixture]
    fn redis_connection() -> redis::Connection {
        let config = DatabaseConfig::default();
        let mut con = create_redis_connection(MSGBUS_STREAM, config).unwrap();
        flush_redis(&mut con).unwrap();
        con
    }

    #[rstest]
    #[tokio::test]
    async fn test_stream_messages_terminate_signal(redis_connection: redis::Connection) {
        let mut con = redis_connection;
        let (tx, mut rx) = tokio::sync::mpsc::channel::<BusMessage>(100);

        let trader_id = TraderId::from("tester-001");
        let instance_id = UUID4::new();
        let mut config = MessageBusConfig::default();
        config.database = Some(DatabaseConfig::default());

        let stream_key = get_stream_key(trader_id, instance_id, &config);
        let external_streams = vec![stream_key.clone()];
        let stream_signal = Arc::new(AtomicBool::new(false));
        let stream_signal_clone = stream_signal.clone();

        // Start the message streaming in a separate thread
        let handle = thread::spawn(move || {
            stream_messages(
                tx,
                DatabaseConfig::default(),
                external_streams,
                stream_signal_clone,
            )
            .unwrap();
        });

        stream_signal.store(true, Ordering::Relaxed);
        let _ = rx.recv().await; // Wait for the tx to close

        // Shutdown and cleanup
        rx.close();
        handle.join().unwrap();
        flush_redis(&mut con).unwrap()
    }

    #[rstest]
    #[tokio::test]
    async fn test_stream_messages_when_receiver_closed(redis_connection: redis::Connection) {
        let mut con = redis_connection;
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
            .unwrap();

        // Immediately close channel
        rx.close();

        // Start the message streaming in a separate thread
        let handle = thread::spawn(move || {
            stream_messages(
                tx,
                DatabaseConfig::default(),
                external_streams,
                stream_signal_clone,
            )
            .unwrap();
        });

        // Shutdown and cleanup
        handle.join().unwrap();
        flush_redis(&mut con).unwrap()
    }

    #[rstest]
    #[tokio::test]
    async fn test_stream_messages(redis_connection: redis::Connection) {
        let mut con = redis_connection;
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
            .unwrap();

        // Start the message streaming in a separate thread
        let handle = thread::spawn(move || {
            stream_messages(
                tx,
                DatabaseConfig::default(),
                external_streams,
                stream_signal_clone,
            )
            .unwrap();
        });

        // Receive and verify the message
        let msg = rx.recv().await.unwrap();
        assert_eq!(msg.topic, "topic1");
        assert_eq!(msg.payload, Bytes::from("data1"));

        // Shutdown and cleanup
        rx.close();
        stream_signal.store(true, Ordering::Relaxed);
        handle.join().unwrap();
        flush_redis(&mut con).unwrap()
    }

    #[rstest]
    #[tokio::test]
    async fn test_publish_messages(redis_connection: redis::Connection) {
        let mut con = redis_connection;
        let (tx, rx) = std::sync::mpsc::channel::<BusMessage>();

        let trader_id = TraderId::from("tester-001");
        let instance_id = UUID4::new();
        let mut config = MessageBusConfig::default();
        config.database = Some(DatabaseConfig::default());
        config.stream_per_topic = false;
        let stream_key = get_stream_key(trader_id, instance_id, &config);

        // Start the publish_messages function in a separate thread
        let handle = thread::spawn(move || {
            publish_messages(rx, trader_id, instance_id, config).unwrap();
        });

        // Send a test message
        let msg = BusMessage {
            topic: "test_topic".to_string(),
            payload: Bytes::from("test_payload"),
        };
        tx.send(msg).unwrap();

        // Wait until the message is published to Redis
        wait_until(
            || {
                let messages: RedisStreamBulk = con.xread(&[&stream_key], &["0"]).unwrap();
                !messages.is_empty()
            },
            Duration::from_secs(2),
        );

        // Verify the message was published to Redis
        let messages: RedisStreamBulk = con.xread(&[&stream_key], &["0"]).unwrap();
        assert_eq!(messages.len(), 1);
        let stream_msgs = messages[0].get(&stream_key).unwrap();
        let stream_msg_array = &stream_msgs[0].values().next().unwrap();
        let decoded_message = decode_bus_message(stream_msg_array).unwrap();
        assert_eq!(decoded_message.topic, "test_topic");
        assert_eq!(decoded_message.payload, Bytes::from("test_payload"));

        // Close publishing thread
        let msg = BusMessage {
            topic: CLOSE_TOPIC.to_string(),
            payload: Bytes::new(), // Empty
        };
        tx.send(msg).unwrap();

        // Shutdown and cleanup
        handle.join().unwrap();
        flush_redis(&mut con).unwrap();
    }

    #[rstest]
    #[tokio::test]
    async fn test_close() {
        let trader_id = TraderId::from("tester-001");
        let instance_id = UUID4::new();
        let mut config = MessageBusConfig::default();
        config.database = Some(DatabaseConfig::default());

        let mut db = RedisMessageBusDatabase::new(trader_id, instance_id, config).unwrap();

        // Close the message bus database (test should not hang)
        db.close().unwrap();
    }
}
