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

use std::{collections::VecDeque, fmt::Debug, ops::ControlFlow, pin::Pin, time::Duration};

use ahash::AHashMap;
use bytes::Bytes;
use nautilus_common::{
    cache::{
        CacheConfig,
        database::{CacheDatabaseAdapter, CacheDatabaseFactory, CacheMap},
    },
    live::get_runtime,
    logging::{log_task_awaiting, log_task_started, log_task_stopped},
    signal::Signal,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    accounts::AccountAny,
    data::{Bar, CustomData, DataType, FundingRateUpdate, QuoteTick, TradeTick},
    events::{
        AccountState, OrderEventAny, OrderFilled, OrderInitialized, OrderSnapshot,
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
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, postgres::PgConnectOptions};
use tokio::{time::Instant, try_join};
use ustr::Ustr;

use crate::sql::{
    pg::{connect_pg, get_postgres_connect_options},
    queries::DatabaseQueries,
};

// Task and connection names
const CACHE_PROCESS: &str = "cache-process";

/// Configuration for a Postgres-backed cache database.
///
/// Missing fields are resolved from Postgres environment variables and then built-in defaults.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.infrastructure",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.infrastructure")
)]
pub struct PostgresCacheConfig {
    /// The Postgres host address.
    pub host: Option<String>,
    /// The Postgres port.
    pub port: Option<u16>,
    /// The Postgres account username.
    pub username: Option<String>,
    /// The Postgres account password.
    pub password: Option<String>,
    /// The Postgres database name.
    pub database: Option<String>,
}

impl Debug for PostgresCacheConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let redacted = self.password.as_ref().map(|_| "***");
        f.debug_struct(stringify!(PostgresCacheConfig))
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("password", &redacted)
            .field("database", &self.database)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json::json;

    use super::*;

    #[rstest]
    fn test_default_postgres_cache_config() {
        let config = PostgresCacheConfig::default();

        assert_eq!(config.host, None);
        assert_eq!(config.port, None);
        assert_eq!(config.username, None);
        assert_eq!(config.password, None);
        assert_eq!(config.database, None);
    }

    #[rstest]
    fn test_deserialize_postgres_cache_config() {
        let config_json = json!({
            "host": "localhost",
            "port": 5432,
            "username": "user",
            "password": "pass",
            "database": "nautilus"
        });

        let config: PostgresCacheConfig = serde_json::from_value(config_json).unwrap();

        assert_eq!(config.host, Some("localhost".to_string()));
        assert_eq!(config.port, Some(5432));
        assert_eq!(config.username, Some("user".to_string()));
        assert_eq!(config.password, Some("pass".to_string()));
        assert_eq!(config.database, Some("nautilus".to_string()));
    }

    #[rstest]
    fn test_deserialize_postgres_cache_config_rejects_type_selector() {
        let config_json = json!({
            "type": "postgres",
        });

        let error = serde_json::from_value::<PostgresCacheConfig>(config_json).unwrap_err();

        assert!(error.to_string().contains("unknown field `type`"));
    }
}

#[async_trait::async_trait]
impl CacheDatabaseFactory for PostgresCacheConfig {
    async fn create(
        &self,
        _trader_id: TraderId,
        _instance_id: UUID4,
        _config: CacheConfig,
    ) -> anyhow::Result<Box<dyn CacheDatabaseAdapter>> {
        let database = PostgresCacheDatabase::connect(
            self.host.clone(),
            self.port,
            self.username.clone(),
            self.password.clone(),
            self.database.clone(),
        )
        .await?;
        Ok(Box::new(database))
    }
}

#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.infrastructure")
)]
pub struct PostgresCacheDatabase {
    pub pool: PgPool,
    tx: tokio::sync::mpsc::UnboundedSender<DatabaseQuery>,
    handle: tokio::task::JoinHandle<()>,
}

#[allow(
    clippy::large_enum_variant,
    reason = "variant sizes vary with feature unification; allow stays silent when the lint does not fire"
)]
#[derive(Debug, Clone)]
pub enum DatabaseQuery {
    Close,
    Add(String, Vec<u8>),
    AddCurrency(Currency),
    AddInstrument(InstrumentAny),
    AddOrder(OrderInitialized, Option<ClientId>),
    AddOrderSnapshot(OrderSnapshot),
    AddPosition(PositionId, OrderFilled),
    AddPositionSnapshot(PositionSnapshot),
    AddAccount(AccountState, bool),
    AddSignal(Signal),
    AddCustom(CustomData),
    AddQuote(QuoteTick),
    AddTrade(TradeTick),
    AddBar(Bar),
    UpdateOrder(OrderEventAny),
    UpdatePosition(OrderFilled),
    IndexOrderPosition(ClientOrderId, PositionId),
}

impl PostgresCacheDatabase {
    /// Connects to the Postgres cache database using the provided connection parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if establishing the database connection fails.
    ///
    /// # Panics
    ///
    /// Panics if the internal Postgres pool connection attempt (`connect_pg`) unwraps on error.
    pub async fn connect(
        host: Option<String>,
        port: Option<u16>,
        username: Option<String>,
        password: Option<String>,
        database: Option<String>,
    ) -> Result<Self, sqlx::Error> {
        let pg_connect_options =
            get_postgres_connect_options(host, port, username, password, database);
        let pool = connect_pg(pg_connect_options.clone().into()).await.unwrap();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<DatabaseQuery>();

        let handle = get_runtime().spawn(async move {
            Box::pin(Self::process_commands(
                rx,
                pg_connect_options.clone().into(),
            ))
            .await;
        });
        Ok(Self { pool, tx, handle })
    }

