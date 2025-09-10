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

use alloy::primitives::Address;
use nautilus_core::UnixNanos;
use nautilus_model::defi::validation::validate_address;
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
