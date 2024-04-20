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

use sqlx::{postgres::PgRow, FromRow, Row};

impl<'r> FromRow<'r, PgRow> for InstrumentAny {
    fn from_row(row: &'r PgRow) -> Result<Self, Error> {
        let kind = row.get::<String, _>("kind");
        if kind == "CRYPTO_FUTURE" {
            Ok(InstrumentAny::CryptoFuture(
                CryptoFuture::from_row(row).unwrap(),
            ))
        } else if kind == "CRYPTO_PERPETUAL" {
            Ok(InstrumentAny::CryptoPerpetual(
                CryptoPerpetual::from_row(row).unwrap(),
            ))
        } else if kind == "CURRENCY_PAIR" {
            Ok(InstrumentAny::CurrencyPair(
                CurrencyPair::from_row(row).unwrap(),
            ))
        } else if kind == "EQUITY" {
            Ok(InstrumentAny::Equity(Equity::from_row(row).unwrap()))
        } else if kind == "FUTURES_CONTRACT" {
            Ok(InstrumentAny::FuturesContract(
                FuturesContract::from_row(row).unwrap(),
            ))
        } else if kind == "FUTURES_SPREAD" {
            Ok(InstrumentAny::FuturesSpread(
                FuturesSpread::from_row(row).unwrap(),
            ))
        } else if kind == "OPTIONS_CONTRACT" {
            Ok(InstrumentAny::OptionsContract(
                OptionsContract::from_row(row).unwrap(),
            ))
        } else if kind == "OPTIONS_SPREAD" {
            Ok(InstrumentAny::OptionsSpread(
                OptionsSpread::from_row(row).unwrap(),
            ))
        } else {
            panic!("Unknown instrument type")
        }
    }
}

impl<'r> FromRow<'r, PgRow> for CryptoFuture {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let id = row
            .try_get::<String, _>("id")
            .map(|res| InstrumentId::from(res.as_str()))?;
        let raw_symbol = row
            .try_get::<String, _>("raw_symbol")
            .map(|res| Symbol::from(res.as_str()))?;
        let underlying = row
            .try_get::<String, _>("underlying")
            .map(|res| Currency::from(res.as_str()))?;
        let quote_currency = row
            .try_get::<String, _>("quote_currency")
            .map(|res| Currency::from(res.as_str()))?;
        let settlement_currency = row
            .try_get::<String, _>("settlement_currency")
            .map(|res| Currency::from(res.as_str()))?;
        let is_inverse = row.try_get::<bool, _>("is_inverse")?;
        let activation_ns = row
            .try_get::<String, _>("activation_ns")
            .map(|res| UnixNanos::from(res.as_str()))?;
        let expiration_ns = row
            .try_get::<String, _>("expiration_ns")
            .map(|res| UnixNanos::from(res.as_str()))?;
        let price_precision = row.try_get::<i32, _>("price_precision")?;
        let size_precision = row.try_get::<i32, _>("size_precision")?;
        let price_increment = row
            .try_get::<String, _>("price_increment")
            .map(|res| Price::from_str(res.as_str()).unwrap())?;
        let size_increment = row
            .try_get::<String, _>("size_increment")
            .map(|res| Quantity::from_str(res.as_str()).unwrap())?;
        let maker_fee = row
            .try_get::<String, _>("maker_fee")
            .map(|res| Decimal::from_str(res.as_str()).unwrap())?;
        let taker_fee = row
            .try_get::<String, _>("taker_fee")
            .map(|res| Decimal::from_str(res.as_str()).unwrap())?;
        let margin_init = row
            .try_get::<String, _>("margin_init")
            .map(|res| Decimal::from_str(res.as_str()).unwrap())?;
        let margin_maint = row
            .try_get::<String, _>("margin_maint")
            .map(|res| Decimal::from_str(res.as_str()).unwrap())?;
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
        let ts_event = row
            .try_get::<String, _>("ts_event")
            .map(|res| UnixNanos::from(res.as_str()))?;
        let ts_init = row
            .try_get::<String, _>("ts_init")
            .map(|res| UnixNanos::from(res.as_str()))?;
        Ok(Self::new(
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
            maker_fee,
            taker_fee,
            margin_init,
            margin_maint,
            Some(lot_size),
            max_quantity,
            min_quantity,
            max_notional,
            min_notional,
            max_price,
            min_price,
            ts_event,
            ts_init,
        )
        .unwrap())
    }
}

