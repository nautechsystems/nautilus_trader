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
    thread,
    time::{Duration, Instant},
};

use nautilus_common::{
    cache::{CacheDatabase, DatabaseCommand, DatabaseOperation},
    redis::{create_redis_connection, get_buffer_interval},
};
use nautilus_core::uuid::UUID4;
use nautilus_model::identifiers::trader_id::TraderId;
use redis::{Commands, Connection, Pipeline};
use serde_json::{json, Value};
use tracing::debug;

// Error constants
const CHANNEL_TX_FAILED: &str = "Failed to send to channel";

// Redis constants
const FLUSHDB: &str = "FLUSHDB";
const DELIMITER: char = ':';

// Collection keys
const INDEX: &str = "index";
const GENERAL: &str = "general";
const CURRENCIES: &str = "currencies";
const INSTRUMENTS: &str = "instruments";
const SYNTHETICS: &str = "synthetics";
const ACCOUNTS: &str = "accounts";
const ORDERS: &str = "orders";
const POSITIONS: &str = "positions";
const ACTORS: &str = "actors";
const STRATEGIES: &str = "strategies";
const SNAPSHOTS: &str = "snapshots";
const HEALTH: &str = "health";

// Index keys
const INDEX_ORDER_IDS: &str = "index:order_ids";
const INDEX_ORDER_POSITION: &str = "index:order_position";
const INDEX_ORDER_CLIENT: &str = "index:order_client";
const INDEX_ORDERS: &str = "index:orders";
const INDEX_ORDERS_OPEN: &str = "index:orders_open";
const INDEX_ORDERS_CLOSED: &str = "index:orders_closed";
const INDEX_ORDERS_EMULATED: &str = "index:orders_emulated";
const INDEX_ORDERS_INFLIGHT: &str = "index:orders_inflight";
const INDEX_POSITIONS: &str = "index:positions";
const INDEX_POSITIONS_OPEN: &str = "index:positions_open";
const INDEX_POSITIONS_CLOSED: &str = "index:positions_closed";

#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.infrastructure")
)]
pub struct RedisCacheDatabase {
    pub trader_id: TraderId,
    trader_key: String,
    conn: Connection,
    tx: Sender<DatabaseCommand>,
}

impl CacheDatabase for RedisCacheDatabase {
    type DatabaseType = RedisCacheDatabase;

    fn new(
        trader_id: TraderId,
        instance_id: UUID4,
        config: HashMap<String, serde_json::Value>,
    ) -> anyhow::Result<RedisCacheDatabase> {
        let database_config = config
            .get("database")
            .ok_or(anyhow::anyhow!("No database config"))?;
        debug!("Creating cache-read redis connection");
        let conn = create_redis_connection(&database_config.clone())?;

        let (tx, rx) = channel::<DatabaseCommand>();
        let trader_key = get_trader_key(trader_id, instance_id, &config);
        let trader_key_clone = trader_key.clone();

        let _join_handle = thread::Builder::new()
            .name("cache".to_string())
            .spawn(move || {
                Self::handle_messages(rx, trader_key_clone, config);
            })
            .expect("Error spawning `cache` thread");

        Ok(RedisCacheDatabase {
            trader_id,
            trader_key,
            conn,
            tx,
        })
    }

