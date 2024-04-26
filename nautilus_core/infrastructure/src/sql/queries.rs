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

use std::collections::HashMap;

use nautilus_model::types::currency::Currency;
use sqlx::PgPool;

use crate::sql::models::{general::GeneralRow, types::CurrencyModel};

pub struct DatabaseQueries;

impl DatabaseQueries {
    pub async fn add(pool: &PgPool, key: String, value: Vec<u8>) -> anyhow::Result<()> {
        sqlx::query("INSERT INTO general (key, value) VALUES ($1, $2)")
            .bind(key)
            .bind(value)
            .execute(pool)
            .await
            .map(|_| ())
            .map_err(|err| anyhow::anyhow!("Failed to insert into general table: {err}"))
    }

    pub async fn load(pool: &PgPool) -> anyhow::Result<HashMap<String, Vec<u8>>> {
        sqlx::query_as::<_, GeneralRow>("SELECT * FROM general")
            .fetch_all(pool)
            .await
            .map(|rows| {
                let mut cache: HashMap<String, Vec<u8>> = HashMap::new();
                for row in rows {
                    cache.insert(row.key, row.value);
                }
                cache
            })
            .map_err(|err| anyhow::anyhow!("Failed to load general table: {err}"))
    }

    pub async fn add_currency(pool: &PgPool, currency: Currency) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO currency (code, precision, iso4217, name, currency_type) VALUES ($1, $2, $3, $4, $5) ON CONFLICT (code) DO NOTHING"
        )
            .bind(currency.code.as_str())
            .bind(currency.precision as i32)
            .bind(currency.iso4217 as i32)
            .bind(currency.name.as_str())
            .bind(currency.currency_type.to_string())
            .execute(pool)
            .await
            .map(|_| ())
            .map_err(|err| anyhow::anyhow!("Failed to insert into currency table: {err}"))
    }

    pub async fn load_currencies(pool: &PgPool) -> anyhow::Result<Vec<Currency>> {
        sqlx::query_as::<_, CurrencyModel>("SELECT * FROM currency ORDER BY code ASC")
            .fetch_all(pool)
            .await
            .map(|rows| rows.into_iter().map(|row| row.0).collect())
            .map_err(|err| anyhow::anyhow!("Failed to load currencies: {err}"))
    }

    pub async fn load_currency(pool: &PgPool, code: &str) -> anyhow::Result<Option<Currency>> {
        sqlx::query_as::<_, CurrencyModel>("SELECT * FROM currency WHERE code = $1")
            .bind(code)
            .fetch_optional(pool)
            .await
            .map(|currency| currency.map(|row| row.0))
            .map_err(|err| anyhow::anyhow!("Failed to load currency: {err}"))
    }

    pub async fn truncate(pool: &PgPool, table: String) -> anyhow::Result<()> {
        sqlx::query(format!("TRUNCATE TABLE {} CASCADE", table).as_str())
            .execute(pool)
            .await
            .map(|_| ())
            .map_err(|err| anyhow::anyhow!("Failed to truncate table: {err}"))
    }
}
