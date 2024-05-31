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

use nautilus_model::{
    identifiers::{client_order_id::ClientOrderId, instrument_id::InstrumentId},
    instruments::any::InstrumentAny,
    orders::any::OrderAny,
    types::currency::Currency,
};
use sqlx::{postgres::PgConnectOptions, PgPool};
use tokio::{
    sync::mpsc::{channel, error::TryRecvError, Receiver, Sender},
    time::sleep,
};

use crate::sql::{
    models::general::GeneralRow,
    pg::{connect_pg, get_postgres_connect_options},
    queries::DatabaseQueries,
};

#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.infrastructure")
)]
pub struct PostgresCacheDatabase {
    pub pool: PgPool,
    tx: Sender<DatabaseQuery>,
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
        let (tx, rx) = channel::<DatabaseQuery>(1000);
        // spawn a thread to handle messages
        let _join_handle = tokio::spawn(async move {
            PostgresCacheDatabase::handle_message(rx, pg_connect_options.clone().into()).await;
        });
        Ok(PostgresCacheDatabase { pool, tx })
    }

    async fn handle_message(mut rx: Receiver<DatabaseQuery>, pg_connect_options: PgConnectOptions) {
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
                    cache.insert(row.key, row.value);
                }
                Ok(cache)
            }
            Err(e) => {
                panic!("Failed to load general table: {e}")
            }
        }
    }

    pub async fn add(&self, key: String, value: Vec<u8>) -> anyhow::Result<()> {
        let query = DatabaseQuery::Add(key, value);
        self.tx.send(query).await.map_err(|err| {
            anyhow::anyhow!("Failed to send query to database message handler: {err}")
        })
    }

    pub async fn add_currency(&self, currency: Currency) -> anyhow::Result<()> {
        let query = DatabaseQuery::AddCurrency(currency);
        self.tx.send(query).await.map_err(|err| {
            anyhow::anyhow!("Failed to query add_currency to database message handler: {err}")
        })
    }

    pub async fn load_currencies(&self) -> anyhow::Result<Vec<Currency>> {
        DatabaseQueries::load_currencies(&self.pool).await
    }

    pub async fn load_currency(&self, code: &str) -> anyhow::Result<Option<Currency>> {
        DatabaseQueries::load_currency(&self.pool, code).await
    }

    pub async fn add_instrument(&self, instrument: InstrumentAny) -> anyhow::Result<()> {
        let query = DatabaseQuery::AddInstrument(instrument);
        self.tx.send(query).await.map_err(|err| {
            anyhow::anyhow!(
                "Failed to send query add_instrument to database message handler: {err}"
            )
        })
    }

    pub async fn load_instrument(
        &self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<Option<InstrumentAny>> {
        DatabaseQueries::load_instrument(&self.pool, instrument_id).await
    }

    pub async fn load_instruments(&self) -> anyhow::Result<Vec<InstrumentAny>> {
        DatabaseQueries::load_instruments(&self.pool).await
    }

    pub async fn add_order(&self, order: OrderAny) -> anyhow::Result<()> {
        let query = DatabaseQuery::AddOrder(order, false);
        self.tx.send(query).await.map_err(|err| {
            anyhow::anyhow!("Failed to send query add_order to database message handler: {err}")
        })
    }

    pub async fn update_order(&self, order: OrderAny) -> anyhow::Result<()> {
        let query = DatabaseQuery::AddOrder(order, true);
        self.tx.send(query).await.map_err(|err| {
            anyhow::anyhow!("Failed to send query add_order to database message handler: {err}")
        })
    }

    pub async fn load_order(
        &self,
        client_order_id: &ClientOrderId,
    ) -> anyhow::Result<Option<OrderAny>> {
        DatabaseQueries::load_order(&self.pool, client_order_id).await
    }
}
