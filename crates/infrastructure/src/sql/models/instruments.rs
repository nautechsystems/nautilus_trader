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

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

use std::str::FromStr;

use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::OptionKind,
    identifiers::{InstrumentId, Symbol},
    instruments::{
        BettingInstrument, BinaryOption, CryptoFuture, CryptoOption, CryptoPerpetual, CurrencyPair,
        Equity, FuturesContract, FuturesSpread, InstrumentAny, OptionContract, OptionSpread,
    },
    types::{Currency, Money, Price, Quantity},
};
use rust_decimal::Decimal;
use sqlx::{FromRow, Row, postgres::PgRow};
use ustr::Ustr;

use crate::sql::models::enums::AssetClassModel;

pub struct InstrumentAnyModel(pub InstrumentAny);
pub struct BettingInstrumentModel(pub BettingInstrument);
pub struct BinaryOptionModel(pub BinaryOption);
pub struct CryptoFutureModel(pub CryptoFuture);
pub struct CryptoOptionModel(pub CryptoOption);
pub struct CryptoPerpetualModel(pub CryptoPerpetual);
pub struct CurrencyPairModel(pub CurrencyPair);
pub struct EquityModel(pub Equity);
pub struct FuturesContractModel(pub FuturesContract);
pub struct FuturesSpreadModel(pub FuturesSpread);
pub struct OptionContractModel(pub OptionContract);
pub struct OptionSpreadModel(pub OptionSpread);

impl<'r> FromRow<'r, PgRow> for InstrumentAnyModel {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let kind = row.get::<String, _>("kind");
        if kind == "BETTING" {
            Ok(InstrumentAnyModel(InstrumentAny::Betting(
                BettingInstrumentModel::from_row(row).unwrap().0,
            )))
        } else if kind == "BINARY_OPTION" {
            Ok(InstrumentAnyModel(InstrumentAny::BinaryOption(
                BinaryOptionModel::from_row(row).unwrap().0,
            )))
        } else if kind == "CRYPTO_FUTURE" {
            Ok(InstrumentAnyModel(InstrumentAny::CryptoFuture(
                CryptoFutureModel::from_row(row).unwrap().0,
            )))
        } else if kind == "CRYPTO_OPTION" {
            Ok(InstrumentAnyModel(InstrumentAny::CryptoOption(
                CryptoOptionModel::from_row(row).unwrap().0,
            )))
        } else if kind == "CRYPTO_PERPETUAL" {
            Ok(InstrumentAnyModel(InstrumentAny::CryptoPerpetual(
                CryptoPerpetualModel::from_row(row).unwrap().0,
            )))
        } else if kind == "CURRENCY_PAIR" {
            Ok(InstrumentAnyModel(InstrumentAny::CurrencyPair(
                CurrencyPairModel::from_row(row).unwrap().0,
            )))
        } else if kind == "EQUITY" {
            Ok(InstrumentAnyModel(InstrumentAny::Equity(
                EquityModel::from_row(row).unwrap().0,
            )))
        } else if kind == "FUTURES_CONTRACT" {
            Ok(InstrumentAnyModel(InstrumentAny::FuturesContract(
                FuturesContractModel::from_row(row).unwrap().0,
            )))
        } else if kind == "FUTURES_SPREAD" {
            Ok(InstrumentAnyModel(InstrumentAny::FuturesSpread(
                FuturesSpreadModel::from_row(row).unwrap().0,
            )))
        } else if kind == "OPTION_CONTRACT" {
            Ok(InstrumentAnyModel(InstrumentAny::OptionContract(
                OptionContractModel::from_row(row).unwrap().0,
            )))
        } else if kind == "OPTION_SPREAD" {
            Ok(InstrumentAnyModel(InstrumentAny::OptionSpread(
                OptionSpreadModel::from_row(row).unwrap().0,
            )))
        } else {
            panic!("Unknown instrument type")
        }
    }
}

