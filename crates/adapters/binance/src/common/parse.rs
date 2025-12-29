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

//! Parsing utilities for Binance API responses.
//!
//! Provides conversion functions to transform raw Binance exchange data
//! into Nautilus domain objects such as instruments and market data.

use std::str::FromStr;

use anyhow::Context;
use nautilus_core::nanos::UnixNanos;
use nautilus_model::{
    identifiers::{InstrumentId, Symbol, Venue},
    instruments::{any::InstrumentAny, crypto_perpetual::CryptoPerpetual},
    types::{Currency, Price, Quantity},
};
use rust_decimal::Decimal;
use serde_json::Value;

use crate::http::models::BinanceFuturesUsdSymbol;

const BINANCE_VENUE: &str = "BINANCE";
const CONTRACT_TYPE_PERPETUAL: &str = "PERPETUAL";

/// Returns a currency from the internal map or creates a new crypto currency.
pub fn get_currency(code: &str) -> Currency {
    Currency::get_or_create_crypto(code)
}

/// Extracts filter values from Binance symbol filters array.
fn get_filter<'a>(filters: &'a [Value], filter_type: &str) -> Option<&'a Value> {
    filters.iter().find(|f| {
        f.get("filterType")
            .and_then(|v| v.as_str())
            .is_some_and(|t| t == filter_type)
    })
}

/// Parses a string field from a JSON value.
fn parse_filter_string(filter: &Value, field: &str) -> anyhow::Result<String> {
    filter
        .get(field)
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| anyhow::anyhow!("Missing field '{field}' in filter"))
}

/// Parses a Price from a filter field.
fn parse_filter_price(filter: &Value, field: &str) -> anyhow::Result<Price> {
    let value = parse_filter_string(filter, field)?;
    Price::from_str(&value).map_err(|e| anyhow::anyhow!("Failed to parse {field}='{value}': {e}"))
}

/// Parses a Quantity from a filter field.
fn parse_filter_quantity(filter: &Value, field: &str) -> anyhow::Result<Quantity> {
    let value = parse_filter_string(filter, field)?;
    Quantity::from_str(&value)
        .map_err(|e| anyhow::anyhow!("Failed to parse {field}='{value}': {e}"))
}

