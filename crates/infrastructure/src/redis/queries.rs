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

use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures::future::join_all;
use indexmap::IndexMap;
use nautilus_common::{cache::database::CacheMap, enums::SerializationEncoding};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    accounts::AccountAny,
    enums::{ContingencyType, OrderSide, OrderType, TimeInForce, TrailingOffsetType, TriggerType},
    events::{OrderEventAny, OrderFilled, OrderInitialized},
    identifiers::{
        AccountId, ClientOrderId, ExecAlgorithmId, InstrumentId, OrderListId, PositionId,
        StrategyId, TraderId,
    },
    instruments::{InstrumentAny, SyntheticInstrument},
    orders::{LimitOrder, MarketOrder, OrderAny},
    position::Position,
    types::{Currency, Price, Quantity},
};
use redis::{AsyncCommands, aio::ConnectionManager};
use rust_decimal::Decimal;
use serde::Serialize;
use serde_json::Value;
use tokio::try_join;
use ustr::Ustr;

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
const REDIS_DELIMITER: char = ':';

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

pub struct DatabaseQueries;

impl DatabaseQueries {
    pub fn serialize_payload<T: Serialize>(
        encoding: SerializationEncoding,
        payload: &T,
    ) -> anyhow::Result<Vec<u8>> {
        let mut value = serde_json::to_value(payload)?;
        convert_timestamps(&mut value);
        match encoding {
            SerializationEncoding::MsgPack => rmp_serde::to_vec(&value).map_err(|e| {
                anyhow::anyhow!(
                    "Failed to serialize msgpack `payload` for {}: {e}",
                    std::any::type_name::<T>()
                )
            }),
            SerializationEncoding::Json => serde_json::to_vec(&value).map_err(|e| {
                anyhow::anyhow!(
                    "Failed to serialize json `payload` for {}: {e}",
                    std::any::type_name::<T>()
                )
            }),
        }
    }

