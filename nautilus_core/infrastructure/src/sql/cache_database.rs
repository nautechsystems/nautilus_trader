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
    time::{Duration, Instant},
};

use log::error;
use nautilus_common::cache::database::CacheDatabaseAdapter;
use nautilus_core::nanos::UnixNanos;
use nautilus_model::{
    account::any::AccountAny,
    identifiers::{
        AccountId, ClientId, ClientOrderId, ComponentId, InstrumentId, PositionId, StrategyId,
        VenueOrderId,
    },
    instruments::{any::InstrumentAny, synthetic::SyntheticInstrument},
    orders::any::OrderAny,
    position::Position,
    types::currency::Currency,
};
use sqlx::{postgres::PgConnectOptions, PgPool};
use tokio::{
    sync::mpsc::{error::TryRecvError, unbounded_channel, UnboundedReceiver, UnboundedSender},
    time::sleep,
};
use ustr::Ustr;

use crate::sql::{
    models::general::GeneralRow,
    pg::{
        connect_pg, delete_nautilus_postgres_tables, get_postgres_connect_options,
        PostgresConnectOptions, PostgresConnectOptionsBuilder,
    },
    queries::DatabaseQueries,
};

#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.infrastructure")
)]
pub struct PostgresCacheDatabase {
    pub pool: PgPool,
    tx: UnboundedSender<DatabaseQuery>,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum DatabaseQuery {
    Add(String, Vec<u8>),
    AddCurrency(Currency),
    AddInstrument(InstrumentAny),
    AddOrder(OrderAny, bool),
}

fn get_buffer_interval() -> Duration {
    Duration::from_millis(0)
}

async fn drain_buffer(pool: &PgPool, buffer: &mut VecDeque<DatabaseQuery>) {
    for cmd in buffer.drain(..) {
        match cmd {
            DatabaseQuery::Add(key, value) => {
                DatabaseQueries::add(pool, key, value).await.unwrap();
            }
            DatabaseQuery::AddCurrency(currency) => {
                DatabaseQueries::add_currency(pool, currency).await.unwrap();
            }
            DatabaseQuery::AddInstrument(instrument_any) => match instrument_any {
                InstrumentAny::CryptoFuture(instrument) => {
                    DatabaseQueries::add_instrument(pool, "CRYPTO_FUTURE", Box::new(instrument))
                        .await
                        .unwrap()
                }
                InstrumentAny::CryptoPerpetual(instrument) => {
                    DatabaseQueries::add_instrument(pool, "CRYPTO_PERPETUAL", Box::new(instrument))
                        .await
                        .unwrap()
                }
                InstrumentAny::CurrencyPair(instrument) => {
                    DatabaseQueries::add_instrument(pool, "CURRENCY_PAIR", Box::new(instrument))
                        .await
                        .unwrap()
                }
                InstrumentAny::Equity(equity) => {
                    DatabaseQueries::add_instrument(pool, "EQUITY", Box::new(equity))
                        .await
                        .unwrap()
                }
                InstrumentAny::FuturesContract(instrument) => {
                    DatabaseQueries::add_instrument(pool, "FUTURES_CONTRACT", Box::new(instrument))
                        .await
                        .unwrap()
                }
                InstrumentAny::FuturesSpread(instrument) => {
                    DatabaseQueries::add_instrument(pool, "FUTURES_SPREAD", Box::new(instrument))
                        .await
                        .unwrap()
                }
                InstrumentAny::OptionsContract(instrument) => {
                    DatabaseQueries::add_instrument(pool, "OPTIONS_CONTRACT", Box::new(instrument))
                        .await
                        .unwrap()
                }
                InstrumentAny::OptionsSpread(instrument) => {
                    DatabaseQueries::add_instrument(pool, "OPTIONS_SPREAD", Box::new(instrument))
                        .await
                        .unwrap()
                }
            },
            DatabaseQuery::AddOrder(order_any, updated) => match order_any {
                OrderAny::Limit(order) => {
                    DatabaseQueries::add_order(pool, "LIMIT", updated, Box::new(order))
                        .await
                        .unwrap()
                }
                OrderAny::LimitIfTouched(order) => {
                    DatabaseQueries::add_order(pool, "LIMIT_IF_TOUCHED", updated, Box::new(order))
                        .await
                        .unwrap()
                }
                OrderAny::Market(order) => {
                    DatabaseQueries::add_order(pool, "MARKET", updated, Box::new(order))
                        .await
                        .unwrap()
                }
                OrderAny::MarketIfTouched(order) => {
                    DatabaseQueries::add_order(pool, "MARKET_IF_TOUCHED", updated, Box::new(order))
                        .await
                        .unwrap()
                }
                OrderAny::MarketToLimit(order) => {
                    DatabaseQueries::add_order(pool, "MARKET_TO_LIMIT", updated, Box::new(order))
                        .await
                        .unwrap()
                }
                OrderAny::StopLimit(order) => {
                    DatabaseQueries::add_order(pool, "STOP_LIMIT", updated, Box::new(order))
                        .await
                        .unwrap()
                }
                OrderAny::StopMarket(order) => {
                    DatabaseQueries::add_order(pool, "STOP_MARKET", updated, Box::new(order))
                        .await
                        .unwrap()
                }
                OrderAny::TrailingStopLimit(order) => DatabaseQueries::add_order(
                    pool,
                    "TRAILING_STOP_LIMIT",
                    updated,
                    Box::new(order),
                )
                .await
                .unwrap(),
                OrderAny::TrailingStopMarket(order) => DatabaseQueries::add_order(
                    pool,
                    "TRAILING_STOP_MARKET",
                    updated,
                    Box::new(order),
                )
                .await
                .unwrap(),
            },
        }
    }
}

impl PostgresCacheDatabase {
    pub async fn connect(
        host: Option<String>,
        port: Option<u16>,
        username: Option<String>,
        password: Option<String>,
        database: Option<String>,
    ) -> Result<Self, sqlx::Error> {
        let pg_connect_options =
            get_postgres_connect_options(host, port, username, password, database).unwrap();
        let pool = connect_pg(pg_connect_options.clone().into()).await.unwrap();
        let (tx, rx) = unbounded_channel::<DatabaseQuery>();
        // spawn a thread to handle messages
        let _join_handle = tokio::spawn(async move {
            PostgresCacheDatabase::handle_message(rx, pg_connect_options.clone().into()).await;
        });
        Ok(PostgresCacheDatabase { pool, tx })
    }

