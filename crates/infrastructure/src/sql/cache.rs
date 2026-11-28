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
use futures::future::LocalBoxFuture;
#[cfg(feature = "defi")]
use nautilus_common::defi::cache::InstrumentMigration;
use nautilus_common::{
    cache::{
        CacheConfig,
        database::{
            CacheDatabaseAdapter, CacheDatabaseFactory, CacheMap, CacheOrderIndexes,
            POSITION_OMS_KEY_PREFIX, register_loaded_currencies,
        },
    },
    live::{block_on_nautilus_with, get_runtime},
    logging::{log_task_awaiting, log_task_started, log_task_stopped},
    signal::Signal,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    accounts::AccountAny,
    data::{
        Bar, CustomData, DataType, FundingRateUpdate, InstrumentClose, QuoteTick, TradeTick,
        greeks::{GreeksData, YieldCurve},
    },
    enums::OmsType,
    events::{
        AccountState, OrderEventAny, OrderFilled, OrderInitialized, OrderSnapshot,
        position::snapshot::PositionSnapshot,
    },
    identifiers::{
        AccountId, ActorId, ClientId, ClientOrderId, InstrumentId, PositionId, StrategyId,
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
    models::general::GeneralRow,
    pg::get_postgres_connect_options,
    queries::{
        CACHE_DATABASE_KEY_PREFIX, DatabaseQueries, trader_general_key_prefix, trader_key_prefix,
    },
};

// Task and connection names
const CACHE_PROCESS: &str = "cache-process";
const SCHEMA_MIGRATION_COMMAND: &str = "run `nautilus database init` to migrate";

/// Configuration for a Postgres-backed cache database.
///
/// Missing fields are resolved from Postgres environment variables and then built-in defaults.
#[cfg_attr(
    feature = "python",
    expect(
        clippy::unsafe_derive_deserialize,
        reason = "config deserializes plain fields; unsafe methods come from generated PyO3 integration"
    )
)]
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.infrastructure", from_py_object)
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
    use nautilus_common::cache::Cache;
    use nautilus_model::enums::BookType;
    use rstest::rstest;
    use serde_json::json;

    use super::*;
    #[cfg(feature = "defi")]
    use crate::defi::IntegrityError;

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

    #[tokio::test]
    async fn test_flush_queues_behind_pending_write_and_waits_for_result() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy_with(PgConnectOptions::new().host("127.0.0.1").port(1));
        pool.close().await;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let handle = tokio::spawn(async {});
        let trader_id = TraderId::from("TRADER-001");
        let writer_pool = pool.clone();

        let mut database = PostgresCacheDatabase {
            pool,
            trader_id,
            tx: DatabaseWriter(tx),
            handle,
        };

        database
            .add("flush-order".to_string(), Bytes::from_static(b"pending"))
            .unwrap();

        let flush = tokio::task::spawn_blocking(move || database.flush());
        let deadline = Duration::from_secs(5);

        let (write, command) = tokio::time::timeout(deadline, async {
            // Leave the write unprocessed until the flush command arrives
            (rx.recv().await, rx.recv().await)
        })
        .await
        .unwrap();

        let Some(WriteCommand::Query(DatabaseQuery::Add(key, value))) = write else {
            panic!("Expected the pending write before the flush command");
        };

        assert!(
            matches!(command.as_ref(), Some(WriteCommand::Flush(_))),
            "Expected a flush command on the writer queue"
        );

        let mut buffer = WriteBuffer::default();
        let flow = handle_query(
            command,
            &mut buffer,
            Duration::ZERO,
            &writer_pool,
            &trader_id,
        )
        .await;
        let error = tokio::time::timeout(deadline, flush)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();

        assert_eq!(
            key,
            "cache-database:trader:10:TRADER-001:general:flush-order"
        );
        assert_eq!(value, b"pending");
        assert_eq!(flow, ControlFlow::Continue(()));
        assert_eq!(
            error.to_string(),
            "Failed to flush Postgres cache for TRADER-001: attempted to acquire a connection on a closed pool"
        );
    }

    #[cfg(feature = "defi")]
    #[tokio::test]
    async fn test_defi_write_closed_channel_preserves_transport_error() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy_with(PgConnectOptions::new().host("127.0.0.1").port(1));
        pool.close().await;
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        drop(rx);

        let handle = tokio::spawn(async {});

        let database = PostgresCacheDatabase {
            pool,
            trader_id: TraderId::from("TRADER-001"),
            tx: DatabaseWriter(tx),
            handle,
        };

        let error = database
            .write_defi(|reply| DatabaseQuery::DefiCurrency(Currency::USD(), reply))
            .unwrap_err();
        assert!(!error.is::<IntegrityError>());
        assert_eq!(error.to_string(), "channel closed");
    }

    #[tokio::test]
    async fn test_unsupported_order_book_persistence_keeps_memory_current() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy_with(PgConnectOptions::new().host("127.0.0.1").port(1));
        pool.close().await;
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        drop(rx);

        let handle = tokio::spawn(async {});

        let database = PostgresCacheDatabase {
            pool,
            trader_id: TraderId::from("TRADER-001"),
            tx: DatabaseWriter(tx),
            handle,
        };

        let instrument_id = InstrumentId::from("AUD/USD.SIM");
        let book_type = BookType::L2_MBP;
        let book = OrderBook::new(instrument_id, book_type);
        let error = database.add_order_book(&book).unwrap_err();

        let mut cache = Cache::new(
            Some(CacheConfig {
                save_market_data: true,
                ..Default::default()
            }),
            Some(Box::new(database)),
        );

        cache.add_order_book(book).unwrap();
        let book = cache.order_book(&instrument_id).unwrap();

        assert_eq!(
            error.to_string(),
            "Postgres order book persistence is not supported"
        );
        assert_eq!(book.instrument_id, instrument_id);
        assert_eq!(book.book_type, book_type);
    }

    #[tokio::test]
    async fn test_check_account_ownership_propagates_query_failure() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy_with(PgConnectOptions::new().host("127.0.0.1").port(1));
        pool.close().await;

        let error = check_account_ownership(&pool).await.unwrap_err();

        assert!(
            error
                .to_string()
                .contains("Failed to check account event ownership"),
            "was: {error}"
        );
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
        trader_id: TraderId,
        _instance_id: UUID4,
        _config: CacheConfig,
    ) -> anyhow::Result<Box<dyn CacheDatabaseAdapter>> {
        let database = PostgresCacheDatabase::connect(self.clone(), trader_id).await?;
        Ok(Box::new(database))
    }
}

#[derive(Debug)]
pub struct PostgresCacheDatabase {
    pub pool: PgPool,
    trader_id: TraderId,
    tx: DatabaseWriter,
    handle: tokio::task::JoinHandle<()>,
}

