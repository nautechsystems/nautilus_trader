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

use nautilus_model::{
    identifiers::instrument_id::InstrumentId,
    instruments::{Instrument, InstrumentAny},
    types::currency::Currency,
};
use sqlx::PgPool;

use crate::sql::models::{
    general::GeneralRow, instruments::InstrumentAnyModel, types::CurrencyModel,
};

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

    pub async fn add_instrument(
        pool: &PgPool,
        kind: &str,
        instrument: Box<dyn Instrument>,
    ) -> anyhow::Result<()> {
        sqlx::query(r#"
            INSERT INTO "instrument" (
                id, kind, raw_symbol, base_currency, underlying, quote_currency, settlement_currency, isin, asset_class, exchange,
                multiplier, option_kind, is_inverse, strike_price, activation_ns, expiration_ns, price_precision, size_precision,
                price_increment, size_increment, maker_fee, taker_fee, margin_init, margin_maint, lot_size, max_quantity, min_quantity, max_notional,
                min_notional, max_price, min_price, ts_init, ts_event, created_at, updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29, $30, $31, $32, $33, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
            ON CONFLICT (id)
            DO UPDATE
            SET
                kind = $2, raw_symbol = $3, base_currency= $4, underlying = $5, quote_currency = $6, settlement_currency = $7, isin = $8, asset_class = $9, exchange = $10,
                 multiplier = $11, option_kind = $12, is_inverse = $13, strike_price = $14, activation_ns = $15, expiration_ns = $16 , price_precision = $17, size_precision = $18,
                 price_increment = $19, size_increment = $20, maker_fee = $21, taker_fee = $22, margin_init = $23, margin_maint = $24, lot_size = $25, max_quantity = $26,
                 min_quantity = $27, max_notional = $28, min_notional = $29, max_price = $30, min_price = $31, ts_init = $32,  ts_event = $33, updated_at = CURRENT_TIMESTAMP
            "#)
            .bind(instrument.id().to_string())
            .bind(kind)
            .bind(instrument.raw_symbol().to_string())
            .bind(instrument.base_currency().map(|x| x.code.as_str()))
            .bind(instrument.underlying().map(|x| x.to_string()))
            .bind(instrument.quote_currency().code.as_str())
            .bind(instrument.settlement_currency().code.as_str())
            .bind(instrument.isin().map(|x| x.to_string()))
            .bind(instrument.asset_class().to_string())
            .bind(instrument.exchange().map(|x| x.to_string()))
            .bind(instrument.multiplier().to_string())
            .bind(instrument.option_kind().map(|x| x.to_string()))
            .bind(instrument.is_inverse())
            .bind(instrument.strike_price().map(|x| x.to_string()))
            .bind(instrument.activation_ns().map(|x| x.to_string()))
            .bind(instrument.expiration_ns().map(|x| x.to_string()))
            .bind(instrument.price_precision() as i32)
            .bind(instrument.size_precision() as i32)
            .bind(instrument.price_increment().to_string())
            .bind(instrument.size_increment().to_string())
            .bind(instrument.maker_fee().to_string())
            .bind(instrument.taker_fee().to_string())
            .bind(instrument.margin_init().to_string())
            .bind(instrument.margin_maint().to_string())
            .bind(instrument.lot_size().map(|x| x.to_string()))
            .bind(instrument.max_quantity().map(|x| x.to_string()))
            .bind(instrument.min_quantity().map(|x| x.to_string()))
            .bind(instrument.max_notional().map(|x| x.to_string()))
            .bind(instrument.min_notional().map(|x| x.to_string()))
            .bind(instrument.max_price().map(|x| x.to_string()))
            .bind(instrument.min_price().map(|x| x.to_string()))
            .bind(instrument.ts_init().to_string())
            .bind(instrument.ts_event().to_string())
            .execute(pool)
            .await
            .map(|_| ())
            .map_err(|err| anyhow::anyhow!(format!("Failed to insert item {} into instrument table: {:?}", instrument.id().to_string(), err)))
    }

    pub async fn load_instrument(
        pool: &PgPool,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<Option<InstrumentAny>> {
        sqlx::query_as::<_, InstrumentAnyModel>("SELECT * FROM instrument WHERE id = $1")
            .bind(instrument_id.to_string())
            .fetch_optional(pool)
            .await
            .map(|instrument| instrument.map(|row| row.0))
            .map_err(|err| {
                anyhow::anyhow!("Failed to load instrument with id {instrument_id},error is: {err}")
            })
    }

    pub async fn load_instruments(pool: &PgPool) -> anyhow::Result<Vec<InstrumentAny>> {
        sqlx::query_as::<_, InstrumentAnyModel>("SELECT * FROM instrument")
            .fetch_all(pool)
            .await
            .map(|rows| rows.into_iter().map(|row| row.0).collect())
            .map_err(|err| anyhow::anyhow!("Failed to load instruments: {err}"))
    }
}
