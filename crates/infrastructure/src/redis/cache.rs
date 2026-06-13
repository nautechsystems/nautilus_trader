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

//! Redis-backed cache database for the system.
//!
//! # Architecture
//!
//! Uses two Redis connections with distinct roles:
//! - **READ** (`self.con`): synchronous queries (`keys`, `read`, `load_all`),
//!   owned by the main struct.
//! - **WRITE**: owned by a background task on `get_runtime()`, receives
//!   commands via an unbounded `tokio::sync::mpsc` channel.
//!
//! All write operations (`insert`, `update`, `delete`, `flush`) are routed
//! through the command channel so they execute on the WRITE connection. This
//! avoids cross-runtime I/O issues since the WRITE connection is always
//! created on the Nautilus runtime.
//!
//! Synchronous callers (`close`, `flushdb_sync`) use `std::sync::mpsc` reply
//! channels to block until the background task confirms completion. When
//! called from the Nautilus runtime itself, `block_in_place` is used
//! automatically to avoid stalling the worker thread.

use std::{
    collections::VecDeque,
    fmt::{Debug, Write as _},
    ops::ControlFlow,
    pin::Pin,
    sync::mpsc::{self, SyncSender},
    time::Duration,
};

use ahash::AHashMap;
use anyhow::Context;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use nautilus_common::{
    cache::{
        CacheConfig,
        database::{CacheDatabaseAdapter, CacheMap},
    },
    enums::SerializationEncoding,
    live::get_runtime,
    logging::{log_task_awaiting, log_task_started, log_task_stopped},
    signal::Signal,
};
use nautilus_core::{UUID4, UnixNanos, correctness::check_slice_not_empty};
use nautilus_cryptography::providers::install_cryptographic_provider;
use nautilus_model::{
    accounts::AccountAny,
    data::{Bar, CustomData, DataType, FundingRateUpdate, HasTsInit, QuoteTick, TradeTick},
    enums::TriggerType,
    events::{
        AccountState, OrderEventAny, OrderFilled, OrderSnapshot,
        position::snapshot::PositionSnapshot,
    },
    identifiers::{
        AccountId, ClientId, ClientOrderId, ComponentId, InstrumentId, PositionId, StrategyId,
        TraderId, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny, SyntheticInstrument},
    orderbook::OrderBook,
    orders::{Order, OrderAny},
    position::Position,
    types::{Currency, Money},
};
use redis::{AsyncCommands, Pipeline, aio::ConnectionManager};
use ustr::Ustr;

use super::{REDIS_DELIMITER, REDIS_FLUSHDB, get_index_key};
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
const CUSTOM: &str = "custom";

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
    UpdateOrder,
    ReplaceList,
    Delete,
    Flush(SyncSender<()>),
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
    pub bulk_read_batch_size: Option<usize>,
    tx: tokio::sync::mpsc::UnboundedSender<DatabaseCommand>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl Debug for RedisCacheDatabase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(RedisCacheDatabase))
            .field("trader_id", &self.trader_id)
            .field("encoding", &self.encoding)
            .finish_non_exhaustive()
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
        let bulk_read_batch_size = config.bulk_read_batch_size;

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
            bulk_read_batch_size,
            tx,
            handle: Some(handle),
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

        let Some(handle) = self.handle.take() else {
            log::debug!("Already closed");
            return;
        };

        if let Err(e) = self.tx.send(DatabaseCommand::close()) {
            log::debug!("Error sending close command: {e:?}");
        }

        log_task_awaiting(CACHE_PROCESS);

        let (tx, rx) = mpsc::sync_channel(1);

        get_runtime().spawn(async move {
            if let Err(e) = handle.await {
                log::error!("Error awaiting task '{CACHE_PROCESS}': {e:?}");
            }
            let _ = tx.send(());
        });
        let _ = blocking_recv(&rx);

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

    /// Sends a flush command through the background task channel and blocks
    /// until it completes. Safe to call from any runtime context.
    ///
    /// # Errors
    ///
    /// Returns an error if the command channel is closed or the reply is lost.
    pub fn flushdb_sync(&self) -> anyhow::Result<()> {
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        let cmd = DatabaseCommand {
            op_type: DatabaseOperation::Flush(reply_tx),
            key: None,
            payload: None,
        };
        self.tx
            .send(cmd)
            .map_err(|e| anyhow::anyhow!("{FAILED_TX_CHANNEL}: {e}"))?;
        blocking_recv(&reply_rx).map_err(|e| anyhow::anyhow!("Failed to flush database: {e}"))?;
        Ok(())
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
        match self.bulk_read_batch_size {
            Some(batch_size) => {
                DatabaseQueries::read_bulk_batched(&self.con, keys, batch_size).await
            }
            None => DatabaseQueries::read_bulk(&self.con, keys).await,
        }
    }

    /// Loads custom data from Redis matching the given `data_type` (blocking).
    ///
    /// Spawns the async query on the global Nautilus runtime and blocks until
    /// the result arrives via a channel. Safe from any thread context (Python,
    /// test runtimes, plain threads).
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails or the reply channel is closed.
    pub fn load_custom_data(&self, data_type: &DataType) -> anyhow::Result<Vec<CustomData>> {
        let con = self.con.clone();
        let trader_key = self.trader_key.clone();
        let data_type = data_type.clone();
        let (tx, rx) = mpsc::channel();

        get_runtime().spawn(async move {
            let result = DatabaseQueries::load_custom_data(&con, &trader_key, &data_type).await;
            if let Err(e) = tx.send(result) {
                log::error!("Failed to send custom data result for '{data_type}': {e:?}");
            }
        });

        blocking_recv(&rx).map_err(|e| anyhow::anyhow!("load_custom_data channel closed: {e}"))?
    }

    /// Sends an insert command for `key` with optional `payload` to Redis via the background task.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be sent to the background task channel.
    pub fn insert(&self, key: String, payload: Option<Vec<Bytes>>) -> anyhow::Result<()> {
        let op = DatabaseCommand::new(DatabaseOperation::Insert, key, payload);
        match self.tx.send(op) {
            Ok(()) => Ok(()),
            Err(e) => anyhow::bail!("{FAILED_TX_CHANNEL}: {e}"),
        }
    }

    /// Stores custom data in Redis (key format: `custom:<ts_init_020>:<uuid>`, value: full JSON).
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails or the insert command cannot be sent.
    pub fn add_custom_data(&self, data: &CustomData) -> anyhow::Result<()> {
        let json_bytes = serde_json::to_vec(data)
            .map_err(|e| anyhow::anyhow!("CustomData serialization failed: {e}"))?;
        let ts_init = data.ts_init().as_u64();
        let key = format!(
            "{CUSTOM}{REDIS_DELIMITER}{:020}{REDIS_DELIMITER}{}",
            ts_init,
            UUID4::new()
        );
        self.insert(key, Some(vec![Bytes::from(json_bytes)]))
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

    /// Delete the given order from the database with full index cleanup.
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

    /// Delete the given position from the database with full index cleanup.
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
        log::warn!("Deleting account events currently a no-op (pending redesign)");
        Ok(())
    }
}