#[allow(
    clippy::large_enum_variant,
    reason = "variant sizes vary with feature unification; allow stays silent when the lint does not fire"
)]
#[derive(Debug, Clone)]
pub enum DatabaseQuery {
    #[cfg(feature = "defi")]
    DefiCurrency(Currency, std::sync::mpsc::SyncSender<anyhow::Result<()>>),
    #[cfg(feature = "defi")]
    DefiInstrument(
        InstrumentAny,
        Option<InstrumentMigration>,
        std::sync::mpsc::SyncSender<anyhow::Result<()>>,
    ),
    Close,
    Add(String, Vec<u8>),
    Delete(String),
    DeleteOrder(ClientOrderId),
    DeletePosition(PositionId),
    AddCurrency(Currency),
    AddInstrument(InstrumentAny),
    AddInstrumentClose(InstrumentClose),
    AddOrder(OrderInitialized, Option<ClientId>),
    AddOrderSnapshot(OrderSnapshot),
    AddPosition(PositionId, OrderFilled),
    AddPositionWithSnapshot(PositionId, OrderFilled, Box<PositionSnapshot>),
    AddPositionSnapshot(PositionSnapshot),
    AddAccount(AccountState, bool),
    AddSignal(Signal),
    AddCustom(CustomData),
    AddQuote(QuoteTick),
    AddTrade(TradeTick),
    AddBar(Bar),
    AddGreeks(GreeksData),
    AddYieldCurve(YieldCurve),
    UpdateOrder(OrderEventAny),
    UpdatePosition(OrderFilled),
    IndexOrderPosition(ClientOrderId, PositionId),
    IndexOrderClients(Vec<(ClientOrderId, ClientId)>),
}

impl PostgresCacheDatabase {
    // panics-doc-ok (transitive via PostgreSQL connection options)
    /// Connects to the Postgres cache database using the concrete backend configuration.
    ///
    /// Loads, trader-owned writes and flushes are scoped to `trader_id`, and account events are
    /// stamped with it.
    ///
    /// # Errors
    ///
    /// Returns an error if either database pool fails to connect, if the schema is out of
    /// date, if any account events have no trader (the error lists the accounts to assign), or if
    /// checking account ownership fails.
    ///
    /// # Panics
    ///
    /// Panics if `config.port` is unset and `POSTGRES_PORT` is not a valid `u16`.
    pub async fn connect(
        config: PostgresCacheConfig,
        trader_id: TraderId,
    ) -> Result<Self, sqlx::Error> {
        let pg_connect_options: PgConnectOptions = get_postgres_connect_options(
            config.host,
            config.port,
            config.username,
            config.password,
            config.database,
        )
        .into();
        let pool = PgPool::connect_with(pg_connect_options.clone()).await?;
        check_schema_migrated(&pool).await?;
        check_account_ownership(&pool).await?;

        let writer_pool = get_runtime()
            .spawn(PgPool::connect_with(pg_connect_options))
            .await
            .map_err(|e| sqlx::Error::Io(std::io::Error::other(e)))??;
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = get_runtime().spawn(async move {
            Box::pin(Self::process_commands(rx, writer_pool, trader_id)).await;
        });
        Ok(Self {
            pool,
            trader_id,
            tx: DatabaseWriter(tx),
            handle,
        })
    }

    #[cfg(feature = "defi")]
    fn write_defi(
        &self,
        query: impl FnOnce(std::sync::mpsc::SyncSender<anyhow::Result<()>>) -> DatabaseQuery,
    ) -> anyhow::Result<()> {
        let (reply, receiver) = std::sync::mpsc::sync_channel(1);
        self.tx.send(query(reply))?;
        let on_runtime = tokio::runtime::Handle::try_current()
            .is_ok_and(|handle| handle.id() == get_runtime().handle().id());

        if on_runtime {
            tokio::task::block_in_place(|| receiver.recv())?
        } else {
            receiver.recv()?
        }
    }

    /// Returns the trader that loads, writes and flushes are scoped to.
    #[must_use]
    pub const fn trader_id(&self) -> TraderId {
        self.trader_id
    }

    async fn process_commands(
        mut rx: tokio::sync::mpsc::UnboundedReceiver<WriteCommand>,
        pool: PgPool,
        trader_id: TraderId,
    ) {
        log_task_started(CACHE_PROCESS);

        // Buffering
        let mut buffer = WriteBuffer::default();

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
                        &trader_id,
                    ))
                    .await;

                    if result.is_break() {
                        break;
                    }
                }
                () = &mut flush_timer, if !buffer_interval.is_zero() => {
                    flush_buffer(
                        &mut buffer,
                        &pool,
                        &trader_id,
                        &mut flush_timer,
                        buffer_interval,
                    )
                    .await;
                }
            }
        }

        if !buffer.commands.is_empty() {
            drain_buffer(&pool, &trader_id, &mut buffer).await;
        }

        log_task_stopped(CACHE_PROCESS);
    }

    fn write_general(&self, key: String, value: Bytes) -> anyhow::Result<()> {
        self.tx
            .send(DatabaseQuery::Add(key, value.into()))
            .map_err(|e| anyhow::anyhow!("Failed to send query to database message handler: {e}"))
    }

    async fn load_cache_without_positions(&self) -> anyhow::Result<CacheMap> {
        let mut currencies = self.load_currencies().await?;
        register_loaded_currencies(&mut currencies)?;

        let (instruments, instrument_closes, synthetics, accounts, orders, greeks, yield_curves) =
            try_join!(
                self.load_instruments(),
                self.load_instrument_closes(),
                self.load_synthetics(),
                self.load_accounts(),
                self.load_orders(),
                self.load_greeks(),
                self.load_yield_curves()
            )
            .map_err(|e| {
                let message = format!("Error loading cache data: {e}");
                e.context(message)
            })?;

        Ok(CacheMap {
            currencies,
            instruments,
            instrument_closes,
            synthetics,
            accounts,
            orders,
            greeks,
            yield_curves,
            ..CacheMap::default()
        })
    }

    fn general_entries(&self, rows: Vec<GeneralRow>) -> AHashMap<String, Bytes> {
        let trader_id = self.trader_id;
        let trader_prefix = trader_key_prefix(trader_id);
        let general_prefix = trader_general_key_prefix(trader_id);
        let mut entries = AHashMap::with_capacity(rows.len());
        let mut legacy_count = 0;

        for row in rows {
            let mut key = row.id;
            if key.starts_with(&general_prefix) {
                key.drain(..general_prefix.len());
            } else if key.starts_with(&trader_prefix) {
                key.drain(..trader_prefix.len());
            } else if key.starts_with(POSITION_OMS_KEY_PREFIX) {
                legacy_count += 1;
            }

            entries.insert(key, Bytes::from(row.value));
        }

        if legacy_count > 0 {
            log::warn!(
                "Restored unspecified OMS for {legacy_count} positions with unowned legacy metadata, trader {trader_id}"
            );
        }

        entries
    }

    fn load_state(&self, key: String) -> anyhow::Result<AHashMap<String, Bytes>> {
        let pool = self.pool.clone();
        block_on_nautilus_with(move || async move {
            let value = DatabaseQueries::load_value(&pool, &key).await?;
            match value {
                Some(value) => serde_json::from_slice(&value).map_err(Into::into),
                None => Ok(AHashMap::new()),
            }
        })
    }
}

