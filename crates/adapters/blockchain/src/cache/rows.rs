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

use std::{fmt::Debug, num::ParseIntError, str::FromStr};

use alloy::primitives::{Address, I256, U160, U256};
use nautilus_core::{
    UnixNanos,
    datetime::{NANOSECONDS_IN_MICROSECOND, NANOSECONDS_IN_MILLISECOND, NANOSECONDS_IN_SECOND},
};
use nautilus_model::{
    defi::{
        PoolLiquidityUpdate, PoolLiquidityUpdateType, PoolSwap, SharedChain, SharedDex,
        data::{
            DexPoolData, PoolFeeCollect, PoolFeeProtocolCollect, PoolFeeProtocolUpdate, PoolFlash,
        },
        validation::validate_address,
    },
    identifiers::InstrumentId,
};
use sqlx::{FromRow, Row, postgres::PgRow};

const MAX_UNIX_SECONDS_TIMESTAMP: u64 = 9_999_999_999;
const MAX_UNIX_MILLISECONDS_TIMESTAMP: u64 = MAX_UNIX_SECONDS_TIMESTAMP * 1_000 + 999;
const MAX_UNIX_MICROSECONDS_TIMESTAMP: u64 = MAX_UNIX_SECONDS_TIMESTAMP * 1_000_000 + 999_999;

fn decode_address(value: &str, field: &str) -> Result<Address, sqlx::Error> {
    validate_address(value)
        .map_err(|e| sqlx::Error::Decode(format!("Invalid {field} address '{value}': {e}").into()))
}

fn decode_u64(value: i64, field: &str) -> Result<u64, sqlx::Error> {
    u64::try_from(value)
        .map_err(|e| sqlx::Error::Decode(format!("Invalid {field} '{value}': {e}").into()))
}

fn decode_u32(value: i32, field: &str) -> Result<u32, sqlx::Error> {
    u32::try_from(value)
        .map_err(|e| sqlx::Error::Decode(format!("Invalid {field} '{value}': {e}").into()))
}

fn decode_u8(value: i32, field: &str) -> Result<u8, sqlx::Error> {
    u8::try_from(value)
        .map_err(|e| sqlx::Error::Decode(format!("Invalid {field} '{value}': {e}").into()))
}

/// A data transfer object that maps database rows to token data.
///
/// Implements `FromRow` trait to automatically convert PostgreSQL results into `TokenRow`
/// objects that can be transformed into domain entity `Token` objects.
#[derive(Debug)]
pub struct TokenRow {
    pub address: Address,
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
}

impl<'r> FromRow<'r, PgRow> for TokenRow {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let address_value = row.try_get::<String, _>("address")?;
        let address = decode_address(&address_value, "token")?;
        let name = row.try_get::<String, _>("name")?;
        let symbol = row.try_get::<String, _>("symbol")?;
        let decimals = decode_u8(row.try_get::<i32, _>("decimals")?, "token decimals")?;

        let token = Self {
            address,
            name,
            symbol,
            decimals,
        };
        Ok(token)
    }
}

#[derive(Debug)]
pub struct PoolRow {
    pub address: Address,
    pub pool_identifier: String,
    pub dex_name: String,
    pub creation_block: u64,
    pub creation_block_timestamp: Option<UnixNanos>,
    pub token0_chain: i32,
    pub token0_address: Address,
    pub token1_chain: i32,
    pub token1_address: Address,
    pub fee: Option<u32>,
    pub tick_spacing: Option<u32>,
    pub initial_tick: Option<i32>,
    pub initial_sqrt_price_x96: Option<String>,
    pub hook_address: Option<String>,
}