    async fn process_commands(
        mut rx: tokio::sync::mpsc::UnboundedReceiver<DatabaseQuery>,
        pg_connect_options: PgConnectOptions,
    ) {
        log_task_started(CACHE_PROCESS);

        let pool = connect_pg(pg_connect_options).await.unwrap();

        // Buffering
        let mut buffer: VecDeque<DatabaseQuery> = VecDeque::new();

        // TODO: expose this via configuration once tests are fixed
        let buffer_interval = Duration::from_millis(0);

        // A sleep used to trigger periodic flushing of the buffer.
        // When `buffer_interval` is zero we skip using the timer and flush immediately
        // after every message.
        let flush_timer = tokio::time::sleep(buffer_interval);
        tokio::pin!(flush_timer);

        // Continue to receive and handle messages until channel is hung up
        loop {
            tokio::select! {
                maybe_msg = rx.recv() => {
                    let result = Box::pin(handle_query(
                        maybe_msg,
                        &mut buffer,
                        buffer_interval,
                        &pool,
                    ))
                    .await;

                    if result.is_break() {
                        break;
                    }
                }
                () = &mut flush_timer, if !buffer_interval.is_zero() => {
                    flush_buffer(&mut buffer, &pool, &mut flush_timer, buffer_interval).await;
                }
            }
        }

        if !buffer.is_empty() {
            drain_buffer(&pool, &mut buffer).await;
        }

        log_task_stopped(CACHE_PROCESS);
    }
}

async fn handle_query(
    maybe_msg: Option<DatabaseQuery>,
    buffer: &mut VecDeque<DatabaseQuery>,
    buffer_interval: Duration,
    pool: &PgPool,
) -> ControlFlow<()> {
    let Some(msg) = maybe_msg else {
        log::debug!("Command channel closed");
        return ControlFlow::Break(());
    };

    if matches!(msg, DatabaseQuery::Close) {
        if !buffer.is_empty() {
            drain_buffer(pool, buffer).await;
        }
        return ControlFlow::Break(());
    }

    buffer.push_back(msg);

    if buffer_interval.is_zero() {
        drain_buffer(pool, buffer).await;
    }

    ControlFlow::Continue(())
}

async fn flush_buffer(
    buffer: &mut VecDeque<DatabaseQuery>,
    pool: &PgPool,
    flush_timer: &mut Pin<&mut tokio::time::Sleep>,
    buffer_interval: Duration,
) {
    if !buffer.is_empty() {
        drain_buffer(pool, buffer).await;
    }
    flush_timer.as_mut().reset(Instant::now() + buffer_interval);
}

/// Retrieves a `PostgresCacheDatabase` using default connection options.
///
/// # Errors
///
/// Returns an error if connecting to the database or initializing the cache adapter fails.
pub async fn get_pg_cache_database() -> anyhow::Result<PostgresCacheDatabase> {
    let connect_options = get_postgres_connect_options(None, None, None, None, None);
    Ok(PostgresCacheDatabase::connect(
        Some(connect_options.host),
        Some(connect_options.port),
        Some(connect_options.username),
        Some(connect_options.password),
        Some(connect_options.database),
    )
    .await?)
}

#[async_trait::async_trait]
impl CacheDatabaseAdapter for PostgresCacheDatabase {
    fn close(&mut self) -> anyhow::Result<()> {
        let pool = self.pool.clone();
        let (tx, rx) = std::sync::mpsc::channel();

        log::debug!("Closing connection pool");

        tokio::task::block_in_place(|| {
            get_runtime().block_on(async {
                pool.close().await;

                if let Err(e) = tx.send(()) {
                    log::error!("Error closing pool: {e:?}");
                }
            });
        });

        // Cancel message handling task
        if let Err(e) = self.tx.send(DatabaseQuery::Close) {
            log::warn!("Error sending close: {e:?}");
        }

        log_task_awaiting("cache-write");

        tokio::task::block_in_place(|| {
            if let Err(e) = get_runtime().block_on(&mut self.handle) {
                log::error!("Error awaiting task 'cache-write': {e:?}");
            }
        });

        log::debug!("Closed");

        Ok(rx.recv()?)
    }

    fn flush(&mut self) -> anyhow::Result<()> {
        let pool = self.pool.clone();
        let (tx, rx) = std::sync::mpsc::channel();

        tokio::task::block_in_place(|| {
            get_runtime().block_on(async {
                if let Err(e) = DatabaseQueries::truncate(&pool).await {
                    log::error!("Error flushing pool: {e:?}");
                }

                if let Err(e) = tx.send(()) {
                    log::error!("Error sending flush result: {e:?}");
                }
            });
        });

        Ok(rx.recv()?)
    }