    pub fn deserialize_payload<T>(
        encoding: SerializationEncoding,
        payload: &[u8],
    ) -> anyhow::Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        match encoding {
            SerializationEncoding::MsgPack => rmp_serde::from_slice(payload).map_err(|e| {
                anyhow::anyhow!(
                    "Failed to deserialize msgpack `payload` for {}: {e}",
                    std::any::type_name::<T>()
                )
            }),
            SerializationEncoding::Json => serde_json::from_slice(payload).map_err(|e| {
                anyhow::anyhow!(
                    "Failed to deserialize json `payload` for {}: {e}",
                    std::any::type_name::<T>()
                )
            }),
        }
    }

    pub async fn scan_keys(
        con: &mut ConnectionManager,
        pattern: String,
    ) -> anyhow::Result<Vec<String>> {
        tracing::debug!("Starting scan for pattern: {}", pattern);

        let mut keys = Vec::new();
        let mut cursor = 0;

        loop {
            let result: (i64, Vec<String>) = redis::cmd("SCAN")
                .arg(cursor)
                .arg("MATCH")
                .arg(&pattern)
                .arg("COUNT")
                .arg(1000)
                .query_async(con)
                .await?;

            cursor = result.0;
            tracing::debug!(
                "Scan batch found {} keys, next cursor: {cursor}",
                result.1.len(),
            );
            keys.extend(result.1);

            if cursor == 0 {
                break;
            }
        }

        tracing::debug!("Scan complete, found {} keys total", keys.len());
        Ok(keys)
    }

    pub async fn read(
        con: &ConnectionManager,
        trader_key: &str,
        key: &str,
    ) -> anyhow::Result<Vec<Bytes>> {
        let collection = Self::get_collection_key(key)?;
        let key = format!("{trader_key}{REDIS_DELIMITER}{key}");
        let mut con = con.clone();

        match collection {
            INDEX => Self::read_index(&mut con, &key).await,
            GENERAL => Self::read_string(&mut con, &key).await,
            CURRENCIES => Self::read_string(&mut con, &key).await,
            INSTRUMENTS => Self::read_string(&mut con, &key).await,
            SYNTHETICS => Self::read_string(&mut con, &key).await,
            ACCOUNTS => Self::read_list(&mut con, &key).await,
            ORDERS => Self::read_list(&mut con, &key).await,
            POSITIONS => Self::read_list(&mut con, &key).await,
            ACTORS => Self::read_string(&mut con, &key).await,
            STRATEGIES => Self::read_string(&mut con, &key).await,
            _ => anyhow::bail!("Unsupported operation: `read` for collection '{collection}'"),
        }
    }

    pub async fn load_all(
        con: &ConnectionManager,
        encoding: SerializationEncoding,
        trader_key: &str,
    ) -> anyhow::Result<CacheMap> {
        let (currencies, instruments, synthetics, accounts, orders, positions) = try_join!(
            Self::load_currencies(con, trader_key, encoding),
            Self::load_instruments(con, trader_key, encoding),
            Self::load_synthetics(con, trader_key, encoding),
            Self::load_accounts(con, trader_key, encoding),
            Self::load_orders(con, trader_key, encoding),
            Self::load_positions(con, trader_key, encoding),
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
        trader_key: &str,
        encoding: SerializationEncoding,
    ) -> anyhow::Result<HashMap<Ustr, Currency>> {
        let mut currencies = HashMap::new();
        let pattern = format!("{trader_key}{REDIS_DELIMITER}{CURRENCIES}*");
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
                            tracing::error!("Invalid key format: {key}");
                            return None;
                        }
                    };

                    match Self::load_currency(&con, trader_key, &currency_code, encoding).await {
                        Ok(Some(currency)) => Some((currency_code, currency)),
                        Ok(None) => {
                            tracing::error!("Currency not found: {currency_code}");
                            None
                        }
                        Err(e) => {
                            tracing::error!("Failed to load currency {currency_code}: {e}");
                            None
                        }
                    }
                }
            })
            .collect();

        // Insert all Currency_code (key) and Currency (value) into the HashMap, filtering out None values
        currencies.extend(join_all(futures).await.into_iter().flatten());
        tracing::debug!("Loaded {} currencies(s)", currencies.len());

        Ok(currencies)
    }

    pub async fn load_instruments(
        con: &ConnectionManager,
        trader_key: &str,
        encoding: SerializationEncoding,
    ) -> anyhow::Result<HashMap<InstrumentId, InstrumentAny>> {
        let mut instruments = HashMap::new();
        let pattern = format!("{trader_key}{REDIS_DELIMITER}{INSTRUMENTS}*");
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
                            tracing::error!("Invalid key format: {key}");
                            "Invalid key format"
                        })
                        .and_then(|code| {
                            InstrumentId::from_str(code).map_err(|e| {
                                tracing::error!("Failed to convert to InstrumentId for {key}: {e}");
                                "Invalid instrument ID"
                            })
                        });

                    let instrument_id = match instrument_id {
                        Ok(id) => id,
                        Err(_) => return None,
                    };

                    match Self::load_instrument(&con, trader_key, &instrument_id, encoding).await {
                        Ok(Some(instrument)) => Some((instrument_id, instrument)),
                        Ok(None) => {
                            tracing::error!("Instrument not found: {instrument_id}");
                            None
                        }
                        Err(e) => {
                            tracing::error!("Failed to load instrument {instrument_id}: {e}");
                            None
                        }
                    }
                }
            })
            .collect();

        // Insert all Instrument_id (key) and Instrument (value) into the HashMap, filtering out None values
        instruments.extend(join_all(futures).await.into_iter().flatten());
        tracing::debug!("Loaded {} instruments(s)", instruments.len());

        Ok(instruments)
    }

    pub async fn load_synthetics(
        con: &ConnectionManager,
        trader_key: &str,
        encoding: SerializationEncoding,
    ) -> anyhow::Result<HashMap<InstrumentId, SyntheticInstrument>> {
        let mut synthetics = HashMap::new();
        let pattern = format!("{trader_key}{REDIS_DELIMITER}{SYNTHETICS}*");
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
                            tracing::error!("Invalid key format: {key}");
                            "Invalid key format"
                        })
                        .and_then(|code| {
                            InstrumentId::from_str(code).map_err(|e| {
                                tracing::error!("Failed to parse InstrumentId for {key}: {e}");
                                "Invalid instrument ID"
                            })
                        });

                    let instrument_id = match instrument_id {
                        Ok(id) => id,
                        Err(_) => return None,
                    };

                    match Self::load_synthetic(&con, trader_key, &instrument_id, encoding).await {
                        Ok(Some(synthetic)) => Some((instrument_id, synthetic)),
                        Ok(None) => {
                            tracing::error!("Synthetic not found: {instrument_id}");
                            None
                        }
                        Err(e) => {
                            tracing::error!("Failed to load synthetic {instrument_id}: {e}");
                            None
                        }
                    }
                }
            })
            .collect();

        // Insert all Instrument_id (key) and Synthetic (value) into the HashMap, filtering out None values
        synthetics.extend(join_all(futures).await.into_iter().flatten());
        tracing::debug!("Loaded {} synthetics(s)", synthetics.len());

        Ok(synthetics)
    }

    pub async fn load_accounts(
        con: &ConnectionManager,
        trader_key: &str,
        encoding: SerializationEncoding,
    ) -> anyhow::Result<HashMap<AccountId, AccountAny>> {
        let mut accounts = HashMap::new();
        let pattern = format!("{trader_key}{REDIS_DELIMITER}{ACCOUNTS}*");
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
                            tracing::error!("Invalid key format: {key}");
                            return None;
                        }
                    };

                    match Self::load_account(&con, trader_key, &account_id, encoding).await {
                        Ok(Some(account)) => Some((account_id, account)),
                        Ok(None) => {
                            tracing::error!("Account not found: {account_id}");
                            None
                        }
                        Err(e) => {
                            tracing::error!("Failed to load account {account_id}: {e}");
                            None
                        }
                    }
                }
            })
            .collect();

        // Insert all Account_id (key) and Account (value) into the HashMap, filtering out None values
        accounts.extend(join_all(futures).await.into_iter().flatten());
        tracing::debug!("Loaded {} accounts(s)", accounts.len());

        Ok(accounts)
    }

    pub async fn load_orders(
        con: &ConnectionManager,
        trader_key: &str,
        encoding: SerializationEncoding,
    ) -> anyhow::Result<HashMap<ClientOrderId, OrderAny>> {
        let mut orders = HashMap::new();
        let pattern = format!("{trader_key}{REDIS_DELIMITER}{ORDERS}*");
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
                            tracing::error!("Invalid key format: {key}");
                            return None;
                        }
                    };

                    match Self::load_order(&con, trader_key, &client_order_id, encoding).await {
                        Ok(Some(order)) => Some((client_order_id, order)),
                        Ok(None) => {
                            tracing::error!("Order not found: {client_order_id}");
                            None
                        }
                        Err(e) => {
                            tracing::error!("Failed to load order {client_order_id}: {e}");
                            None
                        }
                    }
                }
            })
            .collect();

        // Insert all Client-Order-Id (key) and Order (value) into the HashMap, filtering out None values
        orders.extend(join_all(futures).await.into_iter().flatten());
        tracing::debug!("Loaded {} order(s)", orders.len());

        Ok(orders)
    }

    pub async fn load_positions(
        con: &ConnectionManager,
        trader_key: &str,
        encoding: SerializationEncoding,
    ) -> anyhow::Result<HashMap<PositionId, Position>> {
        let mut positions = HashMap::new();
        let pattern = format!("{trader_key}{REDIS_DELIMITER}{POSITIONS}*");
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
                            tracing::error!("Invalid key format: {key}");
                            return None;
                        }
                    };

                    match Self::load_position(&con, trader_key, &position_id, encoding).await {
                        Ok(Some(position)) => Some((position_id, position)),
                        Ok(None) => {
                            tracing::error!("Position not found: {position_id}");
                            None
                        }
                        Err(e) => {
                            tracing::error!("Failed to load position {position_id}: {e}");
                            None
                        }
                    }
                }
            })
            .collect();

        // Insert all Position_id (key) and Position (value) into the HashMap, filtering out None values
        positions.extend(join_all(futures).await.into_iter().flatten());
        tracing::debug!("Loaded {} position(s)", positions.len());

        Ok(positions)
    }

    pub async fn load_currency(
        con: &ConnectionManager,
        trader_key: &str,
        code: &Ustr,
        encoding: SerializationEncoding,
    ) -> anyhow::Result<Option<Currency>> {
        let key = format!("{CURRENCIES}{REDIS_DELIMITER}{code}");
        let result = Self::read(con, trader_key, &key).await?;

        if result.is_empty() {
            return Ok(None);
        }

        let currency = Self::deserialize_payload::<Currency>(encoding, &result[0])?;

        Ok(Some(currency))
    }

    pub async fn load_instrument(
        con: &ConnectionManager,
        trader_key: &str,
        instrument_id: &InstrumentId,
        encoding: SerializationEncoding,
    ) -> anyhow::Result<Option<InstrumentAny>> {
        let key = format!("{INSTRUMENTS}{REDIS_DELIMITER}{instrument_id}");
        let result = Self::read(con, trader_key, &key).await?;
        if result.is_empty() {
            return Ok(None);
        }

        let instrument = Self::deserialize_payload::<InstrumentAny>(encoding, &result[0])?;

        Ok(Some(instrument))
    }

    pub async fn load_synthetic(
        con: &ConnectionManager,
        trader_key: &str,
        instrument_id: &InstrumentId,
        encoding: SerializationEncoding,
    ) -> anyhow::Result<Option<SyntheticInstrument>> {
        let key = format!("{SYNTHETICS}{REDIS_DELIMITER}{instrument_id}");
        let result = Self::read(con, trader_key, &key).await?;
        if result.is_empty() {
            return Ok(None);
        }

        let synthetic = Self::deserialize_payload::<SyntheticInstrument>(encoding, &result[0])?;

        Ok(Some(synthetic))
    }

    pub async fn load_account(
        con: &ConnectionManager,
        trader_key: &str,
        account_id: &AccountId,
        encoding: SerializationEncoding,
    ) -> anyhow::Result<Option<AccountAny>> {
        let key = format!("{ACCOUNTS}{REDIS_DELIMITER}{account_id}");
        let result = Self::read(con, trader_key, &key).await?;
        if result.is_empty() {
            return Ok(None);
        }

        let account = Self::deserialize_payload::<AccountAny>(encoding, &result[0])?;

        Ok(Some(account))
    }

    pub async fn load_order(
        con: &ConnectionManager,
        trader_key: &str,
        client_order_id: &ClientOrderId,
        encoding: SerializationEncoding,
    ) -> anyhow::Result<Option<OrderAny>> {
        let key = format!("{ORDERS}{REDIS_DELIMITER}{client_order_id}");
        let result = Self::read(con, trader_key, &key).await?;
        if result.is_empty() {
            return Ok(None);
        }

        let bytes = result
            .iter()
            .flat_map(|b| b.iter().copied())
            .collect::<Vec<u8>>();

        let order_initialized_value = match encoding {
            SerializationEncoding::MsgPack => rmp_serde::from_slice(&bytes)
                .map_err(|e| anyhow::anyhow!("Failed to deserialize msgpack `payload`: {e}"))?,
            SerializationEncoding::Json => serde_json::from_slice(&bytes)
                .map_err(|e| anyhow::anyhow!("Failed to deserialize json `payload`: {e}"))?,
        };

        let order_initialized: OrderInitialized =
            order_initialized_from_value(order_initialized_value)?;
        let mut order = OrderAny::from_events(vec![OrderEventAny::Initialized(order_initialized)])?;

        // Skip first initialized event
        for (event_count, event) in result.iter().skip(1).enumerate() {
            let event: OrderEventAny = Self::deserialize_payload(encoding, event)?;

            if order.events().contains(&&event) {
                anyhow::bail!("Corrupt cache with duplicate event for order {event}");
            }

            if event_count > 0 {
                if let OrderEventAny::Initialized(order_initialized) = &event {
                    match order_initialized.order_type {
                        OrderType::Market => {
                            let time_in_force = if order.time_in_force() != TimeInForce::Gtd {
                                order.time_in_force()
                            } else {
                                TimeInForce::Gtc
                            };

                            let mut transformed = MarketOrder::new(
                                order.trader_id(),
                                order.strategy_id(),
                                order.instrument_id(),
                                order.client_order_id(),
                                order.order_side(),
                                order.quantity(),
                                time_in_force,
                                UUID4::new(),
                                order.ts_init(),
                                order.is_reduce_only(),
                                order.is_quote_quantity(),
                                order.contingency_type(),
                                order.order_list_id(),
                                order.linked_order_ids(),
                                order.parent_order_id(),
                                order.exec_algorithm_id(),
                                order.exec_algorithm_params(),
                                order.exec_spawn_id(),
                                order.tags(),
                            );

                            let original_events = order.events();
                            for event in original_events.into_iter().rev() {
                                transformed.events.insert(0, event.clone());
                            }

                            order = OrderAny::from_market(transformed);
                        }
                        OrderType::Limit => {
                            let price = order_initialized.price.unwrap_or(order.price().unwrap());
                            let trigger_instrument_id = order
                                .trigger_instrument_id()
                                .unwrap_or(order.instrument_id());

                            let mut transformed = if let Ok(transformed) = LimitOrder::new(
                                order.trader_id(),
                                order.strategy_id(),
                                order.instrument_id(),
                                order.client_order_id(),
                                order.order_side(),
                                order.quantity(),
                                price,
                                order.time_in_force(),
                                order.expire_time(),
                                order.is_post_only(),
                                order.is_reduce_only(),
                                order.is_quote_quantity(),
                                order.display_qty(),
                                Some(TriggerType::NoTrigger),
                                Some(trigger_instrument_id),
                                order.contingency_type(),
                                order.order_list_id(),
                                order.linked_order_ids(),
                                order.parent_order_id(),
                                order.exec_algorithm_id(),
                                order.exec_algorithm_params(),
                                order.exec_spawn_id(),
                                order.tags(),
                                UUID4::new(),
                                order.ts_init(),
                            ) {
                                transformed
                            } else {
                                tracing::error!("Cannot create limit order");
                                return Ok(None);
                            };

                            transformed.liquidity_side = order.liquidity_side();

                            // TODO: fix
                            // let triggered_price = order.trigger_price();
                            // if triggered_price.is_some() {
                            //     transformed.trigger_price() = (triggered_price.unwrap());
                            // }

                            let original_events = order.events();
                            for event in original_events.into_iter().rev() {
                                transformed.events.insert(0, event.clone());
                            }

                            order = OrderAny::from_limit(transformed);
                        }
                        _ => {
                            anyhow::bail!(
                                "Cannot transform order to {}",
                                order_initialized.order_type
                            );
                        }
                    }
                } else {
                    order.apply(event)?;
                }
            } else {
                order.apply(event)?;
            }
        }

        Ok(Some(order))
    }

    pub async fn load_position(
        con: &ConnectionManager,
        trader_key: &str,
        position_id: &PositionId,
        encoding: SerializationEncoding,
    ) -> anyhow::Result<Option<Position>> {
        let key = format!("{POSITIONS}{REDIS_DELIMITER}{position_id}");
        let result = Self::read(con, trader_key, &key).await?;
        if result.is_empty() {
            return Ok(None);
        }

        let initial_fill: OrderFilled = Self::deserialize_payload(encoding, &result[0])?;

        let instrument = if let Some(instrument) =
            Self::load_instrument(con, trader_key, &initial_fill.instrument_id, encoding).await?
        {
            instrument
        } else {
            tracing::error!("Instrument not found: {}", initial_fill.instrument_id);
            return Ok(None);
        };

        let mut position = Position::new(&instrument, initial_fill);
        // then loop through rest of the events(orderfill) present in reult expect first one
        for event in result.iter().skip(1) {
            let order_filled: OrderFilled = Self::deserialize_payload(encoding, event)?;

            if position.events.contains(&order_filled) {
                anyhow::bail!("Corrupt cache with duplicate event for position {order_filled}");
            }

            position.apply(&order_filled);
        }

        Ok(Some(position))
    }

    fn get_collection_key(key: &str) -> anyhow::Result<&str> {
        key.split_once(REDIS_DELIMITER)
            .map(|(collection, _)| collection)
            .ok_or_else(|| {
                anyhow::anyhow!("Invalid `key`, missing a '{REDIS_DELIMITER}' delimiter, was {key}")
            })
    }

    async fn read_index(conn: &mut ConnectionManager, key: &str) -> anyhow::Result<Vec<Bytes>> {
        let index_key = Self::get_index_key(key)?;
        match index_key {
            INDEX_ORDER_IDS => Self::read_set(conn, key).await,
            INDEX_ORDER_POSITION => Self::read_hset(conn, key).await,
            INDEX_ORDER_CLIENT => Self::read_hset(conn, key).await,
            INDEX_ORDERS => Self::read_set(conn, key).await,
            INDEX_ORDERS_OPEN => Self::read_set(conn, key).await,
            INDEX_ORDERS_CLOSED => Self::read_set(conn, key).await,
            INDEX_ORDERS_EMULATED => Self::read_set(conn, key).await,
            INDEX_ORDERS_INFLIGHT => Self::read_set(conn, key).await,
            INDEX_POSITIONS => Self::read_set(conn, key).await,
            INDEX_POSITIONS_OPEN => Self::read_set(conn, key).await,
            INDEX_POSITIONS_CLOSED => Self::read_set(conn, key).await,
            _ => anyhow::bail!("Index unknown '{index_key}' on read"),
        }
    }

    async fn read_string(conn: &mut ConnectionManager, key: &str) -> anyhow::Result<Vec<Bytes>> {
        let result: Vec<u8> = conn.get(key).await?;

        if result.is_empty() {
            Ok(vec![])
        } else {
            Ok(vec![Bytes::from(result)])
        }
    }

    async fn read_set(conn: &mut ConnectionManager, key: &str) -> anyhow::Result<Vec<Bytes>> {
        let result: Vec<Bytes> = conn.smembers(key).await?;
        Ok(result)
    }

    async fn read_hset(conn: &mut ConnectionManager, key: &str) -> anyhow::Result<Vec<Bytes>> {
        let result: HashMap<String, String> = conn.hgetall(key).await?;
        let json = serde_json::to_string(&result)?;
        Ok(vec![Bytes::from(json.into_bytes())])
    }

    async fn read_list(conn: &mut ConnectionManager, key: &str) -> anyhow::Result<Vec<Bytes>> {
        let result: Vec<Bytes> = conn.lrange(key, 0, -1).await?;
        Ok(result)
    }

    fn get_index_key(key: &str) -> anyhow::Result<&str> {
        key.split_once(REDIS_DELIMITER)
            .map(|(_, index_key)| index_key)
            .ok_or_else(|| {
                anyhow::anyhow!("Invalid `key`, missing a '{REDIS_DELIMITER}' delimiter, was {key}")
            })
    }
}