impl<'r> FromRow<'r, PgRow> for PoolRow {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let address_value = row.try_get::<String, _>("address")?;
        let address = decode_address(&address_value, "pool")?;
        let pool_identifier = row.try_get::<String, _>("pool_identifier")?;
        let dex_name = row.try_get::<String, _>("dex_name")?;
        let creation_block = decode_u64(
            row.try_get::<i64, _>("creation_block")?,
            "pool creation block",
        )?;
        let creation_block_timestamp =
            row.try_get::<Option<String>, _>("creation_block_timestamp")?;
        let creation_block_timestamp = creation_block_timestamp
            .as_deref()
            .map(parse_cached_block_timestamp)
            .transpose()
            .map_err(|e| {
                sqlx::Error::Decode(
                    format!("Invalid creation block timestamp '{creation_block_timestamp:?}': {e}")
                        .into(),
                )
            })?;
        let token0_chain = row.try_get::<i32, _>("token0_chain")?;
        let token0_address_value = row.try_get::<String, _>("token0_address")?;
        let token0_address = decode_address(&token0_address_value, "token0")?;
        let token1_chain = row.try_get::<i32, _>("token1_chain")?;
        let token1_address_value = row.try_get::<String, _>("token1_address")?;
        let token1_address = decode_address(&token1_address_value, "token1")?;
        let fee = row
            .try_get::<Option<i32>, _>("fee")?
            .map(|value| decode_u32(value, "pool fee"))
            .transpose()?;
        let tick_spacing = row
            .try_get::<Option<i32>, _>("tick_spacing")?
            .map(|value| decode_u32(value, "pool tick spacing"))
            .transpose()?;
        let initial_tick = row.try_get::<Option<i32>, _>("initial_tick")?;
        let initial_sqrt_price_x96 = row.try_get::<Option<String>, _>("initial_sqrt_price_x96")?;
        let hook_address = row.try_get::<Option<String>, _>("hook_address")?;

        Ok(Self {
            address,
            pool_identifier,
            dex_name,
            creation_block,
            creation_block_timestamp,
            token0_chain,
            token0_address,
            token1_chain,
            token1_address,
            fee,
            tick_spacing,
            initial_tick,
            initial_sqrt_price_x96,
            hook_address,
        })
    }
}

/// A data transfer object that maps database rows to block timestamp data.
#[derive(Debug)]
pub struct BlockTimestampRow {
    /// The block number.
    pub number: u64,
    /// The block timestamp.
    pub timestamp: UnixNanos,
}

impl FromRow<'_, PgRow> for BlockTimestampRow {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let number = decode_u64(row.try_get::<i64, _>("number")?, "block number")?;
        let timestamp = row.try_get::<String, _>("timestamp")?;
        let timestamp = parse_cached_block_timestamp(&timestamp).map_err(|e| {
            sqlx::Error::Decode(format!("Invalid block timestamp '{timestamp}': {e}").into())
        })?;
        Ok(Self { number, timestamp })
    }
}

pub(crate) fn parse_cached_block_timestamp(value: &str) -> Result<UnixNanos, ParseIntError> {
    let timestamp = value.parse::<u64>()?;
    if timestamp <= MAX_UNIX_SECONDS_TIMESTAMP {
        return Ok(UnixNanos::from(timestamp * NANOSECONDS_IN_SECOND));
    }

    if timestamp <= MAX_UNIX_MILLISECONDS_TIMESTAMP {
        return Ok(UnixNanos::from(timestamp * NANOSECONDS_IN_MILLISECOND));
    }

    if timestamp <= MAX_UNIX_MICROSECONDS_TIMESTAMP {
        return Ok(UnixNanos::from(timestamp * NANOSECONDS_IN_MICROSECOND));
    }

    Ok(UnixNanos::from(timestamp))
}

