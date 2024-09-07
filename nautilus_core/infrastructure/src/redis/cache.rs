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
    str::FromStr,
    time::{Duration, Instant},
};

use bytes::Bytes;
use nautilus_common::{
    cache::{database::CacheDatabaseAdapter, CacheConfig},
    enums::SerializationEncoding,
    runtime::get_runtime,
};
use nautilus_core::{correctness::check_slice_not_empty, nanos::UnixNanos, uuid::UUID4};
use nautilus_model::{
    accounts::any::AccountAny,
    data::{bar::Bar, quote::QuoteTick, trade::TradeTick},
    identifiers::{
        AccountId, ClientId, ClientOrderId, ComponentId, InstrumentId, PositionId, StrategyId,
        TraderId, VenueOrderId,
    },
    instruments::{any::InstrumentAny, synthetic::SyntheticInstrument},
    orderbook::book::OrderBook,
    orders::any::OrderAny,
    position::Position,
    types::currency::Currency,
};
use redis::{Commands, Connection, Pipeline};
use ustr::Ustr;

use super::{REDIS_DELIMITER, REDIS_FLUSHDB};
use crate::redis::create_redis_connection;

// Task and connection names
const CACHE_READ: &str = "cache-read";
const CACHE_WRITE: &str = "cache-write";

// Error constants
const FAILED_TX_CHANNEL: &str = "Failed to send to channel";

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

/// A type of database operation.
#[derive(Clone, Debug)]
pub enum DatabaseOperation {
    Insert,
    Update,
    Delete,
    Close,
}

/// Represents a database command to be performed which may be executed in a task.
#[derive(Clone, Debug)]
pub struct DatabaseCommand {
    /// The database operation type.
    pub op_type: DatabaseOperation,
    /// The primary key for the operation.
    pub key: Option<String>,
    /// The data payload for the operation.
    pub payload: Option<Vec<Bytes>>,
}

impl DatabaseCommand {
    /// Creates a new [`DatabaseCommand`] instance.
    #[must_use]
    pub fn new(op_type: DatabaseOperation, key: String, payload: Option<Vec<Bytes>>) -> Self {
        Self {
            op_type,
            key: Some(key),
            payload,
        }
    }

    /// Initialize a `Close` database command, this is meant to close the database cache channel.
    #[must_use]
    pub fn close() -> Self {
        Self {
            op_type: DatabaseOperation::Close,
            key: None,
            payload: None,
        }
    }
}

#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.infrastructure")
)]
pub struct RedisCacheDatabase {
    pub trader_id: TraderId,
    trader_key: String,
    con: Connection,
    tx: tokio::sync::mpsc::UnboundedSender<DatabaseCommand>,
    handle: tokio::task::JoinHandle<()>,
}

impl RedisCacheDatabase {
    /// Creates a new [`RedisCacheDatabase`] instance.
    pub fn new(
        trader_id: TraderId,
        instance_id: UUID4,
        config: CacheConfig,
    ) -> anyhow::Result<RedisCacheDatabase> {
        let db_config = config
            .database
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No database config"))?;
        let con = create_redis_connection(CACHE_READ, db_config.clone())?;

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<DatabaseCommand>();
        let trader_key = get_trader_key(trader_id, instance_id, &config);
        let trader_key_clone = trader_key.clone();

        let handle = get_runtime().spawn(async move {
            process_commands(rx, trader_key_clone, config.clone())
                .await
                .expect("Error spawning task '{CACHE_WRITE}'")
        });

        Ok(RedisCacheDatabase {
            trader_id,
            trader_key,
            con,
            tx,
            handle,
        })
    }

    pub fn close(&mut self) {
        log::debug!("Closing");

        if let Err(e) = self.tx.send(DatabaseCommand::close()) {
            log::debug!("Error sending close message: {e:?}")
        }

        log::debug!("Awaiting task '{CACHE_WRITE}'");
        tokio::task::block_in_place(|| {
            if let Err(e) = get_runtime().block_on(&mut self.handle) {
                log::error!("Error awaiting task '{CACHE_WRITE}': {:?}", e);
            }
        });

        log::debug!("Closed");
    }

    pub fn flushdb(&mut self) {
        if let Err(e) = redis::cmd(REDIS_FLUSHDB).query::<()>(&mut self.con) {
            log::error!("Failed to flush database: {:?}", e);
        }
    }