// TODO: New/updated schema required to support betting instrument loading
impl<'r> FromRow<'r, PgRow> for BettingInstrumentModel {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let id = row.try_get::<String, _>("id").map(InstrumentId::from)?;
        let raw_symbol = row.try_get::<String, _>("raw_symbol").map(Symbol::from)?;
        let event_type_id = row.try_get::<i64, _>("event_type_id")? as u64;
        let event_type_name = row
            .try_get::<String, _>("event_type_name")
            .map(|res| Ustr::from(res.as_str()))?;
        let competition_id = row.try_get::<i64, _>("competition_id")? as u64;
        let competition_name = row
            .try_get::<String, _>("competition_name")
            .map(|res| Ustr::from(res.as_str()))?;
        let event_id = row.try_get::<i64, _>("event_id")? as u64;
        let event_name = row
            .try_get::<String, _>("event_name")
            .map(|res| Ustr::from(res.as_str()))?;
        let event_country_code = row
            .try_get::<String, _>("event_country_code")
            .map(|res| Ustr::from(res.as_str()))?;
        let event_open_date = row
            .try_get::<String, _>("event_open_date")
            .map(UnixNanos::from)?;
        let betting_type = row
            .try_get::<String, _>("betting_type")
            .map(|res| Ustr::from(res.as_str()))?;
        let market_id = row
            .try_get::<String, _>("market_id")
            .map(|res| Ustr::from(res.as_str()))?;
        let market_name = row
            .try_get::<String, _>("market_name")
            .map(|res| Ustr::from(res.as_str()))?;
        let market_type = row
            .try_get::<String, _>("market_type")
            .map(|res| Ustr::from(res.as_str()))?;
        let market_start_time = row
            .try_get::<String, _>("market_start_time")
            .map(UnixNanos::from)?;
        let selection_id = row.try_get::<i64, _>("selection_id")? as u64;
        let selection_name = row
            .try_get::<String, _>("selection_name")
            .map(|res| Ustr::from(res.as_str()))?;
        let selection_handicap = row.try_get::<f64, _>("selection_handicap")?;
        let currency = row
            .try_get::<String, _>("quote_currency")
            .map(Currency::from)?;
        let price_precision = row.try_get::<i32, _>("price_precision")? as u8;
        let size_precision = row.try_get::<i32, _>("size_precision")? as u8;
        let price_increment = row
            .try_get::<String, _>("price_increment")
            .map(Price::from)?;
        let size_increment = row
            .try_get::<String, _>("size_increment")
            .map(Quantity::from)?;
        let max_quantity = row
            .try_get::<Option<String>, _>("max_quantity")
            .ok()
            .and_then(|res| res.map(|value| Quantity::from(value.as_str())));
        let min_quantity = row
            .try_get::<Option<String>, _>("min_quantity")
            .ok()
            .and_then(|res| res.map(|value| Quantity::from(value.as_str())));
        let max_notional = row
            .try_get::<Option<String>, _>("max_notional")
            .ok()
            .and_then(|res| res.map(|value| Money::from(value.as_str())));
        let min_notional = row
            .try_get::<Option<String>, _>("min_notional")
            .ok()
            .and_then(|res| res.map(|value| Money::from(value.as_str())));
        let max_price = row
            .try_get::<Option<String>, _>("max_price")
            .ok()
            .and_then(|res| res.map(|value| Price::from(value.as_str())));
        let min_price = row
            .try_get::<Option<String>, _>("min_price")
            .ok()
            .and_then(|res| res.map(|value| Price::from(value.as_str())));
        let margin_init = row
            .try_get::<String, _>("margin_init")
            .map(|res| Some(Decimal::from_str(res.as_str()).unwrap()))?;
        let margin_maint = row
            .try_get::<String, _>("margin_maint")
            .map(|res| Some(Decimal::from_str(res.as_str()).unwrap()))?;
        let maker_fee = row
            .try_get::<String, _>("maker_fee")
            .map(|res| Some(Decimal::from_str(res.as_str()).unwrap()))?;
        let taker_fee = row
            .try_get::<String, _>("taker_fee")
            .map(|res| Some(Decimal::from_str(res.as_str()).unwrap()))?;
        let ts_event = row.try_get::<String, _>("ts_event").map(UnixNanos::from)?;
        let ts_init = row.try_get::<String, _>("ts_init").map(UnixNanos::from)?;

        let inst = BettingInstrument::new(
            id,
            raw_symbol,
            event_type_id,
            event_type_name,
            competition_id,
            competition_name,
            event_id,
            event_name,
            event_country_code,
            event_open_date,
            betting_type,
            market_id,
            market_name,
            market_type,
            market_start_time,
            selection_id,
            selection_name,
            selection_handicap,
            currency,
            price_precision,
            size_precision,
            price_increment,
            size_increment,
            max_quantity,
            min_quantity,
            max_notional,
            min_notional,
            max_price,
            min_price,
            margin_init,
            margin_maint,
            maker_fee,
            taker_fee,
            ts_event,
            ts_init,
        );
        Ok(BettingInstrumentModel(inst))
    }
}

