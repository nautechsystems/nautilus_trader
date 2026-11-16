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

use std::{collections::HashMap, str::FromStr};

use ahash::AHashMap;
use bytes::Bytes;
use futures::future::join_all;
#[cfg(feature = "defi")]
use nautilus_common::{cache::CacheSnapshotRef, defi::cache::InstrumentMigration};
use nautilus_common::{
    cache::database::{CacheMap, register_loaded_currencies},
    enums::SerializationEncoding,
};
use nautilus_core::serialization::timestamps;
use nautilus_model::{
    accounts::AccountAny,
    data::{CustomData, DataType, HasTsInit, InstrumentClose},
    enums::CurrencyType,
    events::{AccountState, OrderEventAny, OrderFilled, PositionSnapshot},
    identifiers::{AccountId, ClientId, ClientOrderId, InstrumentId, PositionId},
    instruments::{InstrumentAny, SyntheticInstrument},
    orders::{Order, OrderAny},
    position::Position,
    types::Currency,
};
#[cfg(feature = "defi")]
use nautilus_model::{
    defi::currency::{self, TokenCurrency},
    events::OrderSnapshot,
    instruments::Instrument,
};
use redis::{AsyncCommands, aio::ConnectionManager};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use ustr::Ustr;

use super::get_index_key;
#[cfg(feature = "defi")]
use crate::defi::{contract_conflict, integrity_error, redis_read_error, validate_marker};

/// Persisted representation of a [`Currency`].
///
/// A `Currency` serializes as its bare code, which only resolves in a process where that code is
/// already registered, so a currency minted at runtime could not be restored. This record carries
/// the same fields the Postgres `currency` table stores, keeping the two backends equivalent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CurrencyRecord {
    pub code: String,
    pub precision: u8,
    pub iso4217: u16,
    pub name: String,
    pub currency_type: CurrencyType,
    #[cfg(feature = "defi")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defi: Option<TokenCurrency>,
    #[cfg(not(feature = "defi"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defi: Option<Value>,
}

impl From<&Currency> for CurrencyRecord {
    fn from(currency: &Currency) -> Self {
        Self {
            code: currency.code.to_string(),
            precision: currency.precision,
            iso4217: currency.iso4217,
            name: currency.name.to_string(),
            currency_type: currency.currency_type,
            #[cfg(feature = "defi")]
            defi: currency::lookup(currency.code).map(|(_, metadata)| metadata),
            #[cfg(not(feature = "defi"))]
            defi: None,
        }
    }
}

impl TryFrom<CurrencyRecord> for Currency {
    type Error = anyhow::Error;

    fn try_from(record: CurrencyRecord) -> Result<Self, Self::Error> {
        let currency = Self::new_checked(
            record.code.as_str(),
            record.precision,
            record.iso4217,
            record.name.as_str(),
            record.currency_type,
        )
        .map_err(|e| anyhow::anyhow!("Invalid currency record: {e}"))?;

        #[cfg(feature = "defi")]
        if let Some(metadata) = record.defi {
            return currency::register(currency, metadata).map_err(integrity_error);
        } else if currency::lookup(currency.code).is_some() {
            return Err(integrity_error(format!(
                "Missing token metadata for {}",
                currency.code
            )));
        }

        #[cfg(not(feature = "defi"))]
        if record.defi.is_some() {
            anyhow::bail!("DeFi currency records require the defi feature");
        }

        Ok(currency)
    }
}

/// Outcome of decoding one persisted currency entry.
enum CurrencyDecode {
    Currency(Currency),
    /// A record, or a payload that is not a bare code, that does not yield a valid currency.
    InvalidRecord(anyhow::Error),
    /// A bare code that is not registered, so the entry holds nothing recoverable.
    Undecodable(anyhow::Error),
}

