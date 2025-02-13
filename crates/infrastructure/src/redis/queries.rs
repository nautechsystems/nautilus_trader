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

use std::{collections::HashMap, str::FromStr};

use futures::{future::join_all, StreamExt};
use nautilus_common::cache::database::CacheMap;
use nautilus_model::{
    accounts::AccountAny,
    identifiers::{AccountId, ClientOrderId, InstrumentId, PositionId},
    instruments::{InstrumentAny, SyntheticInstrument},
    orders::OrderAny,
    position::Position,
    types::Currency,
};
use redis::{aio::ConnectionManager, AsyncCommands};
use tokio::try_join;
use ustr::Ustr;

// Collection keys
const _INDEX: &str = "index";
const _GENERAL: &str = "general";
const CURRENCIES: &str = "currencies";
const INSTRUMENTS: &str = "instruments";
const SYNTHETICS: &str = "synthetics";
const ACCOUNTS: &str = "accounts";
const ORDERS: &str = "orders";
const POSITIONS: &str = "positions";
const _ACTORS: &str = "actors";
const _STRATEGIES: &str = "strategies";
const _SNAPSHOTS: &str = "snapshots";
const _HEALTH: &str = "health";

pub struct DatabaseQueries;

impl DatabaseQueries {
    pub async fn load_all(con: &ConnectionManager) -> anyhow::Result<CacheMap> {
        let (currencies, instruments, synthetics, accounts, orders, positions) = try_join!(
            Self::load_currencies(con),
            Self::load_instruments(con),
            Self::load_synthetics(con),
            Self::load_accounts(con),
            Self::load_orders(con),
            Self::load_positions(con)
        )
        .map_err(|e| anyhow::anyhow!("Error loading cache data: {e}"))?;

        Ok(CacheMap {
            currencies,
            instruments,
            synthetics,
            accounts,
            orders,
            positions,
        })
    }

    pub async fn load_currencies(
        con: &ConnectionManager,
    ) -> anyhow::Result<HashMap<Ustr, Currency>> {
        let mut currencies = HashMap::new();
        let pattern = format!("{CURRENCIES}*");
        tracing::debug!("Loading {pattern}");

        let mut con = con.clone();
        let keys = Self::scan_keys(&mut con, pattern).await?;

        let futures: Vec<_> = keys
            .iter()
            .map(|key| {
                let con = con.clone();
                async move {
                    let currency_code = match key.as_str().rsplit(':').next() {
                        Some(code) => Ustr::from(code),
                        None => {
                            log::error!("Invalid key format: {key}");
                            return None;
                        }
                    };

                    match Self::load_currency(&con, &currency_code) {
                        Ok(Some(currency)) => Some((currency_code, currency)),
                        Ok(None) => {
                            log::error!("Currency not found: {currency_code}");
                            None
                        }
                        Err(e) => {
                            log::error!("Failed to load currency {currency_code}: {e}");
                            None
                        }
                    }
                }
            })
            .collect();

        // Insert all Currency_code (key) and Currency (value) into the HashMap, filtering out None values.
        currencies.extend(join_all(futures).await.into_iter().flatten());
        tracing::debug!("Loaded {} currencies(s)", currencies.len());

        Ok(currencies)
    }

