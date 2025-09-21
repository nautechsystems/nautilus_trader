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

use std::{collections::VecDeque, fmt::Debug, time::Duration};

use ahash::AHashMap;
use bytes::Bytes;
use nautilus_common::{
    cache::{
        CacheConfig,
        database::{CacheDatabaseAdapter, CacheMap},
    },
    custom::CustomData,
    enums::SerializationEncoding,
    logging::{log_task_awaiting, log_task_started, log_task_stopped},
    runtime::get_runtime,
    signal::Signal,
};
use nautilus_core::{UUID4, UnixNanos, correctness::check_slice_not_empty};
use nautilus_cryptography::providers::install_cryptographic_provider;
use nautilus_model::{
    accounts::AccountAny,
    data::{Bar, DataType, QuoteTick, TradeTick},
    events::{OrderEventAny, OrderSnapshot, position::snapshot::PositionSnapshot},
    identifiers::{
        AccountId, ClientId, ClientOrderId, ComponentId, InstrumentId, PositionId, StrategyId,
        TraderId, VenueOrderId,
    },
    instruments::{InstrumentAny, SyntheticInstrument},
    orderbook::OrderBook,
    orders::OrderAny,
    position::Position,
    types::Currency,
};
use redis::{Pipeline, aio::ConnectionManager};
use tokio::try_join;
use ustr::Ustr;

use super::{REDIS_DELIMITER, REDIS_FLUSHDB};
use crate::redis::{create_redis_connection, queries::DatabaseQueries};

// Task and connection names
const CACHE_READ: &str = "cache-read";
const CACHE_WRITE: &str = "cache-write";
const CACHE_PROCESS: &str = "cache-process";

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
    pub const fn new(op_type: DatabaseOperation, key: String, payload: Option<Vec<Bytes>>) -> Self {
        Self {
            op_type,
            key: Some(key),
            payload,
        }
    }

    /// Initialize a `Close` database command, this is meant to close the database cache channel.
    #[must_use]
    pub const fn close() -> Self {
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
    pub con: ConnectionManager,
    pub trader_id: TraderId,
    pub trader_key: String,
    pub encoding: SerializationEncoding,
    tx: tokio::sync::mpsc::UnboundedSender<DatabaseCommand>,
    handle: tokio::task::JoinHandle<()>,
}

impl Debug for RedisCacheDatabase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(RedisCacheDatabase))
            .field("trader_id", &self.trader_id)
            .field("encoding", &self.encoding)
            .finish()
    }
}

impl RedisCacheDatabase {
    /// Creates a new [`RedisCacheDatabase`] instance for the given `trader_id`, `instance_id`, and `config`.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The database configuration is missing in `config`.
    /// - Establishing the Redis connection fails.
    /// - The command processing task cannot be spawned.
    pub async fn new(
        trader_id: TraderId,
        instance_id: UUID4,
        config: CacheConfig,
    ) -> anyhow::Result<Self> {
        install_cryptographic_provider();

        let db_config = config
            .database
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No database config"))?;
        let con = create_redis_connection(CACHE_READ, db_config.clone()).await?;

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<DatabaseCommand>();
        let trader_key = get_trader_key(trader_id, instance_id, &config);
        let trader_key_clone = trader_key.clone();
        let encoding = config.encoding;
        let handle = get_runtime().spawn(async move {
            if let Err(e) = process_commands(rx, trader_key_clone, config.clone()).await {
                log::error!("Error in task '{CACHE_PROCESS}': {e}");
            }
        });

        Ok(Self {
            con,
            trader_id,
            trader_key,
            encoding,
            tx,
            handle,
        })
    }

    #[must_use]
    pub const fn get_encoding(&self) -> SerializationEncoding {
        self.encoding
    }

    #[must_use]
    pub fn get_trader_key(&self) -> &str {
        &self.trader_key
    }