impl<'r> FromRow<'r, PgRow> for CryptoPerpetual {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let id = row
            .try_get::<String, _>("id")
            .map(|res| InstrumentId::from(res.as_str()))?;
        let raw_symbol = row
            .try_get::<String, _>("raw_symbol")
            .map(|res| Symbol::from(res.as_str()))?;
        let base_currency = row
            .try_get::<String, _>("base_currency")
            .map(|res| Currency::from(res.as_str()))?;
        let quote_currency = row
            .try_get::<String, _>("quote_currency")
            .map(|res| Currency::from(res.as_str()))?;
        let settlement_currency = row
            .try_get::<String, _>("settlement_currency")
            .map(|res| Currency::from(res.as_str()))?;
        let is_inverse = row.try_get::<bool, _>("is_inverse")?;
        let price_precision = row.try_get::<i32, _>("price_precision")?;
        let size_precision = row.try_get::<i32, _>("size_precision")?;
        let price_increment = row
            .try_get::<String, _>("price_increment")
            .map(|res| Price::from_str(res.as_str()).unwrap())?;
        let size_increment = row
            .try_get::<String, _>("size_increment")
            .map(|res| Quantity::from_str(res.as_str()).unwrap())?;
        let maker_fee = row
            .try_get::<String, _>("maker_fee")
            .map(|res| Decimal::from_str(res.as_str()).unwrap())?;
        let taker_fee = row
            .try_get::<String, _>("taker_fee")
            .map(|res| Decimal::from_str(res.as_str()).unwrap())?;
        let margin_init = row
            .try_get::<String, _>("margin_init")
            .map(|res| Decimal::from_str(res.as_str()).unwrap())?;
        let margin_maint = row
            .try_get::<String, _>("margin_maint")
            .map(|res| Decimal::from_str(res.as_str()).unwrap())?;
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
        let ts_event = row
            .try_get::<String, _>("ts_event")
            .map(|res| UnixNanos::from(res.as_str()))?;
        let ts_init = row
            .try_get::<String, _>("ts_init")
            .map(|res| UnixNanos::from(res.as_str()))?;
        Ok(Self::new(
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
            maker_fee,
            taker_fee,
            margin_init,
            margin_maint,
            Some(lot_size),
            max_quantity,
            min_quantity,
            max_notional,
            min_notional,
            max_price,
            min_price,
            ts_event,
            ts_init,
        )
        .unwrap())
    }
}