    pub fn keys(&mut self, pattern: &str) -> anyhow::Result<Vec<String>> {
        let pattern = format!("{}{REDIS_DELIMITER}{}", self.trader_key, pattern);
        log::debug!("Querying keys: {pattern}");
        match self.con.keys(pattern) {
            Ok(keys) => Ok(keys),
            Err(e) => Err(e.into()),
        }
    }

    pub fn read(&mut self, key: &str) -> anyhow::Result<Vec<Bytes>> {
        let collection = get_collection_key(key)?;
        let key = format!("{}{REDIS_DELIMITER}{}", self.trader_key, key);

        match collection {
            INDEX => read_index(&mut self.con, &key),
            GENERAL => read_string(&mut self.con, &key),
            CURRENCIES => read_string(&mut self.con, &key),
            INSTRUMENTS => read_string(&mut self.con, &key),
            SYNTHETICS => read_string(&mut self.con, &key),
            ACCOUNTS => read_list(&mut self.con, &key),
            ORDERS => read_list(&mut self.con, &key),
            POSITIONS => read_list(&mut self.con, &key),
            ACTORS => read_string(&mut self.con, &key),
            STRATEGIES => read_string(&mut self.con, &key),
            _ => anyhow::bail!("Unsupported operation: `read` for collection '{collection}'"),
        }
    }

    pub fn insert(&mut self, key: String, payload: Option<Vec<Bytes>>) -> anyhow::Result<()> {
        let op = DatabaseCommand::new(DatabaseOperation::Insert, key, payload);
        match self.tx.send(op) {
            Ok(_) => Ok(()),
            Err(e) => anyhow::bail!("{FAILED_TX_CHANNEL}: {e}"),
        }
    }

    pub fn update(&mut self, key: String, payload: Option<Vec<Bytes>>) -> anyhow::Result<()> {
        let op = DatabaseCommand::new(DatabaseOperation::Update, key, payload);
        match self.tx.send(op) {
            Ok(_) => Ok(()),
            Err(e) => anyhow::bail!("{FAILED_TX_CHANNEL}: {e}"),
        }
    }

    pub fn delete(&mut self, key: String, payload: Option<Vec<Bytes>>) -> anyhow::Result<()> {
        let op = DatabaseCommand::new(DatabaseOperation::Delete, key, payload);
        match self.tx.send(op) {
            Ok(_) => Ok(()),
            Err(e) => anyhow::bail!("{FAILED_TX_CHANNEL}: {e}"),
        }
    }
}

async fn process_commands(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<DatabaseCommand>,
    trader_key: String,
    config: CacheConfig,
) -> anyhow::Result<()> {
    tracing::debug!("Starting cache processing");

    let db_config = config
        .database
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("No database config"))?;
    let mut con = create_redis_connection(CACHE_WRITE, db_config.clone())?;

    // Buffering
    let mut buffer: VecDeque<DatabaseCommand> = VecDeque::new();
    let mut last_drain = Instant::now();
    let buffer_interval = Duration::from_millis(config.buffer_interval_ms.unwrap_or(0) as u64);

    loop {
        if last_drain.elapsed() >= buffer_interval && !buffer.is_empty() {
            drain_buffer(&mut con, &trader_key, &mut buffer);
            last_drain = Instant::now();
        } else {
            // Continue to receive and handle messages until channel is hung up
            match rx.recv().await {
                Some(msg) => {
                    if let DatabaseOperation::Close = msg.op_type {
                        // Close receiver end of the channel
                        drop(rx);
                        break;
                    }
                    buffer.push_back(msg)
                }
                None => break, // Channel hung up
            }
        }
    }

    // Drain any remaining messages
    if !buffer.is_empty() {
        drain_buffer(&mut con, &trader_key, &mut buffer);
    }

    tracing::debug!("Stopped cache processing");
    Ok(())
}