#[derive(Debug)]
struct DatabaseWriter(tokio::sync::mpsc::UnboundedSender<WriteCommand>);

impl DatabaseWriter {
    fn send(&self, command: impl Into<WriteCommand>) -> anyhow::Result<()> {
        self.0.send(command.into())?;
        Ok(())
    }
}

#[allow(
    clippy::large_enum_variant,
    reason = "the queue keeps ordinary database writes inline"
)]
#[derive(Debug)]
enum WriteCommand {
    Query(DatabaseQuery),
    OrderReplacement(OrderReplacement),
    OrderUpdate(OrderEventAny, UUID4),
    Position {
        id: PositionId,
        event: OrderFilled,
        snapshot: Option<Box<PositionSnapshot>>,
        oms_type: OmsType,
    },
    Flush(tokio::sync::oneshot::Sender<anyhow::Result<()>>),
}

#[derive(Default)]
struct WriteBuffer {
    commands: VecDeque<WriteCommand>,
    order_replacements: OrderReplacements,
}

type OrderReplacements = AHashMap<(TraderId, ClientOrderId), OrderReplacement>;

#[derive(Debug)]
struct OrderReplacement {
    init: OrderInitialized,
    client_id: Option<ClientId>,
    prior_event_ids: Vec<String>,
    first_update_id: Option<UUID4>,
}

impl From<DatabaseQuery> for WriteCommand {
    fn from(query: DatabaseQuery) -> Self {
        Self::Query(query)
    }
}

// Fails fast when the connected database predates required cache schema.
//
// An absent instrument-close table otherwise fails only when first queried. Both directions of the
// exact-average type mismatch are silent: `numeric -> double precision` is an implicit cast so
// writes truncate, and the row readers use `.ok().flatten()` so reads degrade to `None`. Absent
// order event columns are silent in both directions too: every insert names them so writes fail
// and drop the event, while every decoder reads them so `load_orders` yields an empty cache.
// Missing instrument metadata also breaks inserts and panics in the existing row dispatch on load.
async fn check_schema_migrated(pool: &PgPool) -> Result<(), sqlx::Error> {
    let has_instrument_close: bool = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1
            FROM information_schema.tables
            WHERE table_schema = current_schema()
              AND table_name = 'instrument_close'
        )",
    )
    .fetch_one(pool)
    .await?;

    if !has_instrument_close {
        return Err(sqlx::Error::Configuration(
            format!(
                "Postgres schema is out of date, missing `instrument_close` table: {SCHEMA_MIGRATION_COMMAND}"
            )
            .into(),
        ));
    }

    let stale: Vec<String> = sqlx::query_scalar(
        "SELECT table_name || '.' || column_name || ' (' || data_type || ')'
        FROM information_schema.columns
        WHERE table_schema = current_schema()
          AND (table_name, column_name) IN (('order', 'avg_px'), ('order', 'slippage'),
              ('position', 'signed_qty'), ('position', 'avg_px_open'), ('position', 'avg_px_close'))
          AND data_type <> 'numeric'
        ORDER BY table_name, column_name",
    )
    .fetch_all(pool)
    .await?;

    if stale.iter().any(|column| column.starts_with("position.")) {
        return Err(sqlx::Error::Configuration(
            format!("Postgres position accounting requires numeric columns, found {}; create a new cache with the current schema", stale.join(", ")).into(),
        ));
    }

    if !stale.is_empty() {
        return Err(sqlx::Error::Configuration(
            format!(
                "Postgres schema is out of date, {} should be `numeric`: {SCHEMA_MIGRATION_COMMAND}",
                stale.join(", "),
            )
            .into(),
        ));
    }

    check_trader_keys(pool).await?;
    check_required_columns(pool).await?;

    let has_betting_cleanup: bool = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM pg_trigger
            WHERE tgrelid = 'instrument'::regclass
              AND tgname = 'instrument_delete_betting_metadata'
              AND tgenabled IN ('O', 'A')
        )",
    )
    .fetch_one(pool)
    .await?;

    if has_betting_cleanup {
        return Ok(());
    }

    Err(sqlx::Error::Configuration(
        format!(
            "Postgres schema is out of date, missing or disabled instrument_delete_betting_metadata trigger: {SCHEMA_MIGRATION_COMMAND}"
        )
        .into(),
    ))
}

async fn check_required_columns(pool: &PgPool) -> Result<(), sqlx::Error> {
    let missing: Vec<String> = sqlx::query_scalar(
        "SELECT required.table_name || '.' || required.column_name
        FROM (VALUES
            ('greeks', 'instrument_id'),
            ('greeks', 'value'),
            ('yield_curve', 'curve_name'),
            ('yield_curve', 'value'),
            ('instrument', 'info'),
            ('instrument', 'tick_scheme'),
            ('instrument', 'underlying_instrument_id'),
            ('instrument', 'strike_currency'),
            ('instrument', 'premium_style'),
            ('instrument', 'exercise_style'),
            ('instrument', 'settlement_type'),
            ('betting_instrument', 'instrument_id'),
            ('betting_instrument', 'event_type_id'),
            ('betting_instrument', 'event_type_name'),
            ('betting_instrument', 'competition_id'),
            ('betting_instrument', 'competition_name'),
            ('betting_instrument', 'event_id'),
            ('betting_instrument', 'event_name'),
            ('betting_instrument', 'event_country_code'),
            ('betting_instrument', 'event_open_date'),
            ('betting_instrument', 'betting_type'),
            ('betting_instrument', 'market_id'),
            ('betting_instrument', 'market_name'),
            ('betting_instrument', 'market_type'),
            ('betting_instrument', 'market_start_time'),
            ('betting_instrument', 'selection_id'),
            ('betting_instrument', 'selection_name'),
            ('betting_instrument', 'selection_handicap'),
            ('account_event', 'info'),
            ('account_event', 'trader_id'),
            ('order_position_index', 'trader_id'),
            ('order_event', 'released_price'),
            ('order_event', 'protection_price'),
            ('order_event', 'due_post_only'),
            ('order_event', 'correction_id'),
            ('order_event', 'is_reopened'),
            ('order_event', 'info'),
            ('order_event', 'causation_id'),
            ('position_event', 'reconciliation'),
            ('position_event', 'info'),
            ('position_event', 'causation_id')
            UNION ALL
            SELECT * FROM (VALUES
                ('instrument', 'defi_currency_codes'),
                ('defi_currency', 'currency_id')
            ) AS defi(table_name, column_name)
            WHERE $1
        ) AS required(table_name, column_name)
        WHERE NOT EXISTS (
            SELECT 1
            FROM information_schema.columns
            WHERE table_schema = current_schema()
              AND table_name = required.table_name
              AND column_name = required.column_name
        )
        ORDER BY 1",
    )
    .bind(cfg!(feature = "defi"))
    .fetch_all(pool)
    .await?;

    if missing.is_empty() {
        return Ok(());
    }

    Err(sqlx::Error::Configuration(
        format!(
            "Postgres schema is out of date, missing cache columns {}: {SCHEMA_MIGRATION_COMMAND}",
            missing.join(", "),
        )
        .into(),
    ))
}