    async fn load_all(&self) -> anyhow::Result<CacheMap> {
        let (currencies, instruments, synthetics, accounts, orders, positions) = try_join!(
            self.load_currencies(),
            self.load_instruments(),
            self.load_synthetics(),
            self.load_accounts(),
            self.load_orders(),
            self.load_positions()
        )
        .map_err(|e| anyhow::anyhow!("Error loading cache data: {e}"))?;

        // For now, we don't load greeks and yield curves from the database
        // This will be implemented in the future
        let greeks = AHashMap::new();
        let yield_curves = AHashMap::new();

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
        let pool = self.pool.clone();
        let (tx, rx) = std::sync::mpsc::channel();

        tokio::spawn(async move {
            let result = DatabaseQueries::load(&pool).await;
            match result {
                Ok(items) => {
                    let mapping = items
                        .into_iter()
                        .map(|(k, v)| (k, Bytes::from(v)))
                        .collect();

                    if let Err(e) = tx.send(mapping) {
                        log::error!("Failed to send general items: {e:?}");
                    }
                }
                Err(e) => {
                    log::error!("Failed to load general items: {e:?}");
                    if let Err(e) = tx.send(AHashMap::new()) {
                        log::error!("Failed to send empty general items: {e:?}");
                    }
                }
            }
        });
        Ok(rx.recv()?)
    }

    async fn load_currencies(&self) -> anyhow::Result<AHashMap<Ustr, Currency>> {
        let pool = self.pool.clone();
        let (tx, rx) = std::sync::mpsc::channel();

        tokio::spawn(async move {
            let result = DatabaseQueries::load_currencies(&pool).await;
            match result {
                Ok(currencies) => {
                    let mapping = currencies
                        .into_iter()
                        .map(|currency| (currency.code, currency))
                        .collect();

                    if let Err(e) = tx.send(mapping) {
                        log::error!("Failed to send currencies: {e:?}");
                    }
                }
                Err(e) => {
                    log::error!("Failed to load currencies: {e:?}");
                    if let Err(e) = tx.send(AHashMap::new()) {
                        log::error!("Failed to send empty currencies: {e:?}");
                    }
                }
            }
        });
        Ok(rx.recv()?)
    }

    async fn load_instruments(&self) -> anyhow::Result<AHashMap<InstrumentId, InstrumentAny>> {
        let pool = self.pool.clone();
        let (tx, rx) = std::sync::mpsc::channel();

        tokio::spawn(async move {
            let result = DatabaseQueries::load_instruments(&pool).await;
            match result {
                Ok(instruments) => {
                    let mapping = instruments
                        .into_iter()
                        .map(|instrument| (instrument.id(), instrument))
                        .collect();

                    if let Err(e) = tx.send(mapping) {
                        log::error!("Failed to send instruments: {e:?}");
                    }
                }
                Err(e) => {
                    log::error!("Failed to load instruments: {e:?}");
                    if let Err(e) = tx.send(AHashMap::new()) {
                        log::error!("Failed to send empty instruments: {e:?}");
                    }
                }
            }
        });
        Ok(rx.recv()?)
    }

    async fn load_synthetics(&self) -> anyhow::Result<AHashMap<InstrumentId, SyntheticInstrument>> {
        todo!()
    }

    async fn load_accounts(&self) -> anyhow::Result<AHashMap<AccountId, AccountAny>> {
        let pool = self.pool.clone();
        let (tx, rx) = std::sync::mpsc::channel();

        tokio::spawn(async move {
            let result = DatabaseQueries::load_accounts(&pool).await;
            match result {
                Ok(accounts) => {
                    let mapping = accounts
                        .into_iter()
                        .map(|account| (account.id(), account))
                        .collect();

                    if let Err(e) = tx.send(mapping) {
                        log::error!("Failed to send accounts: {e:?}");
                    }
                }
                Err(e) => {
                    log::error!("Failed to load accounts: {e:?}");
                    if let Err(e) = tx.send(AHashMap::new()) {
                        log::error!("Failed to send empty accounts: {e:?}");
                    }
                }
            }
        });
        Ok(rx.recv()?)
    }

    async fn load_orders(&self) -> anyhow::Result<AHashMap<ClientOrderId, OrderAny>> {
        let pool = self.pool.clone();
        let (tx, rx) = std::sync::mpsc::channel();

        tokio::spawn(async move {
            let result = DatabaseQueries::load_orders(&pool).await;
            match result {
                Ok(orders) => {
                    let mapping = orders
                        .into_iter()
                        .map(|order| (order.client_order_id(), order))
                        .collect();

                    if let Err(e) = tx.send(mapping) {
                        log::error!("Failed to send orders: {e:?}");
                    }
                }
                Err(e) => {
                    log::error!("Failed to load orders: {e:?}");
                    if let Err(e) = tx.send(AHashMap::new()) {
                        log::error!("Failed to send empty orders: {e:?}");
                    }
                }
            }
        });
        Ok(rx.recv()?)
    }

    async fn load_positions(&self) -> anyhow::Result<AHashMap<PositionId, Position>> {
        let pool = self.pool.clone();
        let (tx, rx) = std::sync::mpsc::channel();

        tokio::spawn(async move {
            let result = DatabaseQueries::load_positions(&pool)
                .await
                .map(|positions| {
                    positions
                        .into_iter()
                        .map(|position| (position.id, position))
                        .collect()
                });

            if let Err(e) = tx.send(result) {
                log::error!("Failed to send positions: {e:?}");
            }
        });
        rx.recv()?
    }