    fn flushdb(&mut self) -> anyhow::Result<()> {
        match redis::cmd(FLUSHDB).query::<()>(&mut self.conn) {
            Ok(_) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    fn keys(&mut self, pattern: &str) -> anyhow::Result<Vec<String>> {
        let pattern = format!("{}{DELIMITER}{}", self.trader_key, pattern);
        debug!("Querying keys: {pattern}");
        match self.conn.keys(pattern) {
            Ok(keys) => Ok(keys),
            Err(e) => Err(e.into()),
        }
    }

    fn read(&mut self, key: &str) -> anyhow::Result<Vec<Vec<u8>>> {
        let collection = get_collection_key(key)?;
        let key = format!("{}{DELIMITER}{}", self.trader_key, key);

        match collection {
            INDEX => read_index(&mut self.conn, &key),
            GENERAL => read_string(&mut self.conn, &key),
            CURRENCIES => read_string(&mut self.conn, &key),
            INSTRUMENTS => read_string(&mut self.conn, &key),
            SYNTHETICS => read_string(&mut self.conn, &key),
            ACCOUNTS => read_list(&mut self.conn, &key),
            ORDERS => read_list(&mut self.conn, &key),
            POSITIONS => read_list(&mut self.conn, &key),
            ACTORS => read_string(&mut self.conn, &key),
            STRATEGIES => read_string(&mut self.conn, &key),
            _ => anyhow::bail!("Unsupported operation: `read` for collection '{collection}'"),
        }
    }

    fn insert(&mut self, key: String, payload: Option<Vec<Vec<u8>>>) -> anyhow::Result<()> {
        let op = DatabaseCommand::new(DatabaseOperation::Insert, key, payload);
        match self.tx.send(op) {
            Ok(_) => Ok(()),
            Err(e) => anyhow::bail!("{CHANNEL_TX_FAILED}: {e}"),
        }
    }

    fn update(&mut self, key: String, payload: Option<Vec<Vec<u8>>>) -> anyhow::Result<()> {
        let op = DatabaseCommand::new(DatabaseOperation::Update, key, payload);
        match self.tx.send(op) {
            Ok(_) => Ok(()),
            Err(e) => anyhow::bail!("{CHANNEL_TX_FAILED}: {e}"),
        }
    }

    fn delete(&mut self, key: String, payload: Option<Vec<Vec<u8>>>) -> anyhow::Result<()> {
        let op = DatabaseCommand::new(DatabaseOperation::Delete, key, payload);
        match self.tx.send(op) {
            Ok(_) => Ok(()),
            Err(e) => anyhow::bail!("{CHANNEL_TX_FAILED}: {e}"),
        }
    }

    fn handle_messages(
        rx: Receiver<DatabaseCommand>,
        trader_key: String,
        config: HashMap<String, serde_json::Value>,
    ) {
        let empty = Value::Object(serde_json::Map::new());
        let database_config = config.get("database").unwrap_or(&empty);
        debug!("Creating cache-write redis connection");
        let mut conn = create_redis_connection(&database_config.clone()).unwrap();

        // Buffering
        let mut buffer: VecDeque<DatabaseCommand> = VecDeque::new();
        let mut last_drain = Instant::now();
        let recv_interval = Duration::from_millis(1);
        let buffer_interval = get_buffer_interval(&config);

        loop {
            if last_drain.elapsed() >= buffer_interval && !buffer.is_empty() {
                drain_buffer(&mut conn, &trader_key, &mut buffer);
                last_drain = Instant::now();
            } else {
                // Continue to receive and handle messages until channel is hung up
                match rx.try_recv() {
                    Ok(msg) => buffer.push_back(msg),
                    Err(TryRecvError::Empty) => thread::sleep(recv_interval),
                    Err(TryRecvError::Disconnected) => break, // Channel hung up
                }
            }
        }

        // Drain any remaining messages
        if !buffer.is_empty() {
            drain_buffer(&mut conn, &trader_key, &mut buffer);
        }
    }
}

fn drain_buffer(conn: &mut Connection, trader_key: &str, buffer: &mut VecDeque<DatabaseCommand>) {
    let mut pipe = redis::pipe();
    pipe.atomic();

    for msg in buffer.drain(..) {
        let collection = match get_collection_key(&msg.key) {
            Ok(collection) => collection,
            Err(e) => {
                eprintln!("{e}");
                continue; // Continue to next message
            }
        };

        let key = format!("{trader_key}{DELIMITER}{}", msg.key);

        match msg.op_type {
            DatabaseOperation::Insert => {
                if msg.payload.is_none() {
                    eprintln!("Null `payload` for `insert`");
                    continue; // Continue to next message
                };

                let payload = msg
                    .payload
                    .as_ref()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_slice())
                    .collect::<Vec<&[u8]>>();

                if let Err(e) = insert(&mut pipe, collection, &key, payload) {
                    eprintln!("{e}");
                }
            }
            DatabaseOperation::Update => {
                if msg.payload.is_none() {
                    eprintln!("Null `payload` for `update`");
                    continue; // Continue to next message
                };

                let payload = msg
                    .payload
                    .as_ref()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_slice())
                    .collect::<Vec<&[u8]>>();

                if let Err(e) = update(&mut pipe, collection, &key, payload) {
                    eprintln!("{e}");
                }
            }
            DatabaseOperation::Delete => {
                // `payload` can be `None` for a delete operation
                let payload = msg
                    .payload
                    .as_ref()
                    .map(|v| v.iter().map(|v| v.as_slice()).collect::<Vec<&[u8]>>());

                if let Err(e) = delete(&mut pipe, collection, &key, payload) {
                    eprintln!("{e}");
                }
            }
        }
    }