impl<'r> FromRow<'r, PgRow> for BinaryOptionModel {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let id = row.try_get::<String, _>("id").map(InstrumentId::from)?;
        let raw_symbol = row.try_get::<String, _>("raw_symbol").map(Symbol::from)?;
        let asset_class = row
            .try_get::<AssetClassModel, _>("asset_class")
            .map(|res| res.0)?;
        let currency = row
            .try_get::<String, _>("quote_currency")
            .map(Currency::from)?;
        let activation_ns = row
            .try_get::<String, _>("activation_ns")
            .map(UnixNanos::from)?;
        let expiration_ns = row
            .try_get::<String, _>("expiration_ns")
            .map(UnixNanos::from)?;
        let price_precision = row.try_get::<i32, _>("price_precision")? as u8;
        let size_precision = row.try_get::<i32, _>("size_precision")? as u8;
        let price_increment = row
            .try_get::<String, _>("price_increment")
            .map(|res| Price::from_str(res.as_str()).unwrap())?;
        let size_increment = row
            .try_get::<String, _>("size_increment")
            .map(|res| Quantity::from_str(res.as_str()).unwrap())?;
        let outcome = row
            .try_get::<Option<String>, _>("outcome")
            .ok()
            .and_then(|res| res.map(|value| Ustr::from(value.as_str())));
        let description = row
            .try_get::<Option<String>, _>("description")
            .ok()
            .and_then(|res| res.map(|value| Ustr::from(value.as_str())));
        let max_quantity = row
            .try_get::<Option<String>, _>("max_quantity")
            .ok()
            .and_then(|res| res.map(|value| Quantity::from(value.as_str())));
        let min_quantity = row
            .try_get::<Option<String>, _>("min_quantity")
            .ok()
            .and_then(|res| res.map(|value| Quantity::from(value.as_str())));
        let max_notional = row
            .try_get::<Option<String>, _>("max_notional")
            .ok()
            .and_then(|res| res.map(|value| Money::from(value.as_str())));
        let min_notional = row
            .try_get::<Option<String>, _>("min_notional")
            .ok()
            .and_then(|res| res.map(|value| Money::from(value.as_str())));
        let max_price = row
            .try_get::<Option<String>, _>("max_price")
            .ok()
            .and_then(|res| res.map(|value| Price::from(value.as_str())));
        let min_price = row
            .try_get::<Option<String>, _>("min_price")
            .ok()
            .and_then(|res| res.map(|value| Price::from(value.as_str())));
        let margin_init = row
            .try_get::<String, _>("margin_init")
            .map(|res| Some(Decimal::from_str(res.as_str()).unwrap()))?;
        let margin_maint = row
            .try_get::<String, _>("margin_maint")
            .map(|res| Some(Decimal::from_str(res.as_str()).unwrap()))?;
        let maker_fee = row
            .try_get::<String, _>("maker_fee")
            .map(|res| Some(Decimal::from_str(res.as_str()).unwrap()))?;
        let taker_fee = row
            .try_get::<String, _>("taker_fee")
            .map(|res| Some(Decimal::from_str(res.as_str()).unwrap()))?;
        let ts_event = row.try_get::<String, _>("ts_event").map(UnixNanos::from)?;
        let ts_init = row.try_get::<String, _>("ts_init").map(UnixNanos::from)?;

        let inst = BinaryOption::new(
            id,
            raw_symbol,
            asset_class,
            currency,
            activation_ns,
            expiration_ns,
            price_precision,
            size_precision,
            price_increment,
            size_increment,
            outcome,
            description,
            max_quantity,
            min_quantity,
            max_notional,
            min_notional,
            max_price,
            min_price,
            margin_init,
            margin_maint,
            maker_fee,
            taker_fee,
            ts_event,
            ts_init,
        );
        Ok(BinaryOptionModel(inst))
    }
}

impl<'r> FromRow<'r, PgRow> for CryptoFutureModel {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let id = row.try_get::<String, _>("id").map(InstrumentId::from)?;
        let raw_symbol = row.try_get::<String, _>("raw_symbol").map(Symbol::from)?;
        let underlying = row.try_get::<String, _>("underlying").map(Currency::from)?;
        let quote_currency = row
            .try_get::<String, _>("quote_currency")
            .map(Currency::from)?;
        let settlement_currency = row
            .try_get::<String, _>("settlement_currency")
            .map(Currency::from)?;
        let is_inverse = row.try_get::<bool, _>("is_inverse")?;
        let activation_ns = row
            .try_get::<String, _>("activation_ns")
            .map(UnixNanos::from)?;
        let expiration_ns = row
            .try_get::<String, _>("expiration_ns")
            .map(UnixNanos::from)?;
        let price_precision = row.try_get::<i32, _>("price_precision")?;
        let size_precision = row.try_get::<i32, _>("size_precision")?;
        let price_increment = row
            .try_get::<String, _>("price_increment")
            .map(|res| Price::from_str(res.as_str()).unwrap())?;
        let size_increment = row
            .try_get::<String, _>("size_increment")
            .map(|res| Quantity::from_str(res.as_str()).unwrap())?;
        let multiplier = row
            .try_get::<String, _>("multiplier")
            .map(|res| Quantity::from(res.as_str()))?;
        let lot_size = row
            .try_get::<String, _>("lot_size")
            .map(|res| Quantity::from(res.as_str()))?;
        let max_quantity = row
            .try_get::<Option<String>, _>("max_quantity")
            .ok()
            .and_then(|res| res.map(|value| Quantity::from(value.as_str())));
        let min_quantity = row
            .try_get::<Option<String>, _>("min_quantity")
            .ok()
            .and_then(|res| res.map(|value| Quantity::from(value.as_str())));
        let max_notional = row
            .try_get::<Option<String>, _>("max_notional")
            .ok()
            .and_then(|res| res.map(|value| Money::from(value.as_str())));
        let min_notional = row
            .try_get::<Option<String>, _>("min_notional")
            .ok()
            .and_then(|res| res.map(|value| Money::from(value.as_str())));
        let max_price = row
            .try_get::<Option<String>, _>("max_price")
            .ok()
            .and_then(|res| res.map(|value| Price::from(value.as_str())));
        let min_price = row
            .try_get::<Option<String>, _>("min_price")
            .ok()
            .and_then(|res| res.map(|value| Price::from(value.as_str())));
        let margin_init = row
            .try_get::<String, _>("margin_init")
            .map(|res| Some(Decimal::from_str(res.as_str()).unwrap()))?;
        let margin_maint = row
            .try_get::<String, _>("margin_maint")
            .map(|res| Some(Decimal::from_str(res.as_str()).unwrap()))?;
        let maker_fee = row
            .try_get::<String, _>("maker_fee")
            .map(|res| Some(Decimal::from_str(res.as_str()).unwrap()))?;
        let taker_fee = row
            .try_get::<String, _>("taker_fee")
            .map(|res| Some(Decimal::from_str(res.as_str()).unwrap()))?;
        let ts_event = row.try_get::<String, _>("ts_event").map(UnixNanos::from)?;
        let ts_init = row.try_get::<String, _>("ts_init").map(UnixNanos::from)?;