/// Transforms a database row from the pool events UNION query into a DexPoolData enum variant.
///
/// This function directly processes a PostgreSQL row and creates the appropriate DexPoolData
/// variant based on the event_type discriminator field, using the provided context.
///
/// # Errors
///
/// Returns an error if row field extraction fails or data validation fails.
pub fn transform_row_to_dex_pool_data(
    row: &PgRow,
    chain: SharedChain,
    dex: SharedDex,
    instrument_id: InstrumentId,
) -> Result<DexPoolData, sqlx::Error> {
    let event_type = row.try_get::<String, _>("event_type")?;
    let pool_identifier_str = row.try_get::<String, _>("pool_identifier")?;
    let pool_identifier = pool_identifier_str
        .parse()
        .map_err(|e| sqlx::Error::Decode(format!("Invalid pool identifier: {e}").into()))?;
    let block = decode_u64(row.try_get::<i64, _>("block")?, "event block")?;
    let block_hash = row.try_get::<Option<String>, _>("block_hash")?;
    let transaction_hash = row.try_get::<String, _>("transaction_hash")?;
    let transaction_index = decode_u32(
        row.try_get::<i32, _>("transaction_index")?,
        "transaction index",
    )?;
    let log_index = decode_u32(row.try_get::<i32, _>("log_index")?, "log index")?;
    let block_timestamp = row.try_get::<String, _>("block_timestamp")?;
    let timestamp = parse_cached_block_timestamp(&block_timestamp).map_err(|e| {
        sqlx::Error::Decode(format!("Invalid block timestamp '{block_timestamp}': {e}").into())
    })?;

    match event_type.as_str() {
        "swap" => {
            let sender_str = row
                .try_get::<Option<String>, _>("sender")?
                .ok_or_else(|| sqlx::Error::Decode("Missing sender for swap event".into()))?;
            let sender = validate_address(&sender_str)
                .map_err(|e| sqlx::Error::Decode(e.to_string().into()))?;

            let recipient_str = row
                .try_get::<Option<String>, _>("recipient")?
                .ok_or_else(|| sqlx::Error::Decode("Missing recipient for swap event".into()))?;
            let recipient = validate_address(&recipient_str)
                .map_err(|e| sqlx::Error::Decode(e.to_string().into()))?;

            let sqrt_price_x96_str = row
                .try_get::<Option<String>, _>("sqrt_price_x96")?
                .ok_or_else(|| {
                    sqlx::Error::Decode("Missing sqrt_price_x96 for swap event".into())
                })?;
            let sqrt_price_x96 = U160::from_str(&sqrt_price_x96_str).map_err(|e| {
                sqlx::Error::Decode(
                    format!("Invalid sqrt_price_x96 '{sqrt_price_x96_str}': {e}").into(),
                )
            })?;

            let swap_liquidity_str = row.try_get::<String, _>("swap_liquidity")?;
            let swap_liquidity = u128::from_str(&swap_liquidity_str)
                .map_err(|e| sqlx::Error::Decode(e.to_string().into()))?;

            let swap_tick = row.try_get::<i32, _>("swap_tick")?;

            let swap_amount0_str = row
                .try_get::<Option<String>, _>("swap_amount0")?
                .ok_or_else(|| sqlx::Error::Decode("Missing swap_amount0 for swap event".into()))?;
            let amount0 = I256::from_str(&swap_amount0_str).map_err(|e| {
                sqlx::Error::Decode(
                    format!("Invalid swap_amount0 '{swap_amount0_str}': {e}").into(),
                )
            })?;

            let swap_amount1_str = row
                .try_get::<Option<String>, _>("swap_amount1")?
                .ok_or_else(|| sqlx::Error::Decode("Missing swap_amount1 for swap event".into()))?;
            let amount1 = I256::from_str(&swap_amount1_str).map_err(|e| {
                sqlx::Error::Decode(
                    format!("Invalid swap_amount1 '{swap_amount1_str}': {e}").into(),
                )
            })?;

            let mut pool_swap = PoolSwap::new(
                chain,
                dex,
                instrument_id,
                pool_identifier,
                block,
                transaction_hash,
                transaction_index,
                log_index,
                timestamp, // ts_event
                timestamp, // ts_init (same block timestamp)
                sender,
                recipient,
                amount0,
                amount1,
                sqrt_price_x96,
                swap_liquidity,
                swap_tick,
            );
            pool_swap.block_hash = block_hash;

            Ok(DexPoolData::Swap(pool_swap))
        }
        "liquidity" => {
            let kind_str = row
                .try_get::<Option<String>, _>("liquidity_event_type")?
                .ok_or_else(|| {
                    sqlx::Error::Decode("Missing liquidity_event_type for liquidity event".into())
                })?;

            let kind = match kind_str.as_str() {
                "Mint" => PoolLiquidityUpdateType::Mint,
                "Burn" => PoolLiquidityUpdateType::Burn,
                _ => {
                    return Err(sqlx::Error::Decode(
                        format!("Unknown liquidity update type: {kind_str}").into(),
                    ));
                }
            };

            let sender = row
                .try_get::<Option<String>, _>("sender")?
                .map(|s| validate_address(&s))
                .transpose()
                .map_err(|e| sqlx::Error::Decode(e.to_string().into()))?;

            let owner_str = row
                .try_get::<Option<String>, _>("owner")?
                .ok_or_else(|| sqlx::Error::Decode("Missing owner for liquidity event".into()))?;
            let owner = validate_address(&owner_str)
                .map_err(|e| sqlx::Error::Decode(e.to_string().into()))?;

            // UNION queries return NUMERIC type, not domain types, so we need to read as strings
            let position_liquidity_str = row.try_get::<String, _>("position_liquidity")?;
            let position_liquidity = position_liquidity_str.parse::<u128>().map_err(|e| {
                sqlx::Error::Decode(
                    format!("Invalid position_liquidity '{position_liquidity_str}': {e}").into(),
                )
            })?;

            let amount0_str = row.try_get::<String, _>("amount0")?;
            let amount0 = U256::from_str_radix(&amount0_str, 10).map_err(|e| {
                sqlx::Error::Decode(format!("Invalid amount0 '{amount0_str}': {e}").into())
            })?;

            let amount1_str = row.try_get::<String, _>("amount1")?;
            let amount1 = U256::from_str_radix(&amount1_str, 10).map_err(|e| {
                sqlx::Error::Decode(format!("Invalid amount1 '{amount1_str}': {e}").into())
            })?;

            let tick_lower = row
                .try_get::<Option<i32>, _>("tick_lower")?
                .ok_or_else(|| {
                    sqlx::Error::Decode("Missing tick_lower for liquidity event".into())
                })?;

            let tick_upper = row
                .try_get::<Option<i32>, _>("tick_upper")?
                .ok_or_else(|| {
                    sqlx::Error::Decode("Missing tick_upper for liquidity event".into())
                })?;

            let mut pool_liquidity_update = PoolLiquidityUpdate::new(
                chain,
                dex,
                instrument_id,
                pool_identifier,
                kind,
                block,
                transaction_hash,
                transaction_index,
                log_index,
                sender,
                owner,
                position_liquidity,
                amount0,
                amount1,
                tick_lower,
                tick_upper,
                timestamp, // ts_event
                timestamp, // ts_init (same block timestamp)
            );
            pool_liquidity_update.block_hash = block_hash;

            Ok(DexPoolData::LiquidityUpdate(pool_liquidity_update))
        }
        "collect" => {
            let owner_str = row
                .try_get::<Option<String>, _>("owner")?
                .ok_or_else(|| sqlx::Error::Decode("Missing owner for collect event".into()))?;
            let owner = validate_address(&owner_str)
                .map_err(|e| sqlx::Error::Decode(e.to_string().into()))?;

            // UNION queries return NUMERIC type, not domain types, so we need to read as strings
            let amount0_str = row.try_get::<String, _>("amount0")?;
            let amount0 = amount0_str.parse::<u128>().map_err(|e| {
                sqlx::Error::Decode(format!("Invalid amount0 '{amount0_str}': {e}").into())
            })?;

            let amount1_str = row.try_get::<String, _>("amount1")?;
            let amount1 = amount1_str.parse::<u128>().map_err(|e| {
                sqlx::Error::Decode(format!("Invalid amount1 '{amount1_str}': {e}").into())
            })?;

            let tick_lower = row
                .try_get::<Option<i32>, _>("tick_lower")?
                .ok_or_else(|| {
                    sqlx::Error::Decode("Missing tick_lower for collect event".into())
                })?;

            let tick_upper = row
                .try_get::<Option<i32>, _>("tick_upper")?
                .ok_or_else(|| {
                    sqlx::Error::Decode("Missing tick_upper for collect event".into())
                })?;

            let mut pool_fee_collect = PoolFeeCollect::new(
                chain,
                dex,
                instrument_id,
                pool_identifier,
                block,
                transaction_hash,
                transaction_index,
                log_index,
                owner,
                amount0,
                amount1,
                tick_lower,
                tick_upper,
                timestamp, // ts_event
                timestamp, // ts_init (same block timestamp)
            );
            pool_fee_collect.block_hash = block_hash;

            Ok(DexPoolData::FeeCollect(pool_fee_collect))
        }
        "fee_protocol_update" => {
            let fee_protocol0_new = row.try_get::<i32, _>("fee_protocol0_new")?;
            let fee_protocol1_new = row.try_get::<i32, _>("fee_protocol1_new")?;
            let fee_protocol0_new = u32::try_from(fee_protocol0_new).map_err(|e| {
                sqlx::Error::Decode(
                    format!("Invalid fee_protocol0_new '{fee_protocol0_new}': {e}").into(),
                )
            })?;
            let fee_protocol1_new = u32::try_from(fee_protocol1_new).map_err(|e| {
                sqlx::Error::Decode(
                    format!("Invalid fee_protocol1_new '{fee_protocol1_new}': {e}").into(),
                )
            })?;

            let mut pool_fee_protocol_update = PoolFeeProtocolUpdate::new(
                chain,
                dex,
                instrument_id,
                pool_identifier,
                block,
                transaction_hash,
                transaction_index,
                log_index,
                fee_protocol0_new,
                fee_protocol1_new,
                timestamp, // ts_event
                timestamp, // ts_init (same block timestamp)
            );
            pool_fee_protocol_update.block_hash = block_hash;

            Ok(DexPoolData::FeeProtocolUpdate(pool_fee_protocol_update))
        }
        "fee_protocol_collect" => {
            let sender_str = row.try_get::<Option<String>, _>("sender")?.ok_or_else(|| {
                sqlx::Error::Decode("Missing sender for fee_protocol_collect event".into())
            })?;
            let sender = validate_address(&sender_str)
                .map_err(|e| sqlx::Error::Decode(e.to_string().into()))?;

            let recipient_str =
                row.try_get::<Option<String>, _>("recipient")?
                    .ok_or_else(|| {
                        sqlx::Error::Decode(
                            "Missing recipient for fee_protocol_collect event".into(),
                        )
                    })?;
            let recipient = validate_address(&recipient_str)
                .map_err(|e| sqlx::Error::Decode(e.to_string().into()))?;

            // UNION queries return NUMERIC type, not domain types, so we need to read as strings
            let amount0_str = row.try_get::<String, _>("amount0")?;
            let amount0 = amount0_str.parse::<u128>().map_err(|e| {
                sqlx::Error::Decode(format!("Invalid amount0 '{amount0_str}': {e}").into())
            })?;

            let amount1_str = row.try_get::<String, _>("amount1")?;
            let amount1 = amount1_str.parse::<u128>().map_err(|e| {
                sqlx::Error::Decode(format!("Invalid amount1 '{amount1_str}': {e}").into())
            })?;

            let mut pool_fee_protocol_collect = PoolFeeProtocolCollect::new(
                chain,
                dex,
                instrument_id,
                pool_identifier,
                block,
                transaction_hash,
                transaction_index,
                log_index,
                sender,
                recipient,
                amount0,
                amount1,
                timestamp, // ts_event
                timestamp, // ts_init (same block timestamp)
            );
            pool_fee_protocol_collect.block_hash = block_hash;

            Ok(DexPoolData::FeeProtocolCollect(pool_fee_protocol_collect))
        }
        "flash" => {
            let sender_str = row
                .try_get::<Option<String>, _>("sender")?
                .ok_or_else(|| sqlx::Error::Decode("Missing sender for flash event".into()))?;
            let sender = validate_address(&sender_str)
                .map_err(|e| sqlx::Error::Decode(e.to_string().into()))?;

            let recipient_str = row
                .try_get::<Option<String>, _>("recipient")?
                .ok_or_else(|| sqlx::Error::Decode("Missing recipient for flash event".into()))?;
            let recipient = validate_address(&recipient_str)
                .map_err(|e| sqlx::Error::Decode(e.to_string().into()))?;

            // For flash events, we have flash_amount0, flash_amount1, flash_paid0, flash_paid1
            let flash_amount0_str = row.try_get::<String, _>("flash_amount0")?;
            let amount0 = U256::from_str_radix(&flash_amount0_str, 10).map_err(|e| {
                sqlx::Error::Decode(
                    format!("Invalid flash_amount0 '{flash_amount0_str}': {e}").into(),
                )
            })?;

            let flash_amount1_str = row.try_get::<String, _>("flash_amount1")?;
            let amount1 = U256::from_str_radix(&flash_amount1_str, 10).map_err(|e| {
                sqlx::Error::Decode(
                    format!("Invalid flash_amount1 '{flash_amount1_str}': {e}").into(),
                )
            })?;

            let flash_paid0_str = row.try_get::<String, _>("flash_paid0")?;
            let paid0 = U256::from_str_radix(&flash_paid0_str, 10).map_err(|e| {
                sqlx::Error::Decode(format!("Invalid flash_paid0 '{flash_paid0_str}': {e}").into())
            })?;

            let flash_paid1_str = row.try_get::<String, _>("flash_paid1")?;
            let paid1 = U256::from_str_radix(&flash_paid1_str, 10).map_err(|e| {
                sqlx::Error::Decode(format!("Invalid flash_paid1 '{flash_paid1_str}': {e}").into())
            })?;

            let mut pool_flash = PoolFlash::new(
                chain,
                dex,
                instrument_id,
                pool_identifier,
                block,
                transaction_hash,
                transaction_index,
                log_index,
                timestamp, // ts_event
                timestamp, // ts_init (same block timestamp)
                sender,
                recipient,
                amount0,
                amount1,
                paid0,
                paid1,
            );
            pool_flash.block_hash = block_hash;

            Ok(DexPoolData::Flash(pool_flash))
        }
        _ => Err(sqlx::Error::Decode(
            format!("Unknown event type: {event_type}").into(),
        )),
    }
}