impl<'r> FromRow<'r, PgRow> for CurrencyPair {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let id = row
            .try_get::<String, _>("id")
            .map(|res| InstrumentId::from(res.as_str()))?;
        let raw_symbol = row
            .try_get::<String, _>("raw_symbol")
            .map(|res| Symbol::from(res.as_str()))?;
        let base_currency = row
            .try_get::<String, _>("base_currency")
            .map(|res| Currency::from(res.as_str()))?;
        let quote_currency = row
            .try_get::<String, _>("quote_currency")
            .map(|res| Currency::from(res.as_str()))?;
        let price_precision = row.try_get::<i32, _>("price_precision")?;
        let size_precision = row.try_get::<i32, _>("size_precision")?;
        let price_increment = row
            .try_get::<String, _>("price_increment")
            .map(|res| Price::from(res.as_str()))?;
        let size_increment = row
            .try_get::<String, _>("size_increment")
            .map(|res| Quantity::from(res.as_str()))?;
        let maker_fee = row
            .try_get::<String, _>("maker_fee")
            .map(|res| Decimal::from_str(res.as_str()).unwrap())?;
        let taker_fee = row
            .try_get::<String, _>("taker_fee")
            .map(|res| Decimal::from_str(res.as_str()).unwrap())?;
        let margin_init = row
            .try_get::<String, _>("margin_init")
            .map(|res| Decimal::from_str(res.as_str()).unwrap())?;
        let margin_maint = row
            .try_get::<String, _>("margin_maint")
            .map(|res| Decimal::from_str(res.as_str()).unwrap())?;
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
        let ts_event = row
            .try_get::<String, _>("ts_event")
            .map(|res| UnixNanos::from(res.as_str()))?;
        let ts_init = row
            .try_get::<String, _>("ts_init")
            .map(|res| UnixNanos::from(res.as_str()))?;
        Ok(Self::new(
            id,
            raw_symbol,
            base_currency,
            quote_currency,
            price_precision as u8,
            size_precision as u8,
            price_increment,
            size_increment,
            taker_fee,
            maker_fee,
            margin_init,
            margin_maint,
            lot_size,
            max_quantity,
            min_quantity,
            max_notional,
            min_notional,
            max_price,
            min_price,
            ts_event,
            ts_init,
        )
        .unwrap())
    }
}

impl<'r> FromRow<'r, PgRow> for Equity {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let id = row
            .try_get::<String, _>("id")
            .map(|res| InstrumentId::from(res.as_str()))?;
        let raw_symbol = row
            .try_get::<String, _>("raw_symbol")
            .map(|res| Symbol::from(res.as_str()))?;
        let isin = row
            .try_get::<Option<String>, _>("isin")
            .map(|res| res.map(|s| Ustr::from(s.as_str())))?;
        let currency = row
            .try_get::<String, _>("quote_currency")
            .map(|res| Currency::from(res.as_str()))?;
        let price_precision = row.try_get::<i32, _>("price_precision")?;
        let price_increment = row
            .try_get::<String, _>("price_increment")
            .map(|res| Price::from_str(res.as_str()).unwrap())?;
        let maker_fee = row
            .try_get::<String, _>("maker_fee")
            .map(|res| Decimal::from_str(res.as_str()).unwrap())?;
        let taker_fee = row
            .try_get::<String, _>("taker_fee")
            .map(|res| Decimal::from_str(res.as_str()).unwrap())?;
        let margin_init = row
            .try_get::<String, _>("margin_init")
            .map(|res| Decimal::from_str(res.as_str()).unwrap())?;
        let margin_maint = row
            .try_get::<String, _>("margin_maint")
            .map(|res| Decimal::from_str(res.as_str()).unwrap())?;
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
        let ts_event = row
            .try_get::<String, _>("ts_event")
            .map(|res| UnixNanos::from(res.as_str()))?;
        let ts_init = row
            .try_get::<String, _>("ts_init")
            .map(|res| UnixNanos::from(res.as_str()))?;
        Ok(Self::new(
            id,
            raw_symbol,
            isin,
            currency,
            price_precision as u8,
            price_increment,
            Some(maker_fee),
            Some(taker_fee),
            Some(margin_init),
            Some(margin_maint),
            lot_size,
            max_quantity,
            min_quantity,
            max_price,
            min_price,
            ts_event,
            ts_init,
        )
        .unwrap())
    }
}