        let inst = CryptoFuture::new(
            id,
            raw_symbol,
            underlying,
            quote_currency,
            settlement_currency,
            is_inverse,
            activation_ns,
            expiration_ns,
            price_precision as u8,
            size_precision as u8,
            price_increment,
            size_increment,
            Some(multiplier),
            Some(lot_size),
            max_quantity,
            min_quantity,
            max_notional,
            min_notional,
            max_price,
            min_price,
            margin_init,
            margin_maint,
            maker_fee,
            taker_fee,
            ts_event,
            ts_init,
        );
        Ok(CryptoFutureModel(inst))
    }
}

impl<'r> FromRow<'r, PgRow> for CryptoOptionModel {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let id = row.try_get::<String, _>("id").map(InstrumentId::from)?;
        let raw_symbol = row.try_get::<String, _>("raw_symbol").map(Symbol::from)?;
        let underlying = row.try_get::<String, _>("underlying").map(Currency::from)?;
        let quote_currency = row
            .try_get::<String, _>("quote_currency")
            .map(Currency::from)?;
        let settlement_currency = row
            .try_get::<String, _>("settlement_currency")
            .map(Currency::from)?;
        let is_inverse = row.try_get::<bool, _>("is_inverse")?;
        let option_kind = row
            .try_get::<String, _>("option_kind")
            .map(|res| OptionKind::from_str(res.as_str()).unwrap())?;
        let strike_price = row
            .try_get::<String, _>("strike_price")
            .map(|res| Price::from_str(res.as_str()).unwrap())?;
        let activation_ns = row
            .try_get::<String, _>("activation_ns")
            .map(UnixNanos::from)?;
        let expiration_ns = row
            .try_get::<String, _>("expiration_ns")
            .map(UnixNanos::from)?;
        let price_precision = row.try_get::<i32, _>("price_precision")?;
        let size_precision = row.try_get::<i32, _>("size_precision")?;
        let price_increment = row
            .try_get::<String, _>("price_increment")
            .map(|res| Price::from_str(res.as_str()).unwrap())?;
        let size_increment = row
            .try_get::<String, _>("size_increment")
            .map(|res| Quantity::from_str(res.as_str()).unwrap())?;
        let multiplier = row
            .try_get::<String, _>("multiplier")
            .map(|res| Quantity::from(res.as_str()))?;
        let max_quantity = row
            .try_get::<Option<String>, _>("max_quantity")
            .ok()
            .and_then(|res| res.map(|value| Quantity::from(value.as_str())));
        let min_quantity = row
            .try_get::<Option<String>, _>("min_quantity")
            .ok()
            .and_then(|res| res.map(|value| Quantity::from(value.as_str())));
        let max_notional = row
            .try_get::<Option<String>, _>("max_notional")
            .ok()
            .and_then(|res| res.map(|value| Money::from(value.as_str())));
        let min_notional = row
            .try_get::<Option<String>, _>("min_notional")
            .ok()
            .and_then(|res| res.map(|value| Money::from(value.as_str())));
        let max_price = row
            .try_get::<Option<String>, _>("max_price")
            .ok()
            .and_then(|res| res.map(|value| Price::from(value.as_str())));
        let min_price = row
            .try_get::<Option<String>, _>("min_price")
            .ok()
            .and_then(|res| res.map(|value| Price::from(value.as_str())));
        let margin_init = row
            .try_get::<String, _>("margin_init")
            .map(|res| Some(Decimal::from_str(res.as_str()).unwrap()))?;
        let margin_maint = row
            .try_get::<String, _>("margin_maint")
            .map(|res| Some(Decimal::from_str(res.as_str()).unwrap()))?;
        let maker_fee = row
            .try_get::<String, _>("maker_fee")
            .map(|res| Some(Decimal::from_str(res.as_str()).unwrap()))?;
        let taker_fee = row
            .try_get::<String, _>("taker_fee")
            .map(|res| Some(Decimal::from_str(res.as_str()).unwrap()))?;
        let ts_event = row.try_get::<String, _>("ts_event").map(UnixNanos::from)?;
        let ts_init = row.try_get::<String, _>("ts_init").map(UnixNanos::from)?;