/// A data transfer object that maps database rows to persisted execution transaction records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionTransactionRow {
    pub wallet_address: Option<String>,
    pub nonce: u64,
    pub transaction_hash: String,
    pub purpose: String,
    pub status: String,
    pub client_order_id: Option<String>,
}

/// Values persisted when reserving durable ownership of an execution intent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionIntentInsert {
    pub chain_id: u32,
    pub wallet_address: String,
    pub purpose: String,
    pub client_order_id: Option<String>,
    pub trader_id: Option<String>,
    pub strategy_id: Option<String>,
    pub account_id: Option<String>,
    pub instrument_id: Option<String>,
    pub pool_address: Option<String>,
    pub transaction_to: String,
    pub transaction_input: String,
    pub transaction_value: String,
    pub amount_in: Option<String>,
    pub created_block: u64,
}

/// A durable execution intent which owns the active signer slot and optional client order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionIntentRow {
    pub id: i64,
    pub schema_version: i16,
    pub chain_id: u32,
    pub wallet_address: String,
    pub nonce: Option<u64>,
    pub purpose: String,
    pub status: String,
    pub client_order_id: Option<String>,
    pub trader_id: Option<String>,
    pub strategy_id: Option<String>,
    pub account_id: Option<String>,
    pub instrument_id: Option<String>,
    pub pool_address: Option<String>,
    pub transaction_to: String,
    pub transaction_input: String,
    pub transaction_value: String,
    pub amount_in: Option<String>,
    pub created_block: u64,
    pub acknowledgement_emitted: bool,
    pub fill_emitted: bool,
    pub terminal_emitted: bool,
    pub active: bool,
}

