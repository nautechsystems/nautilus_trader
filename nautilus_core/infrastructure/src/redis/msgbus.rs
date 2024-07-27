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
    fmt,
    sync::mpsc::TryRecvError,
    thread::{self},
    time::{Duration, Instant},
};

use bytes::Bytes;
use futures::stream::Stream;
use nautilus_common::msgbus::{database::MessageBusDatabaseAdapter, CLOSE_TOPIC};
use nautilus_core::{time::duration_since_unix_epoch, uuid::UUID4};
use nautilus_model::identifiers::TraderId;
use redis::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{debug, error};

use crate::redis::{
    create_redis_connection, get_buffer_interval, get_database_config, get_stream_name,
};

const XTRIM: &str = "XTRIM";
const MINID: &str = "MINID";
const MSGBUS_PUBLISH: &str = "msgbus-publish";
const MSGBUS_STREAM: &str = "msgbus-stream";
const TRIM_BUFFER_SECONDS: u64 = 60;

/// Represents a bus message including a topic and payload.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BusMessage {
    /// The topic to publish on.
    pub topic: Bytes,
    /// The serialized payload for the message.
    pub payload: Bytes,
}

impl fmt::Display for BusMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}] {}",
            String::from_utf8_lossy(&self.topic),
            String::from_utf8_lossy(&self.payload)
        )
    }
}

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

        // Conditionally create stream thread and channel
        let config_clone = config.clone();
        let (stream_rx, stream_handle) = if true {
            let (stream_tx, stream_rx) = tokio::sync::mpsc::channel::<BusMessage>(100_000);
            (
                Some(stream_rx),
                Some(
                    std::thread::Builder::new()
                        .name(MSGBUS_STREAM.to_string())
                        .spawn(move || {
                            stream_messages(stream_tx, trader_id, instance_id, config_clone)
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
        })
    }

    fn publish(&self, topic: Bytes, payload: Bytes) -> anyhow::Result<()> {
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
            topic: Bytes::from(CLOSE_TOPIC.to_string()),
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
    let stream_name = get_stream_name(trader_id, instance_id, &config);

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
                &stream_name,
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
            &stream_name,
            autotrim_duration,
            &mut last_trim_index,
            &mut buffer,
        )?;
    }

    Ok(())
}

fn drain_buffer(
    conn: &mut Connection,
    stream_name: &str,
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
        pipe.xadd(stream_name, "*", &items);

        if autotrim_duration.is_none() {
            continue; // Nothing else to do
        }

        // Autotrim stream
        let last_trim_ms = last_trim_index.entry(stream_name.to_string()).or_insert(0); // Remove clone
        let unix_duration_now = duration_since_unix_epoch();
        let trim_buffer = Duration::from_secs(TRIM_BUFFER_SECONDS);

        // Improve efficiency of this by batching
        if *last_trim_ms < (unix_duration_now - trim_buffer).as_millis() as usize {
            let min_timestamp_ms =
                (unix_duration_now - autotrim_duration.unwrap()).as_millis() as usize;
            let result: Result<(), redis::RedisError> = redis::cmd(XTRIM)
                .arg(stream_name)
                .arg(MINID)
                .arg(min_timestamp_ms)
                .query(conn);

            if let Err(e) = result {
                error!("Error trimming stream '{stream_name}': {e}");
            } else {
                last_trim_index.insert(
                    stream_name.to_string(),
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
    trader_id: TraderId,
    instance_id: UUID4,
    config: HashMap<String, Value>,
) -> anyhow::Result<()> {
    debug!("Starting Redis message streaming");

    let db_config = get_database_config(&config)?;
    let mut con = create_redis_connection(MSGBUS_PUBLISH, &db_config)?;
    let stream_name = get_stream_name(trader_id, instance_id, &config);
    let mut last_id = "0".to_string();

    loop {
        let result: Result<Vec<HashMap<String, Vec<HashMap<String, redis::Value>>>>, _> =
            con.xread(&[&stream_name], &[&last_id]);
        match result {
            Ok(stream_bulk) => {
                for entry in stream_bulk.iter() {
                    for (_stream_key, stream_msgs) in entry.iter() {
                        for stream_msg in stream_msgs.iter() {
                            for (id, array) in stream_msg {
                                last_id.clone_from(id);

                                if let redis::Value::Array(array) = array {
                                    if array.len() < 4 {
                                        error!("Invalid stream message format: {:?}", array);
                                        continue;
                                    }

                                    let topic = match &array[1] {
                                        redis::Value::BulkString(bytes) => {
                                            Bytes::copy_from_slice(bytes)
                                        }
                                        _ => {
                                            error!("Invalid topic format: {:?}", array);
                                            continue;
                                        }
                                    };

                                    let payload = match &array[3] {
                                        redis::Value::BulkString(bytes) => {
                                            Bytes::copy_from_slice(bytes)
                                        }
                                        _ => {
                                            error!("Invalid payload format: {:?}", array);
                                            continue;
                                        }
                                    };

                                    let msg = BusMessage { topic, payload };

                                    if tx.blocking_send(msg).is_err() {
                                        debug!("Receiver closed channel");
                                        return Ok(());
                                    }
                                } else {
                                    error!("Invalid stream message format: {:?}", array);
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
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::thread;

    use redis::Commands;
    use serde_json::json;

    use super::*;

    #[tokio::test]
    async fn test_stream_messages() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<BusMessage>(100);

        let trader_id = TraderId::from("tester-001");
        let instance_id = UUID4::new();

        let mut config = HashMap::new();
        let db_config = json!({"type": "redis"});
        config.insert("database".to_string(), db_config.clone());
        config.insert("streams_prefix".to_string(), json!("stream"));
        config.insert("use_trader_prefix".to_string(), json!(true));
        config.insert("use_trader_id".to_string(), json!(true));

        let mut con = create_redis_connection(MSGBUS_STREAM, &db_config).unwrap();
        let stream_name = get_stream_name(trader_id, instance_id, &config);

        // Prepare test data
        let _: () = con
            .xadd(
                stream_name,
                "*",
                &[("topic", "topic1"), ("payload", "data1")],
            )
            .unwrap();

        // Start the message streaming in a separate thread
        thread::spawn(move || {
            stream_messages(tx, trader_id, instance_id, config).unwrap();
        });

        // Receive and verify the message
        let msg = rx.recv().await.unwrap();
        assert_eq!(msg.topic, "topic1");
        assert_eq!(msg.payload, Bytes::from("data1"));

        // Flush Redis database
        let _: () = redis::cmd("FLUSHDB").exec(&mut con).unwrap();
    }
}