    fn load_index_order_position(&self) -> anyhow::Result<AHashMap<ClientOrderId, PositionId>> {
        let pool = self.pool.clone();
        let (tx, rx) = std::sync::mpsc::channel();

        tokio::spawn(async move {
            let result = DatabaseQueries::load_index_order_position(&pool).await;
            match result {
                Ok(index) => {
                    if let Err(e) = tx.send(index) {
                        log::error!("Failed to send load_index_order_position result: {e:?}");
                    }
                }
                Err(e) => {
                    log::error!("Failed to run query load_index_order_position: {e:?}");
                    if let Err(e) = tx.send(AHashMap::new()) {
                        log::error!("Failed to send empty load_index_order_position result: {e:?}");
                    }
                }
            }
        });
        Ok(rx.recv()?)
    }

    fn load_index_order_client(&self) -> anyhow::Result<AHashMap<ClientOrderId, ClientId>> {
        let pool = self.pool.clone();
        let (tx, rx) = std::sync::mpsc::channel();

        tokio::spawn(async move {
            let result = DatabaseQueries::load_distinct_order_event_client_ids(&pool).await;
            match result {
                Ok(currency) => {
                    if let Err(e) = tx.send(currency) {
                        log::error!("Failed to send load_index_order_client result: {e:?}");
                    }
                }
                Err(e) => {
                    log::error!("Failed to run query load_distinct_order_event_client_ids: {e:?}");
                    if let Err(e) = tx.send(AHashMap::new()) {
                        log::error!("Failed to send empty load_index_order_client result: {e:?}");
                    }
                }
            }
        });
        Ok(rx.recv()?)
    }

    async fn load_currency(&self, code: &Ustr) -> anyhow::Result<Option<Currency>> {
        let pool = self.pool.clone();
        let code = code.to_owned(); // Clone the code
        let (tx, rx) = std::sync::mpsc::channel();

        tokio::spawn(async move {
            let result = DatabaseQueries::load_currency(&pool, &code).await;
            match result {
                Ok(currency) => {
                    if let Err(e) = tx.send(currency) {
                        log::error!("Failed to send currency {code}: {e:?}");
                    }
                }
                Err(e) => {
                    log::error!("Failed to load currency {code}: {e:?}");
                    if let Err(e) = tx.send(None) {
                        log::error!("Failed to send None for currency {code}: {e:?}");
                    }
                }
            }
        });
        Ok(rx.recv()?)
    }

    async fn load_instrument(
        &self,
        instrument_id: &InstrumentId,
    ) -> anyhow::Result<Option<InstrumentAny>> {
        let pool = self.pool.clone();
        let instrument_id = instrument_id.to_owned(); // Clone the instrument_id
        let (tx, rx) = std::sync::mpsc::channel();

        tokio::spawn(async move {
            let result = DatabaseQueries::load_instrument(&pool, &instrument_id).await;
            match result {
                Ok(instrument) => {
                    if let Err(e) = tx.send(instrument) {
                        log::error!("Failed to send instrument {instrument_id}: {e:?}");
                    }
                }
                Err(e) => {
                    log::error!("Failed to load instrument {instrument_id}: {e:?}");
                    if let Err(e) = tx.send(None) {
                        log::error!("Failed to send None for instrument {instrument_id}: {e:?}");
                    }
                }
            }
        });
        Ok(rx.recv()?)
    }

    async fn load_synthetic(
        &self,
        _instrument_id: &InstrumentId,
    ) -> anyhow::Result<Option<SyntheticInstrument>> {
        todo!()
    }

    async fn load_account(&self, account_id: &AccountId) -> anyhow::Result<Option<AccountAny>> {
        let pool = self.pool.clone();
        let account_id = account_id.to_owned();
        let (tx, rx) = std::sync::mpsc::channel();

        tokio::spawn(async move {
            let result = DatabaseQueries::load_account(&pool, &account_id).await;
            match result {
                Ok(account) => {
                    if let Err(e) = tx.send(account) {
                        log::error!("Failed to send account {account_id}: {e:?}");
                    }
                }
                Err(e) => {
                    log::error!("Failed to load account {account_id}: {e:?}");
                    if let Err(e) = tx.send(None) {
                        log::error!("Failed to send None for account {account_id}: {e:?}");
                    }
                }
            }
        });
        Ok(rx.recv()?)
    }

    async fn load_order(
        &self,
        client_order_id: &ClientOrderId,
    ) -> anyhow::Result<Option<OrderAny>> {
        let pool = self.pool.clone();
        let client_order_id = client_order_id.to_owned();
        let (tx, rx) = std::sync::mpsc::channel();

        tokio::spawn(async move {
            let result = DatabaseQueries::load_order(&pool, &client_order_id).await;
            match result {
                Ok(order) => {
                    if let Err(e) = tx.send(order) {
                        log::error!("Failed to send order {client_order_id}: {e:?}");
                    }
                }
                Err(e) => {
                    log::error!("Failed to load order {client_order_id}: {e:?}");
                    let _ = tx.send(None);
                }
            }
        });
        Ok(rx.recv()?)
    }

    async fn load_position(&self, position_id: &PositionId) -> anyhow::Result<Option<Position>> {
        let pool = self.pool.clone();
        let position_id = position_id.to_owned();
        let (tx, rx) = std::sync::mpsc::channel();

        tokio::spawn(async move {
            let result = DatabaseQueries::load_position(&pool, &position_id).await;
            if let Err(e) = tx.send(result) {
                log::error!("Failed to send position {position_id}: {e:?}");
            }
        });
        rx.recv()?
    }

