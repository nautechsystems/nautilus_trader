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

use std::str::FromStr;

use alloy::primitives::{Address, I256, U160, U256};
use nautilus_core::UnixNanos;
use nautilus_model::{
    defi::{
        PoolLiquidityUpdate, PoolLiquidityUpdateType, PoolSwap,
        data::{DexPoolData, PoolFeeCollect, PoolFlash},
        validation::validate_address,
    },
    identifiers::InstrumentId,
};
use sqlx::{FromRow, Row, postgres::PgRow};

/// A data transfer object that maps database rows to token data.
///
/// Implements `FromRow` trait to automatically convert PostgreSQL results into `TokenRow`
/// objects that can be transformed into domain entity `Token` objects.
#[derive(Debug)]
pub struct TokenRow {
    pub address: Address,
    pub name: String,
    pub symbol: String,
    pub decimals: i32,
}

impl<'r> FromRow<'r, PgRow> for TokenRow {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let address = validate_address(row.try_get::<String, _>("address")?.as_str()).unwrap();
        let name = row.try_get::<String, _>("name")?;
        let symbol = row.try_get::<String, _>("symbol")?;
        let decimals = row.try_get::<i32, _>("decimals")?;

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
    pub dex_name: String,
    pub creation_block: i64,
    pub token0_chain: i32,
    pub token0_address: Address,
    pub token1_chain: i32,
    pub token1_address: Address,
    pub fee: Option<i32>,
    pub tick_spacing: Option<i32>,
    pub initial_tick: Option<i32>,
    pub initial_sqrt_price_x96: Option<String>,
}

impl<'r> FromRow<'r, PgRow> for PoolRow {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let address = validate_address(row.try_get::<String, _>("address")?.as_str()).unwrap();
        let dex_name = row.try_get::<String, _>("dex_name")?;
        let creation_block = row.try_get::<i64, _>("creation_block")?;
        let token0_chain = row.try_get::<i32, _>("token0_chain")?;
        let token0_address =
            validate_address(row.try_get::<String, _>("token0_address")?.as_str()).unwrap();
        let token1_chain = row.try_get::<i32, _>("token1_chain")?;
        let token1_address =
            validate_address(row.try_get::<String, _>("token1_address")?.as_str()).unwrap();
        let fee = row.try_get::<Option<i32>, _>("fee")?;
        let tick_spacing = row.try_get::<Option<i32>, _>("tick_spacing")?;
        let initial_tick = row.try_get::<Option<i32>, _>("initial_tick")?;
        let initial_sqrt_price_x96 = row.try_get::<Option<String>, _>("initial_sqrt_price_x96")?;