        let inst = CryptoOption::new(
            id,
            raw_symbol,
            underlying,
            quote_currency,
            settlement_currency,
            is_inverse,
            option_kind,
            strike_price,
            activation_ns,
            expiration_ns,
            price_precision as u8,
            size_precision as u8,
            price_increment,
            size_increment,
            Some(multiplier),
            max_quantity,
            min_quantity,
            max_notional,
            min_notional,
            max_price,
            min_price,
            margin_init,
            margin_maint,
            maker_fee,
            taker_fee,
            ts_event,
            ts_init,
        );
        Ok(CryptoOptionModel(inst))
    }
}

impl<'r> FromRow<'r, PgRow> for CryptoPerpetualModel {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let id = row.try_get::<String, _>("id").map(InstrumentId::from)?;
        let raw_symbol = row.try_get::<String, _>("raw_symbol").map(Symbol::from)?;
        let base_currency = row
            .try_get::<String, _>("base_currency")
            .map(Currency::from)?;
        let quote_currency = row
            .try_get::<String, _>("quote_currency")
            .map(Currency::from)?;
        let settlement_currency = row
            .try_get::<String, _>("settlement_currency")
            .map(Currency::from)?;
        let is_inverse = row.try_get::<bool, _>("is_inverse")?;
        let price_precision = row.try_get::<i32, _>("price_precision")?;
        let size_precision = row.try_get::<i32, _>("size_precision")?;
        let price_increment = row
            .try_get::<String, _>("price_increment")
            .map(|res| Price::from_str(res.as_str()).unwrap())?;
        let size_increment = row
            .try_get::<String, _>("size_increment")
            .map(|res| Quantity::from_str(res.as_str()).unwrap())?;
        let multiplier = row
            .try_get::<String, _>("multiplier")
            .map(|res| Quantity::from(res.as_str()))?;
        let lot_size = row
            .try_get::<String, _>("lot_size")
            .map(|res| Quantity::from(res.as_str()))?;
        let max_quantity = row
            .try_get::<Option<String>, _>("max_quantity")
            .ok()
            .and_then(|res| res.map(|res| Quantity::from(res.as_str())));
        let min_quantity = row
            .try_get::<Option<String>, _>("min_quantity")
            .ok()
            .and_then(|res| res.map(|res| Quantity::from(res.as_str())));
        let max_notional = row
            .try_get::<Option<String>, _>("max_notional")
            .ok()
            .and_then(|res| res.map(|res| Money::from(res.as_str())));
        let min_notional = row
            .try_get::<Option<String>, _>("min_notional")
            .ok()
            .and_then(|res| res.map(|res| Money::from(res.as_str())));
        let max_price = row
            .try_get::<Option<String>, _>("max_price")
            .ok()
            .and_then(|res| res.map(|res| Price::from(res.as_str())));
        let min_price = row
            .try_get::<Option<String>, _>("min_price")
            .ok()
            .and_then(|res| res.map(|res| Price::from(res.as_str())));
        let margin_init = row
            .try_get::<String, _>("margin_init")
            .map(|res| Some(Decimal::from_str(res.as_str()).unwrap()))?;
        let margin_maint = row
            .try_get::<String, _>("margin_maint")
            .map(|res| Some(Decimal::from_str(res.as_str()).unwrap()))?;
        let maker_fee = row
            .try_get::<String, _>("maker_fee")
            .map(|res| Some(Decimal::from_str(res.as_str()).unwrap()))?;
        let taker_fee = row
            .try_get::<String, _>("taker_fee")
            .map(|res| Some(Decimal::from_str(res.as_str()).unwrap()))?;
        let ts_event = row.try_get::<String, _>("ts_event").map(UnixNanos::from)?;
        let ts_event = row.try_get::<String, _>("ts_event").map(UnixNanos::from)?;
        let ts_init = row.try_get::<String, _>("ts_init").map(UnixNanos::from)?;