// Collection keys
const INDEX: &str = "index";
const GENERAL: &str = "general";
const CURRENCIES: &str = "currencies";
const INSTRUMENTS: &str = "instruments";
const INSTRUMENT_CLOSES: &str = "instrument_closes";
const SYNTHETICS: &str = "synthetics";
const ACCOUNTS: &str = "accounts";
const ORDERS: &str = "orders";
const POSITIONS: &str = "positions";
const SNAPSHOTS: &str = "snapshots";
const ACTORS: &str = "actors";
const STRATEGIES: &str = "strategies";
const CUSTOM: &str = "custom";
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
        Self::serialize_payload_with_timestamps(encoding, payload, true)
    }

    pub(crate) fn serialize_payload_with_timestamps<T: Serialize>(
        encoding: SerializationEncoding,
        payload: &T,
        timestamps_as_iso8601: bool,
    ) -> anyhow::Result<Vec<u8>> {
        let mut value = serde_json::to_value(payload)?;
        timestamps::serialize(&mut value, timestamps_as_iso8601)?;
        match encoding {
            SerializationEncoding::MsgPack => rmp_serde::to_vec(&value)
                .map_err(|e| anyhow::anyhow!("Failed to serialize msgpack `payload`: {e}")),
            SerializationEncoding::Json => serde_json::to_vec(&value)
                .map_err(|e| anyhow::anyhow!("Failed to serialize json `payload`: {e}")),
            SerializationEncoding::Sbe => {
                anyhow::bail!("SBE encoding is not supported for Redis cache payloads")
            }
        }
    }

    /// Deserializes the given byte slice `payload` into type `T` using the specified `encoding`.
    ///
    /// Timestamp-like keys in general metadata also convert numeric and ISO strings to integers.
    ///
    /// # Errors
    ///
    /// Returns an error if deserialization from the chosen encoding fails or converting to the target type fails.
    pub fn deserialize_payload<T: DeserializeOwned>(
        encoding: SerializationEncoding,
        payload: &[u8],
    ) -> anyhow::Result<T> {
        let mut value = match encoding {
            SerializationEncoding::MsgPack => rmp_serde::from_slice(payload)
                .map_err(|e| anyhow::anyhow!("Failed to deserialize msgpack `payload`: {e}"))?,
            SerializationEncoding::Json => serde_json::from_slice(payload)
                .map_err(|e| anyhow::anyhow!("Failed to deserialize json `payload`: {e}"))?,
            SerializationEncoding::Sbe => {
                anyhow::bail!("SBE encoding is not supported for Redis cache payloads")
            }
        };

        timestamps::deserialize(&mut value)?;

        deserialize_payload_value(value)
    }

    #[cfg(feature = "defi")]
    #[allow(
        clippy::too_many_arguments,
        reason = "the atomic write checks token definitions and all instrument currency roles"
    )]
    pub(crate) async fn write_defi(
        con: &mut ConnectionManager,
        trader_key: &str,
        encoding: SerializationEncoding,
        key: &str,
        codes: &[String],
        payloads: &[Bytes],
        references: &[String],
        migration: Option<&InstrumentMigration>,
    ) -> anyhow::Result<()> {
        let instrument = key.starts_with("instruments:");
        if payloads.len() != codes.len() + usize::from(instrument) {
            return Err(integrity_error("Invalid DeFi write payload count"));
        }

        let mut keys = codes
            .iter()
            .map(|code| format!("{trader_key}:currencies:{code}"))
            .collect::<Vec<_>>();

        if instrument {
            keys.push(format!("{trader_key}:{key}"));
        }

        let contracts = Self::contract_bindings(trader_key, encoding, &payloads[..codes.len()])?;
        let contract_offset = keys.len();
        keys.extend(contracts.iter().map(|(key, _)| key.clone()));
        let write_count = keys.len();
        let references = references
            .iter()
            .filter(|code| !codes.contains(code))
            .collect::<Vec<_>>();
        let reference_offset = keys.len();
        keys.extend(
            references
                .iter()
                .map(|code| format!("{trader_key}:currencies:{code}")),
        );

        for _ in 0..4 {
            let previous = Self::read_bulk(con, &keys)
                .await
                .map_err(redis_read_error)?;
            let mut replacement =
                Self::check_currency_records(encoding, codes, payloads, &previous)?;

            Self::check_contract_bindings(
                con,
                trader_key,
                encoding,
                &contracts,
                &previous[contract_offset..reference_offset],
            )
            .await?;

            Self::check_currency_references(encoding, &previous[reference_offset..])?;

            if instrument {
                Self::check_instrument_replacement(
                    con,
                    trader_key,
                    encoding,
                    previous[codes.len()].as_ref(),
                    &payloads[codes.len()],
                    codes,
                    migration,
                )
                .await?;
                replacement.push(payloads[codes.len()].clone());
            }

            replacement.extend(
                contracts
                    .iter()
                    .map(|(_, code)| Bytes::copy_from_slice(code.as_bytes())),
            );

            replacement.resize(keys.len(), Bytes::new());
            // Compare every snapshot before the single MSET, so a conflict writes nothing
            let mut command = redis::cmd("EVAL");
            command
                .arg(
                    "for i = 1, #KEYS do
                    -- Keep command error codes intact on Redis 6.x
                    local value = redis.pcall('GET', KEYS[i])
                    if type(value) == 'table' and value.err then return value end
                    local offset = 1 + (i - 1) * 3
                    if ARGV[offset + 1] == '0' then
                        if value then return 0 end
                    elseif value ~= ARGV[offset + 2] then
                        return 0
                    end
                end
                local values = {}
                for i = 1, tonumber(ARGV[1]) do
                    values[#values + 1] = KEYS[i]
                    values[#values + 1] = ARGV[(i - 1) * 3 + 4]
                end
                redis.call('MSET', unpack(values))
                return 1
            ",
                )
                .arg(keys.len())
                .arg(&keys)
                .arg(write_count);

            for (old, new) in previous.iter().zip(&replacement) {
                command
                    .arg(if old.is_some() { "1" } else { "0" })
                    .arg(old.as_deref().unwrap_or_default())
                    .arg(new.as_ref());
            }

            let written: i32 = command.query_async(con).await.map_err(redis_read_error)?;
            if written == 1 {
                return Ok(());
            }
        }

        anyhow::bail!("Token dependencies changed concurrently; retry the write")
    }

    #[cfg(feature = "defi")]
    fn check_currency_references(
        encoding: SerializationEncoding,
        previous: &[Option<Bytes>],
    ) -> anyhow::Result<()> {
        for bytes in previous.iter().flatten() {
            let value: Value =
                Self::deserialize_payload(encoding, bytes).map_err(integrity_error)?;

            if value.get("defi").is_some_and(|value| !value.is_null()) {
                return Err(integrity_error(
                    "Instrument token dependencies differ from persisted currency definitions",
                ));
            }
        }

        Ok(())
    }

    #[cfg(feature = "defi")]
    fn contract_bindings(
        trader_key: &str,
        encoding: SerializationEncoding,
        payloads: &[Bytes],
    ) -> anyhow::Result<Vec<(String, String)>> {
        let mut bindings = Vec::new();

        for payload in payloads {
            let record: CurrencyRecord =
                Self::deserialize_payload(encoding, payload).map_err(integrity_error)?;

            if let Some(metadata) = record.defi {
                bindings.push((Self::contract_key(trader_key, &metadata), record.code));
            }
        }

        Ok(bindings)
    }

    #[cfg(feature = "defi")]
    fn contract_key(trader_key: &str, metadata: &TokenCurrency) -> String {
        format!(
            "{trader_key}:defi:contracts:{}:{}",
            metadata.chain_id, metadata.address
        )
    }

    #[cfg(feature = "defi")]
    async fn check_contract_bindings(
        con: &mut ConnectionManager,
        trader_key: &str,
        encoding: SerializationEncoding,
        bindings: &[(String, String)],
        previous: &[Option<Bytes>],
    ) -> anyhow::Result<()> {
        let mut missing = AHashMap::new();

        for ((key, code), old) in bindings.iter().zip(previous) {
            if let Some(old) = old {
                let stored = std::str::from_utf8(old).map_err(integrity_error)?;
                if stored != code {
                    return Err(contract_conflict(stored));
                }
            } else {
                missing.insert(key.as_str(), code.as_str());
            }
        }

        if missing.is_empty() {
            return Ok(());
        }

        Self::check_unindexed_contracts(con, trader_key, encoding, &missing).await
    }

    #[cfg(feature = "defi")]
    async fn check_unindexed_contracts(
        con: &mut ConnectionManager,
        trader_key: &str,
        encoding: SerializationEncoding,
        missing: &AHashMap<&str, &str>,
    ) -> anyhow::Result<()> {
        let keys = Self::scan_keys(con, format!("{trader_key}:currencies:*"))
            .await
            .map_err(redis_read_error)?;

        for bytes in Self::read_bulk(con, &keys)
            .await
            .map_err(redis_read_error)?
            .into_iter()
            .flatten()
        {
            let value: Value =
                Self::deserialize_payload(encoding, &bytes).map_err(integrity_error)?;

            if value.get("defi").is_none_or(Value::is_null) {
                continue;
            }

            let record: CurrencyRecord = serde_json::from_value(value).map_err(integrity_error)?;
            let metadata = record
                .defi
                .ok_or_else(|| integrity_error("Missing token metadata"))?;
            let key = Self::contract_key(trader_key, &metadata);
            if let Some(code) = missing.get(key.as_str())
                && *code != record.code
            {
                return Err(contract_conflict(&record.code));
            }
        }

        Ok(())
    }

    #[cfg(feature = "defi")]
    fn check_currency_records(
        encoding: SerializationEncoding,
        codes: &[String],
        payloads: &[Bytes],
        previous: &[Option<Bytes>],
    ) -> anyhow::Result<Vec<Bytes>> {
        let mut replacement = payloads[..codes.len()].to_vec();
        for (i, code) in codes.iter().enumerate() {
            let incoming: CurrencyRecord =
                Self::deserialize_payload(encoding, &payloads[i]).map_err(integrity_error)?;

            if incoming.code != *code {
                return Err(integrity_error(format!(
                    "Invalid currency record for {code}"
                )));
            }

            let token = incoming.defi.is_some();
            Currency::try_from(incoming).map_err(integrity_error)?;

            let Some(bytes) = &previous[i] else {
                continue;
            };

            let value: Value =
                Self::deserialize_payload(encoding, bytes).map_err(integrity_error)?;

            if !token && value.as_str() == Some(code.as_str()) {
                continue;
            }

            let stored: CurrencyRecord = serde_json::from_value(value).map_err(integrity_error)?;

            if stored.code != *code || stored.defi.is_some() != token {
                return Err(integrity_error(format!(
                    "Currency provenance conflicts at token code {code}"
                )));
            }

            Currency::try_from(stored).map_err(integrity_error)?;

            if token {
                replacement[i] = bytes.clone();
            }
        }

        Ok(replacement)
    }

    #[cfg(feature = "defi")]
    async fn check_instrument_replacement(
        con: &mut ConnectionManager,
        trader_key: &str,
        encoding: SerializationEncoding,
        previous: Option<&Bytes>,
        incoming: &Bytes,
        codes: &[String],
        migration: Option<&InstrumentMigration>,
    ) -> anyhow::Result<()> {
        let Some(bytes) = previous else {
            return Ok(());
        };

        let value: Value = Self::deserialize_payload(encoding, bytes).map_err(integrity_error)?;
        if value.get("instrument").is_some() || value.get("defi_currency_codes").is_some() {
            let record: crate::defi::InstrumentRecord =
                serde_json::from_value(value).map_err(integrity_error)?;

            if record.defi_currency_codes != codes {
                return Err(integrity_error(
                    "Cannot change a persisted token instrument's currency identities",
                ));
            }

            let previous: InstrumentAny =
                serde_json::from_value(record.instrument).map_err(integrity_error)?;
            let record: crate::defi::InstrumentRecord =
                Self::deserialize_payload(encoding, incoming).map_err(integrity_error)?;
            let incoming: InstrumentAny =
                serde_json::from_value(record.instrument).map_err(integrity_error)?;

            if previous.base_currency() != incoming.base_currency()
                || previous.quote_currency() != incoming.quote_currency()
                || previous.settlement_currency() != incoming.settlement_currency()
            {
                return Err(integrity_error(
                    "Cannot change a persisted token instrument's currency identities",
                ));
            }

            return Ok(());
        }

        if codes.is_empty() {
            return Ok(());
        }

        let legacy: InstrumentAny = serde_json::from_value(value).map_err(integrity_error)?;
        migration
            .ok_or_else(|| {
                integrity_error("Historical replacement requires the cache migration check")
            })?
            .validate()
            .map_err(integrity_error)?;

        Self::check_legacy_replacement(con, trader_key, encoding, legacy.id()).await
    }

    #[cfg(feature = "defi")]
    async fn check_legacy_replacement(
        con: &mut ConnectionManager,
        trader_key: &str,
        encoding: SerializationEncoding,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<()> {
        Self::check_position_history(con, trader_key, encoding, instrument_id).await?;

        let prefix = format!("{trader_key}:orders:");

        for key in Self::scan_keys(con, format!("{prefix}*"))
            .await
            .map_err(redis_read_error)?
        {
            let code = key
                .strip_prefix(&prefix)
                .ok_or_else(|| integrity_error("Invalid order key"))?;
            let id = ClientOrderId::new_checked(code).map_err(integrity_error)?;
            let order = Self::load_order(con, trader_key, &id, encoding)
                .await
                .map_err(redis_read_error)?
                .ok_or_else(|| integrity_error(format!("Missing order state at {key}")))?;

            if order.instrument_id() == instrument_id && !order.is_closed() {
                return Err(integrity_error(format!(
                    "Instrument {instrument_id} has nonterminal orders"
                )));
            }
        }

        let prefix = format!("{trader_key}:snapshots:orders:");

        for key in Self::scan_keys(con, format!("{prefix}*"))
            .await
            .map_err(redis_read_error)?
        {
            let values: Vec<Vec<u8>> = con.lrange(&key, 0, -1).await.map_err(redis_read_error)?;
            for bytes in values {
                let snapshot: OrderSnapshot =
                    Self::deserialize_payload(encoding, &bytes).map_err(integrity_error)?;

                if snapshot.instrument_id == instrument_id && !snapshot.status.is_closed() {
                    return Err(integrity_error(format!(
                        "Instrument {instrument_id} has nonterminal order snapshots"
                    )));
                }
            }
        }

        Ok(())
    }

    #[cfg(feature = "defi")]
    async fn check_position_history(
        con: &mut ConnectionManager,
        trader_key: &str,
        encoding: SerializationEncoding,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<()> {
        let expected_id = instrument_id.to_string();

        for collection in ["positions", "snapshots:positions"] {
            let prefix = format!("{trader_key}:{collection}:");

            for key in Self::scan_keys(con, format!("{prefix}*"))
                .await
                .map_err(redis_read_error)?
            {
                let values: Vec<Vec<u8>> =
                    con.lrange(&key, 0, -1).await.map_err(redis_read_error)?;

                if values.is_empty() {
                    return Err(integrity_error(format!(
                        "Unreadable position history at {key}"
                    )));
                }

                for bytes in values {
                    let value: Value =
                        Self::deserialize_payload(encoding, &bytes).map_err(integrity_error)?;

                    let id = value
                        .get("instrument_id")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            integrity_error(format!(
                                "Missing instrument ID in position history at {key}"
                            ))
                        })?;

                    if id == expected_id {
                        return Err(integrity_error(format!(
                            "Instrument {instrument_id} has position history; retain its legacy currencies"
                        )));
                    }
                }
            }
        }

        Self::check_position_archives(con, trader_key, instrument_id).await
    }

    #[cfg(feature = "defi")]
    async fn check_position_archives(
        con: &mut ConnectionManager,
        trader_key: &str,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<()> {
        let prefix = format!("{trader_key}:general:{}", CacheSnapshotRef::POSITION_PREFIX);

        for key in Self::scan_keys(con, format!("{prefix}*"))
            .await
            .map_err(redis_read_error)?
        {
            let blob: Vec<u8> = con.get(&key).await.map_err(redis_read_error)?;
            if CacheSnapshotRef::position_instrument_id(&blob).map_err(integrity_error)?
                == instrument_id
            {
                return Err(integrity_error(format!(
                    "Instrument {instrument_id} has position history; retain its legacy currencies"
                )));
            }
        }

        Ok(())
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
        let mut result = Vec::new();
        let mut cursor = 0u64;

        loop {
            let scan_result: (u64, Vec<String>) = redis::cmd("SCAN")
                .arg(cursor)
                .arg("MATCH")
                .arg(&pattern)
                .arg("COUNT")
                .arg(5000)
                .query_async(con)
                .await?;

            let (new_cursor, keys) = scan_result;
            result.extend(keys);

            // If cursor is 0, we've completed the full scan
            if new_cursor == 0 {
                break;
            }

            cursor = new_cursor;
        }

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
        if keys.is_empty() {
            return Ok(vec![]);
        }

        let mut con = con.clone();

        // Use MGET to fetch all keys in a single network operation
        let results: Vec<Option<Vec<u8>>> =
            redis::cmd("MGET").arg(keys).query_async(&mut con).await?;

        // Convert Vec<u8> to Bytes
        let bytes_results: Vec<Option<Bytes>> = results
            .into_iter()
            .map(|opt| opt.map(Bytes::from))
            .collect();

        Ok(bytes_results)
    }

    /// Bulk reads multiple keys from Redis using MGET, batched into chunks.
    ///
    /// Keys are batched into chunks of `batch_size` to avoid exceeding Redis
    /// request size limits on some providers.
    ///
    /// # Errors
    ///
    /// Returns an error if `batch_size` is zero or if the underlying Redis MGET operation fails.
    pub async fn read_bulk_batched(
        con: &ConnectionManager,
        keys: &[String],
        batch_size: usize,
    ) -> anyhow::Result<Vec<Option<Bytes>>> {
        if batch_size == 0 {
            anyhow::bail!("`batch_size` must be greater than zero");
        }

        if keys.is_empty() {
            return Ok(vec![]);
        }

        let mut all_results: Vec<Option<Bytes>> = Vec::with_capacity(keys.len());

        for chunk in keys.chunks(batch_size) {
            let mut con = con.clone();

            let results: Vec<Option<Vec<u8>>> =
                redis::cmd("MGET").arg(chunk).query_async(&mut con).await?;

            all_results.extend(results.into_iter().map(|opt| opt.map(Bytes::from)));
        }

        Ok(all_results)
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
        let collection = Self::get_collection_key(key)?;
        let full_key = format!("{trader_key}{REDIS_DELIMITER}{key}");

        let mut con = con.clone();

        match collection {
            INDEX => Self::read_index(&mut con, &full_key).await,
            GENERAL | CURRENCIES | INSTRUMENTS | INSTRUMENT_CLOSES | SYNTHETICS | ACTORS
            | STRATEGIES => Self::read_string(&mut con, &full_key).await,
            ACCOUNTS | ORDERS | POSITIONS | SNAPSHOTS => Self::read_list(&mut con, &full_key).await,
            _ => anyhow::bail!("Unsupported operation: `read` for collection '{collection}'"),
        }
    }

    /// Loads the supported cache records for `trader_key`.
    ///
    /// Greeks and yield curves are empty because Redis does not persist them.
    ///
    /// # Errors
    ///
    /// Returns an error if loading any of the individual caches fails or combining data fails.
    pub async fn load_all(
        con: &ConnectionManager,
        encoding: SerializationEncoding,
        trader_key: &str,
    ) -> anyhow::Result<CacheMap> {
        Self::load_all_batched(con, encoding, trader_key, usize::MAX).await
    }

    pub(crate) async fn load_all_batched(
        con: &ConnectionManager,
        encoding: SerializationEncoding,
        trader_key: &str,
        batch_size: usize,
    ) -> anyhow::Result<CacheMap> {
        // Currencies must be registered before the dependent payloads decode, because a `Money`
        // or a `Currency` in them resolves its code through the global registry.
        let mut currencies =
            Self::load_currencies_batched(con, trader_key, encoding, batch_size).await?;
        register_loaded_currencies(&mut currencies)?;

        let (instruments, instrument_closes, synthetics, accounts, orders, positions) =
            tokio::try_join!(
                Self::load_instruments(con, trader_key, encoding),
                Self::load_instrument_closes_batched(con, trader_key, encoding, batch_size),
                Self::load_synthetics(con, trader_key, encoding),
                Self::load_accounts(con, trader_key, encoding),
                Self::load_orders(con, trader_key, encoding),
                Self::load_positions(con, trader_key, encoding)
            )
            .map_err(|e| anyhow::anyhow!("Error loading cache data: {e}"))?;

        Ok(CacheMap {
            currencies,
            instruments,
            instrument_closes,
            synthetics,
            accounts,
            orders,
            positions,
            greeks: AHashMap::new(),
            yield_curves: AHashMap::new(),
        })
    }

    /// Decodes a persisted currency, accepting both the record and a bare code.
    ///
    /// A bare-code entry holds only the code, which resolves if that code is registered; an
    /// unregistered one is skipped by bulk loading, since the entry holds nothing recoverable. Anything
    /// that is not a bare code is a record, and a record that does not decode or describes an
    /// invalid currency is an error, since the intent is recoverable and dropping it would omit a
    /// currency the dependent payloads need.
    fn decode_currency(
        encoding: SerializationEncoding,
        value_bytes: &Bytes,
        code: Ustr,
    ) -> CurrencyDecode {
        let record =
            Self::deserialize_payload::<Value>(encoding, value_bytes).and_then(|mut value| {
                if let Some(object) = value.as_object_mut()
                    && !object.contains_key("code")
                {
                    object.insert("code".into(), Value::String(code.to_string()));
                }

                serde_json::from_value::<CurrencyRecord>(value).map_err(Into::into)
            });

        let record_err = match record {
            Ok(record) => {
                return match Currency::try_from(record) {
                    Ok(currency) => CurrencyDecode::Currency(currency),
                    Err(e) => CurrencyDecode::InvalidRecord(e),
                };
            }
            Err(e) => e,
        };

        match Self::deserialize_payload::<String>(encoding, value_bytes) {
            Ok(code) => match Currency::try_from_str(&code) {
                Some(currency) => {
                    #[cfg(feature = "defi")]
                    if currency::lookup(currency.code).is_some() {
                        return CurrencyDecode::InvalidRecord(integrity_error(format!(
                            "Missing token metadata for {}",
                            currency.code,
                        )));
                    }

                    CurrencyDecode::Currency(currency)
                }
                None => CurrencyDecode::Undecodable(anyhow::anyhow!(
                    "bare currency code {code} is not registered"
                )),
            },
            Err(_) => CurrencyDecode::InvalidRecord(anyhow::anyhow!(
                "payload is not a bare code and does not decode as a currency record: {record_err}"
            )),
        }
    }

    /// Loads all currencies for `trader_key` using the specified `encoding`.
    ///
    /// # Errors
    ///
    /// Returns an error if scanning keys or reading currency data fails, or if a stored record
    /// describes an invalid currency.
    pub async fn load_currencies(
        con: &ConnectionManager,
        trader_key: &str,
        encoding: SerializationEncoding,
    ) -> anyhow::Result<AHashMap<Ustr, Currency>> {
        Self::load_currencies_batched(con, trader_key, encoding, usize::MAX).await
    }

    pub(crate) async fn load_currencies_batched(
        con: &ConnectionManager,
        trader_key: &str,
        encoding: SerializationEncoding,
        batch_size: usize,
    ) -> anyhow::Result<AHashMap<Ustr, Currency>> {
        let mut currencies = AHashMap::new();
        let prefix = format!("{trader_key}{REDIS_DELIMITER}{CURRENCIES}{REDIS_DELIMITER}");
        let pattern = format!("{prefix}*");
        log::debug!("Loading {pattern}");

        let mut con = con.clone();
        let keys = Self::scan_keys(&mut con, pattern).await?;

        if keys.is_empty() {
            return Ok(currencies);
        }

        // Use bulk loading with MGET for efficiency
        let bulk_values = Self::read_bulk_batched(&con, &keys, batch_size).await?;

        // Process the bulk results
        for (key, value_opt) in keys.iter().zip(bulk_values.iter()) {
            // A code can contain the delimiter (Hyperliquid HIP-3 assets such as `xyz:TSLA`), so
            // the code is everything after the collection prefix, not the last segment.
            let currency_code = match key.as_str().strip_prefix(prefix.as_str()) {
                Some(code) if !code.is_empty() => Ustr::from(code),
                _ => {
                    log::error!("Invalid key format: {key}");
                    continue;
                }
            };

            if let Some(value_bytes) = value_opt {
                match Self::decode_currency(encoding, value_bytes, currency_code) {
                    CurrencyDecode::Currency(currency) => {
                        // The key names the code the dependents resolve, so a record filed under
                        // another code would register one currency and index another.
                        if currency.code != currency_code {
                            anyhow::bail!(
                                "Currency record under key {currency_code} holds code {}",
                                currency.code
                            );
                        }
                        currencies.insert(currency_code, currency);
                    }
                    CurrencyDecode::InvalidRecord(e) => {
                        anyhow::bail!("Invalid currency record for {currency_code}: {e}");
                    }
                    CurrencyDecode::Undecodable(e) => {
                        log::error!(
                            "Skipping currency {currency_code}: {e}; payloads denominated in it will not decode"
                        );
                    }
                }
            } else {
                log::error!("Currency not found in Redis: {currency_code}");
            }
        }

        log::debug!("Loaded {} currencies(s)", currencies.len());

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
        let prefix = format!("{trader_key}{REDIS_DELIMITER}{INSTRUMENTS}{REDIS_DELIMITER}");
        let pattern = format!("{prefix}*");
        log::debug!("Loading {pattern}");

        let mut con = con.clone();
        let keys = Self::scan_keys(&mut con, pattern).await?;

        let futures: Vec<_> = keys
            .iter()
            .map(|key| {
                let con = con.clone();
                let prefix = &prefix;
                async move {
                    let instrument_id = parse_instrument_key(key, prefix);

                    let Ok(instrument_id) = instrument_id else {
                        log::error!("Failed to parse InstrumentId from Redis key: {key}");
                        return Ok::<_, anyhow::Error>(None);
                    };

                    match Self::load_instrument(&con, trader_key, &instrument_id, encoding).await {
                        Ok(Some(instrument)) => Ok(Some((instrument_id, instrument))),
                        Ok(None) => {
                            log::error!("Instrument not found: {instrument_id}");
                            Ok(None)
                        }
                        Err(e) => {
                            #[cfg(feature = "defi")]
                            if e.is::<crate::defi::IntegrityError>() {
                                return Err(e);
                            }

                            log::error!("Failed to load instrument {instrument_id}: {e}");
                            Ok(None)
                        }
                    }
                }
            })
            .collect();

        // Insert all Instrument_id (key) and Instrument (value) into the HashMap, filtering out None values.
        for result in join_all(futures).await {
            instruments.extend(result?);
        }

        log::debug!("Loaded {} instruments(s)", instruments.len());

        Ok(instruments)
    }

    /// Loads all instrument closes for `trader_key`.
    ///
    /// Missing or invalid close data fails the load so recovery cannot silently omit a close.
    ///
    /// # Errors
    ///
    /// Returns an error if scanning, reading, parsing, or deserializing instrument closes fails.
    pub async fn load_instrument_closes(
        con: &ConnectionManager,
        trader_key: &str,
        encoding: SerializationEncoding,
    ) -> anyhow::Result<AHashMap<InstrumentId, InstrumentClose>> {
        Self::load_instrument_closes_batched(con, trader_key, encoding, usize::MAX).await
    }

    pub(crate) async fn load_instrument_closes_batched(
        con: &ConnectionManager,
        trader_key: &str,
        encoding: SerializationEncoding,
        batch_size: usize,
    ) -> anyhow::Result<AHashMap<InstrumentId, InstrumentClose>> {
        let prefix = format!("{trader_key}{REDIS_DELIMITER}{INSTRUMENT_CLOSES}{REDIS_DELIMITER}");
        let pattern = format!("{prefix}*");
        log::debug!("Loading {pattern}");

        let mut con = con.clone();
        let keys = Self::scan_keys(&mut con, pattern).await?;
        let values = Self::read_bulk_batched(&con, &keys, batch_size).await?;
        let mut closes = AHashMap::with_capacity(keys.len());

        for (key, value) in keys.into_iter().zip(values) {
            let instrument_id = parse_instrument_key(&key, &prefix)?;
            let value = value
                .ok_or_else(|| anyhow::anyhow!("Instrument close not found in Redis: {key}"))?;
            let close: InstrumentClose = Self::deserialize_payload(encoding, &value)?;
            anyhow::ensure!(
                close.instrument_id == instrument_id,
                "Instrument close key ID {instrument_id} did not match payload ID {}",
                close.instrument_id,
            );
            closes.insert(instrument_id, close);
        }

        log::debug!("Loaded {} instrument close(s)", closes.len());
        Ok(closes)
    }

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
        log::debug!("Loading {pattern}");

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

                    let Ok(instrument_id) = instrument_id else {
                        return None;
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
        log::debug!("Loaded {} synthetics(s)", synthetics.len());

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
        log::debug!("Loading {pattern}");

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
        log::debug!("Loaded {} accounts(s)", accounts.len());

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
        log::debug!("Loading {pattern}");

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
        log::debug!("Loaded {} order(s)", orders.len());

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
        log::debug!("Loading {pattern}");

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
        log::debug!("Loaded {} position(s)", positions.len());

        Ok(positions)
    }

    /// Loads the order ID to position ID index for `trader_key`.
    ///
    /// # Errors
    ///
    /// Returns an error if reading or parsing the index fails.
    pub async fn load_index_order_position(
        con: &ConnectionManager,
        trader_key: &str,
    ) -> anyhow::Result<AHashMap<ClientOrderId, PositionId>> {
        let index = Self::read_index_hash(con, trader_key, INDEX_ORDER_POSITION).await?;
        Ok(index
            .into_iter()
            .map(|(k, v)| {
                (
                    ClientOrderId::from(k.as_str()),
                    PositionId::from(v.as_str()),
                )
            })
            .collect())
    }

    /// Loads the order ID to execution client ID index for `trader_key`.
    ///
    /// # Errors
    ///
    /// Returns an error if reading or parsing the index fails.
    pub async fn load_index_order_client(
        con: &ConnectionManager,
        trader_key: &str,
    ) -> anyhow::Result<AHashMap<ClientOrderId, ClientId>> {
        let index = Self::read_index_hash(con, trader_key, INDEX_ORDER_CLIENT).await?;
        Ok(index
            .into_iter()
            .map(|(k, v)| (ClientOrderId::from(k.as_str()), ClientId::from(v.as_str())))
            .collect())
    }

    async fn read_index_hash(
        con: &ConnectionManager,
        trader_key: &str,
        key: &str,
    ) -> anyhow::Result<HashMap<String, String>> {
        let result = Self::read(con, trader_key, key).await?;
        if result.is_empty() {
            return Ok(HashMap::new());
        }

        serde_json::from_slice(&result[0])
            .map_err(|e| anyhow::anyhow!("Failed to parse index hash '{key}': {e}"))
    }

    /// Loads all custom data for `trader_key` matching the given `data_type`.
    ///
    /// Keys are stored as `custom:<ts_init_020>:<uuid>`; value is full `CustomData` JSON.
    /// Scans all custom keys, deserializes, filters by `type_name` (full or short), metadata,
    /// and identifier to match SQL semantics, then sorts by `ts_init` ascending.
    ///
    /// # Errors
    ///
    /// Returns an error if scanning, bulk read, or deserialization fails.
    pub async fn load_custom_data(
        con: &ConnectionManager,
        trader_key: &str,
        data_type: &DataType,
    ) -> anyhow::Result<Vec<CustomData>> {
        Self::load_custom_data_batched(con, trader_key, data_type, usize::MAX).await
    }

    pub(crate) async fn load_custom_data_batched(
        con: &ConnectionManager,
        trader_key: &str,
        data_type: &DataType,
        batch_size: usize,
    ) -> anyhow::Result<Vec<CustomData>> {
        let pattern = format!("{trader_key}{REDIS_DELIMITER}{CUSTOM}*");
        log::debug!("Loading custom data {pattern}");

        let mut con = con.clone();
        let keys = Self::scan_keys(&mut con, pattern).await?;

        if keys.is_empty() {
            return Ok(Vec::new());
        }

        let values = Self::read_bulk_batched(&con, &keys, batch_size).await?;
        let request_type_name = data_type.type_name();
        let request_short = request_type_name
            .rsplit([':', '.'])
            .next()
            .unwrap_or(request_type_name);
        let request_identifier = data_type.identifier().unwrap_or("");

        let mut results = Vec::new();

        for value_opt in values {
            let Some(value_bytes) = value_opt else {
                continue;
            };
            let custom = match CustomData::from_json_bytes(value_bytes.as_ref()) {
                Ok(c) => c,
                Err(e) => {
                    log::warn!("Failed to deserialize custom data from Redis: {e}");
                    continue;
                }
            };
            let stored_type_name = custom.data_type.type_name();
            let type_match =
                stored_type_name == request_type_name || stored_type_name == request_short;
            let identifier_match =
                custom.data_type.identifier().unwrap_or("") == request_identifier;
            let metadata_match = match (data_type.metadata(), custom.data_type.metadata()) {
                (None, None) => true,
                (Some(a), Some(b)) => serde_json::to_value(a).ok() == serde_json::to_value(b).ok(),
                _ => false,
            };

            if type_match && identifier_match && metadata_match {
                results.push(custom);
            }
        }

        results.sort_by_key(HasTsInit::ts_init);
        log::debug!("Loaded {} custom data item(s)", results.len());
        Ok(results)
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
        let key = format!("{CURRENCIES}{REDIS_DELIMITER}{code}");
        let result = Self::read(con, trader_key, &key).await?;

        if result.is_empty() {
            return Ok(None);
        }

        match Self::decode_currency(encoding, &result[0], *code) {
            CurrencyDecode::Currency(currency) => Ok(Some(currency)),
            CurrencyDecode::InvalidRecord(e) | CurrencyDecode::Undecodable(e) => {
                Err(e.context(format!("Failed to load currency {code}")))
            }
        }
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

        #[cfg(feature = "defi")]
        {
            let value: Value = Self::deserialize_payload(encoding, &result[0])?;
            if value.get("defi_currency_codes").is_some() || value.get("instrument").is_some() {
                let record: crate::defi::InstrumentRecord =
                    serde_json::from_value(value).map_err(integrity_error)?;

                if record.defi_currency_codes.is_empty() {
                    return Err(integrity_error("Empty DeFi instrument dependency marker"));
                }

                for code in &record.defi_currency_codes {
                    let currency =
                        Self::load_currency(con, trader_key, &Ustr::from(code), encoding)
                            .await
                            .map_err(integrity_error)?
                            .ok_or_else(|| {
                                integrity_error(format!("Missing token currency {code}"))
                            })?;

                    if currency::lookup(currency.code).is_none() {
                        return Err(integrity_error(format!(
                            "Missing token metadata for {code}"
                        )));
                    }
                }

                let instrument: InstrumentAny =
                    serde_json::from_value(record.instrument).map_err(integrity_error)?;
                validate_marker(&instrument, &record.defi_currency_codes)?;
                if instrument.id() != *instrument_id {
                    return Err(integrity_error("DeFi instrument key and payload ID differ"));
                }

                return Ok(Some(instrument));
            }
        }

        let instrument: InstrumentAny = Self::deserialize_payload(encoding, &result[0])?;
        #[cfg(feature = "defi")]
        validate_marker(&instrument, &[])?;
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

        let events: Vec<AccountState> = result
            .iter()
            .map(|payload| Self::deserialize_payload(encoding, payload))
            .collect::<anyhow::Result<_>>()?;
        let account = AccountAny::from_events(&events)?;
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

        let order = Self::deserialize_order(encoding, &result)?;
        Ok(Some(order))
    }

    pub(crate) fn deserialize_order(
        encoding: SerializationEncoding,
        payloads: &[Bytes],
    ) -> anyhow::Result<OrderAny> {
        let (first, remaining) = payloads
            .split_first()
            .ok_or_else(|| anyhow::anyhow!("Cannot restore an order without initialization"))?;
        let initial = Self::deserialize_payload(encoding, first)?;
        let mut order = OrderAny::from_events(vec![initial])?;

        for payload in remaining {
            let value: Value = Self::deserialize_payload(encoding, payload)?;
            let legacy_init = value.get("type").and_then(Value::as_str) == Some("OrderInitialized");
            let event = deserialize_payload_value(value)?;
            if legacy_init {
                order = transform_order(&order, event)?;
            } else {
                order.apply(event)?;
            }
        }

        Ok(order)
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
        let snapshot_key =
            format!("{SNAPSHOTS}{REDIS_DELIMITER}{POSITIONS}{REDIS_DELIMITER}{position_id}");
        let snapshots = Self::read(con, trader_key, &snapshot_key).await?;
        for payload in snapshots.iter().rev() {
            let snapshot: PositionSnapshot = Self::deserialize_payload(encoding, payload)?;
            if let Some(replay_state) = snapshot.replay_state {
                let position: Position = serde_json::from_value(replay_state)
                    .map_err(|e| anyhow::anyhow!("Failed to decode position replay state: {e}"))?;
                position.check_accounting()?;
                return Ok(Some(position));
            }
        }

        let key = format!("{POSITIONS}{REDIS_DELIMITER}{position_id}");
        let result = Self::read(con, trader_key, &key).await?;
        if result.is_empty() {
            return Ok(None);
        }

        let fills: Vec<OrderFilled> = result
            .iter()
            .map(|payload| Self::deserialize_payload(encoding, payload))
            .collect::<anyhow::Result<_>>()?;
        let Some((first_fill, remaining_fills)) = fills.split_first() else {
            return Ok(None);
        };
        let Some(instrument) =
            Self::load_instrument(con, trader_key, &first_fill.instrument_id, encoding).await?
        else {
            log::error!(
                "Instrument not found for position {position_id}: {}",
                first_fill.instrument_id
            );
            return Ok(None);
        };

        let mut position = Position::new_checked(&instrument, first_fill.clone())?;
        for fill in remaining_fills {
            if position.trade_ids.contains(&fill.trade_id) {
                anyhow::bail!(
                    "Duplicate fill event for position {position_id}: {}",
                    fill.trade_id
                );
            }

            position.try_apply(fill)?;
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
            | INDEX_POSITIONS_CLOSED => Self::read_set(conn, key).await,
            INDEX_ORDER_POSITION | INDEX_ORDER_CLIENT => Self::read_hset(conn, key).await,
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
}

fn transform_order(order: &OrderAny, event: OrderEventAny) -> anyhow::Result<OrderAny> {
    let history = order.events();
    anyhow::ensure!(
        !history.contains(&&event),
        "Duplicate initialization in legacy order history"
    );
    let mut transformed = OrderAny::from_events(vec![event])?;
    anyhow::ensure!(
        (
            transformed.trader_id(),
            transformed.strategy_id(),
            transformed.instrument_id(),
            transformed.client_order_id()
        ) == (
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id()
        ),
        "Legacy order transformation changes the order identity"
    );
    let history = history.into_iter().cloned();

    match &mut transformed {
        OrderAny::Market(target) => {
            target.ts_init = order.ts_init();
            target.prepend_events(history);
        }
        OrderAny::Limit(target) => {
            target.ts_init = order.ts_init();
            target.liquidity_side = order.liquidity_side();
            target.prepend_events(history);
        }
        _ => anyhow::bail!("Legacy order transformation must produce a market or limit order"),
    }

    Ok(transformed)
}

fn deserialize_payload_value<T: DeserializeOwned>(mut value: Value) -> anyhow::Result<T> {
    normalize_account_state(&mut value);
    if let Some(kind) = value
        .get("type")
        .and_then(Value::as_str)
        .and_then(legacy_variant)
    {
        let kind = kind.to_string();
        if kind == "Initialized" {
            normalize_order_options(&mut value);
        }

        if let Some(object) = value.as_object_mut()
            && let Some(tick_scheme) = object.remove("tick_scheme_name")
        {
            object.entry("tick_scheme").or_insert(tick_scheme);
        }

        if let Ok(payload) = T::deserialize(&value) {
            return Ok(payload);
        }

        value = Value::Object(serde_json::Map::from_iter([(kind, value)]));
    }

    serde_json::from_value(value)
        .map_err(|e| anyhow::anyhow!("Failed to convert value to target type: {e}"))
}

fn normalize_account_state(value: &mut Value) {
    if value.get("type").and_then(Value::as_str) != Some("AccountState") {
        return;
    }

    let Some(object) = value.as_object_mut() else {
        return;
    };

    let Some(reported) = object.remove("reported") else {
        return;
    };

    object.entry("is_reported").or_insert(reported);
    for field in ["balances", "margins"] {
        if let Some(Value::Array(balances)) = object.get_mut(field) {
            for balance in balances {
                normalize_balance(balance);
            }
        }
    }
}

fn normalize_balance(value: &mut Value) {
    let Some(balance) = value.as_object_mut() else {
        return;
    };

    let Some(code) = balance
        .get("currency")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return;
    };

    for field in ["total", "locked", "free", "initial", "maintenance"] {
        if let Some(Value::String(value)) = balance.get_mut(field)
            && !value.contains(' ')
        {
            value.push(' ');
            value.push_str(&code);
        }
    }
}

fn normalize_order_options(value: &mut Value) {
    let Some(object) = value.as_object_mut() else {
        return;
    };

    let Some(Value::Object(options)) = object.remove("options") else {
        return;
    };

    for (key, mut value) in options {
        let key = if key == "expire_time_ns" {
            if value.as_u64() == Some(0) {
                value = Value::Null;
            }

            "expire_time".into()
        } else {
            key
        };

        object.entry(key).or_insert(value);
    }
}

fn legacy_variant(kind: &str) -> Option<&str> {
    match kind {
        "BettingInstrument" => Some("Betting"),
        "BinaryOption"
        | "Cfd"
        | "Commodity"
        | "CryptoFuture"
        | "CryptoFuturesSpread"
        | "CryptoOption"
        | "CryptoOptionSpread"
        | "CryptoPerpetual"
        | "CurrencyPair"
        | "Equity"
        | "FuturesContract"
        | "FuturesSpread"
        | "IndexInstrument"
        | "OptionContract"
        | "OptionSpread"
        | "PerpetualContract" => Some(kind),
        _ => kind.strip_prefix("Order"),
    }
}

fn parse_instrument_key(key: &str, prefix: &str) -> anyhow::Result<InstrumentId> {
    let value = key
        .strip_prefix(prefix)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Invalid instrument key '{key}'"))?;
    InstrumentId::from_str(value)
        .map_err(|e| anyhow::anyhow!("Failed to parse instrument ID from key '{key}': {e}"))
}

#[cfg(test)]
mod tests {
    use std::{hint::black_box, str::FromStr, time::Instant};

    use bytes::Bytes;
    use nautilus_common::enums::SerializationEncoding;
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        data::{Data, stubs::stub_custom_data},
        enums::{AccountType, CurrencyType, OrderType},
        events::{
            AccountState, OrderEventAny, OrderInitialized,
            order::spec::{OrderFillVoidedSpec, OrderFilledSpec},
        },
        identifiers::{AccountId, InstrumentId, PositionId, TradeId},
        instruments::{Instrument, InstrumentAny, stubs::currency_pair_btcusdt},
        orders::{Order, OrderError},
        position::Position,
        types::{AccountBalance, Currency, MarginBalance, Money, Price, Quantity},
    };
    use rstest::rstest;
    use serde::{Deserialize, Serialize, de::DeserializeOwned};
    use ustr::Ustr;

    use super::{CurrencyDecode, CurrencyRecord, DatabaseQueries, parse_instrument_key};

    #[rstest]
    #[case(SerializationEncoding::Json)]
    #[case(SerializationEncoding::MsgPack)]
    fn test_legacy_margin_account_preserves_amounts(#[case] encoding: SerializationEncoding) {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../test_data/redis_cache_v1.json")).unwrap();

        let payload = match encoding {
            SerializationEncoding::Json => serde_json::to_vec(&fixture["margin_account"]).unwrap(),
            SerializationEncoding::MsgPack => {
                rmp_serde::to_vec_named(&fixture["margin_account"]).unwrap()
            }
            SerializationEncoding::Sbe => unreachable!(),
        };

        let expected = AccountState::new(
            AccountId::from("BINANCE-MARGIN-001"),
            AccountType::Margin,
            vec![AccountBalance::new(
                Money::from("999 USDT"),
                Money::from("100 USDT"),
                Money::from("899 USDT"),
            )],
            vec![MarginBalance::new(
                Money::from("12.34 USDT"),
                Money::from("5.67 USDT"),
                Some(InstrumentId::from("BTCUSDT.BINANCE")),
            )],
            false,
            UUID4::from_str("77777777-7777-4777-8777-777777777777").unwrap(),
            UnixNanos::from(700),
            UnixNanos::from(701),
            Some(Currency::USDT()),
        );

        let restored: AccountState =
            DatabaseQueries::deserialize_payload(encoding, &payload).unwrap();

        assert_eq!(
            serde_json::to_value(restored).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
    }

    #[rstest]
    fn test_legacy_order_transformation_rejects_stop_limit() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../test_data/redis_cache_v1.json")).unwrap();
        let original = fixture["order_initialized"].clone();
        let mut transformed = original.clone();
        transformed["order_type"] = "STOP_LIMIT".into();
        transformed["options"]["trigger_price"] = "12340.00".into();
        transformed["options"]["trigger_type"] = "LAST_PRICE".into();
        let payloads =
            [original, transformed].map(|value| Bytes::from(serde_json::to_vec(&value).unwrap()));

        let error =
            DatabaseQueries::deserialize_order(SerializationEncoding::Json, &payloads).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Legacy order transformation must produce a market or limit order"
        );
    }

    #[rstest]
    #[case(SerializationEncoding::Json)]
    #[case(SerializationEncoding::MsgPack)]
    fn test_native_order_timestamp_parameters_round_trip(
        #[case] encoding: SerializationEncoding,
        #[values(false, true)] iso8601: bool,
    ) {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../test_data/redis_cache_v1.json")).unwrap();
        let mut event: OrderInitialized = DatabaseQueries::deserialize_payload(
            SerializationEncoding::Json,
            &serde_json::to_vec(&fixture["order_initialized"]).unwrap(),
        )
        .unwrap();
        event.exec_algorithm_params = Some(
            [
                (Ustr::from("ts_start"), Ustr::from("9007199254740993")),
                (
                    Ustr::from("ts_end"),
                    Ustr::from("1970-01-01T00:00:00.000000987Z"),
                ),
            ]
            .into_iter()
            .collect(),
        );
        let event = OrderEventAny::Initialized(event);
        let payload =
            DatabaseQueries::serialize_payload_with_timestamps(encoding, &event, iso8601).unwrap();

        let restored: OrderEventAny =
            DatabaseQueries::deserialize_payload(encoding, &payload).unwrap();
        let order = DatabaseQueries::deserialize_order(encoding, &[Bytes::from(payload)]).unwrap();

        assert_eq!(restored, event);
        assert_eq!(order.events(), vec![&event]);
    }

    #[rstest]
    #[case(SerializationEncoding::Json)]
    #[case(SerializationEncoding::MsgPack)]
    fn test_native_instrument_legacy_info_timestamps(#[case] encoding: SerializationEncoding) {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../test_data/redis_cache_v1.json")).unwrap();
        let mut instrument = currency_pair_btcusdt();
        instrument.info = Some(
            serde_json::from_value(serde_json::json!({
                "ts_source": 456,
                "label": "legacy-source"
            }))
            .unwrap(),
        );
        let expected = InstrumentAny::CurrencyPair(instrument);
        let mut value = serde_json::to_value(&expected).unwrap();
        value["CurrencyPair"]["info"] = fixture["native_info"].clone();
        let payload = match encoding {
            SerializationEncoding::Json => serde_json::to_vec(&value).unwrap(),
            SerializationEncoding::MsgPack => rmp_serde::to_vec_named(&value).unwrap(),
            SerializationEncoding::Sbe => unreachable!(),
        };

        let restored: InstrumentAny =
            DatabaseQueries::deserialize_payload(encoding, &payload).unwrap();

        assert_eq!(
            serde_json::to_value(restored).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
    }

    #[rstest]
    #[case(SerializationEncoding::Json)]
    #[case(SerializationEncoding::MsgPack)]
    fn test_native_fill_timestamp_info_round_trip(
        #[case] encoding: SerializationEncoding,
        #[values(false, true)] iso8601: bool,
        #[values(false, true)] voided: bool,
    ) {
        let info = [
            ("ts_source", "123"),
            ("expire_time_ns", "456"),
            ("ts_original", "1970-01-01T00:00:00.000000789Z"),
            ("label", "fill-source"),
        ]
        .map(|(key, value)| (Ustr::from(key), Ustr::from(value)))
        .into_iter()
        .collect();

        let event = if voided {
            OrderEventAny::FillVoided(OrderFillVoidedSpec::builder().info(info).build())
        } else {
            OrderEventAny::Filled(OrderFilledSpec::builder().info(info).build())
        };

        let payload =
            DatabaseQueries::serialize_payload_with_timestamps(encoding, &event, iso8601).unwrap();

        let restored: OrderEventAny =
            DatabaseQueries::deserialize_payload(encoding, &payload).unwrap();

        assert_eq!(restored, event);
    }

    #[rstest]
    #[case(SerializationEncoding::Json)]
    #[case(SerializationEncoding::MsgPack)]
    fn test_legacy_order_transformation_preserves_history(
        #[case] encoding: SerializationEncoding,
        #[values("MARKET", "LIMIT")] order_type: &str,
    ) {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../test_data/redis_cache_v1.json")).unwrap();
        let original = fixture["order_initialized"].clone();
        let mut transformed = original.clone();
        transformed["order_type"] = order_type.into();
        transformed["event_id"] = "33333333-3333-4333-8333-333333333333".into();
        transformed["ts_event"] = "600".into();
        transformed["ts_init"] = "600".into();
        transformed["time_in_force"] = "GTC".into();
        transformed["options"]["expire_time_ns"] = 0.into();
        if order_type == "MARKET" {
            transformed["post_only"] = false.into();
            transformed["options"] = serde_json::Value::Object(serde_json::Map::new());
        }

        let payloads = [original, transformed].map(|value| {
            Bytes::from(match encoding {
                SerializationEncoding::Json => serde_json::to_vec(&value).unwrap(),
                SerializationEncoding::MsgPack => rmp_serde::to_vec_named(&value).unwrap(),
                SerializationEncoding::Sbe => unreachable!(),
            })
        });

        let expected_initial: OrderEventAny =
            DatabaseQueries::deserialize_payload(encoding, &payloads[0]).unwrap();
        let expected_transformed: OrderEventAny =
            DatabaseQueries::deserialize_payload(encoding, &payloads[1]).unwrap();
        let concrete: OrderInitialized =
            DatabaseQueries::deserialize_payload(encoding, &payloads[0]).unwrap();

        let order = DatabaseQueries::deserialize_order(encoding, &payloads).unwrap();

        assert_eq!(
            order.order_type(),
            if order_type == "MARKET" {
                OrderType::Market
            } else {
                OrderType::Limit
            }
        );
        assert_eq!(order.ts_init(), UnixNanos::from(100));
        assert_eq!(order.ts_last(), UnixNanos::from(600));
        assert_eq!(
            order.events(),
            vec![&expected_initial, &expected_transformed]
        );
        assert_eq!(expected_initial, OrderEventAny::Initialized(concrete));
    }

    #[rstest]
    #[case::duplicate(false, "Duplicate initialization")]
    #[case::identity(true, "changes the order identity")]
    fn test_legacy_order_transformation_rejects_invalid_history(
        #[case] change_identity: bool,
        #[case] expected: &str,
    ) {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../test_data/redis_cache_v1.json")).unwrap();
        let original = fixture["order_initialized"].clone();
        let mut transformed = original.clone();
        if change_identity {
            transformed["client_order_id"] = "O-OTHER".into();
        }

        let payloads =
            [original, transformed].map(|value| Bytes::from(serde_json::to_vec(&value).unwrap()));

        let error =
            DatabaseQueries::deserialize_order(SerializationEncoding::Json, &payloads).unwrap_err();

        assert!(error.to_string().contains(expected), "{error}");
    }

    #[rstest]
    fn test_native_order_history_still_rejects_duplicate_initialization() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../test_data/redis_cache_v1.json")).unwrap();
        let event: OrderEventAny = DatabaseQueries::deserialize_payload(
            SerializationEncoding::Json,
            &serde_json::to_vec(&fixture["order_initialized"]).unwrap(),
        )
        .unwrap();
        let payload = Bytes::from(serde_json::to_vec(&event).unwrap());

        let error = DatabaseQueries::deserialize_order(
            SerializationEncoding::Json,
            &[payload.clone(), payload],
        )
        .unwrap_err();

        assert!(matches!(
            error.downcast_ref::<OrderError>(),
            Some(OrderError::AlreadyInitialized)
        ));
    }

    /// A currency the process never registered must survive the round trip.
    ///
    /// This is the whole point of the record: `Currency`'s own encoding is the bare code, which
    /// only resolves against the global registry.
    #[rstest]
    #[case(SerializationEncoding::Json)]
    #[case(SerializationEncoding::MsgPack)]
    fn test_currency_record_round_trips_an_unregistered_code(
        #[case] encoding: SerializationEncoding,
    ) {
        // A distinct code per encoding: `CURRENCY_MAP` is process-global and tests run in parallel.
        let code = match encoding {
            SerializationEncoding::MsgPack => "ZZREDISM",
            _ => "ZZREDISJ",
        };
        assert!(
            Currency::try_from_str(code).is_none(),
            "the fixture code must be unregistered for this test to mean anything"
        );

        let currency = Currency::new(code, 3, 0, "Redis Fixture", CurrencyType::Crypto);
        let payload =
            DatabaseQueries::serialize_payload(encoding, &CurrencyRecord::from(&currency)).unwrap();

        let CurrencyDecode::Currency(restored) =
            DatabaseQueries::decode_currency(encoding, &Bytes::from(payload), currency.code)
        else {
            panic!("a valid record decodes to a currency");
        };

        assert_eq!(restored.code.as_str(), code);
        assert_eq!(restored.precision, 3);
        assert_eq!(restored.iso4217, 0);
        assert_eq!(restored.name.as_str(), "Redis Fixture");
        assert_eq!(restored.currency_type, CurrencyType::Crypto);
    }

    /// A record written before the full record existed must keep loading.
    #[rstest]
    #[case(SerializationEncoding::Json)]
    #[case(SerializationEncoding::MsgPack)]
    fn test_legacy_bare_code_still_loads(#[case] encoding: SerializationEncoding) {
        let payload = DatabaseQueries::serialize_payload(encoding, &Currency::USD()).unwrap();

        let CurrencyDecode::Currency(restored) =
            DatabaseQueries::decode_currency(encoding, &Bytes::from(payload), Currency::USD().code)
        else {
            panic!("a registered bare code decodes to a currency");
        };

        assert_eq!(restored, Currency::USD());
    }

    /// A record that decodes but fails validation is an error, not a skip.
    #[rstest]
    #[case(SerializationEncoding::Json)]
    #[case(SerializationEncoding::MsgPack)]
    fn test_invalid_record_is_classified_as_invalid(#[case] encoding: SerializationEncoding) {
        let record = CurrencyRecord {
            code: "ZZQ1".to_string(),
            precision: 20,
            iso4217: 0,
            name: "ZZQ1".to_string(),
            currency_type: CurrencyType::Crypto,
            defi: None,
        };
        let bytes = Bytes::from(DatabaseQueries::serialize_payload(encoding, &record).unwrap());

        assert!(matches!(
            DatabaseQueries::decode_currency(encoding, &bytes, Ustr::from("ZZQ1")),
            CurrencyDecode::InvalidRecord(_)
        ));
    }

    /// A record-shaped payload that does not decode as a record is an error, not a bare code.
    ///
    /// A record written by a build with a currency type this one lacks must fail the load rather
    /// than vanish, or the payloads denominated in it vanish with it.
    #[rstest]
    #[case(SerializationEncoding::Json)]
    #[case(SerializationEncoding::MsgPack)]
    fn test_record_with_an_unknown_currency_type_is_invalid(
        #[case] encoding: SerializationEncoding,
    ) {
        let record = serde_json::json!({
            "code": "ZZQ3",
            "precision": 6,
            "iso4217": 0,
            "name": "ZZQ3",
            "currency_type": "STABLECOIN",
        });
        let bytes = Bytes::from(DatabaseQueries::serialize_payload(encoding, &record).unwrap());

        assert!(matches!(
            DatabaseQueries::decode_currency(encoding, &bytes, Ustr::from("ZZQ3")),
            CurrencyDecode::InvalidRecord(_)
        ));
    }

    /// An unregistered bare code is skipped by bulk loading: the entry holds nothing recoverable.
    #[rstest]
    #[case(SerializationEncoding::Json)]
    #[case(SerializationEncoding::MsgPack)]
    fn test_unknown_bare_code_is_classified_as_undecodable(
        #[case] encoding: SerializationEncoding,
    ) {
        let bytes = Bytes::from(DatabaseQueries::serialize_payload(encoding, &"ZZQ2").unwrap());

        let CurrencyDecode::Undecodable(error) =
            DatabaseQueries::decode_currency(encoding, &bytes, Ustr::from("ZZQ2"))
        else {
            panic!("an unregistered bare code is undecodable");
        };
        assert!(
            error.to_string().contains("ZZQ2"),
            "the error names the code so the operator can register it: {error}"
        );
    }

    #[derive(Debug, Deserialize, PartialEq, Eq)]
    struct TimestampPayload {
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    }

    #[rstest]
    #[case(SerializationEncoding::Json)]
    #[case(SerializationEncoding::MsgPack)]
    fn test_deserialize_chrono_timestamp_payload(#[case] encoding: SerializationEncoding) {
        let json = include_bytes!("../../test_data/redis_cache_timestamp_chrono.json");
        let payload = match encoding {
            SerializationEncoding::Json => json.to_vec(),
            SerializationEncoding::MsgPack => {
                let value = serde_json::from_slice::<serde_json::Value>(json).unwrap();
                rmp_serde::to_vec(&value).unwrap()
            }
            SerializationEncoding::Sbe => unreachable!(),
        };

        let result =
            DatabaseQueries::deserialize_payload::<TimestampPayload>(encoding, &payload).unwrap();

        assert_eq!(
            result,
            TimestampPayload {
                ts_event: UnixNanos::from(1_123_456_789),
                ts_init: UnixNanos::from(2_987_654_321),
            }
        );
    }

    #[rstest]
    #[case(SerializationEncoding::Json)]
    #[case(SerializationEncoding::MsgPack)]
    fn test_wallet_account_state_round_trips_unregistered_currency(
        #[case] encoding: SerializationEncoding,
    ) {
        let currency = Currency::new(
            "ENG729C",
            6,
            0,
            "Cache round-trip token",
            CurrencyType::Crypto,
        );
        let total = Money::from_mantissa_exponent(123_456_789, -6, currency);
        let state = AccountState::new(
            AccountId::new("WALLET-CACHE-001"),
            AccountType::Wallet,
            vec![AccountBalance::new(total, Money::zero(currency), total)],
            vec![],
            true,
            nautilus_core::UUID4::new(),
            UnixNanos::from(1),
            UnixNanos::from(2),
            None,
        );
        assert!(Currency::try_from_str("ENG729C").is_none());

        let payload = DatabaseQueries::serialize_payload(encoding, &state).unwrap();
        let restored: AccountState =
            DatabaseQueries::deserialize_payload(encoding, &payload).unwrap();
        let restored = restored.balances[0];

        assert_eq!(restored.total.raw(), total.raw());
        assert_eq!(restored.locked.raw(), 0);
        assert_eq!(restored.free.raw(), total.raw());
        assert_eq!(restored.currency.code, currency.code);
        assert_eq!(restored.currency.precision, currency.precision);
        assert_eq!(restored.currency.iso4217, currency.iso4217);
        assert_eq!(restored.currency.name, currency.name);
        assert_eq!(restored.currency.currency_type, currency.currency_type);
        assert!(Currency::try_from_str("ENG729C").is_none());
    }

    #[rstest]
    #[ignore = "optimized Redis cache payload measurements without a Redis connection"]
    fn redis_payload_measurements() {
        let samples = std::env::var("NAUTILUS_BENCH_SAMPLES")
            .map_or(10_000, |value| value.parse::<usize>().unwrap());
        let trials = std::env::var("NAUTILUS_BENCH_TRIALS")
            .map_or(3, |value| value.parse::<usize>().unwrap());

        for trial in 0..trials {
            let formats = [SerializationEncoding::Json, SerializationEncoding::MsgPack];
            for offset in 0..formats.len() {
                let encoding = formats[(offset + trial) % formats.len()];

                let format = match encoding {
                    SerializationEncoding::Json => "json",
                    SerializationEncoding::MsgPack => "msgpack",
                    SerializationEncoding::Sbe => unreachable!(),
                };

                if std::env::var("NAUTILUS_BENCH_FORMAT").is_ok_and(|selected| selected != format) {
                    continue;
                }

                cache_cases(encoding, trial, samples);
            }
        }
    }

    fn cache_cases(encoding: SerializationEncoding, trial: usize, samples: usize) {
        let mut state = AccountState::new(
            AccountId::from("SIM-1"),
            AccountType::Cash,
            vec![AccountBalance::new(
                Money::from("1000.75 USD"),
                Money::from("250.25 USD"),
                Money::from("750.50 USD"),
            )],
            vec![],
            true,
            UUID4::new(),
            UnixNanos::from(1_700_000_000_000_000_123),
            UnixNanos::from(1_700_000_000_000_000_456),
            Some(Currency::USD()),
        );
        let instrument = InstrumentAny::CurrencyPair(currency_pair_btcusdt());
        let custom = Data::Custom(stub_custom_data(77, -88, None, None));
        let mut fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .position_id(PositionId::from("POSITION-1"))
            .last_qty(Quantity::from("2.750"))
            .last_px(Price::from("16384.00"))
            .currency(Currency::USDT())
            .commission(Money::from("0.73 USDT"))
            .ts_event(UnixNanos::from(1_700_000_000_000_000_789))
            .ts_init(UnixNanos::from(1_700_000_000_000_000_987))
            .build();

        for populated in [false, true] {
            state.info = populated.then(|| {
                serde_json::from_str(include_str!(
                    "../../../common/test_data/message_params.json"
                ))
                .unwrap()
            });

            let name = if populated {
                "account_state_nested"
            } else {
                "account_state_plain"
            };

            measure_cache_payload(name, &state, encoding, trial, samples);
        }

        measure_cache_payload("instrument", &instrument, encoding, trial, samples);
        measure_cache_payload("custom", &custom, encoding, trial, samples);
        fill.trade_id = TradeId::from("FILL-0");
        let mut position = Position::new(&instrument, fill.clone());
        for count in [1, 32, 256] {
            for index in position.events.len()..count {
                fill.event_id = UUID4::new();
                fill.trade_id = TradeId::new(format!("FILL-{index}"));
                position.apply(&fill);
            }

            assert_eq!(position.events.len(), count);
            measure_cache_payload(
                &format!("position_{count}"),
                &position,
                encoding,
                trial,
                samples,
            );
        }
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        reason = "nanosecond samples fit in u64 and timing means are approximate"
    )]
    fn measure_cache_payload<T: Serialize + DeserializeOwned>(
        name: &str,
        value: &T,
        encoding: SerializationEncoding,
        trial: usize,
        samples: usize,
    ) {
        if std::env::var("NAUTILUS_BENCH_CASE").is_ok_and(|selected| selected != name) {
            return;
        }

        assert!(samples > 0);
        let payload = DatabaseQueries::serialize_payload(encoding, value).unwrap();
        let decoded: T = DatabaseQueries::deserialize_payload(encoding, &payload).unwrap();
        assert_eq!(
            serde_json::to_value(&decoded).unwrap(),
            serde_json::to_value(value).unwrap()
        );

        let warmup = std::env::var("NAUTILUS_BENCH_WARMUP")
            .map_or(1_000, |value| value.parse::<usize>().unwrap());

        for _ in 0..warmup {
            black_box(DatabaseQueries::serialize_payload(encoding, black_box(value)).unwrap());
            black_box(
                DatabaseQueries::deserialize_payload::<T>(encoding, black_box(&payload)).unwrap(),
            );
        }

        if let Ok(operation) = std::env::var("NAUTILUS_BENCH_PROFILE") {
            for _ in 0..samples {
                match operation.as_str() {
                    "encode" => {
                        black_box(cache_encode(encoding, black_box(value)));
                    }
                    "decode" => {
                        black_box(cache_decode::<T>(encoding, black_box(&payload)));
                    }
                    _ => panic!("profile must be encode or decode"),
                }
            }

            println!(
                "{}",
                serde_json::json!({"case": name, "operation": operation,
                "profile_calls": samples, "bytes": payload.len()})
            );
            return;
        }

        let mut encoded = Vec::with_capacity(samples);
        let mut decoded = Vec::with_capacity(samples);
        for _ in 0..samples {
            let started = Instant::now();
            let bytes = cache_encode(encoding, black_box(value));
            encoded.push(started.elapsed().as_nanos() as u64);
            black_box(bytes);
            let started = Instant::now();
            let value = cache_decode::<T>(encoding, black_box(&payload));
            decoded.push(started.elapsed().as_nanos() as u64);
            black_box(value);
        }

        for (operation, mut times) in [("encode", encoded), ("decode", decoded)] {
            let mean = times.iter().map(|value| *value as f64).sum::<f64>() / samples as f64;
            times.sort_unstable();
            let percentile = |n| times[(samples - 1) * n / 1_000];
            println!(
                "{}",
                serde_json::json!({
                    "case": name, "boundary": "redis_payload", "format": encoding,
                    "trial": trial, "operation": operation, "samples": samples, "bytes": payload.len(),
                    "mean_ns": mean, "p50_ns": percentile(500), "p95_ns": percentile(950),
                    "p99_ns": percentile(990), "p999_ns": percentile(999), "max_ns": times.last().unwrap(),
                })
            );
        }
    }

    #[inline(never)]
    fn cache_encode<T: Serialize>(encoding: SerializationEncoding, value: &T) -> Vec<u8> {
        DatabaseQueries::serialize_payload(encoding, value).unwrap()
    }

    #[inline(never)]
    fn cache_decode<T: DeserializeOwned>(encoding: SerializationEncoding, payload: &[u8]) -> T {
        DatabaseQueries::deserialize_payload(encoding, payload).unwrap()
    }

    #[rstest]
    #[case("0xC31E54c7a869B9FcBEcc14363CF510d1c41fa443.Arbitrum:UniswapV3")]
    #[case(concat!(
        "0xc9bc8043294146424a4e4607d8ad837d",
        "6a659142822bbaaabc83bb57e7447461.Arbitrum:UniswapV4",
    ))]
    fn test_parse_instrument_key_preserves_colons_in_venue(#[case] value: &str) {
        let prefix = "TRADER-001:instruments:";
        let key = format!("{prefix}{value}");

        let result = parse_instrument_key(&key, prefix).unwrap();

        assert_eq!(result, InstrumentId::from_str(value).unwrap());
    }
}