fn blocking_recv<T>(rx: &mpsc::Receiver<T>) -> Result<T, mpsc::RecvError> {
    let on_nautilus_runtime = tokio::runtime::Handle::try_current()
        .ok()
        .is_some_and(|h| h.id() == get_runtime().handle().id());

    if on_nautilus_runtime {
        tokio::task::block_in_place(|| rx.recv())
    } else {
        rx.recv()
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
    let buffer_interval = Duration::from_millis(config.buffer_interval_ms.unwrap_or(0) as u64);

    // A sleep used to trigger periodic flushing of the buffer.
    // When `buffer_interval` is zero we skip using the timer and flush immediately
    // after every message.
    let flush_timer = tokio::time::sleep(buffer_interval);
    tokio::pin!(flush_timer);

    // Continue to receive and handle messages until channel is hung up
    loop {
        tokio::select! {
            maybe_cmd = rx.recv() => {
                let result = handle_command(
                    maybe_cmd,
                    &mut buffer,
                    buffer_interval,
                    &mut con,
                    &trader_key,
                    config.encoding,
                ).await;

                if result.is_break() {
                    break;
                }
            }
            () = &mut flush_timer, if !buffer_interval.is_zero() => {
                flush_buffer(
                    &mut buffer,
                    &mut con,
                    &trader_key,
                    config.encoding,
                    &mut flush_timer,
                    buffer_interval,
                ).await;
            }
        }
    }

    // Drain any remaining messages
    if !buffer.is_empty() {
        drain_buffer(&mut con, &trader_key, config.encoding, &mut buffer).await;
    }

    log_task_stopped(CACHE_PROCESS);
    Ok(())
}

async fn handle_command(
    maybe_cmd: Option<DatabaseCommand>,
    buffer: &mut VecDeque<DatabaseCommand>,
    buffer_interval: Duration,
    con: &mut ConnectionManager,
    trader_key: &str,
    encoding: SerializationEncoding,
) -> ControlFlow<()> {
    let Some(cmd) = maybe_cmd else {
        log::debug!("Command channel closed");
        return ControlFlow::Break(());
    };

    log::trace!("Received {cmd:?}");

    match cmd.op_type {
        DatabaseOperation::Close => {
            if !buffer.is_empty() {
                drain_buffer(con, trader_key, encoding, buffer).await;
            }
            return ControlFlow::Break(());
        }
        DatabaseOperation::Flush(reply_tx) => {
            if !buffer.is_empty() {
                drain_buffer(con, trader_key, encoding, buffer).await;
            }

            if let Err(e) = redis::cmd(REDIS_FLUSHDB).query_async::<()>(con).await {
                log::error!("Failed to flush database: {e:?}");
            }
            let _ = reply_tx.send(());
            return ControlFlow::Continue(());
        }
        _ => {}
    }

    buffer.push_back(cmd);

    if buffer_interval.is_zero() {
        drain_buffer(con, trader_key, encoding, buffer).await;
    }

    ControlFlow::Continue(())
}

async fn flush_buffer(
    buffer: &mut VecDeque<DatabaseCommand>,
    con: &mut ConnectionManager,
    trader_key: &str,
    encoding: SerializationEncoding,
    flush_timer: &mut Pin<&mut tokio::time::Sleep>,
    buffer_interval: Duration,
) {
    if !buffer.is_empty() {
        drain_buffer(con, trader_key, encoding, buffer).await;
    }
    flush_timer
        .as_mut()
        .reset(tokio::time::Instant::now() + buffer_interval);
}

async fn drain_buffer(
    conn: &mut ConnectionManager,
    trader_key: &str,
    encoding: SerializationEncoding,
    buffer: &mut VecDeque<DatabaseCommand>,
) {
    let mut pipe = redis::pipe();
    pipe.atomic();
    let mut has_pending_ops = false;

    for msg in buffer.drain(..) {
        let Some(key) = msg.key else {
            log::error!("Null key found for message: {msg:?}");
            continue;
        };
        let collection = match get_collection_key(&key) {
            Ok(collection) => collection,
            Err(e) => {
                log::error!("{e}");
                continue; // Continue to next message
            }
        };

        let key = format!("{trader_key}{REDIS_DELIMITER}{key}");

        match msg.op_type {
            DatabaseOperation::Insert => {
                if let Some(payload) = msg.payload {
                    log::debug!("Processing INSERT for collection: {collection}, key: {key}");
                    if let Err(e) = insert(&mut pipe, collection, &key, &payload) {
                        log::error!("{e}");
                    } else {
                        has_pending_ops = true;
                    }
                } else {
                    log::error!("Null `payload` for `insert`");
                }
            }
            DatabaseOperation::Update => {
                if let Some(payload) = msg.payload {
                    log::debug!("Processing UPDATE for collection: {collection}, key: {key}");
                    if let Err(e) = update(&mut pipe, collection, &key, &payload) {
                        log::error!("{e}");
                    } else {
                        has_pending_ops = true;
                    }
                } else {
                    log::error!("Null `payload` for `update`");
                }
            }
            DatabaseOperation::UpdateOrder => {
                flush_pending_pipeline(conn, &mut pipe, &mut has_pending_ops).await;

                if let Some(payload) = msg.payload {
                    log::debug!("Processing UPDATE_ORDER for key: {key}");
                    if let Err(e) =
                        update_order_event_log(conn, trader_key, encoding, &key, &payload).await
                    {
                        log::error!("{e}");
                    }
                } else {
                    log::error!("Null `payload` for `update_order`");
                }
            }
            DatabaseOperation::ReplaceList => {
                if let Some(payload) = msg.payload {
                    log::debug!("Processing REPLACE_LIST for key: {key}");
                    if let Err(e) = replace_list_operation(&mut pipe, collection, &key, &payload) {
                        log::error!("{e}");
                    } else {
                        has_pending_ops = true;
                    }
                } else {
                    log::error!("Null `payload` for `replace_list`");
                }
            }
            DatabaseOperation::Delete => {
                log::debug!(
                    "Processing DELETE for collection: {}, key: {}, payload: {:?}",
                    collection,
                    key,
                    msg.payload.as_ref().map(std::vec::Vec::len)
                );
                // `payload` can be `None` for a delete operation
                if let Err(e) = delete(&mut pipe, collection, &key, msg.payload) {
                    log::error!("{e}");
                } else {
                    has_pending_ops = true;
                }
            }
            DatabaseOperation::Close => panic!("Close command should not be drained"),
            DatabaseOperation::Flush(_) => panic!("Flush command should not be drained"),
        }
    }

    flush_pending_pipeline(conn, &mut pipe, &mut has_pending_ops).await;
}

async fn flush_pending_pipeline(
    conn: &mut ConnectionManager,
    pipe: &mut Pipeline,
    has_pending_ops: &mut bool,
) {
    if !*has_pending_ops {
        return;
    }

    if let Err(e) = pipe.query_async::<()>(conn).await {
        log::error!("{e}");
    }

    *pipe = redis::pipe();
    pipe.atomic();
    *has_pending_ops = false;
}

async fn update_order_event_log(
    conn: &mut ConnectionManager,
    trader_key: &str,
    encoding: SerializationEncoding,
    key: &str,
    value: &[Bytes],
) -> anyhow::Result<()> {
    check_slice_not_empty(value, stringify!(value))?;

    let result: Vec<Bytes> = conn.lrange(key, 0, -1).await?;
    if result.is_empty() {
        log::warn!("Cannot update order in Redis, no existing state at {key}");
        return Ok(());
    }

    let mut append_pipe = redis::pipe();
    append_pipe.atomic();
    update_list(&mut append_pipe, key, value[0].as_ref());
    append_pipe.query_async::<()>(conn).await?;

    let mut events: Vec<OrderEventAny> = result
        .iter()
        .map(|payload| DatabaseQueries::deserialize_payload(encoding, payload))
        .collect::<anyhow::Result<_>>()
        .with_context(|| {
            format!(
                "Order event append succeeded for {key}, but index replay failed decoding history"
            )
        })?;
    let event: OrderEventAny = DatabaseQueries::deserialize_payload(encoding, value[0].as_ref())
        .with_context(|| {
            format!(
                "Order event append succeeded for {key}, but index replay failed decoding appended event"
            )
        })?;
    events.push(event);
    let order = OrderAny::from_events(events).with_context(|| {
        format!("Order event append succeeded for {key}, but index replay failed rebuilding order")
    })?;

    let mut pipe = redis::pipe();
    pipe.atomic();
    update_order_indexes(&mut pipe, trader_key, &order);
    pipe.query_async::<()>(conn).await?;

    Ok(())
}

fn insert(pipe: &mut Pipeline, collection: &str, key: &str, value: &[Bytes]) -> anyhow::Result<()> {
    check_slice_not_empty(value, stringify!(value))?;

    match collection {
        INDEX => insert_index(pipe, key, value),
        GENERAL | CURRENCIES | INSTRUMENTS | SYNTHETICS | ACTORS | STRATEGIES | HEALTH | CUSTOM => {
            insert_string(pipe, key, value[0].as_ref());
            Ok(())
        }
        ACCOUNTS | ORDERS | POSITIONS | SNAPSHOTS => {
            insert_list(pipe, key, value[0].as_ref());
            Ok(())
        }
        _ => anyhow::bail!("Unsupported operation: `insert` for collection '{collection}'"),
    }
}

fn insert_index(pipe: &mut Pipeline, key: &str, value: &[Bytes]) -> anyhow::Result<()> {
    let index_key = get_index_key(key)?;
    match index_key {
        INDEX_ORDER_IDS
        | INDEX_ORDERS
        | INDEX_ORDERS_OPEN
        | INDEX_ORDERS_CLOSED
        | INDEX_ORDERS_EMULATED
        | INDEX_ORDERS_INFLIGHT
        | INDEX_POSITIONS
        | INDEX_POSITIONS_OPEN
        | INDEX_POSITIONS_CLOSED => {
            insert_set(pipe, key, value[0].as_ref());
            Ok(())
        }
        INDEX_ORDER_POSITION | INDEX_ORDER_CLIENT => {
            insert_hset(pipe, key, value[0].as_ref(), value[1].as_ref());
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

fn replace_list(pipe: &mut Pipeline, key: &str, value: &[u8]) {
    pipe.del(key);
    pipe.rpush(key, value);
}

fn replace_list_operation(
    pipe: &mut Pipeline,
    collection: &str,
    key: &str,
    value: &[Bytes],
) -> anyhow::Result<()> {
    check_slice_not_empty(value, stringify!(value))?;

    match collection {
        ACCOUNTS | ORDERS | POSITIONS => {
            replace_list(pipe, key, value[0].as_ref());
            Ok(())
        }
        _ => anyhow::bail!("Unsupported operation: `replace_list` for collection '{collection}'"),
    }
}

fn update(pipe: &mut Pipeline, collection: &str, key: &str, value: &[Bytes]) -> anyhow::Result<()> {
    check_slice_not_empty(value, stringify!(value))?;

    match collection {
        ACCOUNTS | ORDERS | POSITIONS => {
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
    log::debug!(
        "delete: collection={}, key={}, has_payload={}",
        collection,
        key,
        value.is_some()
    );

    match collection {
        INDEX => delete_from_index(pipe, key, value),
        ORDERS | POSITIONS | ACCOUNTS | ACTORS | STRATEGIES => {
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
        INDEX_ORDER_IDS
        | INDEX_ORDERS
        | INDEX_ORDERS_OPEN
        | INDEX_ORDERS_CLOSED
        | INDEX_ORDERS_EMULATED
        | INDEX_ORDERS_INFLIGHT
        | INDEX_POSITIONS
        | INDEX_POSITIONS_OPEN
        | INDEX_POSITIONS_CLOSED => {
            remove_from_set(pipe, key, value[0].as_ref());
            Ok(())
        }
        INDEX_ORDER_POSITION | INDEX_ORDER_CLIENT => {
            remove_from_hash(pipe, key, value[0].as_ref());
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

fn full_redis_key(trader_key: &str, key: &str) -> String {
    format!("{trader_key}{REDIS_DELIMITER}{key}")
}

fn update_order_indexes(pipe: &mut Pipeline, trader_key: &str, order: &OrderAny) {
    let client_order_id = order.client_order_id();
    let order_id_bytes = client_order_id.to_string();

    insert_set(
        pipe,
        &full_redis_key(trader_key, INDEX_ORDERS),
        order_id_bytes.as_bytes(),
    );

    if order.venue_order_id().is_some() {
        insert_set(
            pipe,
            &full_redis_key(trader_key, INDEX_ORDER_IDS),
            order_id_bytes.as_bytes(),
        );
    }

    if order.is_inflight() {
        insert_set(
            pipe,
            &full_redis_key(trader_key, INDEX_ORDERS_INFLIGHT),
            order_id_bytes.as_bytes(),
        );
    } else {
        remove_from_set(
            pipe,
            &full_redis_key(trader_key, INDEX_ORDERS_INFLIGHT),
            order_id_bytes.as_bytes(),
        );
    }

    if order.is_open() {
        remove_from_set(
            pipe,
            &full_redis_key(trader_key, INDEX_ORDERS_CLOSED),
            order_id_bytes.as_bytes(),
        );
        insert_set(
            pipe,
            &full_redis_key(trader_key, INDEX_ORDERS_OPEN),
            order_id_bytes.as_bytes(),
        );
    } else if order.is_closed() {
        remove_from_set(
            pipe,
            &full_redis_key(trader_key, INDEX_ORDERS_OPEN),
            order_id_bytes.as_bytes(),
        );
        insert_set(
            pipe,
            &full_redis_key(trader_key, INDEX_ORDERS_CLOSED),
            order_id_bytes.as_bytes(),
        );
    }

    if order
        .emulation_trigger()
        .is_some_and(|trigger| trigger != TriggerType::NoTrigger)
        && !order.is_closed()
    {
        insert_set(
            pipe,
            &full_redis_key(trader_key, INDEX_ORDERS_EMULATED),
            order_id_bytes.as_bytes(),
        );
    } else {
        remove_from_set(
            pipe,
            &full_redis_key(trader_key, INDEX_ORDERS_EMULATED),
            order_id_bytes.as_bytes(),
        );
    }
}

fn format_timestamp(timestamp: UnixNanos) -> String {
    let dt = DateTime::<Utc>::from_timestamp_nanos(timestamp.as_u64().cast_signed());
    dt.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
}

fn get_trader_key(trader_id: TraderId, instance_id: UUID4, config: &CacheConfig) -> String {
    let mut key = String::new();

    if config.use_trader_prefix {
        key.push_str("trader-");
    }

    key.push_str(trader_id.as_str());

    if config.use_instance_id {
        key.push(REDIS_DELIMITER);
        write!(key, "{instance_id}").expect("writing to String cannot fail");
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

#[allow(dead_code)]
#[derive(Debug)]
pub struct RedisCacheDatabaseAdapter {
    pub database: RedisCacheDatabase,
}

impl RedisCacheDatabaseAdapter {
    fn encoding(&self) -> SerializationEncoding {
        self.database.get_encoding()
    }

    fn send_command(
        &self,
        op_type: DatabaseOperation,
        key: String,
        payload: Option<Vec<Bytes>>,
    ) -> anyhow::Result<()> {
        let op = DatabaseCommand::new(op_type, key, payload);
        self.database
            .tx
            .send(op)
            .map_err(|e| anyhow::anyhow!("{FAILED_TX_CHANNEL}: {e}"))
    }

    fn append_list(&self, key: String, payload: Bytes) -> anyhow::Result<()> {
        self.send_command(DatabaseOperation::Update, key, Some(vec![payload]))
    }

    fn serialize_account_event(&self, account: &AccountAny) -> anyhow::Result<Bytes> {
        let event: AccountState = account.last_event().ok_or_else(|| {
            anyhow::anyhow!("Cannot persist account with no events: {}", account.id())
        })?;
        let payload = DatabaseQueries::serialize_payload(self.encoding(), &event)?;
        Ok(Bytes::from(payload))
    }

    fn serialize_order_event(&self, order_event: &OrderEventAny) -> anyhow::Result<Bytes> {
        let payload = DatabaseQueries::serialize_payload(self.encoding(), order_event)?;
        Ok(Bytes::from(payload))
    }

    fn serialize_position_event(&self, position: &Position) -> anyhow::Result<Bytes> {
        let event: OrderFilled = position.last_event().ok_or_else(|| {
            anyhow::anyhow!("Cannot persist position with no events: {}", position.id)
        })?;
        let payload = DatabaseQueries::serialize_payload(self.encoding(), &event)?;
        Ok(Bytes::from(payload))
    }

    fn load_state(&self, key: String) -> anyhow::Result<AHashMap<String, Bytes>> {
        let mut con = self.database.con.clone();
        let trader_key = self.database.trader_key.clone();
        let encoding = self.encoding();
        let (tx, rx) = mpsc::channel();

        get_runtime().spawn(async move {
            let result = async {
                let full_key = format!("{trader_key}{REDIS_DELIMITER}{key}");
                let value: Option<Bytes> = con.get(&full_key).await?;
                let Some(value) = value else {
                    return Ok(AHashMap::new());
                };

                DatabaseQueries::deserialize_payload(encoding, &value)
            }
            .await;

            if let Err(e) = tx.send(result) {
                log::error!("Failed to send state load result for '{key}': {e:?}");
            }
        });

        blocking_recv(&rx).map_err(|e| anyhow::anyhow!("load_state channel closed: {e}"))?
    }

    fn update_state(&self, key: String, state: &AHashMap<String, Bytes>) -> anyhow::Result<()> {
        let payload = DatabaseQueries::serialize_payload(self.encoding(), state)?;
        self.database.insert(key, Some(vec![Bytes::from(payload)]))
    }

    fn replace_list(&self, key: String, payload: Bytes) -> anyhow::Result<()> {
        self.send_command(DatabaseOperation::ReplaceList, key, Some(vec![payload]))
    }
}

#[allow(dead_code)]
#[allow(unused)]
#[async_trait::async_trait]
impl CacheDatabaseAdapter for RedisCacheDatabaseAdapter {
    fn close(&mut self) -> anyhow::Result<()> {
        self.database.close();
        Ok(())
    }

    fn flush(&mut self) -> anyhow::Result<()> {
        self.database.flushdb_sync()
    }

    async fn load_all(&self) -> anyhow::Result<CacheMap> {
        log::debug!("Loading all data");

        let (
            currencies,
            instruments,
            synthetics,
            accounts,
            orders,
            positions,
            greeks,
            yield_curves,
        ) = tokio::try_join!(
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
        let con = self.database.con.clone();
        let trader_key = self.database.trader_key.clone();
        let (tx, rx) = mpsc::channel();

        get_runtime().spawn(async move {
            let result = async {
                let pattern = format!("{trader_key}{REDIS_DELIMITER}{GENERAL}:*");
                let mut con_scan = con.clone();
                let keys = DatabaseQueries::scan_keys(&mut con_scan, pattern).await?;
                if keys.is_empty() {
                    return Ok(AHashMap::new());
                }

                let values = DatabaseQueries::read_bulk(&con, &keys).await?;
                let prefix = format!("{trader_key}{REDIS_DELIMITER}{GENERAL}{REDIS_DELIMITER}");
                let mut general = AHashMap::new();

                for (key, value) in keys.into_iter().zip(values) {
                    let Some(value) = value else {
                        continue;
                    };

                    if let Some(clean_key) = key.strip_prefix(&prefix) {
                        general.insert(clean_key.to_string(), value);
                    }
                }

                Ok(general)
            }
            .await;

            if let Err(e) = tx.send(result) {
                log::error!("Failed to send general load result: {e:?}");
            }
        });

        blocking_recv(&rx).map_err(|e| anyhow::anyhow!("load channel closed: {e}"))?
    }

    async fn load_currencies(&self) -> anyhow::Result<AHashMap<Ustr, Currency>> {
        DatabaseQueries::load_currencies(
            &self.database.con,
            &self.database.trader_key,
            self.encoding(),
        )
        .await
    }

    async fn load_instruments(&self) -> anyhow::Result<AHashMap<InstrumentId, InstrumentAny>> {
        DatabaseQueries::load_instruments(
            &self.database.con,
            &self.database.trader_key,
            self.encoding(),
        )
        .await
    }

    async fn load_synthetics(&self) -> anyhow::Result<AHashMap<InstrumentId, SyntheticInstrument>> {
        DatabaseQueries::load_synthetics(
            &self.database.con,
            &self.database.trader_key,
            self.encoding(),
        )
        .await
    }

    async fn load_accounts(&self) -> anyhow::Result<AHashMap<AccountId, AccountAny>> {
        DatabaseQueries::load_accounts(
            &self.database.con,
            &self.database.trader_key,
            self.encoding(),
        )
        .await
    }

    async fn load_orders(&self) -> anyhow::Result<AHashMap<ClientOrderId, OrderAny>> {
        DatabaseQueries::load_orders(
            &self.database.con,
            &self.database.trader_key,
            self.encoding(),
        )
        .await
    }

    async fn load_positions(&self) -> anyhow::Result<AHashMap<PositionId, Position>> {
        DatabaseQueries::load_positions(
            &self.database.con,
            &self.database.trader_key,
            self.encoding(),
        )
        .await
    }

    fn load_index_order_position(&self) -> anyhow::Result<AHashMap<ClientOrderId, PositionId>> {
        let con = self.database.con.clone();
        let trader_key = self.database.trader_key.clone();
        let (tx, rx) = mpsc::channel();

        get_runtime().spawn(async move {
            let result = DatabaseQueries::load_index_order_position(&con, &trader_key).await;
            if let Err(e) = tx.send(result) {
                log::error!("Failed to send load_index_order_position result: {e:?}");
            }
        });

        blocking_recv(&rx)
            .map_err(|e| anyhow::anyhow!("load_index_order_position channel closed: {e}"))?
    }

    fn load_index_order_client(&self) -> anyhow::Result<AHashMap<ClientOrderId, ClientId>> {
        let con = self.database.con.clone();
        let trader_key = self.database.trader_key.clone();
        let (tx, rx) = mpsc::channel();

        get_runtime().spawn(async move {
            let result = DatabaseQueries::load_index_order_client(&con, &trader_key).await;
            if let Err(e) = tx.send(result) {
                log::error!("Failed to send load_index_order_client result: {e:?}");
            }
        });

        blocking_recv(&rx)
            .map_err(|e| anyhow::anyhow!("load_index_order_client channel closed: {e}"))?
    }

    async fn load_currency(&self, code: &Ustr) -> anyhow::Result<Option<Currency>> {
        DatabaseQueries::load_currency(
            &self.database.con,
            &self.database.trader_key,
            code,
            self.encoding(),
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
            self.encoding(),
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
            self.encoding(),
        )
        .await
    }

    async fn load_account(&self, account_id: &AccountId) -> anyhow::Result<Option<AccountAny>> {
        DatabaseQueries::load_account(
            &self.database.con,
            &self.database.trader_key,
            account_id,
            self.encoding(),
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
            self.encoding(),
        )
        .await
    }

    async fn load_position(&self, position_id: &PositionId) -> anyhow::Result<Option<Position>> {
        DatabaseQueries::load_position(
            &self.database.con,
            &self.database.trader_key,
            position_id,
            self.encoding(),
        )
        .await
    }

    fn load_actor(&self, component_id: &ComponentId) -> anyhow::Result<AHashMap<String, Bytes>> {
        let key = format!("{ACTORS}{REDIS_DELIMITER}{component_id}{REDIS_DELIMITER}state");
        self.load_state(key)
    }

    fn load_strategy(&self, strategy_id: &StrategyId) -> anyhow::Result<AHashMap<String, Bytes>> {
        let key = format!("{STRATEGIES}{REDIS_DELIMITER}{strategy_id}{REDIS_DELIMITER}state");
        self.load_state(key)
    }

    fn load_signals(&self, name: &str) -> anyhow::Result<Vec<Signal>> {
        anyhow::bail!("Loading signals from Redis cache adapter not supported")
    }

    fn load_custom_data(&self, data_type: &DataType) -> anyhow::Result<Vec<CustomData>> {
        self.database.load_custom_data(data_type)
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

    fn load_quotes(&self, instrument_id: &InstrumentId) -> anyhow::Result<Vec<QuoteTick>> {
        anyhow::bail!("Loading quote data for Redis cache adapter not supported")
    }

    fn load_trades(&self, instrument_id: &InstrumentId) -> anyhow::Result<Vec<TradeTick>> {
        anyhow::bail!("Loading market data for Redis cache adapter not supported")
    }

    fn load_funding_rates(
        &self,
        instrument_id: &InstrumentId,
    ) -> anyhow::Result<Vec<FundingRateUpdate>> {
        anyhow::bail!("Loading market data for Redis cache adapter not supported")
    }

    fn load_bars(&self, instrument_id: &InstrumentId) -> anyhow::Result<Vec<Bar>> {
        anyhow::bail!("Loading market data for Redis cache adapter not supported")
    }

    fn add(&self, key: String, value: Bytes) -> anyhow::Result<()> {
        let key = format!("{GENERAL}{REDIS_DELIMITER}{key}");
        self.database.insert(key, Some(vec![value]))
    }

    fn add_currency(&self, currency: &Currency) -> anyhow::Result<()> {
        let key = format!("{CURRENCIES}{REDIS_DELIMITER}{}", currency.code);
        let payload = DatabaseQueries::serialize_payload(self.encoding(), currency)?;
        self.database.insert(key, Some(vec![Bytes::from(payload)]))
    }

    fn add_instrument(&self, instrument: &InstrumentAny) -> anyhow::Result<()> {
        let key = format!("{INSTRUMENTS}{REDIS_DELIMITER}{}", instrument.id());
        let payload = DatabaseQueries::serialize_payload(self.encoding(), instrument)?;
        self.database.insert(key, Some(vec![Bytes::from(payload)]))
    }

    fn add_synthetic(&self, synthetic: &SyntheticInstrument) -> anyhow::Result<()> {
        let key = format!("{SYNTHETICS}{REDIS_DELIMITER}{}", synthetic.id);
        let payload = DatabaseQueries::serialize_payload(self.encoding(), synthetic)?;
        self.database.insert(key, Some(vec![Bytes::from(payload)]))
    }

    fn add_account(&self, account: &AccountAny) -> anyhow::Result<()> {
        let account_id = account.id();
        let key = format!("{ACCOUNTS}{REDIS_DELIMITER}{account_id}");

        let payload = self.serialize_account_event(account)?;
        self.database.insert(key, Some(vec![payload]))
    }

    fn add_order(&self, order: &OrderAny, client_id: Option<ClientId>) -> anyhow::Result<()> {
        let client_order_id = order.client_order_id();
        let key = format!("{ORDERS}{REDIS_DELIMITER}{client_order_id}");

        let event = OrderEventAny::Initialized(order.init_event().clone());
        let payload = self.serialize_order_event(&event)?;
        self.replace_list(key, payload)?;

        let order_id_bytes = Bytes::from(client_order_id.to_string());
        self.database
            .insert(INDEX_ORDERS.to_string(), Some(vec![order_id_bytes.clone()]))?;

        if order
            .emulation_trigger()
            .is_some_and(|trigger| trigger != TriggerType::NoTrigger)
        {
            self.database.insert(
                INDEX_ORDERS_EMULATED.to_string(),
                Some(vec![order_id_bytes.clone()]),
            )?;
        }

        if let Some(client_id) = client_id {
            self.database.insert(
                INDEX_ORDER_CLIENT.to_string(),
                Some(vec![order_id_bytes, Bytes::from(client_id.to_string())]),
            )?;
        }

        Ok(())
    }

    fn add_order_snapshot(&self, snapshot: &OrderSnapshot) -> anyhow::Result<()> {
        let key = format!(
            "{SNAPSHOTS}{REDIS_DELIMITER}{ORDERS}{REDIS_DELIMITER}{}",
            snapshot.client_order_id
        );
        let payload = DatabaseQueries::serialize_payload(self.encoding(), snapshot)?;
        self.database.insert(key, Some(vec![Bytes::from(payload)]))
    }

    fn add_position(&self, position: &Position) -> anyhow::Result<()> {
        let position_id = position.id;
        let key = format!("{POSITIONS}{REDIS_DELIMITER}{position_id}");

        let payload = self.serialize_position_event(position)?;
        self.replace_list(key, payload)?;

        let position_id_bytes = Bytes::from(position_id.to_string());
        self.database.insert(
            INDEX_POSITIONS.to_string(),
            Some(vec![position_id_bytes.clone()]),
        )?;
        self.database.insert(
            INDEX_POSITIONS_OPEN.to_string(),
            Some(vec![position_id_bytes.clone()]),
        )?;
        self.send_command(
            DatabaseOperation::Delete,
            INDEX_POSITIONS_CLOSED.to_string(),
            Some(vec![position_id_bytes]),
        )?;

        Ok(())
    }

    fn add_position_snapshot(&self, snapshot: &PositionSnapshot) -> anyhow::Result<()> {
        let key = format!(
            "{SNAPSHOTS}{REDIS_DELIMITER}{POSITIONS}{REDIS_DELIMITER}{}",
            snapshot.position_id
        );
        let payload = DatabaseQueries::serialize_payload(self.encoding(), snapshot)?;
        self.database.insert(key, Some(vec![Bytes::from(payload)]))
    }

    fn add_order_book(&self, order_book: &OrderBook) -> anyhow::Result<()> {
        anyhow::bail!("Saving market data for Redis cache adapter not supported")
    }

    fn add_signal(&self, signal: &Signal) -> anyhow::Result<()> {
        anyhow::bail!("Saving signals for Redis cache adapter not supported")
    }

    fn add_custom_data(&self, data: &CustomData) -> anyhow::Result<()> {
        let json_bytes = serde_json::to_vec(data)
            .map_err(|e| anyhow::anyhow!("CustomData serialization failed: {e}"))?;
        let ts_init = data.ts_init().as_u64();
        let key = format!(
            "{CUSTOM}{REDIS_DELIMITER}{:020}{REDIS_DELIMITER}{}",
            ts_init,
            UUID4::new()
        );
        self.database
            .insert(key, Some(vec![Bytes::from(json_bytes)]))
    }

    fn add_quote(&self, quote: &QuoteTick) -> anyhow::Result<()> {
        anyhow::bail!("Saving market data for Redis cache adapter not supported")
    }

    fn add_trade(&self, trade: &TradeTick) -> anyhow::Result<()> {
        anyhow::bail!("Saving market data for Redis cache adapter not supported")
    }

    fn add_funding_rate(&self, funding_rate: &FundingRateUpdate) -> anyhow::Result<()> {
        anyhow::bail!("Saving market data for Redis cache adapter not supported")
    }

    fn add_bar(&self, bar: &Bar) -> anyhow::Result<()> {
        anyhow::bail!("Saving market data for Redis cache adapter not supported")
    }

    fn delete_actor(&self, component_id: &ComponentId) -> anyhow::Result<()> {
        let key = format!("{ACTORS}{REDIS_DELIMITER}{component_id}{REDIS_DELIMITER}state");
        let op = DatabaseCommand::new(DatabaseOperation::Delete, key, None);
        self.database
            .tx
            .send(op)
            .map_err(|e| anyhow::anyhow!("{FAILED_TX_CHANNEL}: {e}"))
    }

    fn delete_strategy(&self, component_id: &StrategyId) -> anyhow::Result<()> {
        let key = format!("{STRATEGIES}{REDIS_DELIMITER}{component_id}{REDIS_DELIMITER}state");
        let op = DatabaseCommand::new(DatabaseOperation::Delete, key, None);
        self.database
            .tx
            .send(op)
            .map_err(|e| anyhow::anyhow!("{FAILED_TX_CHANNEL}: {e}"))
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
        log::warn!(
            "Deleting account events currently a no-op (pending redesign), {account_id}: {event_id}"
        );
        Ok(())
    }

    fn index_venue_order_id(
        &self,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
    ) -> anyhow::Result<()> {
        self.database.insert(
            INDEX_ORDER_IDS.to_string(),
            Some(vec![Bytes::from(client_order_id.to_string())]),
        )?;
        log::debug!("Indexed {client_order_id:?} -> {venue_order_id:?}");
        Ok(())
    }

    fn index_order_position(
        &self,
        client_order_id: ClientOrderId,
        position_id: PositionId,
    ) -> anyhow::Result<()> {
        self.database.insert(
            INDEX_ORDER_POSITION.to_string(),
            Some(vec![
                Bytes::from(client_order_id.to_string()),
                Bytes::from(position_id.to_string()),
            ]),
        )
    }

    fn update_actor(
        &self,
        component_id: &ComponentId,
        state: &AHashMap<String, Bytes>,
    ) -> anyhow::Result<()> {
        let key = format!("{ACTORS}{REDIS_DELIMITER}{component_id}{REDIS_DELIMITER}state");
        self.update_state(key, state)
    }

    fn update_strategy(
        &self,
        strategy_id: &StrategyId,
        state: &AHashMap<String, Bytes>,
    ) -> anyhow::Result<()> {
        let key = format!("{STRATEGIES}{REDIS_DELIMITER}{strategy_id}{REDIS_DELIMITER}state");
        self.update_state(key, state)
    }

    fn update_account(&self, account: &AccountAny) -> anyhow::Result<()> {
        let account_id = account.id();
        let key = format!("{ACCOUNTS}{REDIS_DELIMITER}{account_id}");
        let payload = self.serialize_account_event(account)?;
        self.append_list(key, payload)
    }

    fn update_order(&self, order_event: &OrderEventAny) -> anyhow::Result<()> {
        let client_order_id = order_event.client_order_id();
        let key = format!("{ORDERS}{REDIS_DELIMITER}{client_order_id}");
        let payload = DatabaseQueries::serialize_payload(self.encoding(), order_event)?;
        let op = DatabaseCommand::new(
            DatabaseOperation::UpdateOrder,
            key,
            Some(vec![Bytes::from(payload)]),
        );
        self.database
            .tx
            .send(op)
            .map_err(|e| anyhow::anyhow!("{FAILED_TX_CHANNEL}: {e}"))
    }

    fn update_position(&self, position: &Position) -> anyhow::Result<()> {
        let position_id = position.id;
        let key = format!("{POSITIONS}{REDIS_DELIMITER}{position_id}");
        let payload = self.serialize_position_event(position)?;
        self.append_list(key, payload)?;

        let position_id_bytes = Bytes::from(position_id.to_string());

        if position.is_open() {
            self.database.insert(
                INDEX_POSITIONS_OPEN.to_string(),
                Some(vec![position_id_bytes.clone()]),
            )?;
            self.send_command(
                DatabaseOperation::Delete,
                INDEX_POSITIONS_CLOSED.to_string(),
                Some(vec![position_id_bytes]),
            )?;
        } else if position.is_closed() {
            self.database.insert(
                INDEX_POSITIONS_CLOSED.to_string(),
                Some(vec![position_id_bytes.clone()]),
            )?;
            self.send_command(
                DatabaseOperation::Delete,
                INDEX_POSITIONS_OPEN.to_string(),
                Some(vec![position_id_bytes]),
            )?;
        }

        Ok(())
    }

    fn snapshot_order_state(&self, order: &OrderAny) -> anyhow::Result<()> {
        let snapshot = OrderSnapshot::from(order.clone());
        self.add_order_snapshot(&snapshot)
    }

    fn snapshot_position_state(
        &self,
        position: &Position,
        ts_snapshot: UnixNanos,
        unrealized_pnl: Option<Money>,
    ) -> anyhow::Result<()> {
        let mut snapshot = PositionSnapshot::from(position, unrealized_pnl);
        snapshot.ts_init = ts_snapshot;
        self.add_position_snapshot(&snapshot)
    }

    fn heartbeat(&self, timestamp: UnixNanos) -> anyhow::Result<()> {
        let timestamp = format_timestamp(timestamp);
        self.database.insert(
            format!("{HEALTH}{REDIS_DELIMITER}heartbeat"),
            Some(vec![Bytes::from(timestamp)]),
        )
    }
}

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
}
