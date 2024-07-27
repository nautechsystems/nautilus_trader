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
    database::{BusMessage, MessageBusDatabaseAdapter},
    CLOSE_TOPIC,
};
use nautilus_core::{time::duration_since_unix_epoch, uuid::UUID4};
use nautilus_model::identifiers::TraderId;
use redis::*;
use serde_json::Value;
use streams::StreamReadOptions;
use tracing::{debug, error};

use super::get_external_stream_keys;
use crate::redis::{
    create_redis_connection, get_buffer_interval, get_database_config, get_stream_key,
};

const XTRIM: &str = "XTRIM";
const MINID: &str = "MINID";
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
        config: HashMap<String, serde_json::Value>,
    ) -> anyhow::Result<Self> {
        let config_clone = config.clone();
        let (pub_tx, pub_rx) = std::sync::mpsc::channel::<BusMessage>();

        // Create publish thread and channel
        let pub_handle = Some(
            thread::Builder::new()
                .name(MSGBUS_PUBLISH.to_string())
                .spawn(move || publish_messages(pub_rx, trader_id, instance_id, config_clone))
                .expect("Error spawning '{MSGBUS_PUBLISH}' thread"),
        );

        // Conditionally create stream thread and channel if external streams configured
        let external_streams = get_external_stream_keys(&config);
        let stream_signal = Arc::new(AtomicBool::new(false));
        let (stream_rx, stream_handle) = if !external_streams.is_empty() {
            let config_clone = config.clone();
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
                                config_clone,
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
            debug!("Failed to send message: {}", e);
        }
        Ok(())
    }

    fn close(&mut self) -> anyhow::Result<()> {
        debug!("Closing message bus database adapter");

        let msg = BusMessage {
            topic: CLOSE_TOPIC.to_string(),
            payload: Bytes::new(), // Empty
        };
        if let Err(e) = self.pub_tx.send(msg) {
            error!("Failed to send close message: {:?}", e);
        };

        if let Some(handle) = self.pub_handle.take() {
            debug!("Joining '{MSGBUS_PUBLISH}' thread");
            if let Err(e) = handle.join().map_err(|e| anyhow::anyhow!("{:?}", e)) {
                error!("Error joining '{MSGBUS_PUBLISH}' thread: {:?}", e);
            }
        };

        self.stream_signal.store(true, Ordering::SeqCst);

        if let Some(handle) = self.stream_handle.take() {
            debug!("Joining '{MSGBUS_STREAM}' thread");
            if let Err(e) = handle.join().map_err(|e| anyhow::anyhow!("{:?}", e)) {
                error!("Error joining '{MSGBUS_STREAM}' thread: {:?}", e);
            }
        };

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
    config: HashMap<String, Value>,
) -> anyhow::Result<()> {
    debug!("Starting Redis message publishing");

    let db_config = get_database_config(&config)?;
    let mut con = create_redis_connection(MSGBUS_PUBLISH, &db_config)?;
    let stream_key = get_stream_key(trader_id, instance_id, &config);

    // Autotrimming
    let autotrim_mins = config
        .get("autotrim_mins")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;
    let autotrim_duration = if autotrim_mins > 0 {
        Some(Duration::from_secs(autotrim_mins as u64 * 60))
    } else {
        None
    };
    let mut last_trim_index: HashMap<String, usize> = HashMap::new();

    // Buffering
    let mut buffer: VecDeque<BusMessage> = VecDeque::new();
    let mut last_drain = Instant::now();
    let recv_interval = Duration::from_millis(1);
    let buffer_interval = get_buffer_interval(&config);

    loop {
        if last_drain.elapsed() >= buffer_interval && !buffer.is_empty() {
            drain_buffer(
                &mut con,
                &stream_key,
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
        pipe.xadd(stream_key, "*", &items);

        if autotrim_duration.is_none() {
            continue; // Nothing else to do
        }

        // Autotrim stream
        let last_trim_ms = last_trim_index.entry(stream_key.to_string()).or_insert(0); // Remove clone
        let unix_duration_now = duration_since_unix_epoch();
        let trim_buffer = Duration::from_secs(TRIM_BUFFER_SECONDS);

        // Improve efficiency of this by batching
        if *last_trim_ms < (unix_duration_now - trim_buffer).as_millis() as usize {
            let min_timestamp_ms =
                (unix_duration_now - autotrim_duration.unwrap()).as_millis() as usize;
            let result: Result<(), redis::RedisError> = redis::cmd(XTRIM)
                .arg(stream_key)
                .arg(MINID)
                .arg(min_timestamp_ms)
                .query(conn);

            if let Err(e) = result {
                error!("Error trimming stream '{stream_key}': {e}");
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

#[allow(clippy::type_complexity)]
pub fn stream_messages(
    tx: tokio::sync::mpsc::Sender<BusMessage>,
    config: HashMap<String, Value>,
    stream_keys: Vec<String>,
    stream_signal: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    debug!("Starting Redis message streaming");

    let db_config = get_database_config(&config)?;
    let mut con = create_redis_connection(MSGBUS_PUBLISH, &db_config)?;
    let stream_keys = &stream_keys
        .iter()
        .map(String::as_str)
        .collect::<Vec<&str>>();
    let mut last_id = "0".to_string();
    let opts = StreamReadOptions::default().block(100);

    loop {
        if stream_signal.load(Ordering::SeqCst) {
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
                                last_id.clone_from(id);
                                match decode_bus_message(array) {
                                    Ok(msg) => {
                                        if tx.blocking_send(msg).is_err() {
                                            debug!("Receiver dropped");
                                            break;
                                        }
                                    }
                                    Err(e) => {
                                        error!("{:?}", e);
                                        continue;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                return Err(anyhow::anyhow!("Error reading from Redis stream: {:?}", e));
            }
        }
    }
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
#[cfg(target_os = "linux")] // Run Redis tests on Linux platforms only
#[cfg(test)]
mod tests {
    use std::thread;

    use redis::Commands;
    use rstest::*;
    use serde_json::json;

    use super::*;
    use crate::redis::flush_redis;

    #[fixture]
    fn redis_connection() -> redis::Connection {
        let db_config = json!({"type": "redis"});
        let mut con = create_redis_connection(MSGBUS_STREAM, &db_config).unwrap();
        flush_redis(&mut con).unwrap();
        con
    }

    #[rstest]
    #[tokio::test]
    async fn test_stream_messages(redis_connection: redis::Connection) {
        let mut con = redis_connection;
        let (tx, mut rx) = tokio::sync::mpsc::channel::<BusMessage>(100);

        let trader_id = TraderId::from("tester-001");
        let instance_id = UUID4::new();

        let mut config = HashMap::new();
        let db_config = json!({"type": "redis"});
        config.insert("database".to_string(), db_config.clone());
        config.insert("streams_prefix".to_string(), json!("stream"));
        config.insert("use_trader_prefix".to_string(), json!(true));
        config.insert("use_trader_id".to_string(), json!(true));

        let stream_key = get_stream_key(trader_id, instance_id, &config);
        let external_streams = vec![stream_key.clone()];
        let stream_signal = Arc::new(AtomicBool::new(false));
        let stream_signal_clone = stream_signal.clone();

        // Publish test message
        let _: () = con
            .xadd(
                stream_key,
                "*",
                &[("topic", "topic1"), ("payload", "data1")],
            )
            .unwrap();

        // Start the message streaming in a separate thread
        thread::spawn(move || {
            stream_messages(tx, config, external_streams, stream_signal_clone).unwrap();
        });

        // Receive and verify the message
        let msg = rx.recv().await.unwrap();
        assert_eq!(msg.topic, "topic1");
        assert_eq!(msg.payload, Bytes::from("data1"));

        // Shutdown and cleanup
        rx.close();
        flush_redis(&mut con).unwrap()
    }
}