    pub fn close(&mut self) {
        log::debug!("Closing");

        if let Err(e) = self.tx.send(DatabaseCommand::close()) {
            log::debug!("Error sending close command: {e:?}");
        }

        log_task_awaiting(CACHE_PROCESS);

        tokio::task::block_in_place(|| {
            if let Err(e) = get_runtime().block_on(&mut self.handle) {
                log::error!("Error awaiting task '{CACHE_PROCESS}': {e:?}");
            }
        });

        log::debug!("Closed");
    }

    pub async fn flushdb(&mut self) {
        if let Err(e) = redis::cmd(REDIS_FLUSHDB)
            .query_async::<()>(&mut self.con)
            .await
        {
            log::error!("Failed to flush database: {e:?}");
        }
    }

    /// Retrieves all keys matching the given `pattern` from Redis for this trader.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying Redis scan operation fails.
    pub async fn keys(&mut self, pattern: &str) -> anyhow::Result<Vec<String>> {
        let pattern = format!("{}{REDIS_DELIMITER}{pattern}", self.trader_key);
        DatabaseQueries::scan_keys(&mut self.con, pattern).await
    }

    /// Reads the value(s) associated with `key` for this trader from Redis.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying Redis read operation fails.
    pub async fn read(&mut self, key: &str) -> anyhow::Result<Vec<Bytes>> {
        DatabaseQueries::read(&self.con, &self.trader_key, key).await
    }

    /// Reads multiple values using bulk operations for efficiency.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying Redis read operation fails.
    pub async fn read_bulk(&mut self, keys: &[String]) -> anyhow::Result<Vec<Option<Bytes>>> {
        DatabaseQueries::read_bulk(&self.con, keys).await
    }

    /// Sends an insert command for `key` with optional `payload` to Redis via the background task.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be sent to the background task channel.
    pub fn insert(&mut self, key: String, payload: Option<Vec<Bytes>>) -> anyhow::Result<()> {
        let op = DatabaseCommand::new(DatabaseOperation::Insert, key, payload);
        match self.tx.send(op) {
            Ok(()) => Ok(()),
            Err(e) => anyhow::bail!("{FAILED_TX_CHANNEL}: {e}"),
        }
    }

    /// Sends an update command for `key` with optional `payload` to Redis via the background task.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be sent to the background task channel.
    pub fn update(&mut self, key: String, payload: Option<Vec<Bytes>>) -> anyhow::Result<()> {
        let op = DatabaseCommand::new(DatabaseOperation::Update, key, payload);
        match self.tx.send(op) {
            Ok(()) => Ok(()),
            Err(e) => anyhow::bail!("{FAILED_TX_CHANNEL}: {e}"),
        }
    }

    /// Sends a delete command for `key` with optional `payload` to Redis via the background task.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be sent to the background task channel.
    pub fn delete(&mut self, key: String, payload: Option<Vec<Bytes>>) -> anyhow::Result<()> {
        let op = DatabaseCommand::new(DatabaseOperation::Delete, key, payload);
        match self.tx.send(op) {
            Ok(()) => Ok(()),
            Err(e) => anyhow::bail!("{FAILED_TX_CHANNEL}: {e}"),
        }
    }

    /// Delete the given order from the database with comprehensive index cleanup.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be sent to the background task channel.
    pub fn delete_order(&self, client_order_id: &ClientOrderId) -> anyhow::Result<()> {
        let order_id_bytes = Bytes::from(client_order_id.to_string());

        // Delete the order itself
        let key = format!("{ORDERS}{REDIS_DELIMITER}{client_order_id}");
        let op = DatabaseCommand::new(DatabaseOperation::Delete, key, None);
        self.tx
            .send(op)
            .map_err(|e| anyhow::anyhow!("Failed to send delete order command: {e}"))?;

        // Delete from all order indexes
        let index_keys = [
            INDEX_ORDER_IDS,
            INDEX_ORDERS,
            INDEX_ORDERS_OPEN,
            INDEX_ORDERS_CLOSED,
            INDEX_ORDERS_EMULATED,
            INDEX_ORDERS_INFLIGHT,
        ];

        for index_key in &index_keys {
            let key = (*index_key).to_string();
            let payload = vec![order_id_bytes.clone()];
            let op = DatabaseCommand::new(DatabaseOperation::Delete, key, Some(payload));
            self.tx
                .send(op)
                .map_err(|e| anyhow::anyhow!("Failed to send delete order index command: {e}"))?;
        }

        // Delete from hash indexes
        let hash_indexes = [INDEX_ORDER_POSITION, INDEX_ORDER_CLIENT];
        for index_key in &hash_indexes {
            let key = (*index_key).to_string();
            let payload = vec![order_id_bytes.clone()];
            let op = DatabaseCommand::new(DatabaseOperation::Delete, key, Some(payload));
            self.tx.send(op).map_err(|e| {
                anyhow::anyhow!("Failed to send delete order hash index command: {e}")
            })?;
        }

        Ok(())
    }