    if let Err(e) = pipe.query::<()>(conn) {
        eprintln!("{e}");
    }
}

fn read_index(conn: &mut Connection, key: &str) -> anyhow::Result<Vec<Vec<u8>>> {
    let index_key = get_index_key(key)?;
    match index_key {
        INDEX_ORDER_IDS => read_set(conn, key),
        INDEX_ORDER_POSITION => read_hset(conn, key),
        INDEX_ORDER_CLIENT => read_hset(conn, key),
        INDEX_ORDERS => read_set(conn, key),
        INDEX_ORDERS_OPEN => read_set(conn, key),
        INDEX_ORDERS_CLOSED => read_set(conn, key),
        INDEX_ORDERS_EMULATED => read_set(conn, key),
        INDEX_ORDERS_INFLIGHT => read_set(conn, key),
        INDEX_POSITIONS => read_set(conn, key),
        INDEX_POSITIONS_OPEN => read_set(conn, key),
        INDEX_POSITIONS_CLOSED => read_set(conn, key),
        _ => anyhow::bail!("Index unknown '{index_key}' on read"),
    }
}

fn read_string(conn: &mut Connection, key: &str) -> anyhow::Result<Vec<Vec<u8>>> {
    let result: Vec<u8> = conn.get(key)?;

    if result.is_empty() {
        Ok(vec![])
    } else {
        Ok(vec![result])
    }
}

fn read_set(conn: &mut Connection, key: &str) -> anyhow::Result<Vec<Vec<u8>>> {
    let result: Vec<Vec<u8>> = conn.smembers(key)?;
    Ok(result)
}

fn read_hset(conn: &mut Connection, key: &str) -> anyhow::Result<Vec<Vec<u8>>> {
    let result: HashMap<String, String> = conn.hgetall(key)?;
    let json = serde_json::to_string(&result)?;
    Ok(vec![json.into_bytes()])
}

fn read_list(conn: &mut Connection, key: &str) -> anyhow::Result<Vec<Vec<u8>>> {
    let result: Vec<Vec<u8>> = conn.lrange(key, 0, -1)?;
    Ok(result)
}

fn insert(
    pipe: &mut Pipeline,
    collection: &str,
    key: &str,
    value: Vec<&[u8]>,
) -> anyhow::Result<()> {
    if value.is_empty() {
        anyhow::bail!("Empty `payload` for `insert`")
    }

    match collection {
        INDEX => insert_index(pipe, key, &value),
        GENERAL => {
            insert_string(pipe, key, value[0]);
            Ok(())
        }
        CURRENCIES => {
            insert_string(pipe, key, value[0]);
            Ok(())
        }
        INSTRUMENTS => {
            insert_string(pipe, key, value[0]);
            Ok(())
        }
        SYNTHETICS => {
            insert_string(pipe, key, value[0]);
            Ok(())
        }
        ACCOUNTS => {
            insert_list(pipe, key, value[0]);
            Ok(())
        }
        ORDERS => {
            insert_list(pipe, key, value[0]);
            Ok(())
        }
        POSITIONS => {
            insert_list(pipe, key, value[0]);
            Ok(())
        }
        ACTORS => {
            insert_string(pipe, key, value[0]);
            Ok(())
        }
        STRATEGIES => {
            insert_string(pipe, key, value[0]);
            Ok(())
        }
        SNAPSHOTS => {
            insert_list(pipe, key, value[0]);
            Ok(())
        }
        HEALTH => {
            insert_string(pipe, key, value[0]);
            Ok(())
        }
        _ => anyhow::bail!("Unsupported operation: `insert` for collection '{collection}'"),
    }
}