    fn load_actor(&self, _component_id: &ComponentId) -> anyhow::Result<AHashMap<String, Bytes>> {
        todo!()
    }

    fn delete_actor(&self, _component_id: &ComponentId) -> anyhow::Result<()> {
        todo!()
    }

    fn load_strategy(&self, _strategy_id: &StrategyId) -> anyhow::Result<AHashMap<String, Bytes>> {
        todo!()
    }

    fn delete_strategy(&self, _strategy_id: &StrategyId) -> anyhow::Result<()> {
        todo!()
    }

    fn delete_order(&self, client_order_id: &ClientOrderId) -> anyhow::Result<()> {
        anyhow::bail!(
            "delete_order not implemented for PostgreSQL cache adapter: {client_order_id}"
        )
    }

    fn delete_position(&self, position_id: &PositionId) -> anyhow::Result<()> {
        anyhow::bail!("delete_position not implemented for PostgreSQL cache adapter: {position_id}")
    }

    fn delete_account_event(&self, account_id: &AccountId, event_id: &str) -> anyhow::Result<()> {
        anyhow::bail!(
            "delete_account_event not implemented for PostgreSQL cache adapter: {account_id}, {event_id}"
        )
    }

    fn add(&self, key: String, value: Bytes) -> anyhow::Result<()> {
        let query = DatabaseQuery::Add(key, value.into());
        self.tx
            .send(query)
            .map_err(|e| anyhow::anyhow!("Failed to send query to database message handler: {e}"))
    }

    fn add_currency(&self, currency: &Currency) -> anyhow::Result<()> {
        let query = DatabaseQuery::AddCurrency(*currency);
        self.tx.send(query).map_err(|e| {
            anyhow::anyhow!("Failed to query add_currency to database message handler: {e}")
        })
    }

    fn add_instrument(&self, instrument: &InstrumentAny) -> anyhow::Result<()> {
        let query = DatabaseQuery::AddInstrument(instrument.clone());
        self.tx.send(query).map_err(|e| {
            anyhow::anyhow!("Failed to send query add_instrument to database message handler: {e}")
        })
    }

    fn add_synthetic(&self, _synthetic: &SyntheticInstrument) -> anyhow::Result<()> {
        todo!()
    }

    fn add_account(&self, account: &AccountAny) -> anyhow::Result<()> {
        let query = DatabaseQuery::AddAccount(account_last_event(account)?, false);
        self.tx.send(query).map_err(|e| {
            anyhow::anyhow!("Failed to send query add_account to database message handler: {e}")
        })
    }

    fn add_order(&self, order: &OrderAny, client_id: Option<ClientId>) -> anyhow::Result<()> {
        let query = DatabaseQuery::AddOrder(order_initialized_event(order), client_id);
        self.tx.send(query).map_err(|e| {
            anyhow::anyhow!("Failed to send query add_order to database message handler: {e}")
        })
    }

    fn add_order_snapshot(&self, snapshot: &OrderSnapshot) -> anyhow::Result<()> {
        let query = DatabaseQuery::AddOrderSnapshot(snapshot.to_owned());
        self.tx.send(query).map_err(|e| {
            anyhow::anyhow!(
                "Failed to send query add_order_snapshot to database message handler: {e}"
            )
        })
    }

    fn add_position(&self, position: &Position) -> anyhow::Result<()> {
        let event = position_last_event(position)?;
        let query = DatabaseQuery::AddPosition(position.id, event);
        self.tx.send(query).map_err(|e| {
            anyhow::anyhow!("Failed to send query add_position to database message handler: {e}")
        })
    }

    fn add_position_snapshot(&self, snapshot: &PositionSnapshot) -> anyhow::Result<()> {
        let query = DatabaseQuery::AddPositionSnapshot(snapshot.to_owned());
        self.tx.send(query).map_err(|e| {
            anyhow::anyhow!(
                "Failed to send query add_position_snapshot to database message handler: {e}"
            )
        })
    }

    fn add_order_book(&self, _order_book: &OrderBook) -> anyhow::Result<()> {
        todo!()
    }

    fn add_quote(&self, quote: &QuoteTick) -> anyhow::Result<()> {
        let query = DatabaseQuery::AddQuote(quote.to_owned());
        self.tx.send(query).map_err(|e| {
            anyhow::anyhow!("Failed to send query add_quote to database message handler: {e}")
        })
    }

    fn load_quotes(&self, instrument_id: &InstrumentId) -> anyhow::Result<Vec<QuoteTick>> {
        let pool = self.pool.clone();
        let instrument_id = instrument_id.to_owned();
        let (tx, rx) = std::sync::mpsc::channel();

        tokio::spawn(async move {
            let result = DatabaseQueries::load_quotes(&pool, &instrument_id).await;
            match result {
                Ok(quotes) => {
                    if let Err(e) = tx.send(quotes) {
                        log::error!("Failed to send quotes for instrument {instrument_id}: {e:?}");
                    }
                }
                Err(e) => {
                    log::error!("Failed to load quotes for instrument {instrument_id}: {e:?}");
                    if let Err(e) = tx.send(Vec::new()) {
                        log::error!(
                            "Failed to send empty quotes for instrument {instrument_id}: {e:?}"
                        );
                    }
                }
            }
        });
        Ok(rx.recv()?)
    }

