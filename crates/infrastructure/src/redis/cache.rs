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
    time::{Duration, Instant},
};

use bytes::Bytes;
use nautilus_common::{
    cache::{
        CacheConfig,
        database::{CacheDatabaseAdapter, CacheMap},
    },
    custom::CustomData,
    enums::SerializationEncoding,
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
    pub con: ConnectionManager,
    pub trader_id: TraderId,
    encoding: SerializationEncoding,
    handle: tokio::task::JoinHandle<()>,
    trader_key: String,
    tx: tokio::sync::mpsc::UnboundedSender<DatabaseCommand>,
}

impl RedisCacheDatabase {
    /// Creates a new [`RedisCacheDatabase`] instance.
    // need to remove async from here
    pub async fn new(
        trader_id: TraderId,
        instance_id: UUID4,
        config: CacheConfig,
    ) -> anyhow::Result<RedisCacheDatabase> {
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
                log::error!("Error in task '{CACHE_WRITE}': {e}");
            }
        });

        Ok(RedisCacheDatabase {
            trader_id,
            trader_key,
            con,
            tx,
            handle,
            encoding,
        })
    }

    pub fn get_encoding(&self) -> SerializationEncoding {
        self.encoding
    }

    pub fn get_trader_key(&self) -> &str {
        &self.trader_key
    }

    pub fn close(&mut self) {
        log::debug!("Closing");

        if let Err(e) = self.tx.send(DatabaseCommand::close()) {
            log::debug!("Error sending close command: {e:?}")
        }

        log::debug!("Awaiting task '{CACHE_WRITE}'");
        tokio::task::block_in_place(|| {
            if let Err(e) = get_runtime().block_on(&mut self.handle) {
                log::error!("Error awaiting task '{CACHE_WRITE}': {e:?}");
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

    pub async fn keys(&mut self, pattern: &str) -> anyhow::Result<Vec<String>> {
        let pattern = format!("{}{REDIS_DELIMITER}{pattern}", self.trader_key);
        log::debug!("Querying keys: {pattern}");
        DatabaseQueries::scan_keys(&mut self.con, pattern).await
    }

    pub async fn read(&mut self, key: &str) -> anyhow::Result<Vec<Bytes>> {
        DatabaseQueries::read(&self.con, &self.trader_key, key).await
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
    let mut con = create_redis_connection(CACHE_WRITE, db_config.clone()).await?;

    // Buffering
    let mut buffer: VecDeque<DatabaseCommand> = VecDeque::new();
    let mut last_drain = Instant::now();
    let buffer_interval = Duration::from_millis(config.buffer_interval_ms.unwrap_or(0) as u64);

    // Continue to receive and handle messages until channel is hung up
    loop {
        if last_drain.elapsed() >= buffer_interval && !buffer.is_empty() {
            drain_buffer(&mut con, &trader_key, &mut buffer).await;
            last_drain = Instant::now();
        } else {
            match rx.recv().await {
                Some(cmd) => {
                    tracing::debug!("Received {cmd:?}");
                    if let DatabaseOperation::Close = cmd.op_type {
                        break;
                    }
                    buffer.push_back(cmd)
                }
                None => {
                    tracing::debug!("Command channel closed");
                    break;
                }
            }
        }
    }

    // Drain any remaining messages
    if !buffer.is_empty() {
        drain_buffer(&mut con, &trader_key, &mut buffer).await;
    }

    tracing::debug!("Stopped cache processing");
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
        let key = match msg.key {
            Some(key) => key,
            None => {
                log::error!("Null key found for message: {msg:?}");
                continue;
            }
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

#[allow(dead_code)] // Under development
pub struct RedisCacheDatabaseAdapter {
    pub encoding: SerializationEncoding,
    database: RedisCacheDatabase,
}

#[allow(dead_code)] // Under development
#[allow(unused)] // Under development
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

    fn load(&self) -> anyhow::Result<HashMap<String, Bytes>> {
        // self.database.load()
        Ok(HashMap::new()) // TODO
    }

    async fn load_currencies(&self) -> anyhow::Result<HashMap<Ustr, Currency>> {
        DatabaseQueries::load_currencies(
            &self.database.con,
            &self.database.trader_key,
            self.encoding,
        )
        .await
    }

    async fn load_instruments(&self) -> anyhow::Result<HashMap<InstrumentId, InstrumentAny>> {
        DatabaseQueries::load_instruments(
            &self.database.con,
            &self.database.trader_key,
            self.encoding,
        )
        .await
    }

    async fn load_synthetics(&self) -> anyhow::Result<HashMap<InstrumentId, SyntheticInstrument>> {
        DatabaseQueries::load_synthetics(
            &self.database.con,
            &self.database.trader_key,
            self.encoding,
        )
        .await
    }

    async fn load_accounts(&self) -> anyhow::Result<HashMap<AccountId, AccountAny>> {
        DatabaseQueries::load_accounts(&self.database.con, &self.database.trader_key, self.encoding)
            .await
    }

    async fn load_orders(&self) -> anyhow::Result<HashMap<ClientOrderId, OrderAny>> {
        DatabaseQueries::load_orders(&self.database.con, &self.database.trader_key, self.encoding)
            .await
    }

    async fn load_positions(&self) -> anyhow::Result<HashMap<PositionId, Position>> {
        DatabaseQueries::load_positions(
            &self.database.con,
            &self.database.trader_key,
            self.encoding,
        )
        .await
    }

    fn load_index_order_position(&self) -> anyhow::Result<HashMap<ClientOrderId, Position>> {
        todo!()
    }

    fn load_index_order_client(&self) -> anyhow::Result<HashMap<ClientOrderId, ClientId>> {
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

    fn load_actor(&self, component_id: &ComponentId) -> anyhow::Result<HashMap<String, Bytes>> {
        todo!()
    }

    fn delete_actor(&self, component_id: &ComponentId) -> anyhow::Result<()> {
        todo!()
    }

    fn load_strategy(&self, strategy_id: &StrategyId) -> anyhow::Result<HashMap<String, Bytes>> {
        todo!()
    }

    fn delete_strategy(&self, component_id: &StrategyId) -> anyhow::Result<()> {
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