    /// Delete the given position from the database with comprehensive index cleanup.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be sent to the background task channel.
    pub fn delete_position(&self, position_id: &PositionId) -> anyhow::Result<()> {
        let position_id_bytes = Bytes::from(position_id.to_string());

        // Delete the position itself
        let key = format!("{POSITIONS}{REDIS_DELIMITER}{position_id}");
        let op = DatabaseCommand::new(DatabaseOperation::Delete, key, None);
        self.tx
            .send(op)
            .map_err(|e| anyhow::anyhow!("Failed to send delete position command: {e}"))?;

        // Delete from all position indexes
        let index_keys = [
            INDEX_POSITIONS,
            INDEX_POSITIONS_OPEN,
            INDEX_POSITIONS_CLOSED,
        ];

        for index_key in &index_keys {
            let key = (*index_key).to_string();
            let payload = vec![position_id_bytes.clone()];
            let op = DatabaseCommand::new(DatabaseOperation::Delete, key, Some(payload));
            self.tx.send(op).map_err(|e| {
                anyhow::anyhow!("Failed to send delete position index command: {e}")
            })?;
        }

        Ok(())
    }

    /// Delete the given account event from the database.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be sent to the background task channel.
    pub fn delete_account_event(
        &self,
        _account_id: &AccountId,
        _event_id: &str,
    ) -> anyhow::Result<()> {
        tracing::warn!("Deleting account events currently a no-op (pending redesign)");
        Ok(())
    }
}

async fn process_commands(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<DatabaseCommand>,
    trader_key: String,
    config: CacheConfig,
) -> anyhow::Result<()> {
    log_task_started(CACHE_PROCESS);

    let db_config = config
        .database
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("No database config"))?;
    let mut con = create_redis_connection(CACHE_WRITE, db_config.clone()).await?;

    // Buffering
    let mut buffer: VecDeque<DatabaseCommand> = VecDeque::new();
    let mut last_drain = std::time::Instant::now();
    let buffer_interval = Duration::from_millis(config.buffer_interval_ms.unwrap_or(0) as u64);

    // Continue to receive and handle messages until channel is hung up
    loop {
        if last_drain.elapsed() >= buffer_interval && !buffer.is_empty() {
            drain_buffer(&mut con, &trader_key, &mut buffer).await;
            last_drain = std::time::Instant::now();
        } else if let Some(cmd) = rx.recv().await {
            tracing::trace!("Received {cmd:?}");

            if matches!(cmd.op_type, DatabaseOperation::Close) {
                break;
            }
            buffer.push_back(cmd);
        } else {
            tracing::debug!("Command channel closed");
            break;
        }
    }

    // Drain any remaining messages
    if !buffer.is_empty() {
        drain_buffer(&mut con, &trader_key, &mut buffer).await;
    }

    log_task_stopped(CACHE_PROCESS);
    Ok(())
}