    fn add_trade(&self, trade: &TradeTick) -> anyhow::Result<()> {
        let query = DatabaseQuery::AddTrade(trade.to_owned());
        self.tx.send(query).map_err(|e| {
            anyhow::anyhow!("Failed to send query add_trade to database message handler: {e}")
        })
    }

    fn load_trades(&self, instrument_id: &InstrumentId) -> anyhow::Result<Vec<TradeTick>> {
        let pool = self.pool.clone();
        let instrument_id = instrument_id.to_owned();
        let (tx, rx) = std::sync::mpsc::channel();

        tokio::spawn(async move {
            let result = DatabaseQueries::load_trades(&pool, &instrument_id).await;
            match result {
                Ok(trades) => {
                    if let Err(e) = tx.send(trades) {
                        log::error!("Failed to send trades for instrument {instrument_id}: {e:?}");
                    }
                }
                Err(e) => {
                    log::error!("Failed to load trades for instrument {instrument_id}: {e:?}");
                    if let Err(e) = tx.send(Vec::new()) {
                        log::error!(
                            "Failed to send empty trades for instrument {instrument_id}: {e:?}"
                        );
                    }
                }
            }
        });
        Ok(rx.recv()?)
    }

    fn add_funding_rate(&self, _funding_rate: &FundingRateUpdate) -> anyhow::Result<()> {
        anyhow::bail!("add_funding_rate not implemented for PostgreSQL cache adapter")
    }

    fn load_funding_rates(
        &self,
        _instrument_id: &InstrumentId,
    ) -> anyhow::Result<Vec<FundingRateUpdate>> {
        anyhow::bail!("load_funding_rates not implemented for PostgreSQL cache adapter")
    }

    fn add_bar(&self, bar: &Bar) -> anyhow::Result<()> {
        let query = DatabaseQuery::AddBar(bar.to_owned());
        self.tx.send(query).map_err(|e| {
            anyhow::anyhow!("Failed to send query add_bar to database message handler: {e}")
        })
    }

    fn load_bars(&self, instrument_id: &InstrumentId) -> anyhow::Result<Vec<Bar>> {
        let pool = self.pool.clone();
        let instrument_id = instrument_id.to_owned();
        let (tx, rx) = std::sync::mpsc::channel();

        tokio::spawn(async move {
            let result = DatabaseQueries::load_bars(&pool, &instrument_id).await;
            match result {
                Ok(bars) => {
                    if let Err(e) = tx.send(bars) {
                        log::error!("Failed to send bars for instrument {instrument_id}: {e:?}");
                    }
                }
                Err(e) => {
                    log::error!("Failed to load bars for instrument {instrument_id}: {e:?}");
                    if let Err(e) = tx.send(Vec::new()) {
                        log::error!(
                            "Failed to send empty bars for instrument {instrument_id}: {e:?}"
                        );
                    }
                }
            }
        });
        Ok(rx.recv()?)
    }

    fn add_signal(&self, signal: &Signal) -> anyhow::Result<()> {
        let query = DatabaseQuery::AddSignal(signal.to_owned());
        self.tx.send(query).map_err(|e| {
            anyhow::anyhow!("Failed to send query add_signal to database message handler: {e}")
        })
    }

    fn load_signals(&self, name: &str) -> anyhow::Result<Vec<Signal>> {
        let pool = self.pool.clone();
        let name = name.to_owned();
        let (tx, rx) = std::sync::mpsc::channel();

        tokio::spawn(async move {
            let result = DatabaseQueries::load_signals(&pool, &name).await;
            match result {
                Ok(signals) => {
                    if let Err(e) = tx.send(signals) {
                        log::error!("Failed to send signals for '{name}': {e:?}");
                    }
                }
                Err(e) => {
                    log::error!("Failed to load signals for '{name}': {e:?}");
                    if let Err(e) = tx.send(Vec::new()) {
                        log::error!("Failed to send empty signals for '{name}': {e:?}");
                    }
                }
            }
        });
        Ok(rx.recv()?)
    }

    fn add_custom_data(&self, data: &CustomData) -> anyhow::Result<()> {
        let query = DatabaseQuery::AddCustom(data.to_owned());
        self.tx.send(query).map_err(|e| {
            anyhow::anyhow!("Failed to send query add_signal to database message handler: {e}")
        })
    }

    fn load_custom_data(&self, data_type: &DataType) -> anyhow::Result<Vec<CustomData>> {
        let pool = self.pool.clone();
        let data_type = data_type.to_owned();
        let (tx, rx) = std::sync::mpsc::channel();

        tokio::spawn(async move {
            let result = DatabaseQueries::load_custom_data(&pool, &data_type).await;
            match result {
                Ok(signals) => {
                    if let Err(e) = tx.send(signals) {
                        log::error!("Failed to send custom data for '{data_type}': {e:?}");
                    }
                }
                Err(e) => {
                    log::error!("Failed to load custom data for '{data_type}': {e:?}");
                    if let Err(e) = tx.send(Vec::new()) {
                        log::error!("Failed to send empty custom data for '{data_type}': {e:?}");
                    }
                }
            }
        });
        Ok(rx.recv()?)
    }