    pub async fn load_instruments(
        con: &ConnectionManager,
    ) -> anyhow::Result<HashMap<InstrumentId, InstrumentAny>> {
        let mut instruments = HashMap::new();
        let pattern = format!("{INSTRUMENTS}*");
        tracing::debug!("Loading {pattern}");

        let mut con = con.clone();
        let keys = Self::scan_keys(&mut con, pattern).await?;

        let futures: Vec<_> = keys
            .iter()
            .map(|key| {
                let con = con.clone();
                async move {
                    let instrument_id = key
                        .as_str()
                        .rsplit(':')
                        .next()
                        .ok_or_else(|| {
                            log::error!("Invalid key format: {key}");
                            "Invalid key format"
                        })
                        .and_then(|code| {
                            InstrumentId::from_str(code).map_err(|e| {
                                log::error!("Failed to convert to InstrumentId for {key}: {e}");
                                "Invalid instrument ID"
                            })
                        });

                    let instrument_id = match instrument_id {
                        Ok(id) => id,
                        Err(_) => return None,
                    };

                    match Self::load_instrument(&con, &instrument_id) {
                        Ok(Some(instrument)) => Some((instrument_id, instrument)),
                        Ok(None) => {
                            log::error!("Instrument not found: {instrument_id}");
                            None
                        }
                        Err(e) => {
                            log::error!("Failed to load instrument {instrument_id}: {e}");
                            None
                        }
                    }
                }
            })
            .collect();

        // Insert all Instrument_id (key) and Instrument (value) into the HashMap, filtering out None values.
        instruments.extend(join_all(futures).await.into_iter().flatten());
        tracing::debug!("Loaded {} instruments(s)", instruments.len());

        Ok(instruments)
    }

    pub async fn load_synthetics(
        con: &ConnectionManager,
    ) -> anyhow::Result<HashMap<InstrumentId, SyntheticInstrument>> {
        let mut synthetics = HashMap::new();
        let pattern = format!("{SYNTHETICS}*");
        tracing::debug!("Loading {pattern}");

        let mut con = con.clone();
        let keys = Self::scan_keys(&mut con, pattern).await?;

        let futures: Vec<_> = keys
            .iter()
            .map(|key| {
                let con = con.clone();
                async move {
                    let instrument_id = key
                        .as_str()
                        .rsplit(':')
                        .next()
                        .ok_or_else(|| {
                            log::error!("Invalid key format: {key}");
                            "Invalid key format"
                        })
                        .and_then(|code| {
                            InstrumentId::from_str(code).map_err(|e| {
                                log::error!("Failed to parse InstrumentId for {key}: {e}");
                                "Invalid instrument ID"
                            })
                        });

                    let instrument_id = match instrument_id {
                        Ok(id) => id,
                        Err(_) => return None,
                    };

                    match Self::load_synthetic(&con, &instrument_id) {
                        Ok(synthetic) => Some((instrument_id, synthetic)),
                        Err(e) => {
                            log::error!("Failed to load synthetic {instrument_id}: {e}");
                            None
                        }
                    }
                }
            })
            .collect();

        // Insert all Instrument_id (key) and Synthetic (value) into the HashMap, filtering out None values.
        synthetics.extend(join_all(futures).await.into_iter().flatten());
        tracing::debug!("Loaded {} synthetics(s)", synthetics.len());

        Ok(synthetics)
    }

    pub async fn load_accounts(
        con: &ConnectionManager,
    ) -> anyhow::Result<HashMap<AccountId, AccountAny>> {
        let mut accounts = HashMap::new();
        let pattern = format!("{ACCOUNTS}*");
        tracing::debug!("Loading {pattern}");

        let mut con = con.clone();
        let keys = Self::scan_keys(&mut con, pattern).await?;

        let futures: Vec<_> = keys
            .iter()
            .map(|key| {
                let con = con.clone();
                async move {
                    let account_id = match key.as_str().rsplit(':').next() {
                        Some(code) => AccountId::from(code),
                        None => {
                            log::error!("Invalid key format: {key}");
                            return None;
                        }
                    };

                    match Self::load_account(&con, &account_id) {
                        Ok(Some(account)) => Some((account_id, account)),
                        Ok(None) => {
                            log::error!("Account not found: {account_id}");
                            None
                        }
                        Err(e) => {
                            log::error!("Failed to load account {account_id}: {e}");
                            None
                        }
                    }
                }
            })
            .collect();

        // Insert all Account_id (key) and Account (value) into the HashMap, filtering out None values.
        accounts.extend(join_all(futures).await.into_iter().flatten());
        tracing::debug!("Loaded {} accounts(s)", accounts.len());

        Ok(accounts)
    }

