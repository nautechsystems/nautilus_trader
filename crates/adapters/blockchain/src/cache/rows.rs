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

use sqlx::{FromRow, Row, postgres::PgRow};

/// A data transfer object that maps database rows to token data.
///
/// Implements `FromRow` trait to automatically convert PostgreSQL results into `TokenRow`
/// objects that can be transformed into domain entity `Token` objects.
#[derive(Debug)]
pub struct TokenRow {
    pub address: String,
    pub name: String,
    pub symbol: String,
    pub decimals: i32,
}

impl<'r> FromRow<'r, PgRow> for TokenRow {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let address = row.try_get::<String, _>("address")?;
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
