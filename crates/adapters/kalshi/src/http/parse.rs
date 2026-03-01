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

//! HTTP instrument parsing for the Kalshi adapter.

use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::AssetClass,
    identifiers::{InstrumentId, Symbol},
    instruments::BinaryOption,
    types::{Currency, Price, Quantity},
};
use ustr::Ustr;

use super::models::KalshiMarket;
use crate::common::{
    consts::{KALSHI_VENUE, MAX_PRICE, MIN_PRICE, PRICE_PRECISION, SIZE_PRECISION},
    parse::parse_datetime_to_nanos,
};

/// Converts a [`KalshiMarket`] into a Nautilus [`BinaryOption`] instrument.
///
/// # Errors
///
/// Returns an error if any field fails to parse or if the `BinaryOption`
/// constructor validation fails.
pub fn market_to_binary_option(market: &KalshiMarket) -> anyhow::Result<BinaryOption> {
    let symbol = Symbol::new(market.ticker);
    let venue = *KALSHI_VENUE;
    let instrument_id = InstrumentId::new(symbol, venue);
    let raw_symbol = Symbol::new(market.ticker);

    let currency = Currency::USD();

    let price_increment = Price::from("0.0001");
    let size_increment = Quantity::from("0.01");

    let activation_ns = market
        .open_time
        .as_deref()
        .map(parse_datetime_to_nanos)
        .transpose()?
        .unwrap_or_default();

    let expiration_ns = market
        .close_time
        .as_deref()
        .or(market.latest_expiration_time.as_deref())
        .map(parse_datetime_to_nanos)
        .transpose()?
        .unwrap_or_default();

    let max_price = Some(Price::from(MAX_PRICE));
    let min_price = Some(Price::from(MIN_PRICE));

    let outcome = Some(Ustr::from("Yes"));
    let description = Some(Ustr::from(market.title.as_str()));

    BinaryOption::new_checked(
        instrument_id,
        raw_symbol,
        AssetClass::Alternative,
        currency,
        activation_ns,
        expiration_ns,
        PRICE_PRECISION,
        SIZE_PRECISION,
        price_increment,
        size_increment,
        outcome,
        description,
        None, // max_quantity
        None, // min_quantity
        None, // max_notional
        None, // min_notional
        max_price,
        min_price,
        None, // margin_init
        None, // margin_maint
        None, // maker_fee
        None, // taker_fee
        None, // info
        UnixNanos::default(),
        UnixNanos::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::models::KalshiMarketsResponse;

    fn load_fixture(name: &str) -> String {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_data")
            .join(name);
        std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("missing: {name}"))
    }

    #[test]
    fn test_market_to_binary_option() {
        let json = load_fixture("http_markets.json");
        let resp: KalshiMarketsResponse = serde_json::from_str(&json).unwrap();
        let instrument = market_to_binary_option(&resp.markets[0]).unwrap();
        assert_eq!(instrument.id.symbol.as_str(), "KXBTC-25MAR15-B100000");
        assert_eq!(instrument.raw_symbol.as_str(), "KXBTC-25MAR15-B100000");
        assert_eq!(instrument.price_precision, 4);
        assert_eq!(instrument.size_precision, 2);
    }
}