        let inst = CryptoPerpetual::new(
            id,
            raw_symbol,
            base_currency,
            quote_currency,
            settlement_currency,
            is_inverse,
            price_precision as u8,
            size_precision as u8,
            price_increment,
            size_increment,
            Some(multiplier),
            Some(lot_size),
            max_quantity,
            min_quantity,
            max_notional,
            min_notional,
            max_price,
            min_price,
            margin_init,
            margin_maint,
            maker_fee,
            taker_fee,
            ts_event,
            ts_init,
        );
        Ok(CryptoPerpetualModel(inst))
    }
}

impl<'r> FromRow<'r, PgRow> for CurrencyPairModel {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let id = row.try_get::<String, _>("id").map(InstrumentId::from)?;
        let raw_symbol = row.try_get::<String, _>("raw_symbol").map(Symbol::from)?;
        let base_currency = row
            .try_get::<String, _>("base_currency")
            .map(Currency::from)?;
        let quote_currency = row
            .try_get::<String, _>("quote_currency")
            .map(Currency::from)?;
        let price_precision = row.try_get::<i32, _>("price_precision")?;
        let size_precision = row.try_get::<i32, _>("size_precision")?;
        let price_increment = row
            .try_get::<String, _>("price_increment")
            .map(|res| Price::from(res.as_str()))?;
        let size_increment = row
            .try_get::<String, _>("size_increment")
            .map(|res| Quantity::from(res.as_str()))?;
        let lot_size = row
            .try_get::<Option<String>, _>("lot_size")
            .ok()
            .and_then(|res| res.map(|res| Quantity::from(res.as_str())));
        let max_quantity = row
            .try_get::<Option<String>, _>("max_quantity")
            .ok()
            .and_then(|res| res.map(|res| Quantity::from(res.as_str())));
        let min_quantity = row
            .try_get::<Option<String>, _>("min_quantity")
            .ok()
            .and_then(|res| res.map(|res| Quantity::from(res.as_str())));
        let max_notional = row
            .try_get::<Option<String>, _>("max_notional")
            .ok()
            .and_then(|res| res.map(|res| Money::from(res.as_str())));
        let min_notional = row
            .try_get::<Option<String>, _>("min_notional")
            .ok()
            .and_then(|res| res.map(|res| Money::from(res.as_str())));
        let max_price = row
            .try_get::<Option<String>, _>("max_price")
            .ok()
            .and_then(|res| res.map(|res| Price::from(res.as_str())));
        let min_price = row
            .try_get::<Option<String>, _>("min_price")
            .ok()
            .and_then(|res| res.map(|res| Price::from(res.as_str())));
        let margin_init = row
            .try_get::<String, _>("margin_init")
            .map(|res| Some(Decimal::from_str(res.as_str()).unwrap()))?;
        let margin_maint = row
            .try_get::<String, _>("margin_maint")
            .map(|res| Some(Decimal::from_str(res.as_str()).unwrap()))?;
        let maker_fee = row
            .try_get::<String, _>("maker_fee")
            .map(|res| Some(Decimal::from_str(res.as_str()).unwrap()))?;
        let taker_fee = row
            .try_get::<String, _>("taker_fee")
            .map(|res| Some(Decimal::from_str(res.as_str()).unwrap()))?;
        let ts_event = row.try_get::<String, _>("ts_event").map(UnixNanos::from)?;
        let ts_init = row.try_get::<String, _>("ts_init").map(UnixNanos::from)?;

        let inst = CurrencyPair::new(
            id,
            raw_symbol,
            base_currency,
            quote_currency,
            price_precision as u8,
            size_precision as u8,
            price_increment,
            size_increment,
            lot_size,
            max_quantity,
            min_quantity,
            max_notional,
            min_notional,
            max_price,
            min_price,
            margin_init,
            margin_maint,
            maker_fee,
            taker_fee,
            ts_event,
            ts_init,
        );
        Ok(CurrencyPairModel(inst))
    }
}