fn insert_index(pipe: &mut Pipeline, key: &str, value: &[&[u8]]) -> anyhow::Result<()> {
    let index_key = get_index_key(key)?;
    match index_key {
        INDEX_ORDER_IDS => {
            insert_set(pipe, key, value[0]);
            Ok(())
        }
        INDEX_ORDER_POSITION => {
            insert_hset(pipe, key, value[0], value[1]);
            Ok(())
        }
        INDEX_ORDER_CLIENT => {
            insert_hset(pipe, key, value[0], value[1]);
            Ok(())
        }
        INDEX_ORDERS => {
            insert_set(pipe, key, value[0]);
            Ok(())
        }
        INDEX_ORDERS_OPEN => {
            insert_set(pipe, key, value[0]);
            Ok(())
        }
        INDEX_ORDERS_CLOSED => {
            insert_set(pipe, key, value[0]);
            Ok(())
        }
        INDEX_ORDERS_EMULATED => {
            insert_set(pipe, key, value[0]);
            Ok(())
        }
        INDEX_ORDERS_INFLIGHT => {
            insert_set(pipe, key, value[0]);
            Ok(())
        }
        INDEX_POSITIONS => {
            insert_set(pipe, key, value[0]);
            Ok(())
        }
        INDEX_POSITIONS_OPEN => {
            insert_set(pipe, key, value[0]);
            Ok(())
        }
        INDEX_POSITIONS_CLOSED => {
            insert_set(pipe, key, value[0]);
            Ok(())
        }
        _ => anyhow::bail!("Index unknown '{index_key}' on insert"),
    }
}

fn insert_string(pipe: &mut Pipeline, key: &str, value: &[u8]) {
    pipe.set(key, value);
}

fn insert_set(pipe: &mut Pipeline, key: &str, value: &[u8]) {
    pipe.sadd(key, value);
}

fn insert_hset(pipe: &mut Pipeline, key: &str, name: &[u8], value: &[u8]) {
    pipe.hset(key, name, value);
}

fn insert_list(pipe: &mut Pipeline, key: &str, value: &[u8]) {
    pipe.rpush(key, value);
}

fn update(
    pipe: &mut Pipeline,
    collection: &str,
    key: &str,
    value: Vec<&[u8]>,
) -> anyhow::Result<()> {
    if value.is_empty() {
        anyhow::bail!("Empty `payload` for `update`")
    }

    match collection {
        ACCOUNTS => {
            update_list(pipe, key, value[0]);
            Ok(())
        }
        ORDERS => {
            update_list(pipe, key, value[0]);
            Ok(())
        }
        POSITIONS => {
            update_list(pipe, key, value[0]);
            Ok(())
        }
        _ => anyhow::bail!("Unsupported operation: `update` for collection '{collection}'"),
    }
}

fn update_list(pipe: &mut Pipeline, key: &str, value: &[u8]) {
    pipe.rpush_exists(key, value);
}

fn delete(
    pipe: &mut Pipeline,
    collection: &str,
    key: &str,
    value: Option<Vec<&[u8]>>,
) -> anyhow::Result<()> {
    match collection {
        INDEX => remove_index(pipe, key, value),
        ACTORS => {
            delete_string(pipe, key);
            Ok(())
        }
        STRATEGIES => {
            delete_string(pipe, key);
            Ok(())
        }
        _ => anyhow::bail!("Unsupported operation: `delete` for collection '{collection}'"),
    }
}

