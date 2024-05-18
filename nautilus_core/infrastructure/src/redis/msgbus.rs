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
    sync::mpsc::{channel, Receiver, Sender, TryRecvError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use nautilus_common::msgbus::{core::CLOSE_TOPIC, database::MessageBusDatabaseAdapter, BusMessage};
use nautilus_core::{time::duration_since_unix_epoch, uuid::UUID4};
use nautilus_model::identifiers::trader_id::TraderId;
use redis::*;
use serde_json::Value;
use tracing::{debug, error};

use crate::redis::{create_redis_connection, get_buffer_interval, get_stream_name};

const XTRIM: &str = "XTRIM";
const MINID: &str = "MINID";
const TRIM_BUFFER_SECONDS: u64 = 60;

#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.infrastructure")
)]
pub struct RedisMessageBusDatabase {
    pub trader_id: TraderId,
    tx: Sender<BusMessage>,
    handle: Option<JoinHandle<anyhow::Result<()>>>,
}

impl MessageBusDatabaseAdapter for RedisMessageBusDatabase {
    type DatabaseType = RedisMessageBusDatabase;

    fn new(
        trader_id: TraderId,
        instance_id: UUID4,
        config: HashMap<String, serde_json::Value>,
    ) -> anyhow::Result<Self> {
        let config_clone = config.clone();
        let (tx, rx) = channel::<BusMessage>();
        let handle = Some(
            thread::Builder::new()
                .name("msgbus".to_string())
                .spawn(move || handle_messages(rx, trader_id, instance_id, config_clone))
                .expect("Error spawning `msgbus` thread"),
        );

        Ok(Self {
            trader_id,
            tx,
            handle,
        })
    }

    fn publish(&self, topic: String, payload: Vec<u8>) -> anyhow::Result<()> {
        let msg = BusMessage { topic, payload };
        self.tx.send(msg).map_err(anyhow::Error::new)
    }

    fn close(&mut self) -> anyhow::Result<()> {
        debug!("Closing message bus database adapter");

        let msg = BusMessage {
            topic: CLOSE_TOPIC.to_string(),
            payload: vec![],
        };
        self.tx.send(msg).map_err(anyhow::Error::new)?;

        if let Some(handle) = self.handle.take() {
            debug!("Joining `msgbus` thread");
            handle.join().map_err(|e| anyhow::anyhow!("{:?}", e))?
        } else {
            Err(anyhow::anyhow!("message bus database already shutdown"))
        }
    }
}

pub fn handle_messages(
    rx: Receiver<BusMessage>,
    trader_id: TraderId,
    instance_id: UUID4,
    config: HashMap<String, Value>,
) -> anyhow::Result<()> {
    let database_config = config
        .get("database")
        .ok_or(anyhow::anyhow!("No database config"))?;
    debug!("Creating msgbus redis connection");
    let mut conn = create_redis_connection(&database_config.clone())?;

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
                &mut conn,
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
            &mut conn,
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
        let key = format!("{stream_name}{}", &msg.topic);
        let items: Vec<(&str, &Vec<u8>)> = vec![("payload", &msg.payload)];
        pipe.xadd(&key, "*", &items);

        if autotrim_duration.is_none() {
            continue; // Nothing else to do
        }

        // Autotrim stream
        let last_trim_ms = last_trim_index.entry(key.clone()).or_insert(0); // Remove clone
        let unix_duration_now = duration_since_unix_epoch();
        let trim_buffer = Duration::from_secs(TRIM_BUFFER_SECONDS);

        // Improve efficiency of this by batching
        if *last_trim_ms < (unix_duration_now - trim_buffer).as_millis() as usize {
            let min_timestamp_ms =
                (unix_duration_now - autotrim_duration.unwrap()).as_millis() as usize;
            let result: Result<(), redis::RedisError> = redis::cmd(XTRIM)
                .arg(&key)
                .arg(MINID)
                .arg(min_timestamp_ms)
                .query(conn);

            if let Err(e) = result {
                error!("Error trimming stream '{key}': {e}");
            } else {
                last_trim_index.insert(key, unix_duration_now.as_millis() as usize);
            }
        }
    }

    pipe.query::<()>(conn).map_err(anyhow::Error::from)
}
