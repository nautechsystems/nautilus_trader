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

use nautilus_core::UnixNanos;
use nautilus_model::{
    currencies::CURRENCY_MAP,
    enums::CurrencyType,
    identifiers::{InstrumentId, Symbol},
    instruments::{CryptoFuture, CryptoOption, CryptoPerpetual, CurrencyPair, InstrumentAny},
    types::{Currency, Price, Quantity},
};
use rust_decimal::Decimal;

use super::{models::InstrumentInfo, parse::parse_settlement_currency};
use crate::parse::parse_option_kind;

/// Returns the currency either from the internal currency map or creates a default crypto.
pub(crate) fn get_currency(code: &str) -> Currency {
    CURRENCY_MAP
        .lock()
        .unwrap()
        .get(code)
        .copied()
        .unwrap_or(Currency::new(code, 8, 0, code, CurrencyType::Crypto))
}

#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn create_currency_pair(
    info: &InstrumentInfo,
    instrument_id: InstrumentId,
    raw_symbol: Symbol,
    price_increment: Price,
    size_increment: Quantity,
    margin_init: Decimal,
    margin_maint: Decimal,
    maker_fee: Decimal,
    taker_fee: Decimal,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> InstrumentAny {
    InstrumentAny::CurrencyPair(CurrencyPair::new(
        instrument_id,
        raw_symbol,
        get_currency(info.base_currency.to_uppercase().as_str()),
        get_currency(info.quote_currency.to_uppercase().as_str()),
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        None, // lot_size TBD
        None,
        Some(Quantity::from(info.min_trade_amount.to_string().as_str())),
        None,
        None,
        None,
        None,
        Some(margin_init),
        Some(margin_maint),
        Some(maker_fee),
        Some(taker_fee),
        ts_event,
        ts_init,
    ))
}

#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn create_crypto_perpetual(
    info: &InstrumentInfo,
    instrument_id: InstrumentId,
    raw_symbol: Symbol,
    price_increment: Price,
    size_increment: Quantity,
    multiplier: Option<Quantity>,
    margin_init: Decimal,
    margin_maint: Decimal,
    maker_fee: Decimal,
    taker_fee: Decimal,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> InstrumentAny {
    let is_inverse = info.inverse.unwrap_or(false);

    InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
        instrument_id,
        raw_symbol,
        get_currency(info.base_currency.to_uppercase().as_str()),
        get_currency(info.quote_currency.to_uppercase().as_str()),
        get_currency(parse_settlement_currency(info, is_inverse).as_str()),
        is_inverse,
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        multiplier,
        None, // lot_size TBD
        None,
        Some(Quantity::from(info.min_trade_amount.to_string().as_str())),
        None,
        None,
        None,
        None,
        Some(margin_init),
        Some(margin_maint),
        Some(maker_fee),
        Some(taker_fee),
        ts_event,
        ts_init,
    ))
}

#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn create_crypto_future(
    info: &InstrumentInfo,
    instrument_id: InstrumentId,
    raw_symbol: Symbol,
    activation: UnixNanos,
    expiration: UnixNanos,
    price_increment: Price,
    size_increment: Quantity,
    multiplier: Option<Quantity>,
    margin_init: Decimal,
    margin_maint: Decimal,
    maker_fee: Decimal,
    taker_fee: Decimal,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> InstrumentAny {
    let is_inverse = info.inverse.unwrap_or(false);

    InstrumentAny::CryptoFuture(CryptoFuture::new(
        instrument_id,
        raw_symbol,
        get_currency(info.base_currency.to_uppercase().as_str()),
        get_currency(info.quote_currency.to_uppercase().as_str()),
        get_currency(parse_settlement_currency(info, is_inverse).as_str()),
        is_inverse,
        activation,
        expiration,
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        multiplier,
        None, // lot_size TBD
        None,
        Some(Quantity::from(info.min_trade_amount.to_string().as_str())),
        None,
        None,
        None,
        None,
        Some(margin_init),
        Some(margin_maint),
        Some(maker_fee),
        Some(taker_fee),
        ts_event,
        ts_init,
    ))
}

#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn create_crypto_option(
    info: &InstrumentInfo,
    instrument_id: InstrumentId,
    raw_symbol: Symbol,
    activation: UnixNanos,
    expiration: UnixNanos,
    price_increment: Price,
    size_increment: Quantity,
    multiplier: Option<Quantity>,
    margin_init: Decimal,
    margin_maint: Decimal,
    maker_fee: Decimal,
    taker_fee: Decimal,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> InstrumentAny {
    let is_inverse = info.inverse.unwrap_or(false);

    InstrumentAny::CryptoOption(CryptoOption::new(
        instrument_id,
        raw_symbol,
        get_currency(info.base_currency.to_uppercase().as_str()),
        get_currency(info.quote_currency.to_uppercase().as_str()),
        get_currency(parse_settlement_currency(info, is_inverse).as_str()),
        is_inverse,
        parse_option_kind(
            info.option_type
                .clone()
                .expect("CryptoOption should have `option_type` field"),
        ),
        Price::new(
            info.strike_price
                .expect("CryptoOption should have `strike_price` field"),
            price_increment.precision,
        ),
        activation,
        expiration,
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        multiplier,
        None,
        Some(Quantity::from(info.min_trade_amount.to_string().as_str())),
        None,
        None,
        None,
        None,
        Some(margin_init),
        Some(margin_maint),
        Some(maker_fee),
        Some(taker_fee),
        ts_event,
        ts_init,
    ))
}
