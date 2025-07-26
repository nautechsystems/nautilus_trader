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

use std::{collections::HashMap, str::FromStr, time::Instant};

use ahash::AHashMap;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures::future::join_all;
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

#[derive(Debug)]
pub struct DatabaseQueries;

impl DatabaseQueries {
    /// Serializes the given `payload` using the specified `encoding` to a byte vector.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization to the chosen encoding fails.
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

    /// Deserializes the given byte slice `payload` into type `T` using the specified `encoding`.
    ///
    /// # Errors
    ///
    /// Returns an error if deserialization from the chosen encoding fails or converting to the target type fails.
    pub fn deserialize_payload<T: DeserializeOwned>(
        encoding: SerializationEncoding,
        payload: &[u8],
    ) -> anyhow::Result<T> {
        let deser_start = Instant::now();
        let payload_len = payload.len();

        let decode_start = Instant::now();
        let mut value = match encoding {
            SerializationEncoding::MsgPack => {
                log::debug!("deserialize_payload: Decoding {payload_len} bytes from MsgPack");
                rmp_serde::from_slice(payload)
                    .map_err(|e| anyhow::anyhow!("Failed to deserialize msgpack `payload`: {e}"))?
            }
            SerializationEncoding::Json => {
                log::debug!("deserialize_payload: Decoding {payload_len} bytes from JSON");
                serde_json::from_slice(payload)
                    .map_err(|e| anyhow::anyhow!("Failed to deserialize json `payload`: {e}"))?
            }
        };
        let decode_time = decode_start.elapsed();

        let convert_start = Instant::now();
        convert_timestamp_strings(&mut value);
        let convert_time = convert_start.elapsed();

        let from_value_start = Instant::now();
        let result = serde_json::from_value(value)
            .map_err(|e| anyhow::anyhow!("Failed to convert value to target type: {e}"))?;
        let from_value_time = from_value_start.elapsed();

        let total_time = deser_start.elapsed();
        log::debug!(
            "deserialize_payload: COMPLETE - Total: {}ms, Decode: {}ms, Convert: {}ms, FromValue: {}ms",
            total_time.as_millis(),
            decode_time.as_millis(),
            convert_time.as_millis(),
            from_value_time.as_millis()
        );

        Ok(result)
    }

    /// Scans Redis for keys matching the given `pattern`.
    ///
    /// # Errors
    ///
    /// Returns an error if the Redis scan operation fails.
    pub async fn scan_keys(
        con: &mut ConnectionManager,
        pattern: String,
    ) -> anyhow::Result<Vec<String>> {
        let start_time = Instant::now();
        log::debug!("Redis scan_keys: Starting scan for pattern: {pattern}");

        let scan_start = Instant::now();
        log::debug!("Redis scan_keys: About to call SCAN with COUNT=5000...");

        let mut result = Vec::new();
        let mut cursor = 0u64;
        let mut iteration = 0;

        loop {
            iteration += 1;
            let iter_start = Instant::now();

            log::debug!("Redis scan_keys: SCAN iteration {iteration} with cursor {cursor}");

            let scan_result: (u64, Vec<String>) = redis::cmd("SCAN")
                .arg(cursor)
                .arg("MATCH")
                .arg(&pattern)
                .arg("COUNT")
                .arg(5000)
                .query_async(con)
                .await?;

            let iter_time = iter_start.elapsed();

            let (new_cursor, keys) = scan_result;
            let keys_found = keys.len();
            result.extend(keys);

            log::debug!(
                "Redis scan_keys: Iteration {iteration} found {keys_found} keys in {}ms, new cursor: {new_cursor}",
                iter_time.as_millis(),
            );

            // If cursor is 0, we've completed the full scan
            if new_cursor == 0 {
                log::debug!("Redis scan_keys: SCAN completed - cursor returned to 0");
                break;
            }

            cursor = new_cursor;
        }

        let scan_setup_time = scan_start.elapsed();
        log::debug!(
            "Redis scan_keys: SCAN completed in {}ms with {iteration} iterations",
            scan_setup_time.as_millis(),
        );

        let total_time = start_time.elapsed();
        log::debug!(
            "Redis scan_keys: COMPLETE - Total time: {}ms, found {} keys",
            total_time.as_millis(),
            result.len()
        );

        Ok(result)
    }