fn is_timestamp_field(key: &str) -> bool {
    key == "expire_time_ns" || key.starts_with("ts_")
}

fn convert_timestamps(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, v) in map {
                if is_timestamp_field(key) {
                    if let Value::Number(n) = v {
                        if let Some(n) = n.as_u64() {
                            let dt = DateTime::<Utc>::from_timestamp_nanos(n as i64);
                            *v = Value::String(
                                dt.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true),
                            );
                        }
                    }
                }
                convert_timestamps(v);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                convert_timestamps(item);
            }
        }
        _ => {}
    }
}

fn order_initialized_from_value(value: Value) -> anyhow::Result<OrderInitialized> {
    let o_map = match value {
        Value::Object(map) => map,
        _ => anyhow::bail!("Invalid order initialized map"),
    };

    let trader_id = TraderId::new_checked(
        o_map
            .get("trader_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing trader_id field"))?,
    )?;

    let strategy_id = StrategyId::new_checked(
        o_map
            .get("strategy_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing strategy_id field"))?,
    )?;

    let instrument_id = InstrumentId::from_str(
        o_map
            .get("instrument_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing instrument_id field"))?,
    )?;

    let client_order_id = ClientOrderId::new_checked(
        o_map
            .get("client_order_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing client_order_id field"))?,
    )?;

    let order_side = OrderSide::from_str(
        o_map
            .get("order_side")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing order_side field"))?,
    )?;

    let order_type = OrderType::from_str(
        o_map
            .get("order_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing order_type field"))?,
    )?;

    let quantity = Quantity::from_str(
        o_map
            .get("quantity")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing quantity field"))?,
    )
    .map_err(|e| anyhow::anyhow!("Invalid quantity: {}", e))?;

    let time_in_force = TimeInForce::from_str(
        o_map
            .get("time_in_force")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing time_in_force field"))?,
    )?;

    let post_only = o_map
        .get("post_only")
        .and_then(|v| v.as_bool())
        .ok_or_else(|| anyhow::anyhow!("Missing post_only field"))?;

    let reduce_only = o_map
        .get("reduce_only")
        .and_then(|v| v.as_bool())
        .ok_or_else(|| anyhow::anyhow!("Missing reduce_only field"))?;

    let quote_quantity = o_map
        .get("quote_quantity")
        .and_then(|v| v.as_bool())
        .ok_or_else(|| anyhow::anyhow!("Missing quote_quantity field"))?;

    let reconciliation = o_map
        .get("reconciliation")
        .and_then(|v| v.as_bool())
        .ok_or_else(|| anyhow::anyhow!("Missing reconciliation field"))?;

    let event_id = UUID4::from_str(
        o_map
            .get("event_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing event_id field"))?,
    )?;

    let ts_event = UnixNanos::from_str(
        &o_map
            .get("ts_event")
            .and_then(|v| v.as_i64())
            .map(|n| n.to_string())
            .unwrap_or_else(|| "0".to_string()),
    )
    .map_err(|e| anyhow::anyhow!("Invalid ts_event: {}", e))?;

    let ts_init = UnixNanos::from_str(
        &o_map
            .get("ts_init")
            .and_then(|v| v.as_i64())
            .map(|n| n.to_string())
            .unwrap_or_else(|| "0".to_string()),
    )
    .map_err(|e| anyhow::anyhow!("Invalid ts_init: {}", e))?;

    let options = o_map.get("options").and_then(|v| v.as_object());

    let price = options
        .and_then(|opts| opts.get("price"))
        .and_then(|v| v.as_str())
        .map_or_else(
            || {
                tracing::error!("Missing price in options");
                None
            },
            |value| match Price::from_str(value) {
                Ok(price) => Some(price),
                Err(e) => {
                    tracing::error!("Invalid price: {}", e);
                    None
                }
            },
        );

    let expire_time = options
        .and_then(|opts| opts.get("expire_time_ns"))
        .and_then(|v| v.as_i64())
        .map_or_else(
            || {
                tracing::error!("Missing expire_time_ns in options");
                None
            },
            |ns| match UnixNanos::from_str(&ns.to_string()) {
                Ok(time) => Some(time),
                Err(e) => {
                    tracing::error!("Invalid expire_time: {}", e);
                    None
                }
            },
        );

    let display_qty = options
        .and_then(|opts| opts.get("display_qty"))
        .and_then(|v| v.as_str())
        .map_or_else(
            || {
                tracing::error!("Missing display_qty in options");
                None
            },
            |value| match Quantity::from_str(value) {
                Ok(qty) => Some(qty),
                Err(e) => {
                    tracing::error!("Invalid display_qty: {}", e);
                    None
                }
            },
        );

    let trigger_price = o_map
        .get("trigger_price")
        .and_then(|v| v.as_str())
        .map_or_else(
            || {
                tracing::error!("Missing trigger_price field");
                None
            },
            |value| {
                Price::from_str(value)
                    .map_err(|e| {
                        tracing::error!("Invalid trigger_price: {}", e);
                        e
                    })
                    .ok()
            },
        );

    let trigger_type = o_map
        .get("trigger_type")
        .and_then(|v| v.as_str())
        .map_or_else(
            || {
                tracing::error!("Missing trigger_type field");
                None
            },
            |value| {
                TriggerType::from_str(value)
                    .inspect_err(|e| {
                        tracing::error!("Invalid trigger_type: {}", e);
                    })
                    .ok()
            },
        );

    let limit_offset = o_map
        .get("limit_offset")
        .and_then(|v| v.as_str())
        .map_or_else(
            || {
                tracing::error!("Missing limit_offset field");
                None
            },
            |value| {
                Decimal::from_str(value)
                    .map_err(|e| {
                        tracing::error!("Invalid limit_offset: {}", e);
                        e
                    })
                    .ok()
            },
        );

    let trailing_offset = o_map
        .get("trailing_offset")
        .and_then(|v| v.as_str())
        .map_or_else(
            || {
                tracing::error!("Missing trailing_offset field");
                None
            },
            |value| {
                Decimal::from_str(value)
                    .map_err(|e| {
                        tracing::error!("Invalid trailing_offset: {}", e);
                        e
                    })
                    .ok()
            },
        );

    let trailing_offset_type = o_map
        .get("trailing_offset_type")
        .and_then(|v| v.as_str())
        .map_or_else(
            || {
                tracing::error!("Missing trailing_offset_type field");
                None
            },
            |value| {
                TrailingOffsetType::from_str(value)
                    .inspect_err(|e| {
                        tracing::error!("Invalid trailing_offset_type: {}", e);
                    })
                    .ok()
            },
        );

    let emulation_trigger = o_map
        .get("emulation_trigger")
        .and_then(|v| v.as_str())
        .map_or_else(
            || {
                tracing::error!("Missing emulation_trigger field");
                None
            },
            |value| {
                TriggerType::from_str(value)
                    .inspect_err(|e| {
                        tracing::error!("Invalid emulation_trigger: {}", e);
                    })
                    .ok()
            },
        );

    let trigger_instrument_id = o_map
        .get("trigger_instrument_id")
        .and_then(|v| v.as_str())
        .map_or_else(
            || {
                tracing::error!("Missing trigger_instrument_id field");
                None
            },
            |value| {
                InstrumentId::from_str(value)
                    .map_err(|e| {
                        tracing::error!("Invalid trigger_instrument_id: {}", e);
                        e
                    })
                    .ok()
            },
        );

    let contingency_type = o_map
        .get("contingency_type")
        .and_then(|v| v.as_str())
        .map_or_else(
            || {
                tracing::error!("Missing contingency_type field");
                None
            },
            |value| {
                ContingencyType::from_str(value)
                    .inspect_err(|e| {
                        tracing::error!("Invalid contingency_type: {}", e);
                    })
                    .ok()
            },
        );

    let order_list_id = o_map
        .get("order_list_id")
        .and_then(|v| v.as_str())
        .map_or_else(
            || {
                tracing::error!("Missing order_list_id field");
                None
            },
            |value| {
                OrderListId::new_checked(value)
                    .map_err(|e| {
                        tracing::error!("Invalid order_list_id: {}", e);
                        e
                    })
                    .ok()
            },
        );

    let linked_order_ids = o_map
        .get("linked_order_ids")
        .and_then(|v| v.as_array())
        .map(|array| {
            array
                .iter()
                .filter_map(|v| match v.as_str() {
                    None => {
                        tracing::error!("Linked order ID is not a string");
                        None
                    }
                    Some(str_val) => match ClientOrderId::new_checked(str_val) {
                        Ok(order_id) => Some(order_id),
                        Err(e) => {
                            tracing::error!("Invalid linked order ID format: {}", e);
                            None
                        }
                    },
                })
                .collect::<Vec<_>>()
        });

    let parent_order_id = o_map
        .get("parent_order_id")
        .and_then(|v| v.as_str())
        .map_or_else(
            || {
                tracing::error!("Missing parent_order_id field");
                None
            },
            |value| {
                ClientOrderId::new_checked(value)
                    .map_err(|e| {
                        tracing::error!("Invalid parent_order_id: {}", e);
                        e
                    })
                    .ok()
            },
        );

    let exec_algorithm_id = o_map
        .get("exec_algorithm_id")
        .and_then(|v| v.as_str())
        .map_or_else(
            || {
                tracing::error!("Missing exec_algorithm_id field");
                None
            },
            |value| {
                ExecAlgorithmId::new_checked(value)
                    .map_err(|e| {
                        tracing::error!("Invalid exec_algorithm_id: {}", e);
                        e
                    })
                    .ok()
            },
        );

    let exec_algorithm_params = o_map
        .get("exec_algorithm_params")
        .and_then(|v| v.as_object())
        .map_or_else(
            || {
                tracing::error!("Missing or invalid exec_algorithm_params field");
                None
            },
            |obj| {
                let params: Option<IndexMap<Ustr, Ustr>> = Some(
                    obj.iter()
                        .filter_map(|(k, v)| {
                            let key = match Ustr::from_str(k) {
                                Ok(key) => key,
                                Err(e) => {
                                    tracing::error!("Invalid exec_algorithm_params key: {}", e);
                                    return None;
                                }
                            };

                            let value = match v.as_str() {
                                Some(str_val) => match Ustr::from_str(str_val) {
                                    Ok(val) => val,
                                    Err(e) => {
                                        tracing::error!(
                                            "Invalid exec_algorithm_params value: {}",
                                            e
                                        );
                                        return None;
                                    }
                                },
                                None => {
                                    tracing::error!("exec_algorithm_params value is not a string");
                                    return None;
                                }
                            };

                            Some((key, value))
                        })
                        .collect(),
                );

                if params.as_ref().is_none_or(|p| p.is_empty()) {
                    None
                } else {
                    params
                }
            },
        );

    let exec_spawn_id = o_map
        .get("exec_spawn_id")
        .and_then(|v| v.as_str())
        .map_or_else(
            || {
                tracing::error!("Missing exec_spawn_id field");
                None
            },
            |value| {
                ClientOrderId::new_checked(value)
                    .map_err(|e| {
                        tracing::error!("Invalid exec_spawn_id: {}", e);
                        e
                    })
                    .ok()
            },
        );

    let tags = o_map.get("tags").and_then(|v| v.as_array()).map(|array| {
        array
            .iter()
            .filter_map(|v| match v.as_str() {
                None => {
                    tracing::error!("Tag is not a string");
                    None
                }
                Some(str_val) => match Ustr::from_str(str_val) {
                    Ok(tag) => Some(tag),
                    Err(e) => {
                        tracing::error!("Invalid tag: {}", e);
                        None
                    }
                },
            })
            .collect::<Vec<_>>()
    });

    let order_initialized = OrderInitialized::new(
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        order_side,
        order_type,
        quantity,
        time_in_force,
        post_only,
        reduce_only,
        quote_quantity,
        reconciliation,
        event_id,
        ts_event,
        ts_init,
        price,
        trigger_price,
        trigger_type,
        limit_offset,
        trailing_offset,
        trailing_offset_type,
        expire_time,
        display_qty,
        emulation_trigger,
        trigger_instrument_id,
        contingency_type,
        order_list_id,
        linked_order_ids,
        parent_order_id,
        exec_algorithm_id,
        exec_algorithm_params,
        exec_spawn_id,
        tags,
    );

    Ok(order_initialized)
}