    fn load_order_snapshot(
        &self,
        client_order_id: &ClientOrderId,
    ) -> anyhow::Result<Option<OrderSnapshot>> {
        let pool = self.pool.clone();
        let client_order_id = client_order_id.to_owned();
        let (tx, rx) = std::sync::mpsc::channel();

        tokio::spawn(async move {
            let result = DatabaseQueries::load_order_snapshot(&pool, &client_order_id).await;
            match result {
                Ok(snapshot) => {
                    if let Err(e) = tx.send(snapshot) {
                        log::error!("Failed to send order snapshot {client_order_id}: {e:?}");
                    }
                }
                Err(e) => {
                    log::error!("Failed to load order snapshot {client_order_id}: {e:?}");
                    if let Err(e) = tx.send(None) {
                        log::error!(
                            "Failed to send None for order snapshot {client_order_id}: {e:?}"
                        );
                    }
                }
            }
        });
        Ok(rx.recv()?)
    }

    fn load_position_snapshot(
        &self,
        position_id: &PositionId,
    ) -> anyhow::Result<Option<PositionSnapshot>> {
        let pool = self.pool.clone();
        let position_id = position_id.to_owned();
        let (tx, rx) = std::sync::mpsc::channel();

        tokio::spawn(async move {
            let result = DatabaseQueries::load_position_snapshot(&pool, &position_id).await;
            match result {
                Ok(snapshot) => {
                    if let Err(e) = tx.send(snapshot) {
                        log::error!("Failed to send position snapshot {position_id}: {e:?}");
                    }
                }
                Err(e) => {
                    log::error!("Failed to load position snapshot {position_id}: {e:?}");
                    if let Err(e) = tx.send(None) {
                        log::error!(
                            "Failed to send None for position snapshot {position_id}: {e:?}"
                        );
                    }
                }
            }
        });
        Ok(rx.recv()?)
    }

    fn index_venue_order_id(
        &self,
        _client_order_id: ClientOrderId,
        _venue_order_id: VenueOrderId,
    ) -> anyhow::Result<()> {
        todo!()
    }

    fn index_order_position(
        &self,
        client_order_id: ClientOrderId,
        position_id: PositionId,
    ) -> anyhow::Result<()> {
        let query = DatabaseQuery::IndexOrderPosition(client_order_id, position_id);
        self.tx.send(query).map_err(|e| {
            anyhow::anyhow!(
                "Failed to send query index_order_position to database message handler: {e}"
            )
        })
    }

    fn update_actor(
        &self,
        _component_id: &ComponentId,
        _state: &AHashMap<String, Bytes>,
    ) -> anyhow::Result<()> {
        todo!()
    }

    fn update_strategy(
        &self,
        _strategy_id: &StrategyId,
        _state: &AHashMap<String, Bytes>,
    ) -> anyhow::Result<()> {
        todo!()
    }

    fn update_account(&self, account: &AccountAny) -> anyhow::Result<()> {
        let query = DatabaseQuery::AddAccount(account_last_event(account)?, true);
        self.tx.send(query).map_err(|e| {
            anyhow::anyhow!("Failed to send query add_account to database message handler: {e}")
        })
    }

    fn update_order(&self, event: &OrderEventAny) -> anyhow::Result<()> {
        let query = DatabaseQuery::UpdateOrder(event.clone());
        self.tx.send(query).map_err(|e| {
            anyhow::anyhow!("Failed to send query update_order to database message handler: {e}")
        })
    }

    fn update_position(&self, position: &Position) -> anyhow::Result<()> {
        let query = DatabaseQuery::UpdatePosition(position_last_event(position)?);
        self.tx.send(query).map_err(|e| {
            anyhow::anyhow!("Failed to send query update_position to database message handler: {e}")
        })
    }

    fn snapshot_order_state(&self, _order: &OrderAny) -> anyhow::Result<()> {
        todo!()
    }

    fn snapshot_position_state(
        &self,
        _position: &Position,
        _ts_snapshot: UnixNanos,
        _unrealized_pnl: Option<Money>,
    ) -> anyhow::Result<()> {
        todo!()
    }

    fn heartbeat(&self, _timestamp: UnixNanos) -> anyhow::Result<()> {
        todo!()
    }
}

fn account_last_event(account: &AccountAny) -> anyhow::Result<AccountState> {
    account
        .last_event()
        .ok_or_else(|| anyhow::anyhow!("Cannot persist account with no events: {}", account.id()))
}

fn order_initialized_event(order: &OrderAny) -> OrderInitialized {
    order.init_event().clone()
}

fn position_last_event(position: &Position) -> anyhow::Result<OrderFilled> {
    position
        .last_event()
        .ok_or_else(|| anyhow::anyhow!("Cannot persist position with no events: {}", position.id))
}