fn drain_buffer(conn: &mut Connection, trader_key: &str, buffer: &mut VecDeque<DatabaseCommand>) {
    let mut pipe = redis::pipe();
    pipe.atomic();

    for msg in buffer.drain(..) {
        let key = msg.key.expect("Null command `key`");
        let collection = match get_collection_key(&key) {
            Ok(collection) => collection,
            Err(e) => {
                tracing::error!("{e}");
                continue; // Continue to next message
            }
        };

        let key = format!("{trader_key}{REDIS_DELIMITER}{}", &key);

        match msg.op_type {
            DatabaseOperation::Insert => {
                if let Some(payload) = msg.payload {
                    if let Err(e) = insert(&mut pipe, collection, &key, payload) {
                        tracing::error!("{e}");
                    }
                } else {
                    tracing::error!("Null `payload` for `insert`");
                }
            }
            DatabaseOperation::Update => {
                if let Some(payload) = msg.payload {
                    if let Err(e) = update(&mut pipe, collection, &key, payload) {
                        tracing::error!("{e}");
                    }
                } else {
                    tracing::error!("Null `payload` for `update`");
                };
            }
            DatabaseOperation::Delete => {
                // `payload` can be `None` for a delete operation
                if let Err(e) = delete(&mut pipe, collection, &key, msg.payload) {
                    tracing::error!("{e}");
                }
            }
            DatabaseOperation::Close => panic!("Close command should not be drained"),
        }
    }

    if let Err(e) = pipe.query::<()>(conn) {
        tracing::error!("{e}");
    }
}