/// Parses a USD-M Futures symbol definition into a Nautilus CryptoPerpetual instrument.
///
/// # Errors
///
/// Returns an error if:
/// - Required filter values are missing (PRICE_FILTER, LOT_SIZE).
/// - Price or quantity values cannot be parsed.
/// - The contract type is not PERPETUAL.
pub fn parse_usdm_instrument(
    symbol: &BinanceFuturesUsdSymbol,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    // Only handle perpetual contracts for now
    if symbol.contract_type != CONTRACT_TYPE_PERPETUAL {
        anyhow::bail!(
            "Unsupported contract type '{}' for symbol '{}', expected '{}'",
            symbol.contract_type,
            symbol.symbol,
            CONTRACT_TYPE_PERPETUAL
        );
    }

    let base_currency = get_currency(symbol.base_asset.as_str());
    let quote_currency = get_currency(symbol.quote_asset.as_str());
    let settlement_currency = get_currency(symbol.margin_asset.as_str());

    let instrument_id = InstrumentId::new(
        Symbol::from_str_unchecked(format!("{}-PERP", symbol.symbol)),
        Venue::new(BINANCE_VENUE),
    );
    let raw_symbol = Symbol::new(symbol.symbol.as_str());

    let price_filter = get_filter(&symbol.filters, "PRICE_FILTER")
        .context("Missing PRICE_FILTER in symbol filters")?;

    let tick_size = parse_filter_price(price_filter, "tickSize")?;
    let max_price = parse_filter_price(price_filter, "maxPrice").ok();
    let min_price = parse_filter_price(price_filter, "minPrice").ok();

    let lot_filter =
        get_filter(&symbol.filters, "LOT_SIZE").context("Missing LOT_SIZE in symbol filters")?;

    let step_size = parse_filter_quantity(lot_filter, "stepSize")?;
    let max_quantity = parse_filter_quantity(lot_filter, "maxQty").ok();
    let min_quantity = parse_filter_quantity(lot_filter, "minQty").ok();

    // Default margin (0.1 = 10x leverage)
    let default_margin = Decimal::new(1, 1);

    let instrument = CryptoPerpetual::new(
        instrument_id,
        raw_symbol,
        base_currency,
        quote_currency,
        settlement_currency,
        false, // is_inverse
        tick_size.precision,
        step_size.precision,
        tick_size,
        step_size,
        None, // multiplier
        Some(step_size),
        max_quantity,
        min_quantity,
        None, // max_notional
        None, // min_notional
        max_price,
        min_price,
        Some(default_margin),
        Some(default_margin),
        None, // maker_fee
        None, // taker_fee
        ts_event,
        ts_init,
    );

    Ok(InstrumentAny::CryptoPerpetual(instrument))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json::json;
    use ustr::Ustr;

    use super::*;
    use crate::{common::enums::BinanceTradingStatus, http::models::BinanceFuturesUsdSymbol};

    fn sample_usdm_symbol() -> BinanceFuturesUsdSymbol {
        BinanceFuturesUsdSymbol {
            symbol: Ustr::from("BTCUSDT"),
            pair: Ustr::from("BTCUSDT"),
            contract_type: "PERPETUAL".to_string(),
            delivery_date: 4133404800000,
            onboard_date: 1569398400000,
            status: BinanceTradingStatus::Trading,
            maint_margin_percent: "2.5000".to_string(),
            required_margin_percent: "5.0000".to_string(),
            base_asset: Ustr::from("BTC"),
            quote_asset: Ustr::from("USDT"),
            margin_asset: Ustr::from("USDT"),
            price_precision: 2,
            quantity_precision: 3,
            base_asset_precision: 8,
            quote_precision: 8,
            underlying_type: Some("COIN".to_string()),
            underlying_sub_type: vec!["PoW".to_string()],
            settle_plan: None,
            trigger_protect: Some("0.0500".to_string()),
            liquidation_fee: Some("0.012500".to_string()),
            market_take_bound: Some("0.05".to_string()),
            order_types: vec!["LIMIT".to_string(), "MARKET".to_string()],
            time_in_force: vec!["GTC".to_string(), "IOC".to_string()],
            filters: vec![
                json!({
                    "filterType": "PRICE_FILTER",
                    "tickSize": "0.10",
                    "maxPrice": "4529764",
                    "minPrice": "556.80"
                }),
                json!({
                    "filterType": "LOT_SIZE",
                    "stepSize": "0.001",
                    "maxQty": "1000",
                    "minQty": "0.001"
                }),
            ],
        }
    }

    #[rstest]
    fn test_parse_usdm_perpetual() {
        let symbol = sample_usdm_symbol();
        let ts = UnixNanos::from(1_700_000_000_000_000_000u64);

        let result = parse_usdm_instrument(&symbol, ts, ts);
        assert!(result.is_ok(), "Failed: {:?}", result.err());

        let instrument = result.unwrap();
        match instrument {
            InstrumentAny::CryptoPerpetual(perp) => {
                assert_eq!(perp.id.to_string(), "BTCUSDT-PERP.BINANCE");
                assert_eq!(perp.raw_symbol.to_string(), "BTCUSDT");
                assert_eq!(perp.base_currency.code.as_str(), "BTC");
                assert_eq!(perp.quote_currency.code.as_str(), "USDT");
                assert_eq!(perp.settlement_currency.code.as_str(), "USDT");
                assert!(!perp.is_inverse);
                assert_eq!(perp.price_increment, Price::from_str("0.10").unwrap());
                assert_eq!(perp.size_increment, Quantity::from_str("0.001").unwrap());
            }
            other => panic!("Expected CryptoPerpetual, got {other:?}"),
        }
    }

    #[rstest]
    fn test_parse_non_perpetual_fails() {
        let mut symbol = sample_usdm_symbol();
        symbol.contract_type = "CURRENT_QUARTER".to_string();
        let ts = UnixNanos::from(1_700_000_000_000_000_000u64);

        let result = parse_usdm_instrument(&symbol, ts, ts);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Unsupported contract type")
        );
    }

    #[rstest]
    fn test_parse_missing_price_filter_fails() {
        let mut symbol = sample_usdm_symbol();
        symbol.filters = vec![json!({
            "filterType": "LOT_SIZE",
            "stepSize": "0.001",
            "maxQty": "1000",
            "minQty": "0.001"
        })];
        let ts = UnixNanos::from(1_700_000_000_000_000_000u64);

        let result = parse_usdm_instrument(&symbol, ts, ts);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Missing PRICE_FILTER")
        );
    }
}