    /// Bulk reads multiple keys from Redis using MGET for efficiency.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying Redis MGET operation fails.
    pub async fn read_bulk(
        con: &ConnectionManager,
        keys: &[String],
    ) -> anyhow::Result<Vec<Option<Bytes>>> {
        let start_time = Instant::now();
        log::debug!("read_bulk: Starting bulk read for {} keys", keys.len());

        // Debug: Show first few keys to understand format
        let sample_keys = if keys.len() <= 3 {
            keys.to_vec()
        } else {
            keys[0..3].to_vec()
        };
        log::debug!("read_bulk: Sample keys: {sample_keys:?}");

        if keys.is_empty() {
            return Ok(vec![]);
        }

        let mut con = con.clone();
        let mget_start = Instant::now();

        // Use MGET to fetch all keys in a single network operation
        let results: Vec<Option<Vec<u8>>> =
            redis::cmd("MGET").arg(keys).query_async(&mut con).await?;

        let mget_time = mget_start.elapsed();
        log::debug!(
            "read_bulk: MGET completed in {}ms for {} keys, got {} results",
            mget_time.as_millis(),
            keys.len(),
            results.len()
        );

        // Convert Vec<u8> to Bytes
        let bytes_results: Vec<Option<Bytes>> = results
            .into_iter()
            .map(|opt| opt.map(Bytes::from))
            .collect();

        let total_time = start_time.elapsed();
        let non_null_count = bytes_results.iter().filter(|r| r.is_some()).count();
        log::debug!(
            "read_bulk: COMPLETE - Total time: {}ms, found {} non-null values out of {} keys",
            total_time.as_millis(),
            non_null_count,
            keys.len()
        );

        Ok(bytes_results)
    }

    /// Reads raw byte payloads for `key` under `trader_key` from Redis.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying Redis read operation fails or if the collection is unsupported.
    pub async fn read(
        con: &ConnectionManager,
        trader_key: &str,
        key: &str,
    ) -> anyhow::Result<Vec<Bytes>> {
        let read_start = Instant::now();
        let collection = Self::get_collection_key(key)?;
        let full_key = format!("{trader_key}{REDIS_DELIMITER}{key}");
        log::debug!("read: Starting read for key: {full_key}");

        let mut con = con.clone();

        let result = match collection {
            INDEX => Self::read_index(&mut con, &full_key).await,
            GENERAL => Self::read_string(&mut con, &full_key).await,
            CURRENCIES => Self::read_string(&mut con, &full_key).await,
            INSTRUMENTS => Self::read_string(&mut con, &full_key).await,
            SYNTHETICS => Self::read_string(&mut con, &full_key).await,
            ACCOUNTS => Self::read_list(&mut con, &full_key).await,
            ORDERS => Self::read_list(&mut con, &full_key).await,
            POSITIONS => Self::read_list(&mut con, &full_key).await,
            ACTORS => Self::read_string(&mut con, &full_key).await,
            STRATEGIES => Self::read_string(&mut con, &full_key).await,
            _ => anyhow::bail!("Unsupported operation: `read` for collection '{collection}'"),
        };

        let read_time = read_start.elapsed();
        log::debug!(
            "read: Completed read for {} in {}ms, collection: {}",
            full_key,
            read_time.as_millis(),
            collection
        );

        result
    }