async fn drain_buffer(
    conn: &mut ConnectionManager,
    trader_key: &str,
    buffer: &mut VecDeque<DatabaseCommand>,
) {
    let mut pipe = redis::pipe();
    pipe.atomic();

    for msg in buffer.drain(..) {
        let key = if let Some(key) = msg.key {
            key
        } else {
            log::error!("Null key found for message: {msg:?}");
            continue;
        };
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
                    log::debug!("Processing INSERT for collection: {collection}, key: {key}");
                    if let Err(e) = insert(&mut pipe, collection, &key, payload) {
                        tracing::error!("{e}");
                    }
                } else {
                    tracing::error!("Null `payload` for `insert`");
                }
            }
            DatabaseOperation::Update => {
                if let Some(payload) = msg.payload {
                    log::debug!("Processing UPDATE for collection: {collection}, key: {key}");
                    if let Err(e) = update(&mut pipe, collection, &key, payload) {
                        tracing::error!("{e}");
                    }
                } else {
                    tracing::error!("Null `payload` for `update`");
                }
            }
            DatabaseOperation::Delete => {
                tracing::debug!(
                    "Processing DELETE for collection: {}, key: {}, payload: {:?}",
                    collection,
                    key,
                    msg.payload.as_ref().map(std::vec::Vec::len)
                );
                // `payload` can be `None` for a delete operation
                if let Err(e) = delete(&mut pipe, collection, &key, msg.payload) {
                    tracing::error!("{e}");
                }
            }
            DatabaseOperation::Close => panic!("Close command should not be drained"),
        }
    }

    if let Err(e) = pipe.query_async::<()>(conn).await {
        tracing::error!("{e}");
    }
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
    tracing::debug!(
        "delete: collection={}, key={}, has_payload={}",
        collection,
        key,
        value.is_some()
    );

    match collection {
        INDEX => delete_from_index(pipe, key, value),
        ORDERS => {
            delete_string(pipe, key);
            Ok(())
        }
        POSITIONS => {
            delete_string(pipe, key);
            Ok(())
        }
        ACCOUNTS => {
            delete_string(pipe, key);
            Ok(())
        }
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

fn delete_from_index(
    pipe: &mut Pipeline,
    key: &str,
    value: Option<Vec<Bytes>>,
) -> anyhow::Result<()> {
    let value = value.ok_or_else(|| anyhow::anyhow!("Empty `payload` for `delete` '{key}'"))?;
    let index_key = get_index_key(key)?;

    match index_key {
        INDEX_ORDER_IDS => {
            remove_from_set(pipe, key, value[0].as_ref());
            Ok(())
        }
        INDEX_ORDER_POSITION => {
            remove_from_hash(pipe, key, value[0].as_ref());
            Ok(())
        }
        INDEX_ORDER_CLIENT => {
            remove_from_hash(pipe, key, value[0].as_ref());
            Ok(())
        }
        INDEX_ORDERS => {
            remove_from_set(pipe, key, value[0].as_ref());
            Ok(())
        }
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
        INDEX_POSITIONS => {
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

fn remove_from_hash(pipe: &mut Pipeline, key: &str, field: &[u8]) {
    pipe.hdel(key, field);
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

#[allow(dead_code, reason = "Under development")]
#[derive(Debug)]
pub struct RedisCacheDatabaseAdapter {
    pub encoding: SerializationEncoding,
    pub database: RedisCacheDatabase,
}

#[allow(dead_code, reason = "Under development")]
#[allow(unused, reason = "Under development")]
#[async_trait::async_trait]
impl CacheDatabaseAdapter for RedisCacheDatabaseAdapter {
    fn close(&mut self) -> anyhow::Result<()> {
        self.database.close();
        Ok(())
    }

    fn flush(&mut self) -> anyhow::Result<()> {
        self.database.flushdb();
        Ok(())
    }

    async fn load_all(&self) -> anyhow::Result<CacheMap> {
        tracing::debug!("Loading all data");

        let (
            currencies,
            instruments,
            synthetics,
            accounts,
            orders,
            positions,
            greeks,
            yield_curves,
        ) = try_join!(
            self.load_currencies(),
            self.load_instruments(),
            self.load_synthetics(),
            self.load_accounts(),
            self.load_orders(),
            self.load_positions(),
            self.load_greeks(),
            self.load_yield_curves()
        )
        .map_err(|e| anyhow::anyhow!("Error loading cache data: {e}"))?;

        Ok(CacheMap {
            currencies,
            instruments,
            synthetics,
            accounts,
            orders,
            positions,
            greeks,
            yield_curves,
        })
    }

    fn load(&self) -> anyhow::Result<AHashMap<String, Bytes>> {
        // self.database.load()
        Ok(AHashMap::new()) // TODO
    }

    async fn load_currencies(&self) -> anyhow::Result<AHashMap<Ustr, Currency>> {
        DatabaseQueries::load_currencies(
            &self.database.con,
            &self.database.trader_key,
            self.encoding,
        )
        .await
    }

    async fn load_instruments(&self) -> anyhow::Result<AHashMap<InstrumentId, InstrumentAny>> {
        DatabaseQueries::load_instruments(
            &self.database.con,
            &self.database.trader_key,
            self.encoding,
        )
        .await
    }

    async fn load_synthetics(&self) -> anyhow::Result<AHashMap<InstrumentId, SyntheticInstrument>> {
        DatabaseQueries::load_synthetics(
            &self.database.con,
            &self.database.trader_key,
            self.encoding,
        )
        .await
    }

    async fn load_accounts(&self) -> anyhow::Result<AHashMap<AccountId, AccountAny>> {
        DatabaseQueries::load_accounts(&self.database.con, &self.database.trader_key, self.encoding)
            .await
    }

    async fn load_orders(&self) -> anyhow::Result<AHashMap<ClientOrderId, OrderAny>> {
        DatabaseQueries::load_orders(&self.database.con, &self.database.trader_key, self.encoding)
            .await
    }

    async fn load_positions(&self) -> anyhow::Result<AHashMap<PositionId, Position>> {
        DatabaseQueries::load_positions(
            &self.database.con,
            &self.database.trader_key,
            self.encoding,
        )
        .await
    }

    fn load_index_order_position(&self) -> anyhow::Result<AHashMap<ClientOrderId, Position>> {
        todo!()
    }

    fn load_index_order_client(&self) -> anyhow::Result<AHashMap<ClientOrderId, ClientId>> {
        todo!()
    }

    async fn load_currency(&self, code: &Ustr) -> anyhow::Result<Option<Currency>> {
        DatabaseQueries::load_currency(
            &self.database.con,
            &self.database.trader_key,
            code,
            self.encoding,
        )
        .await
    }

    async fn load_instrument(
        &self,
        instrument_id: &InstrumentId,
    ) -> anyhow::Result<Option<InstrumentAny>> {
        DatabaseQueries::load_instrument(
            &self.database.con,
            &self.database.trader_key,
            instrument_id,
            self.encoding,
        )
        .await
    }

    async fn load_synthetic(
        &self,
        instrument_id: &InstrumentId,
    ) -> anyhow::Result<Option<SyntheticInstrument>> {
        DatabaseQueries::load_synthetic(
            &self.database.con,
            &self.database.trader_key,
            instrument_id,
            self.encoding,
        )
        .await
    }

    async fn load_account(&self, account_id: &AccountId) -> anyhow::Result<Option<AccountAny>> {
        DatabaseQueries::load_account(
            &self.database.con,
            &self.database.trader_key,
            account_id,
            self.encoding,
        )
        .await
    }

    async fn load_order(
        &self,
        client_order_id: &ClientOrderId,
    ) -> anyhow::Result<Option<OrderAny>> {
        DatabaseQueries::load_order(
            &self.database.con,
            &self.database.trader_key,
            client_order_id,
            self.encoding,
        )
        .await
    }

    async fn load_position(&self, position_id: &PositionId) -> anyhow::Result<Option<Position>> {
        DatabaseQueries::load_position(
            &self.database.con,
            &self.database.trader_key,
            position_id,
            self.encoding,
        )
        .await
    }

    fn load_actor(&self, component_id: &ComponentId) -> anyhow::Result<AHashMap<String, Bytes>> {
        todo!()
    }

    fn delete_actor(&self, component_id: &ComponentId) -> anyhow::Result<()> {
        todo!()
    }

    fn load_strategy(&self, strategy_id: &StrategyId) -> anyhow::Result<AHashMap<String, Bytes>> {
        todo!()
    }

    fn delete_strategy(&self, component_id: &StrategyId) -> anyhow::Result<()> {
        todo!()
    }

    fn delete_order(&self, client_order_id: &ClientOrderId) -> anyhow::Result<()> {
        let order_id_bytes = Bytes::from(client_order_id.to_string());

        log::debug!("Deleting order: {client_order_id} from Redis");
        log::debug!("Trader key: {}", self.database.trader_key);

        // Delete the order itself
        let key = format!("{ORDERS}{REDIS_DELIMITER}{client_order_id}");
        log::debug!("Deleting order key: {key}");
        let op = DatabaseCommand::new(DatabaseOperation::Delete, key, None);
        self.database
            .tx
            .send(op)
            .map_err(|e| anyhow::anyhow!("Failed to send delete order command: {e}"))?;

        // Delete from all order indexes
        let index_keys = [
            INDEX_ORDER_IDS,
            INDEX_ORDERS,
            INDEX_ORDERS_OPEN,
            INDEX_ORDERS_CLOSED,
            INDEX_ORDERS_EMULATED,
            INDEX_ORDERS_INFLIGHT,
        ];

        for index_key in &index_keys {
            let key = (*index_key).to_string();
            log::debug!("Deleting from index: {key} (order_id: {client_order_id})");
            let payload = vec![order_id_bytes.clone()];
            let op = DatabaseCommand::new(DatabaseOperation::Delete, key, Some(payload));
            self.database
                .tx
                .send(op)
                .map_err(|e| anyhow::anyhow!("Failed to send delete order index command: {e}"))?;
        }

        // Delete from hash indexes
        let hash_indexes = [INDEX_ORDER_POSITION, INDEX_ORDER_CLIENT];
        for index_key in &hash_indexes {
            let key = (*index_key).to_string();
            log::debug!("Deleting from hash index: {key} (order_id: {client_order_id})");
            let payload = vec![order_id_bytes.clone()];
            let op = DatabaseCommand::new(DatabaseOperation::Delete, key, Some(payload));
            self.database.tx.send(op).map_err(|e| {
                anyhow::anyhow!("Failed to send delete order hash index command: {e}")
            })?;
        }

        log::debug!("Sent all delete commands for order: {client_order_id}");
        Ok(())
    }

    fn delete_position(&self, position_id: &PositionId) -> anyhow::Result<()> {
        let position_id_bytes = Bytes::from(position_id.to_string());

        // Delete the position itself
        let key = format!("{POSITIONS}{REDIS_DELIMITER}{position_id}");
        let op = DatabaseCommand::new(DatabaseOperation::Delete, key, None);
        self.database
            .tx
            .send(op)
            .map_err(|e| anyhow::anyhow!("Failed to send delete position command: {e}"))?;

        // Delete from all position indexes
        let index_keys = [
            INDEX_POSITIONS,
            INDEX_POSITIONS_OPEN,
            INDEX_POSITIONS_CLOSED,
        ];

        for index_key in &index_keys {
            let key = (*index_key).to_string();
            let payload = vec![position_id_bytes.clone()];
            let op = DatabaseCommand::new(DatabaseOperation::Delete, key, Some(payload));
            self.database.tx.send(op).map_err(|e| {
                anyhow::anyhow!("Failed to send delete position index command: {e}")
            })?;
        }

        Ok(())
    }

    fn delete_account_event(&self, account_id: &AccountId, event_id: &str) -> anyhow::Result<()> {
        todo!()
    }

    fn add(&self, key: String, value: Bytes) -> anyhow::Result<()> {
        todo!()
    }

    fn add_currency(&self, currency: &Currency) -> anyhow::Result<()> {
        todo!()
    }

    fn add_instrument(&self, instrument: &InstrumentAny) -> anyhow::Result<()> {
        todo!()
    }

    fn add_synthetic(&self, synthetic: &SyntheticInstrument) -> anyhow::Result<()> {
        todo!()
    }

    fn add_account(&self, account: &AccountAny) -> anyhow::Result<()> {
        todo!()
    }

    fn add_order(&self, order: &OrderAny, client_id: Option<ClientId>) -> anyhow::Result<()> {
        todo!()
    }

    fn add_order_snapshot(&self, snapshot: &OrderSnapshot) -> anyhow::Result<()> {
        todo!()
    }

    fn add_position(&self, position: &Position) -> anyhow::Result<()> {
        todo!()
    }

    fn add_position_snapshot(&self, snapshot: &PositionSnapshot) -> anyhow::Result<()> {
        todo!()
    }

    fn add_order_book(&self, order_book: &OrderBook) -> anyhow::Result<()> {
        anyhow::bail!("Saving market data for Redis cache adapter not supported")
    }

    fn add_quote(&self, quote: &QuoteTick) -> anyhow::Result<()> {
        anyhow::bail!("Saving market data for Redis cache adapter not supported")
    }

    fn load_quotes(&self, instrument_id: &InstrumentId) -> anyhow::Result<Vec<QuoteTick>> {
        anyhow::bail!("Loading quote data for Redis cache adapter not supported")
    }

    fn add_trade(&self, trade: &TradeTick) -> anyhow::Result<()> {
        anyhow::bail!("Saving market data for Redis cache adapter not supported")
    }

    fn load_trades(&self, instrument_id: &InstrumentId) -> anyhow::Result<Vec<TradeTick>> {
        anyhow::bail!("Loading market data for Redis cache adapter not supported")
    }

    fn add_bar(&self, bar: &Bar) -> anyhow::Result<()> {
        anyhow::bail!("Saving market data for Redis cache adapter not supported")
    }

    fn load_bars(&self, instrument_id: &InstrumentId) -> anyhow::Result<Vec<Bar>> {
        anyhow::bail!("Loading market data for Redis cache adapter not supported")
    }

    fn add_signal(&self, signal: &Signal) -> anyhow::Result<()> {
        anyhow::bail!("Saving signals for Redis cache adapter not supported")
    }

    fn load_signals(&self, name: &str) -> anyhow::Result<Vec<Signal>> {
        anyhow::bail!("Loading signals from Redis cache adapter not supported")
    }

    fn add_custom_data(&self, data: &CustomData) -> anyhow::Result<()> {
        anyhow::bail!("Saving custom data for Redis cache adapter not supported")
    }

    fn load_custom_data(&self, data_type: &DataType) -> anyhow::Result<Vec<CustomData>> {
        anyhow::bail!("Loading custom data from Redis cache adapter not supported")
    }

    fn load_order_snapshot(
        &self,
        client_order_id: &ClientOrderId,
    ) -> anyhow::Result<Option<OrderSnapshot>> {
        anyhow::bail!("Loading order snapshots from Redis cache adapter not supported")
    }

    fn load_position_snapshot(
        &self,
        position_id: &PositionId,
    ) -> anyhow::Result<Option<PositionSnapshot>> {
        anyhow::bail!("Loading position snapshots from Redis cache adapter not supported")
    }

    fn index_venue_order_id(
        &self,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
    ) -> anyhow::Result<()> {
        todo!()
    }

    fn index_order_position(
        &self,
        client_order_id: ClientOrderId,
        position_id: PositionId,
    ) -> anyhow::Result<()> {
        todo!()
    }

    fn update_actor(&self) -> anyhow::Result<()> {
        todo!()
    }

    fn update_strategy(&self) -> anyhow::Result<()> {
        todo!()
    }

    fn update_account(&self, account: &AccountAny) -> anyhow::Result<()> {
        todo!()
    }

    fn update_order(&self, order_event: &OrderEventAny) -> anyhow::Result<()> {
        todo!()
    }

    fn update_position(&self, position: &Position) -> anyhow::Result<()> {
        todo!()
    }

    fn snapshot_order_state(&self, order: &OrderAny) -> anyhow::Result<()> {
        todo!()
    }

    fn snapshot_position_state(&self, position: &Position) -> anyhow::Result<()> {
        todo!()
    }

    fn heartbeat(&self, timestamp: UnixNanos) -> anyhow::Result<()> {
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
        let config = CacheConfig {
            use_instance_id: true,
            ..Default::default()
        };

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