    async fn handle_message(
        mut rx: UnboundedReceiver<DatabaseQuery>,
        pg_connect_options: PgConnectOptions,
    ) {
        let pool = connect_pg(pg_connect_options).await.unwrap();
        // Buffering
        let mut buffer: VecDeque<DatabaseQuery> = VecDeque::new();
        let mut last_drain = Instant::now();
        let buffer_interval = get_buffer_interval();
        let recv_interval = Duration::from_millis(1);

        loop {
            if last_drain.elapsed() >= buffer_interval && !buffer.is_empty() {
                // drain buffer
                drain_buffer(&pool, &mut buffer).await;
                last_drain = Instant::now();
            } else {
                // Continue to receive and handle messages until channel is hung up
                match rx.try_recv() {
                    Ok(msg) => buffer.push_back(msg),
                    Err(TryRecvError::Empty) => sleep(recv_interval).await,
                    Err(TryRecvError::Disconnected) => break,
                }
            }
        }
        // rain any remaining message
        if !buffer.is_empty() {
            drain_buffer(&pool, &mut buffer).await;
        }
    }

    pub async fn load(&self) -> Result<HashMap<String, Vec<u8>>, sqlx::Error> {
        let query = sqlx::query_as::<_, GeneralRow>("SELECT * FROM general");
        let result = query.fetch_all(&self.pool).await;
        match result {
            Ok(rows) => {
                let mut cache: HashMap<String, Vec<u8>> = HashMap::new();
                for row in rows {
                    cache.insert(row.id, row.value);
                }
                Ok(cache)
            }
            Err(e) => {
                panic!("Failed to load general table: {e}")
            }
        }
    }
}

pub async fn reset_pg_database(pg_options: Option<PostgresConnectOptions>) -> anyhow::Result<()> {
    let pg_connect_options = pg_options.unwrap_or(
        PostgresConnectOptionsBuilder::default()
            .username(String::from("postgres"))
            .build()?,
    );
    let pg_pool = connect_pg(pg_connect_options.into()).await.unwrap();
    delete_nautilus_postgres_tables(&pg_pool).await.unwrap();
    Ok(())
}

pub async fn get_pg_cache_database() -> anyhow::Result<PostgresCacheDatabase> {
    reset_pg_database(None).await.unwrap();
    // run tests as nautilus user
    let connect_options = PostgresConnectOptionsBuilder::default()
        .username(String::from("nautilus"))
        .build()?;
    Ok(PostgresCacheDatabase::connect(
        Some(connect_options.host),
        Some(connect_options.port),
        Some(connect_options.username),
        Some(connect_options.password),
        Some(connect_options.database),
    )
    .await
    .unwrap())
}

#[allow(dead_code)]
#[allow(unused)]
impl CacheDatabaseAdapter for PostgresCacheDatabase {
    fn close(&mut self) -> anyhow::Result<()> {
        todo!()
    }