// Client order and position IDs are only unique per trader, so the snapshot and index keys must be
// trader-qualified or two traders sharing one database overwrite each other's rows.
async fn check_trader_keys(pool: &PgPool) -> Result<(), sqlx::Error> {
    let unscoped_keys: Vec<String> = sqlx::query_scalar(
        "SELECT constraints.table_name
        FROM information_schema.table_constraints AS constraints
        WHERE constraints.table_schema = current_schema()
          AND constraints.constraint_type = 'PRIMARY KEY'
          AND constraints.table_name IN ('order', 'position', 'order_position_index')
          AND NOT EXISTS (
            SELECT 1
            FROM information_schema.key_column_usage AS keys
            WHERE keys.constraint_schema = constraints.constraint_schema
              AND keys.constraint_name = constraints.constraint_name
              AND keys.column_name = 'trader_id'
          )
        ORDER BY 1",
    )
    .fetch_all(pool)
    .await?;

    if unscoped_keys.is_empty() {
        return Ok(());
    }

    Err(sqlx::Error::Configuration(
        format!(
            "Postgres schema is out of date, primary keys of {} are not trader-qualified: \
             {SCHEMA_MIGRATION_COMMAND}",
            unscoped_keys.join(", "),
        )
        .into(),
    ))
}

// Account events written before trader-scoped persistence carry no trader. Nothing establishes which
// trader owns them, so connecting would silently omit persisted account state: refuse until every
// such account is assigned. `nautilus database assign-account` connects directly, so it stays usable
// while this blocks the cache.
async fn check_account_ownership(pool: &PgPool) -> Result<(), sqlx::Error> {
    let account_ids = DatabaseQueries::load_unassigned_account_ids(pool)
        .await
        .map_err(|e| {
            sqlx::Error::Configuration(
                format!("Failed to check account event ownership: {e}").into(),
            )
        })?;

    if account_ids.is_empty() {
        return Ok(());
    }

    let account_ids: Vec<String> = account_ids.iter().map(ToString::to_string).collect();
    Err(sqlx::Error::Configuration(
        format!(
            "Postgres cache has account events with no trader for {}: assign each account with \
             `nautilus database assign-account --account-id <ACCOUNT_ID> --trader-id <TRADER_ID>` \
             before connecting",
            account_ids.join(", "),
        )
        .into(),
    ))
}

async fn handle_query(
    maybe_msg: Option<WriteCommand>,
    buffer: &mut WriteBuffer,
    buffer_interval: Duration,
    pool: &PgPool,
    trader_id: &TraderId,
) -> ControlFlow<()> {
    let Some(msg) = maybe_msg else {
        log::debug!("Command channel closed");
        return ControlFlow::Break(());
    };

    let msg = match msg {
        #[cfg(feature = "defi")]
        WriteCommand::Query(DatabaseQuery::DefiCurrency(currency, reply)) => {
            if !buffer.commands.is_empty() {
                drain_buffer(pool, trader_id, buffer).await;
            }

            let result = DatabaseQueries::add_currency(pool, currency).await;
            let _ = reply.send(result);
            return ControlFlow::Continue(());
        }
        #[cfg(feature = "defi")]
        WriteCommand::Query(DatabaseQuery::DefiInstrument(instrument, migration, reply)) => {
            if !buffer.commands.is_empty() {
                drain_buffer(pool, trader_id, buffer).await;
            }

            let result = DatabaseQueries::add_defi_instrument(pool, instrument, migration).await;
            let _ = reply.send(result);
            return ControlFlow::Continue(());
        }
        WriteCommand::Flush(reply) => {
            drain_buffer(pool, trader_id, buffer).await;
            let result = DatabaseQueries::delete_trader(pool, trader_id).await;
            if result.is_ok() {
                buffer
                    .order_replacements
                    .retain(|(owner, _), _| owner != trader_id);
            }

            let _ = reply.send(result);
            return ControlFlow::Continue(());
        }
        WriteCommand::Query(DatabaseQuery::Close) => {
            drain_buffer(pool, trader_id, buffer).await;
            return ControlFlow::Break(());
        }
        other => other,
    };

    buffer.commands.push_back(msg);
    if buffer_interval.is_zero() {
        drain_buffer(pool, trader_id, buffer).await;
    }

    ControlFlow::Continue(())
}

async fn flush_buffer(
    buffer: &mut WriteBuffer,
    pool: &PgPool,
    trader_id: &TraderId,
    flush_timer: &mut Pin<&mut tokio::time::Sleep>,
    buffer_interval: Duration,
) {
    if !buffer.commands.is_empty() {
        drain_buffer(pool, trader_id, buffer).await;
    }
    flush_timer.as_mut().reset(Instant::now() + buffer_interval);
}

/// Retrieves a `PostgresCacheDatabase` for `trader_id` using default connection options.
///
/// # Errors
///
/// Returns an error if connecting to the database or initializing the cache adapter fails.
pub async fn get_pg_cache_database(trader_id: TraderId) -> anyhow::Result<PostgresCacheDatabase> {
    Ok(PostgresCacheDatabase::connect(PostgresCacheConfig::default(), trader_id).await?)
}

#[async_trait::async_trait]
impl CacheDatabaseAdapter for PostgresCacheDatabase {
    fn close(&mut self) -> anyhow::Result<()> {
        let pool = self.pool.clone();
        let (tx, rx) = std::sync::mpsc::channel();

        // The close command follows all pending writes on the FIFO channel and drains the buffer
        if let Err(e) = self.tx.send(DatabaseQuery::Close) {
            log::warn!("Error sending close: {e:?}");
        }

        log_task_awaiting("cache-write");

        tokio::task::block_in_place(|| {
            if let Err(e) = get_runtime().block_on(&mut self.handle) {
                log::error!("Error awaiting task 'cache-write': {e:?}");
            }
        });

        log::debug!("Closing connection pool");

        tokio::task::block_in_place(|| {
            get_runtime().block_on(async {
                pool.close().await;

                if let Err(e) = tx.send(()) {
                    log::error!("Error closing pool: {e:?}");
                }
            });
        });

        log::debug!("Closed");

        Ok(rx.recv()?)
    }

    fn flush(&mut self) -> anyhow::Result<()> {
        let trader_id = self.trader_id;
        block_on_nautilus_with(|| async {
            let (reply, result) = tokio::sync::oneshot::channel();
            self.tx.send(WriteCommand::Flush(reply))?;
            result.await?
        })
        .map_err(|e| anyhow::anyhow!("Failed to flush Postgres cache for {trader_id}: {e}"))
    }