impl<'r> FromRow<'r, PgRow> for ExecutionIntentRow {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let chain_id_i32 = row.try_get::<i32, _>("chain_id")?;
        let chain_id = u32::try_from(chain_id_i32).map_err(|_| {
            sqlx::Error::Decode(format!("Invalid negative chain ID {chain_id_i32}").into())
        })?;
        let nonce = row
            .try_get::<Option<i64>, _>("nonce")?
            .map(|nonce| {
                u64::try_from(nonce).map_err(|_| {
                    sqlx::Error::Decode(format!("Invalid negative nonce {nonce}").into())
                })
            })
            .transpose()?;
        let created_block_i64 = row.try_get::<i64, _>("created_block")?;
        let created_block = u64::try_from(created_block_i64).map_err(|_| {
            sqlx::Error::Decode(
                format!("Invalid negative creation block {created_block_i64}").into(),
            )
        })?;

        Ok(Self {
            id: row.try_get("id")?,
            schema_version: row.try_get("schema_version")?,
            chain_id,
            wallet_address: row.try_get("wallet_address")?,
            nonce,
            purpose: row.try_get("purpose")?,
            status: row.try_get("status")?,
            client_order_id: row.try_get("client_order_id")?,
            trader_id: row.try_get("trader_id")?,
            strategy_id: row.try_get("strategy_id")?,
            account_id: row.try_get("account_id")?,
            instrument_id: row.try_get("instrument_id")?,
            pool_address: row.try_get("pool_address")?,
            transaction_to: row.try_get("transaction_to")?,
            transaction_input: row.try_get("transaction_input")?,
            transaction_value: row.try_get("transaction_value")?,
            amount_in: row.try_get("amount_in")?,
            created_block,
            acknowledgement_emitted: row.try_get("acknowledgement_emitted")?,
            fill_emitted: row.try_get("fill_emitted")?,
            terminal_emitted: row.try_get("terminal_emitted")?,
            active: row.try_get("active")?,
        })
    }
}