    /// Loads all cache data (currencies, instruments, synthetics, accounts, orders, positions) for `trader_key`.
    ///
    /// # Errors
    ///
    /// Returns an error if loading any of the individual caches fails or combining data fails.
    pub async fn load_all(
        con: &ConnectionManager,
        encoding: SerializationEncoding,
        trader_key: &str,
    ) -> anyhow::Result<CacheMap> {
        let load_start = Instant::now();
        log::debug!("load_all: Starting cache loading for trader_key: {trader_key}");

        let try_join_start = Instant::now();
        log::debug!("load_all: Starting concurrent loading of all cache types");

        let (currencies, instruments, synthetics, accounts, orders, positions) = try_join!(
            Self::load_currencies(con, trader_key, encoding),
            Self::load_instruments(con, trader_key, encoding),
            Self::load_synthetics(con, trader_key, encoding),
            Self::load_accounts(con, trader_key, encoding),
            Self::load_orders(con, trader_key, encoding),
            Self::load_positions(con, trader_key, encoding)
        )
        .map_err(|e| anyhow::anyhow!("Error loading cache data: {e}"))?;

        let try_join_time = try_join_start.elapsed();
        log::debug!(
            "load_all: Concurrent loading completed in {}ms",
            try_join_time.as_millis()
        );

        // For now, we don't load greeks and yield curves from the database
        // This will be implemented in the future
        let greeks = AHashMap::new();
        let yield_curves = AHashMap::new();

        let total_time = load_start.elapsed();
        log::debug!(
            "load_all: COMPLETE - Total: {}ms, Currencies: {}, Instruments: {}, Synthetics: {}, Accounts: {}, Orders: {}, Positions: {}",
            total_time.as_millis(),
            currencies.len(),
            instruments.len(),
            synthetics.len(),
            accounts.len(),
            orders.len(),
            positions.len()
        );

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

    /// Loads all currencies for `trader_key` using the specified `encoding`.
    ///
    /// # Errors
    ///
    /// Returns an error if scanning keys or reading currency data fails.
    pub async fn load_currencies(
        con: &ConnectionManager,
        trader_key: &str,
        encoding: SerializationEncoding,
    ) -> anyhow::Result<AHashMap<Ustr, Currency>> {
        let load_start = Instant::now();
        log::debug!("load_currencies: Starting currency loading for trader_key: {trader_key}");

        let mut currencies = AHashMap::new();
        let pattern = format!("{trader_key}{REDIS_DELIMITER}{CURRENCIES}*");
        tracing::debug!("Loading {pattern}");
        log::debug!("load_currencies: Pattern: {pattern}");

        let mut con = con.clone();
        let scan_start = Instant::now();
        let keys = Self::scan_keys(&mut con, pattern).await?;
        let scan_time = scan_start.elapsed();
        log::debug!(
            "load_currencies: Scan completed in {}ms, found {} currency keys",
            scan_time.as_millis(),
            keys.len()
        );

        if keys.is_empty() {
            return Ok(currencies);
        }

        // Use bulk loading with MGET for efficiency
        let bulk_start = Instant::now();
        log::info!(
            "load_currencies: Starting bulk read of {} currency keys",
            keys.len()
        );

        let bulk_values = Self::read_bulk(&con, &keys).await?;
        let bulk_time = bulk_start.elapsed();
        log::info!(
            "load_currencies: Bulk read completed in {}ms for {} keys",
            bulk_time.as_millis(),
            keys.len()
        );

        // Process the bulk results
        let process_start = Instant::now();
        let mut successful_loads = 0;
        let mut failed_loads = 0;

        for (idx, (key, value_opt)) in keys.iter().zip(bulk_values.iter()).enumerate() {
            let currency_code = if let Some(code) = key.as_str().rsplit(':').next() {
                Ustr::from(code)
            } else {
                log::error!("Invalid key format: {key}");
                failed_loads += 1;
                continue;
            };

            if let Some(value_bytes) = value_opt {
                match Self::deserialize_payload(encoding, value_bytes) {
                    Ok(currency) => {
                        currencies.insert(currency_code, currency);
                        successful_loads += 1;

                        // Log progress every 100 currencies
                        if idx % 100 == 0 || idx == keys.len() - 1 {
                            log::debug!(
                                "load_currencies: Processed {} of {} currencies ({})",
                                idx + 1,
                                keys.len(),
                                currency_code
                            );
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to deserialize currency {currency_code}: {e}");
                        failed_loads += 1;
                    }
                }
            } else {
                log::error!("Currency not found in Redis: {currency_code}");
                failed_loads += 1;
            }
        }

        let process_time = process_start.elapsed();
        let total_time = load_start.elapsed();

        log::info!(
            "load_currencies: COMPLETE - Total: {}ms, Scan: {}ms, Bulk: {}ms, Process: {}ms",
            total_time.as_millis(),
            scan_time.as_millis(),
            bulk_time.as_millis(),
            process_time.as_millis()
        );
        log::info!(
            "load_currencies: Loaded {successful_loads} currencies successfully, {failed_loads} failed"
        );

        tracing::debug!("Loaded {} currencies(s)", currencies.len());

        Ok(currencies)
    }

    /// Loads all instruments for `trader_key` using the specified `encoding`.
    ///
    /// # Errors
    ///
    /// Returns an error if scanning keys or reading instrument data fails.
    /// Loads all instruments for `trader_key` using the specified `encoding`.
    ///
    /// # Errors
    ///
    /// Returns an error if scanning keys or reading instrument data fails.
    pub async fn load_instruments(
        con: &ConnectionManager,
        trader_key: &str,
        encoding: SerializationEncoding,
    ) -> anyhow::Result<AHashMap<InstrumentId, InstrumentAny>> {
        let mut instruments = AHashMap::new();
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

    /// Loads all synthetic instruments for `trader_key` using the specified `encoding`.
    ///
    /// # Errors
    ///
    /// Returns an error if scanning keys or reading synthetic instrument data fails.
    /// Loads all synthetic instruments for `trader_key` using the specified `encoding`.
    ///
    /// # Errors
    ///
    /// Returns an error if scanning keys or reading synthetic instrument data fails.
    pub async fn load_synthetics(
        con: &ConnectionManager,
        trader_key: &str,
        encoding: SerializationEncoding,
    ) -> anyhow::Result<AHashMap<InstrumentId, SyntheticInstrument>> {
        let mut synthetics = AHashMap::new();
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

    /// Loads all accounts for `trader_key` using the specified `encoding`.
    ///
    /// # Errors
    ///
    /// Returns an error if scanning keys or reading account data fails.
    /// Loads all accounts for `trader_key` using the specified `encoding`.
    ///
    /// # Errors
    ///
    /// Returns an error if scanning keys or reading account data fails.
    pub async fn load_accounts(
        con: &ConnectionManager,
        trader_key: &str,
        encoding: SerializationEncoding,
    ) -> anyhow::Result<AHashMap<AccountId, AccountAny>> {
        let mut accounts = AHashMap::new();
        let pattern = format!("{trader_key}{REDIS_DELIMITER}{ACCOUNTS}*");
        tracing::debug!("Loading {pattern}");

        let mut con = con.clone();
        let keys = Self::scan_keys(&mut con, pattern).await?;

        let futures: Vec<_> = keys
            .iter()
            .map(|key| {
                let con = con.clone();
                async move {
                    let account_id = if let Some(code) = key.as_str().rsplit(':').next() {
                        AccountId::from(code)
                    } else {
                        log::error!("Invalid key format: {key}");
                        return None;
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

    /// Loads all orders for `trader_key` using the specified `encoding`.
    ///
    /// # Errors
    ///
    /// Returns an error if scanning keys or reading order data fails.
    /// Loads all orders for `trader_key` using the specified `encoding`.
    ///
    /// # Errors
    ///
    /// Returns an error if scanning keys or reading order data fails.
    pub async fn load_orders(
        con: &ConnectionManager,
        trader_key: &str,
        encoding: SerializationEncoding,
    ) -> anyhow::Result<AHashMap<ClientOrderId, OrderAny>> {
        let mut orders = AHashMap::new();
        let pattern = format!("{trader_key}{REDIS_DELIMITER}{ORDERS}*");
        tracing::debug!("Loading {pattern}");

        let mut con = con.clone();
        let keys = Self::scan_keys(&mut con, pattern).await?;

        let futures: Vec<_> = keys
            .iter()
            .map(|key| {
                let con = con.clone();
                async move {
                    let client_order_id = if let Some(code) = key.as_str().rsplit(':').next() {
                        ClientOrderId::from(code)
                    } else {
                        log::error!("Invalid key format: {key}");
                        return None;
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

    /// Loads all positions for `trader_key` using the specified `encoding`.
    ///
    /// # Errors
    ///
    /// Returns an error if scanning keys or reading position data fails.
    /// Loads all positions for `trader_key` using the specified `encoding`.
    ///
    /// # Errors
    ///
    /// Returns an error if scanning keys or reading position data fails.
    pub async fn load_positions(
        con: &ConnectionManager,
        trader_key: &str,
        encoding: SerializationEncoding,
    ) -> anyhow::Result<AHashMap<PositionId, Position>> {
        let mut positions = AHashMap::new();
        let pattern = format!("{trader_key}{REDIS_DELIMITER}{POSITIONS}*");
        tracing::debug!("Loading {pattern}");

        let mut con = con.clone();
        let keys = Self::scan_keys(&mut con, pattern).await?;

        let futures: Vec<_> = keys
            .iter()
            .map(|key| {
                let con = con.clone();
                async move {
                    let position_id = if let Some(code) = key.as_str().rsplit(':').next() {
                        PositionId::from(code)
                    } else {
                        log::error!("Invalid key format: {key}");
                        return None;
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

    /// Loads a single currency for `trader_key` and `code` using the specified `encoding`.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying read or deserialization fails.
    pub async fn load_currency(
        con: &ConnectionManager,
        trader_key: &str,
        code: &Ustr,
        encoding: SerializationEncoding,
    ) -> anyhow::Result<Option<Currency>> {
        let load_start = Instant::now();
        let key = format!("{CURRENCIES}{REDIS_DELIMITER}{code}");
        log::debug!("load_currency: Loading currency {code} with key: {key}");

        let read_start = Instant::now();
        let result = Self::read(con, trader_key, &key).await?;
        let read_time = read_start.elapsed();
        log::debug!(
            "load_currency: Read completed for {} in {}ms, got {} bytes",
            code,
            read_time.as_millis(),
            result.len()
        );

        if result.is_empty() {
            log::debug!("load_currency: No data found for currency {code}");
            return Ok(None);
        }

        let deserialize_start = Instant::now();
        let currency = Self::deserialize_payload(encoding, &result[0])?;
        let deserialize_time = deserialize_start.elapsed();

        let total_time = load_start.elapsed();
        log::debug!(
            "load_currency: Successfully loaded {} - Total: {}ms, Read: {}ms, Deserialize: {}ms",
            code,
            total_time.as_millis(),
            read_time.as_millis(),
            deserialize_time.as_millis()
        );

        Ok(currency)
    }

    /// Loads a single instrument for `trader_key` and `instrument_id` using the specified `encoding`.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying read or deserialization fails.
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

    /// Loads a single synthetic instrument for `trader_key` and `instrument_id` using the specified `encoding`.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying read or deserialization fails.
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

    /// Loads a single account for `trader_key` and `account_id` using the specified `encoding`.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying read or deserialization fails.
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

    /// Loads a single order for `trader_key` and `client_order_id` using the specified `encoding`.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying read or deserialization fails.
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

    /// Loads a single position for `trader_key` and `position_id` using the specified `encoding`.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying read or deserialization fails.
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
        let read_start = Instant::now();
        log::debug!("read_string: Starting Redis GET for key: {key}");

        let result: Vec<u8> = conn.get(key).await?;

        let read_time = read_start.elapsed();
        log::debug!(
            "read_string: Redis GET completed for {} in {}ms, got {} bytes",
            key,
            read_time.as_millis(),
            result.len()
        );

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
                if is_timestamp_field(key)
                    && let Value::Number(n) = v
                    && let Some(n) = n.as_u64()
                {
                    let dt = DateTime::<Utc>::from_timestamp_nanos(n as i64);
                    *v = Value::String(dt.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true));
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
                if is_timestamp_field(key)
                    && let Value::String(s) = v
                    && let Ok(dt) = DateTime::parse_from_rfc3339(s)
                {
                    *v = Value::Number(
                        (dt.with_timezone(&Utc)
                            .timestamp_nanos_opt()
                            .expect("Invalid DateTime") as u64)
                            .into(),
                    );
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