        Ok(Self {
            address,
            dex_name,
            creation_block,
            token0_chain,
            token0_address,
            token1_chain,
            token1_address,
            fee,
            tick_spacing,
            initial_tick,
            initial_sqrt_price_x96,
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
        let number = row.try_get::<i64, _>("number")? as u64;
        let timestamp = row.try_get::<String, _>("timestamp")?;
        Ok(Self {
            number,
            timestamp: UnixNanos::from(timestamp),
        })
    }
}

/// Transforms a database row from the pool events UNION query into a DexPoolData enum variant.
///
/// This function directly processes a PostgreSQL row and creates the appropriate DexPoolData
/// variant based on the event_type discriminator field, using the provided context.
pub fn transform_row_to_dex_pool_data(
    row: &PgRow,
    chain: nautilus_model::defi::SharedChain,
    dex: nautilus_model::defi::SharedDex,
    instrument_id: InstrumentId,
) -> Result<DexPoolData, sqlx::Error> {
    let event_type = row.try_get::<String, _>("event_type")?;
    let pool_address_str = row.try_get::<String, _>("pool_address")?;
    let block = row.try_get::<i64, _>("block")? as u64;
    let transaction_hash = row.try_get::<String, _>("transaction_hash")?;
    let transaction_index = row.try_get::<i32, _>("transaction_index")? as u32;
    let log_index = row.try_get::<i32, _>("log_index")? as u32;

    let pool_address = validate_address(&pool_address_str)
        .map_err(|e| sqlx::Error::Decode(e.to_string().into()))?;

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
                    format!("Invalid sqrt_price_x96 '{}': {}", sqrt_price_x96_str, e).into(),
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
                    format!("Invalid swap_amount0 '{}': {}", swap_amount0_str, e).into(),
                )
            })?;

            let swap_amount1_str = row
                .try_get::<Option<String>, _>("swap_amount1")?
                .ok_or_else(|| sqlx::Error::Decode("Missing swap_amount1 for swap event".into()))?;
            let amount1 = I256::from_str(&swap_amount1_str).map_err(|e| {
                sqlx::Error::Decode(
                    format!("Invalid swap_amount1 '{}': {}", swap_amount1_str, e).into(),
                )
            })?;

            let pool_swap = PoolSwap::new(
                chain,
                dex,
                instrument_id,
                pool_address,
                block,
                transaction_hash,
                transaction_index,
                log_index,
                None, // timestamp
                sender,
                recipient,
                amount0,
                amount1,
                sqrt_price_x96,
                swap_liquidity,
                swap_tick,
                None,
                None,
                None,
            );

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
                        format!("Unknown liquidity update type: {}", kind_str).into(),
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
            let position_liquidity =
                u128::from_str_radix(&position_liquidity_str, 10).map_err(|e| {
                    sqlx::Error::Decode(
                        format!(
                            "Invalid position_liquidity '{}': {}",
                            position_liquidity_str, e
                        )
                        .into(),
                    )
                })?;

            let amount0_str = row.try_get::<String, _>("amount0")?;
            let amount0 = U256::from_str_radix(&amount0_str, 10).map_err(|e| {
                sqlx::Error::Decode(format!("Invalid amount0 '{}': {}", amount0_str, e).into())
            })?;

            let amount1_str = row.try_get::<String, _>("amount1")?;
            let amount1 = U256::from_str_radix(&amount1_str, 10).map_err(|e| {
                sqlx::Error::Decode(format!("Invalid amount1 '{}': {}", amount1_str, e).into())
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

            let pool_liquidity_update = PoolLiquidityUpdate::new(
                chain,
                dex,
                instrument_id,
                pool_address,
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
                None, // timestamp
            );

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
            let amount0 = u128::from_str_radix(&amount0_str, 10).map_err(|e| {
                sqlx::Error::Decode(format!("Invalid amount0 '{}': {}", amount0_str, e).into())
            })?;

            let amount1_str = row.try_get::<String, _>("amount1")?;
            let amount1 = u128::from_str_radix(&amount1_str, 10).map_err(|e| {
                sqlx::Error::Decode(format!("Invalid amount1 '{}': {}", amount1_str, e).into())
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

            let pool_fee_collect = PoolFeeCollect::new(
                chain,
                dex,
                instrument_id,
                pool_address,
                block,
                transaction_hash,
                transaction_index,
                log_index,
                owner,
                amount0,
                amount1,
                tick_lower,
                tick_upper,
                None, // timestamp
            );

            Ok(DexPoolData::FeeCollect(pool_fee_collect))
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
                    format!("Invalid flash_amount0 '{}': {}", flash_amount0_str, e).into(),
                )
            })?;

            let flash_amount1_str = row.try_get::<String, _>("flash_amount1")?;
            let amount1 = U256::from_str_radix(&flash_amount1_str, 10).map_err(|e| {
                sqlx::Error::Decode(
                    format!("Invalid flash_amount1 '{}': {}", flash_amount1_str, e).into(),
                )
            })?;

            let flash_paid0_str = row.try_get::<String, _>("flash_paid0")?;
            let paid0 = U256::from_str_radix(&flash_paid0_str, 10).map_err(|e| {
                sqlx::Error::Decode(
                    format!("Invalid flash_paid0 '{}': {}", flash_paid0_str, e).into(),
                )
            })?;

            let flash_paid1_str = row.try_get::<String, _>("flash_paid1")?;
            let paid1 = U256::from_str_radix(&flash_paid1_str, 10).map_err(|e| {
                sqlx::Error::Decode(
                    format!("Invalid flash_paid1 '{}': {}", flash_paid1_str, e).into(),
                )
            })?;

            let pool_flash = PoolFlash::new(
                chain,
                dex,
                instrument_id,
                pool_address,
                block,
                transaction_hash,
                transaction_index,
                log_index,
                None, // timestamp
                sender,
                recipient,
                amount0,
                amount1,
                paid0,
                paid1,
            );

            Ok(DexPoolData::Flash(pool_flash))
        }
        _ => Err(sqlx::Error::Decode(
            format!("Unknown event type: {}", event_type).into(),
        )),
    }
}