impl<'r> FromRow<'r, PgRow> for EquityModel {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let id = row.try_get::<String, _>("id").map(InstrumentId::from)?;
        let raw_symbol = row.try_get::<String, _>("raw_symbol").map(Symbol::from)?;
        let isin = row
            .try_get::<Option<String>, _>("isin")
            .map(|res| res.map(|s| Ustr::from(s.as_str())))?;
        let currency = row
            .try_get::<String, _>("quote_currency")
            .map(Currency::from)?;
        let price_precision = row.try_get::<i32, _>("price_precision")?;
        let price_increment = row
            .try_get::<String, _>("price_increment")
            .map(|res| Price::from_str(res.as_str()).unwrap())?;
        let lot_size = row
            .try_get::<Option<String>, _>("lot_size")
            .map(|res| res.map(|s| Quantity::from_str(s.as_str()).unwrap()))?;
        let max_quantity = row
            .try_get::<Option<String>, _>("max_quantity")
            .ok()
            .and_then(|res| res.map(|s| Quantity::from_str(s.as_str()).unwrap()));
        let min_quantity = row
            .try_get::<Option<String>, _>("min_quantity")
            .ok()
            .and_then(|res| res.map(|s| Quantity::from_str(s.as_str()).unwrap()));
        let max_price = row
            .try_get::<Option<String>, _>("max_price")
            .ok()
            .and_then(|res| res.map(|s| Price::from(s.as_str())));
        let min_price = row
            .try_get::<Option<String>, _>("min_price")
            .ok()
            .and_then(|res| res.map(|s| Price::from(s.as_str())));
        let margin_init = row
            .try_get::<String, _>("margin_init")
            .map(|res| Some(Decimal::from_str(res.as_str()).unwrap()))?;
        let margin_maint = row
            .try_get::<String, _>("margin_maint")
            .map(|res| Some(Decimal::from_str(res.as_str()).unwrap()))?;
        let maker_fee = row
            .try_get::<String, _>("maker_fee")
            .map(|res| Some(Decimal::from_str(res.as_str()).unwrap()))?;
        let taker_fee = row
            .try_get::<String, _>("taker_fee")
            .map(|res| Some(Decimal::from_str(res.as_str()).unwrap()))?;
        let ts_event = row.try_get::<String, _>("ts_event").map(UnixNanos::from)?;
        let ts_init = row.try_get::<String, _>("ts_init").map(UnixNanos::from)?;

        let inst = Equity::new(
            id,
            raw_symbol,
            isin,
            currency,
            price_precision as u8,
            price_increment,
            lot_size,
            max_quantity,
            min_quantity,
            max_price,
            min_price,
            margin_init,
            margin_maint,
            maker_fee,
            taker_fee,
            ts_event,
            ts_init,
        );
        Ok(EquityModel(inst))
    }
}

impl<'r> FromRow<'r, PgRow> for FuturesContractModel {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let id = row.try_get::<String, _>("id").map(InstrumentId::from)?;
        let raw_symbol = row.try_get::<String, _>("raw_symbol").map(Symbol::new)?;
        let asset_class = row
            .try_get::<AssetClassModel, _>("asset_class")
            .map(|res| res.0)?;
        let exchange = row
            .try_get::<Option<String>, _>("exchange")
            .map(|res| res.map(|s| Ustr::from(s.as_str())))?;
        let underlying = row
            .try_get::<String, _>("underlying")
            .map(|res| Ustr::from(res.as_str()))?;
        let currency = row
            .try_get::<String, _>("quote_currency")
            .map(Currency::from)?;
        let activation_ns = row
            .try_get::<String, _>("activation_ns")
            .map(UnixNanos::from)?;
        let expiration_ns = row
            .try_get::<String, _>("expiration_ns")
            .map(UnixNanos::from)?;
        let price_precision = row.try_get::<i32, _>("price_precision")?;
        let price_increment = row
            .try_get::<String, _>("price_increment")
            .map(|res| Price::from(res.as_str()))?;
        let multiplier = row
            .try_get::<String, _>("multiplier")
            .map(|res| Quantity::from(res.as_str()))?;
        let lot_size = row
            .try_get::<String, _>("lot_size")
            .map(|res| Quantity::from(res.as_str()))?;
        let max_quantity = row
            .try_get::<Option<String>, _>("max_quantity")
            .ok()
            .and_then(|res| res.map(|s| Quantity::from(s.as_str())));
        let min_quantity = row
            .try_get::<Option<String>, _>("min_quantity")
            .ok()
            .and_then(|res| res.map(|s| Quantity::from(s.as_str())));
        let max_price = row
            .try_get::<Option<String>, _>("max_price")
            .ok()
            .and_then(|res| res.map(|s| Price::from(s.as_str())));
        let min_price = row
            .try_get::<Option<String>, _>("min_price")
            .ok()
            .and_then(|res| res.map(|s| Price::from(s.as_str())));
        let margin_init = row
            .try_get::<String, _>("margin_init")
            .map(|res| Some(Decimal::from_str(res.as_str()).unwrap()))?;
        let margin_maint = row
            .try_get::<String, _>("margin_maint")
            .map(|res| Some(Decimal::from_str(res.as_str()).unwrap()))?;
        let maker_fee = row
            .try_get::<String, _>("maker_fee")
            .map(|res| Some(Decimal::from_str(res.as_str()).unwrap()))?;
        let taker_fee = row
            .try_get::<String, _>("taker_fee")
            .map(|res| Some(Decimal::from_str(res.as_str()).unwrap()))?;
        let ts_event = row.try_get::<String, _>("ts_event").map(UnixNanos::from)?;
        let ts_init = row.try_get::<String, _>("ts_init").map(UnixNanos::from)?;

