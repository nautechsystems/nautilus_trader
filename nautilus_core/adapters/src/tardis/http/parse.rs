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

use std::str::FromStr;

use nautilus_core::nanos::UnixNanos;
use nautilus_model::{
    instruments::{any::InstrumentAny, currency_pair::CurrencyPair},
    types::{currency::Currency, price::Price, quantity::Quantity},
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use super::types::InstrumentInfo;
use crate::tardis::{enums::InstrumentType, parse::parse_instrument_id_with_enum};

pub fn parse_instrument_any(info: InstrumentInfo, ts_init: UnixNanos) -> InstrumentAny {
    match info.instrument_type {
        InstrumentType::Spot => parse_spot_instrument(info, ts_init),
        InstrumentType::Perpetual => parse_perp_instrument(info),
        InstrumentType::Future => parse_future_instrument(info),
        InstrumentType::Option => parse_option_instrument(info),
    }
}

fn parse_spot_instrument(info: InstrumentInfo, ts_init: UnixNanos) -> InstrumentAny {
    let instrument_id = parse_instrument_id_with_enum(&info.id, &info.exchange);
    let price_increment = Price::from(info.price_increment.to_string().as_str());
    let size_increment = Quantity::from(info.amount_increment.to_string().as_str());

    let currency_pair = CurrencyPair::new(
        instrument_id,
        instrument_id.symbol,
        Currency::from(info.base_currency.to_uppercase().as_str()),
        Currency::from(info.quote_currency.to_uppercase().as_str()),
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        Decimal::from_str(info.taker_fee.to_string().as_str()).expect("Invalid decimal value"),
        Decimal::from_str(info.maker_fee.to_string().as_str()).expect("Invalid decimal value"),
        dec!(0), // TBD
        dec!(0), // TBD
        None,    // TBD
        None,
        Some(Quantity::from(info.min_trade_amount.to_string().as_str())),
        None,
        None,
        None,
        None,
        ts_init, // ts_event same as ts_init (no local timestamp)
        ts_init,
    );

    InstrumentAny::CurrencyPair(currency_pair)
}

fn parse_perp_instrument(_info: InstrumentInfo) -> InstrumentAny {
    todo!()
}

fn parse_future_instrument(_info: InstrumentInfo) -> InstrumentAny {
    todo!()
}

fn parse_option_instrument(_info: InstrumentInfo) -> InstrumentAny {
    todo!()
}