fn remove_index(pipe: &mut Pipeline, key: &str, value: Option<Vec<&[u8]>>) -> anyhow::Result<()> {
    let value = value.ok_or_else(|| anyhow::anyhow!("Empty `payload` for `delete` '{key}'"))?;
    let index_key = get_index_key(key)?;

    match index_key {
        INDEX_ORDERS_OPEN => {
            remove_from_set(pipe, key, value[0]);
            Ok(())
        }
        INDEX_ORDERS_CLOSED => {
            remove_from_set(pipe, key, value[0]);
            Ok(())
        }
        INDEX_ORDERS_EMULATED => {
            remove_from_set(pipe, key, value[0]);
            Ok(())
        }
        INDEX_ORDERS_INFLIGHT => {
            remove_from_set(pipe, key, value[0]);
            Ok(())
        }
        INDEX_POSITIONS_OPEN => {
            remove_from_set(pipe, key, value[0]);
            Ok(())
        }
        INDEX_POSITIONS_CLOSED => {
            remove_from_set(pipe, key, value[0]);
            Ok(())
        }
        _ => anyhow::bail!("Unsupported index operation: remove from '{index_key}'"),
    }
}

fn remove_from_set(pipe: &mut Pipeline, key: &str, member: &[u8]) {
    pipe.srem(key, member);
}

fn delete_string(pipe: &mut Pipeline, key: &str) {
    pipe.del(key);
}

fn get_trader_key(
    trader_id: TraderId,
    instance_id: UUID4,
    config: &HashMap<String, serde_json::Value>,
) -> String {
    let mut key = String::new();

    if let Some(json!(true)) = config.get("use_trader_prefix") {
        key.push_str("trader-");
    }

    key.push_str(trader_id.value.as_str());

    if let Some(json!(true)) = config.get("use_instance_id") {
        key.push(DELIMITER);
        key.push_str(&format!("{instance_id}"));
    }

    key
}

fn get_collection_key(key: &str) -> anyhow::Result<&str> {
    key.split_once(DELIMITER)
        .map(|(collection, _)| collection)
        .ok_or_else(|| {
            anyhow::anyhow!("Invalid `key`, missing a '{DELIMITER}' delimiter, was {key}")
        })
}

fn get_index_key(key: &str) -> anyhow::Result<&str> {
    key.split_once(DELIMITER)
        .map(|(_, index_key)| index_key)
        .ok_or_else(|| {
            anyhow::anyhow!("Invalid `key`, missing a '{DELIMITER}' delimiter, was {key}")
        })
}

// This function can be used when we handle cache serialization in Rust
#[allow(dead_code)]
fn get_encoding(config: &HashMap<String, serde_json::Value>) -> String {
    config
        .get("encoding")
        .and_then(|v| v.as_str())
        .unwrap_or("msgpack")
        .to_string()
}

// This function can be used when we handle cache serialization in Rust
#[allow(dead_code)]
fn deserialize_payload(
    encoding: &str,
    payload: &[u8],
) -> anyhow::Result<HashMap<String, serde_json::Value>> {
    match encoding {
        "msgpack" => rmp_serde::from_slice(payload)
            .map_err(|e| anyhow::anyhow!("Failed to deserialize msgpack `payload`: {e}")),
        "json" => serde_json::from_slice(payload)
            .map_err(|e| anyhow::anyhow!("Failed to deserialize json `payload`: {e}")),
        _ => Err(anyhow::anyhow!("Unsupported encoding: {encoding}")),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use rstest::rstest;
    use serde_json::json;

    use super::*;

    #[rstest]
    fn test_get_trader_key_with_prefix_and_instance_id() {
        let trader_id = TraderId::from("tester-123");
        let instance_id = UUID4::new();
        let mut config = HashMap::new();
        config.insert("use_trader_prefix".to_string(), json!(true));
        config.insert("use_instance_id".to_string(), json!(true));

        let key = get_trader_key(trader_id, instance_id, &config);
        assert!(key.starts_with("trader-tester-123:"));
        assert!(key.ends_with(&instance_id.to_string()));
    }

    #[rstest]
    fn test_get_collection_key_valid() {
        let key = "collection:123";
        assert_eq!(get_collection_key(key).unwrap(), "collection");
    }

    #[rstest]
    fn test_get_collection_key_invalid() {
        let key = "no_delimiter";
        assert!(get_collection_key(key).is_err());
    }

    #[rstest]
    fn test_get_index_key_valid() {
        let key = "index:123";
        assert_eq!(get_index_key(key).unwrap(), "123");
    }

    #[rstest]
    fn test_get_index_key_invalid() {
        let key = "no_delimiter";
        assert!(get_index_key(key).is_err());
    }
}