fn read_index(conn: &mut Connection, key: &str) -> anyhow::Result<Vec<Bytes>> {
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

fn read_string(conn: &mut Connection, key: &str) -> anyhow::Result<Vec<Bytes>> {
    let result: Vec<u8> = conn.get(key)?;

    if result.is_empty() {
        Ok(vec![])
    } else {
        Ok(vec![Bytes::from(result)])
    }
}

fn read_set(conn: &mut Connection, key: &str) -> anyhow::Result<Vec<Bytes>> {
    let result: Vec<Bytes> = conn.smembers(key)?;
    Ok(result)
}

fn read_hset(conn: &mut Connection, key: &str) -> anyhow::Result<Vec<Bytes>> {
    let result: HashMap<String, String> = conn.hgetall(key)?;
    let json = serde_json::to_string(&result)?;
    Ok(vec![Bytes::from(json.into_bytes())])
}

fn read_list(conn: &mut Connection, key: &str) -> anyhow::Result<Vec<Bytes>> {
    let result: Vec<Bytes> = conn.lrange(key, 0, -1)?;
    Ok(result)
}

fn insert(
    pipe: &mut Pipeline,
    collection: &str,
    key: &str,
    value: Vec<Bytes>,
) -> anyhow::Result<()> {
    check_slice_not_empty(value.as_slice(), stringify!(value))?;

    match collection {
        INDEX => insert_index(pipe, key, &value),
        GENERAL => {
            insert_string(pipe, key, value[0].as_ref());
            Ok(())
        }
        CURRENCIES => {
            insert_string(pipe, key, value[0].as_ref());
            Ok(())
        }
        INSTRUMENTS => {
            insert_string(pipe, key, value[0].as_ref());
            Ok(())
        }
        SYNTHETICS => {
            insert_string(pipe, key, value[0].as_ref());
            Ok(())
        }
        ACCOUNTS => {
            insert_list(pipe, key, value[0].as_ref());
            Ok(())
        }
        ORDERS => {
            insert_list(pipe, key, value[0].as_ref());
            Ok(())
        }
        POSITIONS => {
            insert_list(pipe, key, value[0].as_ref());
            Ok(())
        }
        ACTORS => {
            insert_string(pipe, key, value[0].as_ref());
            Ok(())
        }
        STRATEGIES => {
            insert_string(pipe, key, value[0].as_ref());
            Ok(())
        }
        SNAPSHOTS => {
            insert_list(pipe, key, value[0].as_ref());
            Ok(())
        }
        HEALTH => {
            insert_string(pipe, key, value[0].as_ref());
            Ok(())
        }
        _ => anyhow::bail!("Unsupported operation: `insert` for collection '{collection}'"),
    }
}

fn insert_index(pipe: &mut Pipeline, key: &str, value: &[Bytes]) -> anyhow::Result<()> {
    let index_key = get_index_key(key)?;
    match index_key {
        INDEX_ORDER_IDS => {
            insert_set(pipe, key, value[0].as_ref());
            Ok(())
        }
        INDEX_ORDER_POSITION => {
            insert_hset(pipe, key, value[0].as_ref(), value[1].as_ref());
            Ok(())
        }
        INDEX_ORDER_CLIENT => {
            insert_hset(pipe, key, value[0].as_ref(), value[1].as_ref());
            Ok(())
        }
        INDEX_ORDERS => {
            insert_set(pipe, key, value[0].as_ref());
            Ok(())
        }
        INDEX_ORDERS_OPEN => {
            insert_set(pipe, key, value[0].as_ref());
            Ok(())
        }
        INDEX_ORDERS_CLOSED => {
            insert_set(pipe, key, value[0].as_ref());
            Ok(())
        }
        INDEX_ORDERS_EMULATED => {
            insert_set(pipe, key, value[0].as_ref());
            Ok(())
        }
        INDEX_ORDERS_INFLIGHT => {
            insert_set(pipe, key, value[0].as_ref());
            Ok(())
        }
        INDEX_POSITIONS => {
            insert_set(pipe, key, value[0].as_ref());
            Ok(())
        }
        INDEX_POSITIONS_OPEN => {
            insert_set(pipe, key, value[0].as_ref());
            Ok(())
        }
        INDEX_POSITIONS_CLOSED => {
            insert_set(pipe, key, value[0].as_ref());
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
    value: Vec<Bytes>,
) -> anyhow::Result<()> {
    check_slice_not_empty(value.as_slice(), stringify!(value))?;

    match collection {
        ACCOUNTS => {
            update_list(pipe, key, value[0].as_ref());
            Ok(())
        }
        ORDERS => {
            update_list(pipe, key, value[0].as_ref());
            Ok(())
        }
        POSITIONS => {
            update_list(pipe, key, value[0].as_ref());
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
    value: Option<Vec<Bytes>>,
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

fn remove_index(pipe: &mut Pipeline, key: &str, value: Option<Vec<Bytes>>) -> anyhow::Result<()> {
    let value = value.ok_or_else(|| anyhow::anyhow!("Empty `payload` for `delete` '{key}'"))?;
    let index_key = get_index_key(key)?;

    match index_key {
        INDEX_ORDERS_OPEN => {
            remove_from_set(pipe, key, value[0].as_ref());
            Ok(())
        }
        INDEX_ORDERS_CLOSED => {
            remove_from_set(pipe, key, value[0].as_ref());
            Ok(())
        }
        INDEX_ORDERS_EMULATED => {
            remove_from_set(pipe, key, value[0].as_ref());
            Ok(())
        }
        INDEX_ORDERS_INFLIGHT => {
            remove_from_set(pipe, key, value[0].as_ref());
            Ok(())
        }
        INDEX_POSITIONS_OPEN => {
            remove_from_set(pipe, key, value[0].as_ref());
            Ok(())
        }
        INDEX_POSITIONS_CLOSED => {
            remove_from_set(pipe, key, value[0].as_ref());
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

fn get_trader_key(trader_id: TraderId, instance_id: UUID4, config: &CacheConfig) -> String {
    let mut key = String::new();

    if config.use_trader_prefix {
        key.push_str("trader-");
    }

    key.push_str(trader_id.as_str());

    if config.use_instance_id {
        key.push(REDIS_DELIMITER);
        key.push_str(&format!("{instance_id}"));
    }

    key
}

fn get_collection_key(key: &str) -> anyhow::Result<&str> {
    key.split_once(REDIS_DELIMITER)
        .map(|(collection, _)| collection)
        .ok_or_else(|| {
            anyhow::anyhow!("Invalid `key`, missing a '{REDIS_DELIMITER}' delimiter, was {key}")
        })
}

fn get_index_key(key: &str) -> anyhow::Result<&str> {
    key.split_once(REDIS_DELIMITER)
        .map(|(_, index_key)| index_key)
        .ok_or_else(|| {
            anyhow::anyhow!("Invalid `key`, missing a '{REDIS_DELIMITER}' delimiter, was {key}")
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

#[allow(dead_code)] // Under development
pub struct RedisCacheDatabaseAdapter {
    pub encoding: SerializationEncoding,
    database: RedisCacheDatabase,
}

#[allow(dead_code)] // Under development
#[allow(unused)] // Under development
impl CacheDatabaseAdapter for RedisCacheDatabaseAdapter {
    fn close(&mut self) {
        self.database.close()
    }

    fn flush(&mut self) {
        self.database.flushdb()
    }

    fn load(&mut self) -> anyhow::Result<HashMap<String, Bytes>> {
        // self.database.load()
        Ok(HashMap::new()) // TODO
    }

    fn load_currencies(&mut self) -> anyhow::Result<HashMap<Ustr, Currency>> {
        let mut currencies = HashMap::new();

        for key in self.database.keys(&format!("{CURRENCIES}*"))? {
            let parts: Vec<&str> = key.as_str().rsplitn(2, ':').collect();
            let currency_code = Ustr::from(parts.first().unwrap());
            let result = self.load_currency(&currency_code)?;
            match result {
                Some(currency) => {
                    currencies.insert(currency_code, currency);
                }
                None => {
                    log::error!("Currency not found: {currency_code}");
                }
            }
        }
        Ok(currencies)
    }

    fn load_instruments(&mut self) -> anyhow::Result<HashMap<InstrumentId, InstrumentAny>> {
        let mut instruments = HashMap::new();

        for key in self.database.keys(&format!("{INSTRUMENTS}*"))? {
            let parts: Vec<&str> = key.as_str().rsplitn(2, ':').collect();
            let instrument_id = InstrumentId::from_str(parts.first().unwrap())?;
            let result = self.load_instrument(&instrument_id)?;
            match result {
                Some(instrument) => {
                    instruments.insert(instrument_id, instrument);
                }
                None => {
                    log::error!("Instrument not found: {instrument_id}");
                }
            }
        }

        Ok(instruments)
    }

    fn load_synthetics(&mut self) -> anyhow::Result<HashMap<InstrumentId, SyntheticInstrument>> {
        let mut synthetics = HashMap::new();

        for key in self.database.keys(&format!("{SYNTHETICS}*"))? {
            let parts: Vec<&str> = key.as_str().rsplitn(2, ':').collect();
            let instrument_id = InstrumentId::from_str(parts.first().unwrap())?;
            let synthetic = self.load_synthetic(&instrument_id)?;
            synthetics.insert(instrument_id, synthetic);
        }

        Ok(synthetics)
    }

    fn load_accounts(&mut self) -> anyhow::Result<HashMap<AccountId, AccountAny>> {
        let mut accounts = HashMap::new();

        for key in self.database.keys(&format!("{ACCOUNTS}*"))? {
            let parts: Vec<&str> = key.as_str().rsplitn(2, ':').collect();
            let account_id = AccountId::from(*parts.first().unwrap());
            let result = self.load_account(&account_id)?;
            match result {
                Some(account) => {
                    accounts.insert(account_id, account);
                }
                None => {
                    log::error!("Account not found: {account_id}");
                }
            }
        }

        Ok(accounts)
    }

    fn load_orders(&mut self) -> anyhow::Result<HashMap<ClientOrderId, OrderAny>> {
        let mut orders = HashMap::new();

        for key in self.database.keys(&format!("{ORDERS}*"))? {
            let parts: Vec<&str> = key.as_str().rsplitn(2, ':').collect();
            let client_order_id = ClientOrderId::from(*parts.first().unwrap());
            let result = self.load_order(&client_order_id)?;
            match result {
                Some(order) => {
                    orders.insert(client_order_id, order);
                }
                None => {
                    log::error!("Order not found: {client_order_id}");
                }
            }
        }
        Ok(orders)
    }

    fn load_positions(&mut self) -> anyhow::Result<HashMap<PositionId, Position>> {
        let mut positions = HashMap::new();

        for key in self.database.keys(&format!("{POSITIONS}*"))? {
            let parts: Vec<&str> = key.as_str().rsplitn(2, ':').collect();
            let position_id = PositionId::from(*parts.first().unwrap());
            let position = self.load_position(&position_id)?;
            positions.insert(position_id, position);
        }

        Ok(positions)
    }

    fn load_index_order_position(&mut self) -> anyhow::Result<HashMap<ClientOrderId, Position>> {
        todo!()
    }

    fn load_index_order_client(&mut self) -> anyhow::Result<HashMap<ClientOrderId, ClientId>> {
        todo!()
    }

    fn load_currency(&mut self, code: &Ustr) -> anyhow::Result<Option<Currency>> {
        todo!()
    }

    fn load_instrument(
        &mut self,
        instrument_id: &InstrumentId,
    ) -> anyhow::Result<Option<InstrumentAny>> {
        todo!()
    }

    fn load_synthetic(
        &mut self,
        instrument_id: &InstrumentId,
    ) -> anyhow::Result<SyntheticInstrument> {
        todo!()
    }

    fn load_account(&mut self, account_id: &AccountId) -> anyhow::Result<Option<AccountAny>> {
        todo!()
    }

    fn load_order(&mut self, client_order_id: &ClientOrderId) -> anyhow::Result<Option<OrderAny>> {
        todo!()
    }

    fn load_position(&mut self, position_id: &PositionId) -> anyhow::Result<Position> {
        todo!()
    }

    fn load_actor(&mut self, component_id: &ComponentId) -> anyhow::Result<HashMap<String, Bytes>> {
        todo!()
    }

    fn delete_actor(&mut self, component_id: &ComponentId) -> anyhow::Result<()> {
        todo!()
    }

    fn load_strategy(
        &mut self,
        strategy_id: &StrategyId,
    ) -> anyhow::Result<HashMap<String, Bytes>> {
        todo!()
    }

    fn delete_strategy(&mut self, component_id: &StrategyId) -> anyhow::Result<()> {
        todo!()
    }

    fn add(&mut self, key: String, value: Bytes) -> anyhow::Result<()> {
        todo!()
    }

    fn add_currency(&mut self, currency: &Currency) -> anyhow::Result<()> {
        todo!()
    }

    fn add_instrument(&mut self, instrument: &InstrumentAny) -> anyhow::Result<()> {
        todo!()
    }

    fn add_synthetic(&mut self, synthetic: &SyntheticInstrument) -> anyhow::Result<()> {
        todo!()
    }

    fn add_account(&mut self, account: &AccountAny) -> anyhow::Result<()> {
        todo!()
    }

    fn add_order(&mut self, order: &OrderAny, client_id: Option<ClientId>) -> anyhow::Result<()> {
        todo!()
    }

    fn add_position(&mut self, position: &Position) -> anyhow::Result<()> {
        todo!()
    }

    fn add_order_book(&mut self, order_book: &OrderBook) -> anyhow::Result<()> {
        anyhow::bail!("Saving market data for Redis cache adapter not supported")
    }

    fn add_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        anyhow::bail!("Saving market data for Redis cache adapter not supported")
    }

    fn load_quotes(&mut self, instrument_id: &InstrumentId) -> anyhow::Result<Vec<QuoteTick>> {
        anyhow::bail!("Loading quote data for Redis cache adapter not supported")
    }

    fn add_trade(&mut self, trade: &TradeTick) -> anyhow::Result<()> {
        anyhow::bail!("Saving market data for Redis cache adapter not supported")
    }

    fn load_trades(&mut self, instrument_id: &InstrumentId) -> anyhow::Result<Vec<TradeTick>> {
        anyhow::bail!("Loading market data for Redis cache adapter not supported")
    }

    fn add_bar(&mut self, bar: &Bar) -> anyhow::Result<()> {
        anyhow::bail!("Saving market data for Redis cache adapter not supported")
    }

    fn load_bars(&mut self, instrument_id: &InstrumentId) -> anyhow::Result<Vec<Bar>> {
        anyhow::bail!("Loading market data for Redis cache adapter not supported")
    }

    fn index_venue_order_id(
        &mut self,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
    ) -> anyhow::Result<()> {
        todo!()
    }

    fn index_order_position(
        &mut self,
        client_order_id: ClientOrderId,
        position_id: PositionId,
    ) -> anyhow::Result<()> {
        todo!()
    }

    fn update_actor(&mut self) -> anyhow::Result<()> {
        todo!()
    }

    fn update_strategy(&mut self) -> anyhow::Result<()> {
        todo!()
    }

    fn update_account(&mut self, account: &AccountAny) -> anyhow::Result<()> {
        todo!()
    }

    fn update_order(&mut self, order: &OrderAny) -> anyhow::Result<()> {
        todo!()
    }

    fn update_position(&mut self, position: &Position) -> anyhow::Result<()> {
        todo!()
    }

    fn snapshot_order_state(&mut self, order: &OrderAny) -> anyhow::Result<()> {
        todo!()
    }

    fn snapshot_position_state(&mut self, position: &Position) -> anyhow::Result<()> {
        todo!()
    }

    fn heartbeat(&mut self, timestamp: UnixNanos) -> anyhow::Result<()> {
        todo!()
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_get_trader_key_with_prefix_and_instance_id() {
        let trader_id = TraderId::from("tester-123");
        let instance_id = UUID4::new();
        let mut config = CacheConfig::default();
        config.use_instance_id = true;

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