/// A signed transaction hash associated with an execution intent.
#[derive(Clone, PartialEq, Eq)]
pub struct ExecutionTransactionHashRow {
    pub id: i64,
    pub intent_id: i64,
    pub chain_id: u32,
    pub transaction_hash: String,
    pub payload_expected: bool,
    pub raw_transaction: Option<Vec<u8>>,
    pub sealed_transaction: Option<Vec<u8>>,
    pub status: String,
    pub block_number: Option<u64>,
    pub block_hash: Option<String>,
    pub receipt_success: Option<bool>,
    pub gas_used: Option<u64>,
    pub effective_gas_price: Option<String>,
    pub current: bool,
}

impl Debug for ExecutionTransactionHashRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(ExecutionTransactionHashRow))
            .field("id", &self.id)
            .field("intent_id", &self.intent_id)
            .field("chain_id", &self.chain_id)
            .field("transaction_hash", &self.transaction_hash)
            .field("payload_expected", &self.payload_expected)
            .field(
                "raw_transaction",
                &self.raw_transaction.as_ref().map(|_| "<redacted>"),
            )
            .field(
                "sealed_transaction",
                &self.sealed_transaction.as_ref().map(|_| "<redacted>"),
            )
            .field("status", &self.status)
            .field("block_number", &self.block_number)
            .field("block_hash", &self.block_hash)
            .field("receipt_success", &self.receipt_success)
            .field("gas_used", &self.gas_used)
            .field("effective_gas_price", &self.effective_gas_price)
            .field("current", &self.current)
            .finish()
    }
}