        let inst = FuturesContract::new(
            id,
            raw_symbol,
            asset_class,
            exchange,
            underlying,
            activation_ns,
            expiration_ns,
            currency,
            price_precision as u8,
            price_increment,
            multiplier,
            lot_size,
            max_quantity,
            min_quantity,
            max_price,
            min_price,
            margin_init,
            margin_maint,
            maker_fee,
            taker_fee,
            ts_event,
            ts_init,
        );
        Ok(FuturesContractModel(inst))
    }
}

impl<'r> FromRow<'r, PgRow> for FuturesSpreadModel {
    fn from_row(_row: &'r PgRow) -> Result<Self, sqlx::Error> {
        todo!("Implement FromRow for FuturesSpread")
    }
}

impl<'r> FromRow<'r, PgRow> for OptionContractModel {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let id = row.try_get::<String, _>("id").map(InstrumentId::from)?;
        let raw_symbol = row.try_get::<String, _>("raw_symbol").map(Symbol::new)?;
        let asset_class = row
            .try_get::<AssetClassModel, _>("asset_class")
            .map(|res| res.0)?;
        let exchange = row
            .try_get::<Option<String>, _>("exchange")
            .map(|res| res.map(|s| Ustr::from(s.as_str())))?;
        let underlying = row
            .try_get::<String, _>("underlying")
            .map(|res| Ustr::from(res.as_str()))?;
        let option_kind = row
            .try_get::<String, _>("option_kind")
            .map(|res| OptionKind::from_str(res.as_str()).unwrap())?;
        let activation_ns = row
            .try_get::<String, _>("activation_ns")
            .map(UnixNanos::from)?;
        let expiration_ns = row
            .try_get::<String, _>("expiration_ns")
            .map(UnixNanos::from)?;
        let strike_price = row
            .try_get::<String, _>("strike_price")
            .map(|res| Price::from_str(res.as_str()).unwrap())?;
        let currency = row
            .try_get::<String, _>("quote_currency")
            .map(Currency::from)?;
        let price_precision = row.try_get::<i32, _>("price_precision").unwrap();
        let price_increment = row
            .try_get::<String, _>("price_increment")
            .map(|res| Price::from_str(res.as_str()).unwrap())?;
        let multiplier = row
            .try_get::<String, _>("multiplier")
            .map(|res| Quantity::from(res.as_str()))?;
        let lot_size = row
            .try_get::<String, _>("lot_size")
            .map(|res| Quantity::from(res.as_str()))
            .unwrap();
        let max_quantity = row
            .try_get::<Option<String>, _>("max_quantity")
            .ok()
            .and_then(|res| res.map(|s| Quantity::from(s.as_str())));
        let min_quantity = row
            .try_get::<Option<String>, _>("min_quantity")
            .ok()
            .and_then(|res| res.map(|s| Quantity::from(s.as_str())));
        let max_price = row
            .try_get::<Option<String>, _>("max_price")
            .ok()
            .and_then(|res| res.map(|s| Price::from(s.as_str())));
        let min_price = row
            .try_get::<Option<String>, _>("min_price")
            .ok()
            .and_then(|res| res.map(|s| Price::from(s.as_str())));
        let margin_init = row
            .try_get::<String, _>("margin_init")
            .map(|res| Some(Decimal::from_str(res.as_str()).unwrap()))?;
        let margin_maint = row
            .try_get::<String, _>("margin_maint")
            .map(|res| Some(Decimal::from_str(res.as_str()).unwrap()))?;
        let maker_fee = row
            .try_get::<String, _>("maker_fee")
            .map(|res| Some(Decimal::from_str(res.as_str()).unwrap()))?;
        let taker_fee = row
            .try_get::<String, _>("taker_fee")
            .map(|res| Some(Decimal::from_str(res.as_str()).unwrap()))?;
        let ts_event = row.try_get::<String, _>("ts_event").map(UnixNanos::from)?;
        let ts_init = row.try_get::<String, _>("ts_init").map(UnixNanos::from)?;

        let inst = OptionContract::new(
            id,
            raw_symbol,
            asset_class,
            exchange,
            underlying,
            option_kind,
            strike_price,
            currency,
            activation_ns,
            expiration_ns,
            price_precision as u8,
            price_increment,
            multiplier,
            lot_size,
            max_quantity,
            min_quantity,
            max_price,
            min_price,
            margin_init,
            margin_maint,
            maker_fee,
            taker_fee,
            ts_event,
            ts_init,
        );
        Ok(OptionContractModel(inst))
    }
}

impl<'r> FromRow<'r, PgRow> for OptionSpreadModel {
    fn from_row(_row: &'r PgRow) -> Result<Self, sqlx::Error> {
        todo!("Implement FromRow for OptionSpread")
    }
}