impl<'r> FromRow<'r, PgRow> for FuturesContract {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let id = row
            .try_get::<String, _>("id")
            .map(|res| InstrumentId::from(res.as_str()))?;
        let raw_symbol = row
            .try_get::<String, _>("raw_symbol")
            .map(|res| Symbol::new(res.as_str()).unwrap())?;
        let asset_class = row
            .try_get::<String, _>("asset_class")
            .map(|res| AssetClass::from_str(res.as_str()).unwrap())?;
        let exchange = row
            .try_get::<Option<String>, _>("exchange")
            .map(|res| res.map(|s| Ustr::from(s.as_str())))?;
        let underlying = row
            .try_get::<String, _>("underlying")
            .map(|res| Ustr::from(res.as_str()))?;
        let currency = row
            .try_get::<String, _>("quote_currency")
            .map(|res| Currency::from_str(res.as_str()).unwrap())?;
        let activation_ns = row
            .try_get::<String, _>("activation_ns")
            .map(|res| UnixNanos::from(res.as_str()))?;
        let expiration_ns = row
            .try_get::<String, _>("expiration_ns")
            .map(|res| UnixNanos::from(res.as_str()))?;
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
            .map(|res| Decimal::from_str(res.as_str()).unwrap())?;
        let margin_maint = row
            .try_get::<String, _>("margin_maint")
            .map(|res| Decimal::from_str(res.as_str()).unwrap())?;
        let ts_event = row
            .try_get::<String, _>("ts_event")
            .map(|res| UnixNanos::from(res.as_str()))?;
        let ts_init = row
            .try_get::<String, _>("ts_init")
            .map(|res| UnixNanos::from(res.as_str()))?;
        Ok(Self::new(
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
            Some(margin_init),
            Some(margin_maint),
            ts_event,
            ts_init,
        )
        .unwrap())
    }
}

impl<'r> FromRow<'r, PgRow> for FuturesSpread {
    fn from_row(_row: &'r PgRow) -> Result<Self, sqlx::Error> {
        todo!("Implement FromRow for FuturesSpread")
    }
}

impl<'r> FromRow<'r, PgRow> for OptionsContract {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let id = row
            .try_get::<String, _>("id")
            .map(|res| InstrumentId::from(res.as_str()))?;
        let raw_symbol = row
            .try_get::<String, _>("raw_symbol")
            .map(|res| Symbol::new(res.as_str()).unwrap())?;
        let asset_class = row
            .try_get::<String, _>("asset_class")
            .map(|res| AssetClass::from_str(res.as_str()).unwrap())?;
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
            .map(|res| UnixNanos::from(res.as_str()))?;
        let expiration_ns = row
            .try_get::<String, _>("expiration_ns")
            .map(|res| UnixNanos::from(res.as_str()))?;
        let strike_price = row
            .try_get::<String, _>("strike_price")
            .map(|res| Price::from_str(res.as_str()).unwrap())?;
        let currency = row
            .try_get::<String, _>("quote_currency")
            .map(|res| Currency::from_str(res.as_str()).unwrap())?;
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
            .map(|res| Decimal::from_str(res.as_str()).unwrap())?;
        let margin_maint = row
            .try_get::<String, _>("margin_maint")
            .map(|res| Decimal::from_str(res.as_str()).unwrap())?;
        let ts_event = row
            .try_get::<String, _>("ts_event")
            .map(|res| UnixNanos::from(res.as_str()))?;
        let ts_init = row
            .try_get::<String, _>("ts_init")
            .map(|res| UnixNanos::from(res.as_str()))?;
        Ok(Self::new(
            id,
            raw_symbol,
            asset_class,
            exchange,
            underlying,
            option_kind,
            activation_ns,
            expiration_ns,
            strike_price,
            currency,
            price_precision as u8,
            price_increment,
            multiplier,
            lot_size,
            max_quantity,
            min_quantity,
            max_price,
            min_price,
            Some(margin_init),
            Some(margin_maint),
            ts_event,
            ts_init,
        )
        .unwrap())
    }
}

impl<'r> FromRow<'r, PgRow> for OptionsSpread {
    fn from_row(_row: &'r PgRow) -> Result<Self, sqlx::Error> {
        todo!("Implement FromRow for OptionsSpread")
    }
}