    pub async fn load_orders(
        con: &ConnectionManager,
    ) -> anyhow::Result<HashMap<ClientOrderId, OrderAny>> {
        let mut orders = HashMap::new();
        let pattern = format!("{ORDERS}*");
        tracing::debug!("Loading {pattern}");

        let mut con = con.clone();
        let keys = Self::scan_keys(&mut con, pattern).await?;

        let futures: Vec<_> = keys
            .iter()
            .map(|key| {
                let con = con.clone();
                async move {
                    let client_order_id = match key.as_str().rsplit(':').next() {
                        Some(code) => ClientOrderId::from(code),
                        None => {
                            log::error!("Invalid key format: {key}");
                            return None;
                        }
                    };

                    match Self::load_order(&con, &client_order_id) {
                        Ok(Some(order)) => Some((client_order_id, order)),
                        Ok(None) => {
                            log::error!("Order not found: {client_order_id}");
                            None
                        }
                        Err(e) => {
                            log::error!("Failed to load order {client_order_id}: {e}");
                            None
                        }
                    }
                }
            })
            .collect();

        // Insert all Client-Order-Id (key) and Order (value) into the HashMap, filtering out None values.
        orders.extend(join_all(futures).await.into_iter().flatten());
        tracing::debug!("Loaded {} order(s)", orders.len());

        Ok(orders)
    }

    pub async fn load_positions(
        con: &ConnectionManager,
    ) -> anyhow::Result<HashMap<PositionId, Position>> {
        let mut positions = HashMap::new();
        let pattern = format!("{POSITIONS}*");
        tracing::debug!("Loading {pattern}");

        let mut con = con.clone();
        let keys = Self::scan_keys(&mut con, pattern).await?;

        let futures: Vec<_> = keys
            .iter()
            .map(|key| {
                let con = con.clone();
                async move {
                    let position_id = match key.as_str().rsplit(':').next() {
                        Some(code) => PositionId::from(code),
                        None => {
                            log::error!("Invalid key format: {key}");
                            return None;
                        }
                    };

                    match Self::load_position(&con, &position_id) {
                        Ok(position) => Some((position_id, position)),
                        Err(e) => {
                            log::error!("Failed to load position {position_id}: {e}");
                            None
                        }
                    }
                }
            })
            .collect();

        // Insert all Position_id (key) and Position (value) into the HashMap, filtering out None values.
        positions.extend(join_all(futures).await.into_iter().flatten());
        tracing::debug!("Loaded {} position(s)", positions.len());

        Ok(positions)
    }

    pub fn load_currency(
        _con: &ConnectionManager,
        _code: &Ustr,
    ) -> anyhow::Result<Option<Currency>> {
        todo!()
    }

    pub fn load_instrument(
        _con: &ConnectionManager,
        _instrument_id: &InstrumentId,
    ) -> anyhow::Result<Option<InstrumentAny>> {
        todo!()
    }

    pub fn load_synthetic(
        _con: &ConnectionManager,
        _instrument_id: &InstrumentId,
    ) -> anyhow::Result<SyntheticInstrument> {
        todo!()
    }

    pub fn load_account(
        _con: &ConnectionManager,
        _account_id: &AccountId,
    ) -> anyhow::Result<Option<AccountAny>> {
        todo!()
    }

    pub fn load_order(
        _con: &ConnectionManager,
        _client_order_id: &ClientOrderId,
    ) -> anyhow::Result<Option<OrderAny>> {
        todo!()
    }

    pub fn load_position(
        _con: &ConnectionManager,
        _position_id: &PositionId,
    ) -> anyhow::Result<Position> {
        todo!()
    }

    pub async fn scan_keys(
        con: &mut ConnectionManager,
        pattern: String,
    ) -> anyhow::Result<Vec<String>> {
        Ok(con
            .scan_match::<String, String>(pattern)
            .await?
            .collect()
            .await)
    }
}