    fn flush(&mut self) -> anyhow::Result<()> {
        todo!()
    }

    fn load(&mut self) -> anyhow::Result<HashMap<String, Vec<u8>>> {
        todo!()
    }

    fn load_currencies(&mut self) -> anyhow::Result<HashMap<Ustr, Currency>> {
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
                    let _ = tx.send(mapping);
                }
                Err(e) => {
                    error!("Failed to load currencies: {:?}", e);
                    let _ = tx.send(HashMap::new());
                }
            }
        });
        Ok(rx.recv().unwrap())
    }

    fn load_instruments(&mut self) -> anyhow::Result<HashMap<InstrumentId, InstrumentAny>> {
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
                    let _ = tx.send(mapping);
                }
                Err(e) => {
                    error!("Failed to load instruments: {:?}", e);
                    let _ = tx.send(HashMap::new());
                }
            }
        });
        Ok(rx.recv().unwrap())
    }

    fn load_synthetics(&mut self) -> anyhow::Result<HashMap<InstrumentId, SyntheticInstrument>> {
        todo!()
    }

    fn load_accounts(&mut self) -> anyhow::Result<HashMap<AccountId, AccountAny>> {
        todo!()
    }

    fn load_orders(&mut self) -> anyhow::Result<HashMap<ClientOrderId, OrderAny>> {
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
                    let _ = tx.send(mapping);
                }
                Err(e) => {
                    error!("Failed to load orders: {:?}", e);
                    let _ = tx.send(HashMap::new());
                }
            }
        });
        Ok(rx.recv().unwrap())
    }

    fn load_positions(&mut self) -> anyhow::Result<HashMap<PositionId, Position>> {
        todo!()
    }

    fn load_index_order_position(&mut self) -> anyhow::Result<HashMap<ClientOrderId, Position>> {
        todo!()
    }

    fn load_index_order_client(&mut self) -> anyhow::Result<HashMap<ClientOrderId, ClientId>> {
        todo!()
    }

    fn load_currency(&mut self, code: &Ustr) -> anyhow::Result<Option<Currency>> {
        let pool = self.pool.clone();
        let code = code.to_owned(); // Clone the code
        let (tx, rx) = std::sync::mpsc::channel();
        tokio::spawn(async move {
            let result = DatabaseQueries::load_currency(&pool, &code).await;
            match result {
                Ok(currency) => {
                    let _ = tx.send(currency);
                }
                Err(e) => {
                    error!("Failed to load currency {}: {:?}", code, e);
                    let _ = tx.send(None);
                }
            }
        });
        let res = rx.recv().unwrap();
        Ok(res)
    }

    fn load_instrument(
        &mut self,
        instrument_id: &InstrumentId,
    ) -> anyhow::Result<Option<InstrumentAny>> {
        let pool = self.pool.clone();
        let instrument_id = instrument_id.to_owned(); // Clone the instrument_id
        let (tx, rx) = std::sync::mpsc::channel();
        tokio::spawn(async move {
            let result = DatabaseQueries::load_instrument(&pool, &instrument_id).await;
            match result {
                Ok(instrument) => {
                    let _ = tx.send(instrument);
                }
                Err(e) => {
                    error!("Failed to load instrument {}: {:?}", instrument_id, e);
                    let _ = tx.send(None);
                }
            }
        });
        Ok(rx.recv().unwrap())
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
        let pool = self.pool.clone();
        let client_order_id = client_order_id.to_owned();
        let (tx, rx) = std::sync::mpsc::channel();
        tokio::spawn(async move {
            let result = DatabaseQueries::load_order(&pool, &client_order_id).await;
            match result {
                Ok(order) => {
                    let _ = tx.send(order);
                }
                Err(e) => {
                    error!("Failed to load order {}: {:?}", client_order_id, e);
                    let _ = tx.send(None);
                }
            }
        });
        Ok(rx.recv().unwrap())
    }

    fn load_position(&mut self, position_id: &PositionId) -> anyhow::Result<Position> {
        todo!()
    }

    fn load_actor(
        &mut self,
        component_id: &ComponentId,
    ) -> anyhow::Result<HashMap<String, Vec<u8>>> {
        todo!()
    }

    fn delete_actor(&mut self, component_id: &ComponentId) -> anyhow::Result<()> {
        todo!()
    }

    fn load_strategy(
        &mut self,
        strategy_id: &StrategyId,
    ) -> anyhow::Result<HashMap<String, Vec<u8>>> {
        todo!()
    }

    fn delete_strategy(&mut self, component_id: &StrategyId) -> anyhow::Result<()> {
        todo!()
    }

    fn add(&mut self, key: String, value: Vec<u8>) -> anyhow::Result<()> {
        let query = DatabaseQuery::Add(key, value);
        self.tx.send(query).map_err(|err| {
            anyhow::anyhow!("Failed to send query to database message handler: {err}")
        })
    }

    fn add_currency(&mut self, currency: &Currency) -> anyhow::Result<()> {
        let query = DatabaseQuery::AddCurrency(*currency);
        self.tx.send(query).map_err(|err| {
            anyhow::anyhow!("Failed to query add_currency to database message handler: {err}")
        })
    }

    fn add_instrument(&mut self, instrument: &InstrumentAny) -> anyhow::Result<()> {
        let query = DatabaseQuery::AddInstrument(instrument.clone());
        self.tx.send(query).map_err(|err| {
            anyhow::anyhow!(
                "Failed to send query add_instrument to database message handler: {err}"
            )
        })
    }

    fn add_synthetic(&mut self, synthetic: &SyntheticInstrument) -> anyhow::Result<()> {
        todo!()
    }

    fn add_account(&mut self, account: &AccountAny) -> anyhow::Result<()> {
        todo!()
    }

    fn add_order(&mut self, order: &OrderAny) -> anyhow::Result<()> {
        let query = DatabaseQuery::AddOrder(order.clone(), false);
        self.tx.send(query).map_err(|err| {
            anyhow::anyhow!("Failed to send query add_order to database message handler: {err}")
        })
    }

    fn add_position(&mut self, position: &Position) -> anyhow::Result<()> {
        todo!()
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
        let query = DatabaseQuery::AddOrder(order.clone(), true);
        self.tx.send(query).map_err(|err| {
            anyhow::anyhow!("Failed to send query add_order to database message handler: {err}")
        })
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