impl<'r> FromRow<'r, PgRow> for ExecutionTransactionHashRow {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let chain_id_i32 = row.try_get::<i32, _>("chain_id")?;
        let chain_id = u32::try_from(chain_id_i32).map_err(|_| {
            sqlx::Error::Decode(format!("Invalid negative chain ID {chain_id_i32}").into())
        })?;
        let block_number = row
            .try_get::<Option<i64>, _>("block_number")?
            .map(|block| {
                u64::try_from(block).map_err(|_| {
                    sqlx::Error::Decode(format!("Invalid negative block number {block}").into())
                })
            })
            .transpose()?;
        let gas_used = row
            .try_get::<Option<i64>, _>("gas_used")?
            .map(|gas| {
                u64::try_from(gas).map_err(|_| {
                    sqlx::Error::Decode(format!("Invalid negative gas used {gas}").into())
                })
            })
            .transpose()?;

        Ok(Self {
            id: row.try_get("id")?,
            intent_id: row.try_get("intent_id")?,
            chain_id,
            transaction_hash: row.try_get("transaction_hash")?,
            payload_expected: row.try_get("payload_expected")?,
            raw_transaction: row.try_get("raw_transaction")?,
            sealed_transaction: row.try_get("sealed_transaction")?,
            status: row.try_get("status")?,
            block_number,
            block_hash: row.try_get("block_hash")?,
            receipt_success: row.try_get("receipt_success")?,
            gas_used,
            effective_gas_price: row.try_get("effective_gas_price")?,
            current: row.try_get("current")?,
        })
    }
}

