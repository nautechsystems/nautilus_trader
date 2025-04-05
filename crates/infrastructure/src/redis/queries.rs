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
use futures::{StreamExt, future::join_all};
use nautilus_common::{cache::database::CacheMap, enums::SerializationEncoding};
use nautilus_model::{
    accounts::AccountAny,
    identifiers::{AccountId, ClientOrderId, InstrumentId, PositionId},
    instruments::{InstrumentAny, SyntheticInstrument},
    orders::OrderAny,
    position::Position,
    types::Currency,
};
use redis::{AsyncCommands, aio::ConnectionManager};
use serde::{Serialize, de::DeserializeOwned};
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
            SerializationEncoding::MsgPack => rmp_serde::to_vec(&value)
                .map_err(|e| anyhow::anyhow!("Failed to serialize msgpack `payload`: {e}")),
            SerializationEncoding::Json => serde_json::to_vec(&value)
                .map_err(|e| anyhow::anyhow!("Failed to serialize json `payload`: {e}")),
        }
    }

    pub fn deserialize_payload<T: DeserializeOwned>(
        encoding: SerializationEncoding,
        payload: &[u8],
    ) -> anyhow::Result<T> {
        let mut value = match encoding {
            SerializationEncoding::MsgPack => rmp_serde::from_slice(payload)
                .map_err(|e| anyhow::anyhow!("Failed to deserialize msgpack `payload`: {e}"))?,
            SerializationEncoding::Json => serde_json::from_slice(payload)
                .map_err(|e| anyhow::anyhow!("Failed to deserialize json `payload`: {e}"))?,
        };

        convert_timestamp_strings(&mut value);

        serde_json::from_value(value)
            .map_err(|e| anyhow::anyhow!("Failed to convert value to target type: {e}"))
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
            Self::load_positions(con, trader_key, encoding)
        )
        .map_err(|e| anyhow::anyhow!("Error loading cache data: {e}"))?;

        // For now, we don't load greeks and yield curves from the database
        // This will be implemented in the future
        let greeks = HashMap::new();
        let yield_curves = HashMap::new();

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
                            log::error!("Invalid key format: {key}");
                            return None;
                        }
                    };

                    match Self::load_currency(&con, trader_key, &currency_code, encoding).await {
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

                    match Self::load_instrument(&con, trader_key, &instrument_id, encoding).await {
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

                    match Self::load_synthetic(&con, trader_key, &instrument_id, encoding).await {
                        Ok(Some(synthetic)) => Some((instrument_id, synthetic)),
                        Ok(None) => {
                            log::error!("Synthetic not found: {instrument_id}");
                            None
                        }
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
                            log::error!("Invalid key format: {key}");
                            return None;
                        }
                    };

                    match Self::load_account(&con, trader_key, &account_id, encoding).await {
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
                            log::error!("Invalid key format: {key}");
                            return None;
                        }
                    };

                    match Self::load_order(&con, trader_key, &client_order_id, encoding).await {
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
                            log::error!("Invalid key format: {key}");
                            return None;
                        }
                    };

                    match Self::load_position(&con, trader_key, &position_id, encoding).await {
                        Ok(Some(position)) => Some((position_id, position)),
                        Ok(None) => {
                            log::error!("Position not found: {position_id}");
                            None
                        }
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

        let currency = Self::deserialize_payload(encoding, &result[0])?;
        Ok(currency)
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

        let instrument: InstrumentAny = Self::deserialize_payload(encoding, &result[0])?;
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

        let synthetic: SyntheticInstrument = Self::deserialize_payload(encoding, &result[0])?;
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

        let account: AccountAny = Self::deserialize_payload(encoding, &result[0])?;
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

        let order: OrderAny = Self::deserialize_payload(encoding, &result[0])?;
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

        let position: Position = Self::deserialize_payload(encoding, &result[0])?;
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
    let expire_match = key == "expire_time_ns";
    let ts_match = key.starts_with("ts_");
    expire_match || ts_match
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

fn convert_timestamp_strings(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, v) in map {
                if is_timestamp_field(key) {
                    if let Value::String(s) = v {
                        if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
                            *v = Value::Number(
                                (dt.with_timezone(&Utc)
                                    .timestamp_nanos_opt()
                                    .expect("Invalid DateTime")
                                    as u64)
                                    .into(),
                            );
                        }
                    }
                }
                convert_timestamp_strings(v);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                convert_timestamp_strings(item);
            }
        }
        _ => {}
    }
}