    async fn load_all(&self) -> anyhow::Result<CacheMap> {
        let mut cache = self.load_cache_without_positions().await?;
        cache.positions = self.load_positions().await?;
        Ok(cache)
    }

    fn load_for_recovery(
        &self,
        positions_only: bool,
    ) -> LocalBoxFuture<'_, anyhow::Result<(AHashMap<String, Bytes>, CacheMap)>> {
        Box::pin(async move {
            let mut cache = if positions_only {
                CacheMap::default()
            } else {
                self.load_cache_without_positions().await?
            };

            let pool = self.pool.clone();
            let trader_id = self.trader_id;
            let (rows, positions) = tokio::spawn(async move {
                DatabaseQueries::load_positions_and_general(&pool, trader_id).await
            })
            .await??;

            cache.positions = positions
                .into_iter()
                .map(|position| (position.id, position))
                .collect();
            Ok((self.general_entries(rows), cache))
        })
    }

    fn load_order_indexes(&self) -> LocalBoxFuture<'_, anyhow::Result<CacheOrderIndexes>> {
        let pool = self.pool.clone();
        let trader_id = self.trader_id;
        Box::pin(async move {
            tokio::spawn(async move {
                let order_position =
                    DatabaseQueries::load_index_order_position(&pool, &trader_id).await?;
                let order_client =
                    DatabaseQueries::load_distinct_order_event_client_ids(&pool, &trader_id)
                        .await?;
                Ok(CacheOrderIndexes {
                    order_position,
                    order_client,
                })
            })
            .await?
        })
    }

    fn load(&self) -> anyhow::Result<AHashMap<String, Bytes>> {
        let pool = self.pool.clone();
        let trader_id = self.trader_id;
        let (reply, result) = std::sync::mpsc::sync_channel(1);

        tokio::spawn(async move {
            let rows = DatabaseQueries::load_cache_general(&pool, trader_id).await;
            let _ = reply.send(rows);
        });

        Ok(self.general_entries(result.recv()??))
    }

    async fn load_currencies(&self) -> anyhow::Result<AHashMap<Ustr, Currency>> {
        let currencies = DatabaseQueries::load_currencies(&self.pool).await?;
        Ok(currencies
            .into_iter()
            .map(|currency| (currency.code, currency))
            .collect())
    }

    async fn load_instruments(&self) -> anyhow::Result<AHashMap<InstrumentId, InstrumentAny>> {
        let instruments = DatabaseQueries::load_instruments(&self.pool).await?;
        Ok(instruments
            .into_iter()
            .map(|instrument| (instrument.id(), instrument))
            .collect())
    }

    async fn load_instrument_closes(
        &self,
    ) -> anyhow::Result<AHashMap<InstrumentId, InstrumentClose>> {
        Ok(DatabaseQueries::load_instrument_closes(&self.pool)
            .await?
            .into_iter()
            .map(|close| (close.instrument_id, close))
            .collect())
    }

    async fn load_synthetics(&self) -> anyhow::Result<AHashMap<InstrumentId, SyntheticInstrument>> {
        let prefix = format!("{CACHE_DATABASE_KEY_PREFIX}synthetic:");
        let values: Vec<Vec<u8>> =
            sqlx::query_scalar("SELECT value FROM general WHERE starts_with(id, $1)")
                .bind(prefix)
                .fetch_all(&self.pool)
                .await?;
        values
            .into_iter()
            .map(|value| {
                let synthetic: SyntheticInstrument = serde_json::from_slice(&value)?;
                Ok((synthetic.id, synthetic))
            })
            .collect()
    }

    async fn load_accounts(&self) -> anyhow::Result<AHashMap<AccountId, AccountAny>> {
        let pool = self.pool.clone();
        let trader_id = self.trader_id;
        let accounts =
            tokio::spawn(async move { DatabaseQueries::load_accounts(&pool, &trader_id).await })
                .await??;
        Ok(accounts
            .into_iter()
            .map(|account| (account.id(), account))
            .collect())
    }

    async fn load_orders(&self) -> anyhow::Result<AHashMap<ClientOrderId, OrderAny>> {
        let pool = self.pool.clone();
        let trader_id = self.trader_id;
        let orders =
            tokio::spawn(async move { DatabaseQueries::load_orders(&pool, &trader_id).await })
                .await??;
        Ok(orders
            .into_iter()
            .map(|order| (order.client_order_id(), order))
            .collect())
    }

    async fn load_positions(&self) -> anyhow::Result<AHashMap<PositionId, Position>> {
        let pool = self.pool.clone();
        let trader_id = self.trader_id;
        let positions =
            tokio::spawn(async move { DatabaseQueries::load_positions(&pool, &trader_id).await })
                .await??;
        Ok(positions
            .into_iter()
            .map(|position| (position.id, position))
            .collect())
    }

    async fn load_greeks(&self) -> anyhow::Result<AHashMap<InstrumentId, GreeksData>> {
        DatabaseQueries::load_greeks(&self.pool).await
    }

    async fn load_yield_curves(&self) -> anyhow::Result<AHashMap<String, YieldCurve>> {
        DatabaseQueries::load_yield_curves(&self.pool).await
    }

    fn load_index_order_position(&self) -> anyhow::Result<AHashMap<ClientOrderId, PositionId>> {
        let pool = self.pool.clone();
        let trader_id = self.trader_id;
        let (reply, result) = std::sync::mpsc::sync_channel(1);

        tokio::spawn(async move {
            let index = DatabaseQueries::load_index_order_position(&pool, &trader_id).await;
            let _ = reply.send(index);
        });

        result.recv()?
    }

    fn load_index_order_client(&self) -> anyhow::Result<AHashMap<ClientOrderId, ClientId>> {
        let pool = self.pool.clone();
        let trader_id = self.trader_id;
        let (reply, result) = std::sync::mpsc::sync_channel(1);

        tokio::spawn(async move {
            let index =
                DatabaseQueries::load_distinct_order_event_client_ids(&pool, &trader_id).await;
            let _ = reply.send(index);
        });

        result.recv()?
    }

    async fn load_currency(&self, code: &Ustr) -> anyhow::Result<Option<Currency>> {
        match DatabaseQueries::load_currency(&self.pool, code).await {
            Ok(items) => Ok(items),
            Err(e) => {
                #[cfg(feature = "defi")]
                if e.is::<crate::defi::IntegrityError>() {
                    return Err(e);
                }

                log::error!("Failed to load_currency: {e}");
                Ok(None)
            }
        }
    }

    async fn load_instrument(
        &self,
        instrument_id: &InstrumentId,
    ) -> anyhow::Result<Option<InstrumentAny>> {
        load_instrument(&self.pool, instrument_id).await
    }

    async fn load_synthetic(
        &self,
        instrument_id: &InstrumentId,
    ) -> anyhow::Result<Option<SyntheticInstrument>> {
        DatabaseQueries::load_synthetic(&self.pool, instrument_id).await
    }

    fn load_instrument_sync(
        &self,
        instrument_id: &InstrumentId,
    ) -> anyhow::Result<Option<InstrumentAny>> {
        let pool = self.pool.clone();
        let instrument_id = *instrument_id;
        block_on_nautilus_with(move || async move { load_instrument(&pool, &instrument_id).await })
    }

    fn load_synthetic_sync(
        &self,
        instrument_id: &InstrumentId,
    ) -> anyhow::Result<Option<SyntheticInstrument>> {
        let pool = self.pool.clone();
        let instrument_id = *instrument_id;
        block_on_nautilus_with(move || async move {
            DatabaseQueries::load_synthetic(&pool, &instrument_id).await
        })
    }

    async fn load_account(&self, account_id: &AccountId) -> anyhow::Result<Option<AccountAny>> {
        let pool = self.pool.clone();
        let trader_id = self.trader_id;
        let account_id = *account_id;
        let result = tokio::spawn(async move {
            DatabaseQueries::load_account(&pool, &account_id, &trader_id).await
        })
        .await?;

        match result {
            Ok(account) => Ok(account),
            Err(e) => {
                log::error!("Failed to load account {account_id}: {e:?}");
                Ok(None)
            }
        }
    }

    async fn load_order(
        &self,
        client_order_id: &ClientOrderId,
    ) -> anyhow::Result<Option<OrderAny>> {
        let pool = self.pool.clone();
        let trader_id = self.trader_id;
        let client_order_id = *client_order_id;
        let result = tokio::spawn(async move {
            DatabaseQueries::load_order(&pool, &client_order_id, &trader_id).await
        })
        .await?;

        match result {
            Ok(order) => Ok(order),
            Err(e) => {
                log::error!("Failed to load order {client_order_id}: {e:?}");
                Ok(None)
            }
        }
    }

    async fn load_position(&self, position_id: &PositionId) -> anyhow::Result<Option<Position>> {
        let pool = self.pool.clone();
        let trader_id = self.trader_id;
        let position_id = *position_id;
        tokio::spawn(async move {
            DatabaseQueries::load_position(&pool, &position_id, &trader_id).await
        })
        .await?
    }

    fn load_actor(&self, actor_id: &ActorId) -> anyhow::Result<AHashMap<String, Bytes>> {
        self.load_state(format!(
            "{}actor:{actor_id}",
            trader_key_prefix(self.trader_id)
        ))
    }

    fn delete_actor(&self, actor_id: &ActorId) -> anyhow::Result<()> {
        let query = DatabaseQuery::Delete(format!(
            "{}actor:{actor_id}",
            trader_key_prefix(self.trader_id)
        ));
        self.tx
            .send(query)
            .map_err(|e| anyhow::anyhow!("Failed to queue state deletion: {e}"))
    }

    fn load_strategy(&self, strategy_id: &StrategyId) -> anyhow::Result<AHashMap<String, Bytes>> {
        self.load_state(format!(
            "{}strategy:{strategy_id}",
            trader_key_prefix(self.trader_id)
        ))
    }

    fn delete_strategy(&self, strategy_id: &StrategyId) -> anyhow::Result<()> {
        let query = DatabaseQuery::Delete(format!(
            "{}strategy:{strategy_id}",
            trader_key_prefix(self.trader_id)
        ));
        self.tx
            .send(query)
            .map_err(|e| anyhow::anyhow!("Failed to queue state deletion: {e}"))
    }

    fn delete_order(&self, client_order_id: &ClientOrderId) -> anyhow::Result<()> {
        self.tx
            .send(DatabaseQuery::DeleteOrder(*client_order_id))
            .map_err(|e| anyhow::anyhow!("Failed to queue order deletion: {e}"))
    }

    fn delete_position(&self, position_id: &PositionId) -> anyhow::Result<()> {
        self.tx
            .send(DatabaseQuery::DeletePosition(*position_id))
            .map_err(|e| anyhow::anyhow!("Failed to queue position deletion: {e}"))
    }

    fn delete_account_event(&self, account_id: &AccountId, event_id: &str) -> anyhow::Result<()> {
        log::warn!(
            "Deleting account events is a no-op pending replay-safe compaction, {account_id}: {event_id}"
        );
        Ok(())
    }

    fn add(&self, key: String, value: Bytes) -> anyhow::Result<()> {
        anyhow::ensure!(
            !key.starts_with(CACHE_DATABASE_KEY_PREFIX)
                && !key.starts_with(POSITION_OMS_KEY_PREFIX),
            "Postgres cache keys cannot use the {CACHE_DATABASE_KEY_PREFIX} or {POSITION_OMS_KEY_PREFIX} prefixes"
        );

        self.write_general(
            format!("{}{key}", trader_general_key_prefix(self.trader_id)),
            value,
        )
    }

    fn add_currency(&self, currency: &Currency) -> anyhow::Result<()> {
        #[cfg(feature = "defi")]
        if nautilus_model::defi::currency::check(*currency)
            .map_err(crate::defi::integrity_error)?
            .is_some()
            || currency.code.contains('@')
        {
            return self.write_defi(|reply| DatabaseQuery::DefiCurrency(*currency, reply));
        }

        let query = DatabaseQuery::AddCurrency(*currency);
        self.tx.send(query).map_err(|e| {
            anyhow::anyhow!("Failed to query add_currency to database message handler: {e}")
        })
    }

    fn add_instrument(&self, instrument: &InstrumentAny) -> anyhow::Result<()> {
        #[cfg(feature = "defi")]
        {
            self.write_defi(|reply| DatabaseQuery::DefiInstrument(instrument.clone(), None, reply))
        }

        #[cfg(not(feature = "defi"))]
        {
            let query = DatabaseQuery::AddInstrument(instrument.clone());
            self.tx.send(query).map_err(|e| {
                anyhow::anyhow!(
                    "Failed to send query add_instrument to database message handler: {e}"
                )
            })
        }
    }

    #[cfg(feature = "defi")]
    fn add_defi_instrument(
        &self,
        instrument: &InstrumentAny,
        migration: InstrumentMigration,
    ) -> anyhow::Result<()> {
        self.write_defi(|reply| {
            DatabaseQuery::DefiInstrument(instrument.clone(), Some(migration), reply)
        })
    }

    fn add_instrument_close(&self, close: &InstrumentClose) -> anyhow::Result<()> {
        self.tx
            .send(DatabaseQuery::AddInstrumentClose(*close))
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to send query add_instrument_close to database message handler: {e}"
                )
            })
    }

    fn add_synthetic(&self, synthetic: &SyntheticInstrument) -> anyhow::Result<()> {
        self.write_general(
            format!("{CACHE_DATABASE_KEY_PREFIX}synthetic:{}", synthetic.id),
            Bytes::from(serde_json::to_vec(synthetic)?),
        )
    }

    fn add_account(&self, account: &AccountAny) -> anyhow::Result<()> {
        let query = DatabaseQuery::AddAccount(account_last_event(account)?, false);
        self.tx.send(query).map_err(|e| {
            anyhow::anyhow!("Failed to send query add_account to database message handler: {e}")
        })
    }

    fn add_order(&self, order: &OrderAny, client_id: Option<ClientId>) -> anyhow::Result<()> {
        let original = order.init_event();
        let command = match order.last_event() {
            OrderEventAny::Initialized(init) if init.event_id != original.event_id => {
                let prior_event_ids = order
                    .events()
                    .into_iter()
                    .rev()
                    .skip(1)
                    .map(|event| event.clone().into_boxed().id().to_string())
                    .collect();
                WriteCommand::OrderReplacement(OrderReplacement {
                    init: init.clone(),
                    client_id,
                    prior_event_ids,
                    first_update_id: None,
                })
            }
            _ => DatabaseQuery::AddOrder(original.clone(), client_id).into(),
        };

        self.tx.send(command).map_err(|e| {
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
        let query = if let Some(snapshot) = position_replay_snapshot(position) {
            DatabaseQuery::AddPositionWithSnapshot(position.id, event, snapshot)
        } else {
            DatabaseQuery::AddPosition(position.id, event)
        };

        self.tx.send(query).map_err(|e| {
            anyhow::anyhow!("Failed to send query add_position to database message handler: {e}")
        })
    }

    fn add_position_with_oms(&self, position: &Position, oms_type: OmsType) -> anyhow::Result<()> {
        let event = position_last_event(position)?;
        if position.trader_id != self.trader_id || event.trader_id != self.trader_id {
            anyhow::bail!(
                "Cannot persist position {} with OMS metadata for trader {}",
                position.id,
                self.trader_id,
            );
        }

        self.tx.send(WriteCommand::Position {
            id: position.id,
            event,
            snapshot: position_replay_snapshot(position),
            oms_type,
        })?;

        Ok(())
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
        anyhow::bail!("Postgres order book persistence is not supported")
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

    fn add_greeks(&self, greeks: &GreeksData) -> anyhow::Result<()> {
        anyhow::ensure!(
            [
                greeks.strike,
                greeks.expiry_in_years,
                greeks.multiplier,
                greeks.quantity,
                greeks.underlying_price,
                greeks.interest_rate,
                greeks.cost_of_carry,
                greeks.vol,
                greeks.pnl,
                greeks.price,
                greeks.delta,
                greeks.gamma,
                greeks.vega,
                greeks.theta,
                greeks.rho,
                greeks.itm_prob,
            ]
            .iter()
            .all(|value| value.is_finite()),
            "Cannot persist non-finite GreeksData"
        );
        self.tx
            .send(DatabaseQuery::AddGreeks(greeks.clone()))
            .map_err(|e| {
                anyhow::anyhow!("Failed to send query add_greeks to database message handler: {e}")
            })
    }

    fn add_yield_curve(&self, curve: &YieldCurve) -> anyhow::Result<()> {
        curve.validate()?;
        self.tx
            .send(DatabaseQuery::AddYieldCurve(curve.clone()))
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to send query add_yield_curve to database message handler: {e}"
                )
            })
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
        let trader_id = self.trader_id;
        let client_order_id = client_order_id.to_owned();
        let (tx, rx) = std::sync::mpsc::channel();

        tokio::spawn(async move {
            let result =
                DatabaseQueries::load_order_snapshot(&pool, &client_order_id, &trader_id).await;

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
        let trader_id = self.trader_id;
        let position_id = position_id.to_owned();
        let (tx, rx) = std::sync::mpsc::channel();

        tokio::spawn(async move {
            let result =
                DatabaseQueries::load_position_snapshot(&pool, &position_id, &trader_id).await;

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

    fn index_order_clients(&self, claims: &[(ClientOrderId, ClientId)]) -> anyhow::Result<()> {
        if claims.is_empty() {
            return Ok(());
        }

        let query = DatabaseQuery::IndexOrderClients(claims.to_vec());
        self.tx.send(query).map_err(|e| {
            anyhow::anyhow!(
                "Failed to send query index_order_clients to database message handler: {e}"
            )
        })
    }

    fn update_actor(
        &self,
        actor_id: &ActorId,
        state: &AHashMap<String, Bytes>,
    ) -> anyhow::Result<()> {
        self.write_general(
            format!("{}actor:{actor_id}", trader_key_prefix(self.trader_id)),
            Bytes::from(serde_json::to_vec(state)?),
        )
    }

    fn update_strategy(
        &self,
        strategy_id: &StrategyId,
        state: &AHashMap<String, Bytes>,
    ) -> anyhow::Result<()> {
        self.write_general(
            format!(
                "{}strategy:{strategy_id}",
                trader_key_prefix(self.trader_id)
            ),
            Bytes::from(serde_json::to_vec(state)?),
        )
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

    fn update_order_from_state(&self, order: &OrderAny) -> anyhow::Result<()> {
        let command = WriteCommand::OrderUpdate(order.last_event().clone(), order.init_id());
        self.tx.send(command).map_err(|e| {
            anyhow::anyhow!("Failed to send query update_order to database message handler: {e}")
        })
    }

    fn update_position(&self, position: &Position) -> anyhow::Result<()> {
        let query = if position.requires_replay_state() {
            DatabaseQuery::AddPositionSnapshot(PositionSnapshot::from_replay_state(position, None))
        } else {
            DatabaseQuery::UpdatePosition(position_last_event(position)?)
        };
        self.tx.send(query).map_err(|e| {
            anyhow::anyhow!("Failed to send query update_position to database message handler: {e}")
        })
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
        let mut snapshot = if position.requires_replay_state() {
            PositionSnapshot::from_replay_state(position, unrealized_pnl)
        } else {
            PositionSnapshot::from(position, unrealized_pnl)
        };
        snapshot.ts_init = ts_snapshot;
        self.add_position_snapshot(&snapshot)
    }

    fn heartbeat(&self, timestamp: UnixNanos) -> anyhow::Result<()> {
        self.write_general(
            format!("{}health:heartbeat", trader_key_prefix(self.trader_id)),
            Bytes::from(serde_json::to_vec(&timestamp)?),
        )
    }
}

async fn load_instrument(
    pool: &PgPool,
    instrument_id: &InstrumentId,
) -> anyhow::Result<Option<InstrumentAny>> {
    let mut currencies = DatabaseQueries::load_currencies(pool)
        .await?
        .into_iter()
        .map(|currency| (currency.code, currency))
        .collect();
    register_loaded_currencies(&mut currencies)?;
    DatabaseQueries::load_instrument(pool, instrument_id).await
}

fn account_last_event(account: &AccountAny) -> anyhow::Result<AccountState> {
    account
        .last_event()
        .ok_or_else(|| anyhow::anyhow!("Cannot persist account with no events: {}", account.id()))
}

fn position_last_event(position: &Position) -> anyhow::Result<OrderFilled> {
    position
        .last_event()
        .ok_or_else(|| anyhow::anyhow!("Cannot persist position with no events: {}", position.id))
}

fn position_replay_snapshot(position: &Position) -> Option<Box<PositionSnapshot>> {
    position
        .requires_replay_state()
        .then(|| Box::new(PositionSnapshot::from_replay_state(position, None)))
}

async fn drain_buffer(pool: &PgPool, trader_id: &TraderId, buffer: &mut WriteBuffer) {
    for command in buffer.commands.drain(..) {
        let result = match command {
            WriteCommand::OrderReplacement(replacement) => {
                // Expose the replacement only once a later event can commit with it.
                let key = (replacement.init.trader_id, replacement.init.client_order_id);
                if let Some(pending) = buffer.order_replacements.get_mut(&key)
                    && pending.init.event_id == replacement.init.event_id
                {
                    pending.client_id = replacement.client_id.or(pending.client_id);
                } else {
                    buffer.order_replacements.insert(key, replacement);
                }

                Ok(())
            }
            WriteCommand::OrderUpdate(event, init_id) => {
                write_order_update(pool, &mut buffer.order_replacements, event, Some(init_id)).await
            }
            WriteCommand::Query(query) => {
                execute_query(pool, trader_id, &mut buffer.order_replacements, query).await
            }
            WriteCommand::Position {
                id,
                event,
                snapshot,
                oms_type,
            } => {
                DatabaseQueries::replace_position(
                    pool,
                    id,
                    &event,
                    snapshot.map(|snapshot| *snapshot),
                    Some(oms_type),
                )
                .await
            }
            WriteCommand::Flush(_) => unreachable!("Flush runs before buffering"),
        };

        if let Err(e) = result {
            log::error!("Error on query: {e:?}");
        }
    }
}

async fn execute_query(
    pool: &PgPool,
    trader_id: &TraderId,
    order_replacements: &mut OrderReplacements,
    query: DatabaseQuery,
) -> anyhow::Result<()> {
    match query {
        #[cfg(feature = "defi")]
        DatabaseQuery::DefiCurrency(..) | DatabaseQuery::DefiInstrument(..) => {
            unreachable!("DeFi writes execute before buffering")
        }
        DatabaseQuery::Close => Ok(()),
        DatabaseQuery::Add(key, value) => DatabaseQueries::add(pool, key, value).await,
        DatabaseQuery::Delete(key) => DatabaseQueries::delete(pool, &key).await,
        DatabaseQuery::DeleteOrder(id) => {
            DatabaseQueries::delete_order(pool, *trader_id, id).await?;
            order_replacements.remove(&(*trader_id, id));
            Ok(())
        }
        DatabaseQuery::DeletePosition(id) => {
            DatabaseQueries::delete_position(pool, *trader_id, id).await
        }
        DatabaseQuery::AddCurrency(currency) => DatabaseQueries::add_currency(pool, currency).await,
        DatabaseQuery::AddInstrument(instrument) => {
            DatabaseQueries::add_instrument_any(pool, instrument).await
        }
        DatabaseQuery::AddInstrumentClose(close) => {
            DatabaseQueries::add_instrument_close(pool, &close).await
        }
        DatabaseQuery::AddOrder(event, client_id) => {
            DatabaseQueries::add_order(pool, event, client_id).await
        }
        DatabaseQuery::AddOrderSnapshot(snapshot) => {
            DatabaseQueries::add_order_snapshot(pool, snapshot).await
        }
        DatabaseQuery::AddPosition(position_id, event) => {
            DatabaseQueries::add_position(pool, position_id, &event).await
        }
        DatabaseQuery::AddPositionWithSnapshot(position_id, event, snapshot) => {
            DatabaseQueries::replace_position(pool, position_id, &event, Some(*snapshot), None)
                .await
        }
        DatabaseQuery::AddPositionSnapshot(snapshot) => {
            DatabaseQueries::add_position_snapshot(pool, snapshot).await
        }
        DatabaseQuery::AddAccount(event, updated) => {
            DatabaseQueries::add_account(pool, updated, event, trader_id).await
        }
        DatabaseQuery::AddSignal(signal) => DatabaseQueries::add_signal(pool, &signal).await,
        DatabaseQuery::AddCustom(data) => DatabaseQueries::add_custom_data(pool, &data).await,
        DatabaseQuery::AddQuote(quote) => DatabaseQueries::add_quote(pool, &quote).await,
        DatabaseQuery::AddTrade(trade) => DatabaseQueries::add_trade(pool, &trade).await,
        DatabaseQuery::AddBar(bar) => DatabaseQueries::add_bar(pool, &bar).await,
        DatabaseQuery::AddGreeks(greeks) => DatabaseQueries::add_greeks(pool, &greeks).await,
        DatabaseQuery::AddYieldCurve(curve) => DatabaseQueries::add_yield_curve(pool, &curve).await,
        DatabaseQuery::UpdateOrder(event) => {
            write_order_update(pool, order_replacements, event, None).await
        }
        DatabaseQuery::UpdatePosition(event) => {
            DatabaseQueries::update_position(pool, &event).await
        }
        DatabaseQuery::IndexOrderPosition(client_order_id, position_id) => {
            DatabaseQueries::index_order_position(pool, trader_id, client_order_id, position_id)
                .await
        }
        DatabaseQuery::IndexOrderClients(claims) => {
            DatabaseQueries::index_order_clients(pool, trader_id, &claims).await
        }
    }
}

async fn write_order_update(
    pool: &PgPool,
    order_replacements: &mut OrderReplacements,
    event: OrderEventAny,
    init_id: Option<UUID4>,
) -> anyhow::Result<()> {
    let key = (event.trader_id(), event.client_order_id());
    let event = event.into_boxed();

    let Some(replacement) = order_replacements.get_mut(&key) else {
        return DatabaseQueries::add_order_event(pool, event, None).await;
    };

    // Reload can restore the original order while this replacement is pending
    if init_id.is_some_and(|id| id != replacement.init.event_id) {
        order_replacements.remove(&key);
        return DatabaseQueries::add_order_event(pool, event, None).await;
    }

    let event_id = event.id();
    let first_update_id = replacement.first_update_id.get_or_insert(event_id);
    let client_order_id = key.1;
    anyhow::ensure!(
        *first_update_id == event_id,
        "Cannot persist order {client_order_id} event {event_id} before replacement event {first_update_id}"
    );

    DatabaseQueries::add_order_replacement(
        pool,
        replacement.init.clone(),
        replacement.client_id,
        &replacement.prior_event_ids,
        event,
    )
    .await?;
    order_replacements.remove(&key);
    Ok(())
}