#[expect(
    clippy::too_many_lines,
    reason = "database command dispatch enumerates each cache query variant explicitly"
)]
async fn drain_buffer(pool: &PgPool, buffer: &mut VecDeque<DatabaseQuery>) {
    for cmd in buffer.drain(..) {
        let result: anyhow::Result<()> = match cmd {
            DatabaseQuery::Close => Ok(()),
            DatabaseQuery::Add(key, value) => DatabaseQueries::add(pool, key, value).await,
            DatabaseQuery::AddCurrency(currency) => {
                DatabaseQueries::add_currency(pool, currency).await
            }
            DatabaseQuery::AddInstrument(instrument_any) => match instrument_any {
                InstrumentAny::Betting(instrument) => {
                    DatabaseQueries::add_instrument(pool, "BETTING", Box::new(instrument)).await
                }
                InstrumentAny::BinaryOption(instrument) => {
                    DatabaseQueries::add_instrument(pool, "BINARY_OPTION", Box::new(instrument))
                        .await
                }
                InstrumentAny::CryptoFuture(instrument) => {
                    DatabaseQueries::add_instrument(pool, "CRYPTO_FUTURE", Box::new(instrument))
                        .await
                }
                InstrumentAny::CryptoFuturesSpread(instrument) => {
                    DatabaseQueries::add_instrument(
                        pool,
                        "CRYPTO_FUTURES_SPREAD",
                        Box::new(instrument),
                    )
                    .await
                }
                InstrumentAny::CryptoOption(instrument) => {
                    DatabaseQueries::add_instrument(pool, "CRYPTO_OPTION", Box::new(instrument))
                        .await
                }
                InstrumentAny::CryptoOptionSpread(instrument) => {
                    DatabaseQueries::add_instrument(
                        pool,
                        "CRYPTO_OPTION_SPREAD",
                        Box::new(instrument),
                    )
                    .await
                }
                InstrumentAny::CryptoPerpetual(instrument) => {
                    DatabaseQueries::add_instrument(pool, "CRYPTO_PERPETUAL", Box::new(instrument))
                        .await
                }
                InstrumentAny::CurrencyPair(instrument) => {
                    DatabaseQueries::add_instrument(pool, "CURRENCY_PAIR", Box::new(instrument))
                        .await
                }
                InstrumentAny::Equity(equity) => {
                    DatabaseQueries::add_instrument(pool, "EQUITY", Box::new(equity)).await
                }
                InstrumentAny::FuturesContract(instrument) => {
                    DatabaseQueries::add_instrument(pool, "FUTURES_CONTRACT", Box::new(instrument))
                        .await
                }
                InstrumentAny::FuturesSpread(instrument) => {
                    DatabaseQueries::add_instrument(pool, "FUTURES_SPREAD", Box::new(instrument))
                        .await
                }
                InstrumentAny::OptionContract(instrument) => {
                    DatabaseQueries::add_instrument(pool, "OPTION_CONTRACT", Box::new(instrument))
                        .await
                }
                InstrumentAny::Commodity(instrument) => {
                    DatabaseQueries::add_instrument(pool, "COMMODITY", Box::new(instrument)).await
                }
                InstrumentAny::IndexInstrument(instrument) => {
                    DatabaseQueries::add_instrument(pool, "INDEX_INSTRUMENT", Box::new(instrument))
                        .await
                }
                InstrumentAny::Cfd(instrument) => {
                    DatabaseQueries::add_instrument(pool, "CFD", Box::new(instrument)).await
                }
                InstrumentAny::OptionSpread(instrument) => {
                    DatabaseQueries::add_instrument(pool, "OPTION_SPREAD", Box::new(instrument))
                        .await
                }
                InstrumentAny::PerpetualContract(instrument) => {
                    DatabaseQueries::add_instrument(
                        pool,
                        "PERPETUAL_CONTRACT",
                        Box::new(instrument),
                    )
                    .await
                }
                InstrumentAny::TokenizedAsset(instrument) => {
                    DatabaseQueries::add_instrument(pool, "TOKENIZED_ASSET", Box::new(instrument))
                        .await
                }
            },
            DatabaseQuery::AddOrder(event, client_id) => {
                DatabaseQueries::add_order(pool, event, client_id).await
            }
            DatabaseQuery::AddOrderSnapshot(snapshot) => {
                DatabaseQueries::add_order_snapshot(pool, snapshot).await
            }
            DatabaseQuery::AddPosition(position_id, event) => {
                DatabaseQueries::add_position(pool, position_id, &event).await
            }
            DatabaseQuery::AddPositionSnapshot(snapshot) => {
                DatabaseQueries::add_position_snapshot(pool, snapshot).await
            }
            DatabaseQuery::AddAccount(event, updated) => {
                DatabaseQueries::add_account(pool, updated, event).await
            }
            DatabaseQuery::AddSignal(signal) => DatabaseQueries::add_signal(pool, &signal).await,
            DatabaseQuery::AddCustom(data) => DatabaseQueries::add_custom_data(pool, &data).await,
            DatabaseQuery::AddQuote(quote) => DatabaseQueries::add_quote(pool, &quote).await,
            DatabaseQuery::AddTrade(trade) => DatabaseQueries::add_trade(pool, &trade).await,
            DatabaseQuery::AddBar(bar) => DatabaseQueries::add_bar(pool, &bar).await,
            DatabaseQuery::UpdateOrder(event) => {
                DatabaseQueries::add_order_event(pool, event.into_boxed(), None).await
            }
            DatabaseQuery::UpdatePosition(event) => {
                DatabaseQueries::update_position(pool, &event).await
            }
            DatabaseQuery::IndexOrderPosition(client_order_id, position_id) => {
                DatabaseQueries::index_order_position(pool, client_order_id, position_id).await
            }
        };

        if let Err(e) = result {
            log::error!("Error on query: {e:?}");
        }
    }
}