impl<'r> FromRow<'r, PgRow> for ExecutionTransactionRow {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let wallet_address = row.try_get::<Option<String>, _>("wallet_address")?;
        let nonce_i64 = row.try_get::<i64, _>("nonce")?;
        let nonce = u64::try_from(nonce_i64).map_err(|_| {
            sqlx::Error::Decode(format!("Invalid negative nonce {nonce_i64}").into())
        })?;
        let transaction_hash = row.try_get::<String, _>("transaction_hash")?;
        let purpose = row.try_get::<String, _>("purpose")?;
        let status = row.try_get::<String, _>("status")?;
        let client_order_id = row.try_get::<Option<String>, _>("client_order_id")?;

        Ok(Self {
            wallet_address,
            nonce,
            transaction_hash,
            purpose,
            status,
            client_order_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::datetime::{
        NANOSECONDS_IN_MICROSECOND, NANOSECONDS_IN_MILLISECOND, NANOSECONDS_IN_SECOND,
    };
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("1700000000", 1_700_000_000 * NANOSECONDS_IN_SECOND)]
    #[case("9999999999", 9_999_999_999 * NANOSECONDS_IN_SECOND)]
    #[case("1700000000123", 1_700_000_000_123 * NANOSECONDS_IN_MILLISECOND)]
    #[case("9999999999999", 9_999_999_999_999 * NANOSECONDS_IN_MILLISECOND)]
    #[case("1700000000123456", 1_700_000_000_123_456 * NANOSECONDS_IN_MICROSECOND)]
    #[case("9999999999999999", 9_999_999_999_999_999 * NANOSECONDS_IN_MICROSECOND)]
    #[case("1700000000123456789", 1_700_000_000_123_456_789)]
    fn parse_cached_block_timestamp_returns_unix_nanos(#[case] value: &str, #[case] expected: u64) {
        let timestamp = parse_cached_block_timestamp(value).unwrap();

        assert_eq!(timestamp, UnixNanos::from(expected));
    }

    #[rstest]
    fn parse_cached_block_timestamp_rejects_invalid_text() {
        let result = parse_cached_block_timestamp("not-a-timestamp");

        assert!(result.is_err());
    }

    #[rstest]
    fn decode_address_rejects_malformed_database_value() {
        let error = decode_address("not-an-address", "token").unwrap_err();

        assert!(error.to_string().contains("Invalid token address"));
    }

    #[rstest]
    fn decode_unsigned_fields_reject_negative_database_values() {
        assert!(decode_u64(-1_i64, "block number").is_err());
        assert!(decode_u32(-1, "log index").is_err());
        assert!(decode_u8(-1, "token decimals").is_err());
        assert!(decode_u8(256, "token decimals").is_err());
    }

    #[rstest]
    fn execution_transaction_hash_debug_redacts_raw_transaction() {
        let row = ExecutionTransactionHashRow {
            id: 1,
            intent_id: 2,
            chain_id: 42161,
            transaction_hash: "0xhash".to_string(),
            payload_expected: true,
            raw_transaction: Some(vec![0xde, 0xad, 0xbe, 0xef]),
            sealed_transaction: Some(vec![0xca, 0xfe]),
            status: "signed".to_string(),
            block_number: None,
            block_hash: None,
            receipt_success: None,
            gas_used: None,
            effective_gas_price: None,
            current: true,
        };

        let debug = format!("{row:?}");

        assert!(debug.contains("raw_transaction: Some(\"<redacted>\")"));
        assert!(debug.contains("sealed_transaction: Some(\"<redacted>\")"));
        assert!(!debug.contains("[222, 173, 190, 239]"));
        assert!(!debug.contains("[202, 254]"));
    }
}
