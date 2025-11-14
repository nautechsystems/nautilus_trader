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

//! Conversion helpers that translate Kraken API schemas into Nautilus domain models.

use std::str::FromStr;

use anyhow::Context;
use nautilus_core::{datetime::NANOSECONDS_IN_MILLISECOND, nanos::UnixNanos};
use nautilus_model::{
    data::{Bar, BarType, TradeTick},
    enums::AggressorSide,
    identifiers::{InstrumentId, Symbol, TradeId},
    instruments::{Instrument, any::InstrumentAny, currency_pair::CurrencyPair},
    types::{Currency, Price, Quantity},
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::{
    common::consts::KRAKEN_VENUE,
    http::models::{AssetPairInfo, OhlcData},
};

/// Parse a decimal string, handling empty strings and "0" values.
pub fn parse_decimal(value: &str) -> anyhow::Result<Decimal> {
    if value.is_empty() || value == "0" {
        return Ok(dec!(0));
    }
    value
        .parse::<Decimal>()
        .map_err(|e| anyhow::anyhow!("Failed to parse decimal '{value}': {e}"))
}

/// Parse an optional decimal string.
pub fn parse_decimal_opt(value: Option<&str>) -> anyhow::Result<Option<Decimal>> {
    match value {
        Some(s) if !s.is_empty() && s != "0" => Ok(Some(parse_decimal(s)?)),
        _ => Ok(None),
    }
}

/// Parses a Kraken asset pair definition into a Nautilus currency pair instrument.
///
/// # Errors
///
/// Returns an error if:
/// - Tick size, order minimum, or cost minimum cannot be parsed.
/// - Price or quantity precision is invalid.
/// - Currency codes are invalid.
pub fn parse_spot_instrument(
    pair_name: &str,
    definition: &AssetPairInfo,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let symbol_str = definition.wsname.as_ref().unwrap_or(&definition.altname);
    let instrument_id = InstrumentId::new(Symbol::new(symbol_str.as_str()), *KRAKEN_VENUE);
    let raw_symbol = Symbol::new(pair_name);

    let base_currency = get_currency(definition.base.as_str());
    let quote_currency = get_currency(definition.quote.as_str());

    let price_increment = parse_price(
        definition
            .tick_size
            .as_ref()
            .context("tick_size is required")?,
        "tick_size",
    )?;

    // lot_decimals specifies the decimal precision for the lot size
    let size_precision = definition.lot_decimals;
    let size_increment = Quantity::new(10.0_f64.powi(-(size_precision as i32)), size_precision);

    let min_quantity = definition
        .ordermin
        .as_ref()
        .map(|s| parse_quantity(s, "ordermin"))
        .transpose()?;

    // Use base tier fees, convert from percentage
    let taker_fee = definition
        .fees
        .first()
        .map(|(_, fee)| Decimal::try_from(*fee))
        .transpose()
        .context("Failed to parse taker fee")?
        .map(|f| f / dec!(100));

    let maker_fee = definition
        .fees_maker
        .first()
        .map(|(_, fee)| Decimal::try_from(*fee))
        .transpose()
        .context("Failed to parse maker fee")?
        .map(|f| f / dec!(100));

    let instrument = CurrencyPair::new(
        instrument_id,
        raw_symbol,
        base_currency,
        quote_currency,
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        None,
        None,
        None,
        min_quantity,
        None,
        None,
        None,
        None,
        maker_fee,
        taker_fee,
        None,
        None,
        ts_event,
        ts_init,
    );

    Ok(InstrumentAny::CurrencyPair(instrument))
}

fn parse_price(value: &str, field: &str) -> anyhow::Result<Price> {
    Price::from_str(value)
        .map_err(|err| anyhow::anyhow!("Failed to parse {field}='{value}': {err}"))
}

fn parse_quantity(value: &str, field: &str) -> anyhow::Result<Quantity> {
    Quantity::from_str(value)
        .map_err(|err| anyhow::anyhow!("Failed to parse {field}='{value}': {err}"))
}

/// Returns a currency from the internal map or creates a new crypto currency.
///
/// Uses [`Currency::get_or_create_crypto`] to handle unknown currency codes,
/// which automatically registers newly listed Kraken assets.
pub fn get_currency(code: &str) -> Currency {
    Currency::get_or_create_crypto(code)
}

/// Parses a Kraken trade array into a Nautilus trade tick.
///
/// The Kraken API returns trades as arrays: [price, volume, time, side, type, misc, trade_id]
///
/// # Errors
///
/// Returns an error if:
/// - Price or volume cannot be parsed.
/// - Timestamp is invalid.
/// - Trade ID is invalid.
pub fn parse_trade_tick_from_array(
    trade_array: &[serde_json::Value],
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let price_str = trade_array
        .first()
        .and_then(|v| v.as_str())
        .context("Missing or invalid price")?;
    let price = parse_price_with_precision(price_str, instrument.price_precision(), "trade.price")?;

    let size_str = trade_array
        .get(1)
        .and_then(|v| v.as_str())
        .context("Missing or invalid volume")?;
    let size = parse_quantity_with_precision(size_str, instrument.size_precision(), "trade.size")?;

    let time = trade_array
        .get(2)
        .and_then(|v| v.as_f64())
        .context("Missing or invalid timestamp")?;
    let ts_event = parse_millis_timestamp(time, "trade.time")?;

    let side_str = trade_array
        .get(3)
        .and_then(|v| v.as_str())
        .context("Missing or invalid side")?;
    let aggressor = match side_str {
        "b" => AggressorSide::Buyer,
        "s" => AggressorSide::Seller,
        _ => AggressorSide::NoAggressor,
    };

    let trade_id_value = trade_array.get(6).context("Missing trade_id")?;
    let trade_id = if let Some(id) = trade_id_value.as_i64() {
        TradeId::new_checked(id.to_string())?
    } else if let Some(id_str) = trade_id_value.as_str() {
        TradeId::new_checked(id_str)?
    } else {
        anyhow::bail!("Invalid trade_id format");
    };

    TradeTick::new_checked(
        instrument.id(),
        price,
        size,
        aggressor,
        trade_id,
        ts_event,
        ts_init,
    )
    .context("Failed to construct TradeTick from Kraken trade")
}

/// Parses a Kraken OHLC entry into a Nautilus bar.
///
/// # Errors
///
/// Returns an error if:
/// - OHLC values cannot be parsed.
/// - Timestamp is invalid.
pub fn parse_bar(
    ohlc: &OhlcData,
    instrument: &InstrumentAny,
    bar_type: BarType,
    ts_init: UnixNanos,
) -> anyhow::Result<Bar> {
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let open = parse_price_with_precision(&ohlc.open, price_precision, "ohlc.open")?;
    let high = parse_price_with_precision(&ohlc.high, price_precision, "ohlc.high")?;
    let low = parse_price_with_precision(&ohlc.low, price_precision, "ohlc.low")?;
    let close = parse_price_with_precision(&ohlc.close, price_precision, "ohlc.close")?;
    let volume = parse_quantity_with_precision(&ohlc.volume, size_precision, "ohlc.volume")?;

    let ts_event = UnixNanos::from((ohlc.time as u64) * 1_000_000_000);

    Bar::new_checked(bar_type, open, high, low, close, volume, ts_event, ts_init)
        .context("Failed to construct Bar from Kraken OHLC")
}

fn parse_price_with_precision(value: &str, precision: u8, field: &str) -> anyhow::Result<Price> {
    let parsed = value
        .parse::<f64>()
        .with_context(|| format!("Failed to parse {field}='{value}' as f64"))?;
    Price::new_checked(parsed, precision).with_context(|| {
        format!("Failed to construct Price for {field} with precision {precision}")
    })
}

fn parse_quantity_with_precision(
    value: &str,
    precision: u8,
    field: &str,
) -> anyhow::Result<Quantity> {
    let parsed = value
        .parse::<f64>()
        .with_context(|| format!("Failed to parse {field}='{value}' as f64"))?;
    Quantity::new_checked(parsed, precision).with_context(|| {
        format!("Failed to construct Quantity for {field} with precision {precision}")
    })
}

pub fn parse_millis_timestamp(value: f64, field: &str) -> anyhow::Result<UnixNanos> {
    let millis = (value * 1000.0) as u64;
    let nanos = millis
        .checked_mul(NANOSECONDS_IN_MILLISECOND)
        .with_context(|| format!("{field} timestamp overflowed when converting to nanoseconds"))?;
    Ok(UnixNanos::from(nanos))
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use nautilus_model::{
        data::BarSpecification,
        enums::{AggregationSource, BarAggregation, PriceType},
    };
    use rstest::rstest;

    use super::*;
    use crate::http::models::AssetPairsResponse;

    const TS: UnixNanos = UnixNanos::new(1_700_000_000_000_000_000);

    fn load_test_json(filename: &str) -> String {
        let path = format!("test_data/{filename}");
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to load test data from {path}: {e}"))
    }

    #[rstest]
    fn test_parse_decimal() {
        assert_eq!(parse_decimal("123.45").unwrap(), dec!(123.45));
        assert_eq!(parse_decimal("0").unwrap(), dec!(0));
        assert_eq!(parse_decimal("").unwrap(), dec!(0));
    }

    #[rstest]
    fn test_parse_decimal_opt() {
        assert_eq!(
            parse_decimal_opt(Some("123.45")).unwrap(),
            Some(dec!(123.45))
        );
        assert_eq!(parse_decimal_opt(Some("0")).unwrap(), None);
        assert_eq!(parse_decimal_opt(Some("")).unwrap(), None);
        assert_eq!(parse_decimal_opt(None).unwrap(), None);
    }

    #[rstest]
    fn test_parse_spot_instrument() {
        let json = load_test_json("http_asset_pairs.json");
        let wrapper: serde_json::Value = serde_json::from_str(&json).unwrap();
        let result = wrapper.get("result").unwrap();
        let pairs: AssetPairsResponse = serde_json::from_value(result.clone()).unwrap();

        let (pair_name, definition) = pairs.iter().next().unwrap();

        let instrument = parse_spot_instrument(pair_name, definition, TS, TS).unwrap();

        match instrument {
            InstrumentAny::CurrencyPair(pair) => {
                assert_eq!(pair.id.venue.as_str(), "KRAKEN");
                assert_eq!(pair.base_currency.code.as_str(), "XXBT");
                assert_eq!(pair.quote_currency.code.as_str(), "USDT");
                assert!(pair.price_increment.as_f64() > 0.0);
                assert!(pair.size_increment.as_f64() > 0.0);
                assert!(pair.min_quantity.is_some());
            }
            _ => panic!("Expected CurrencyPair"),
        }
    }

    #[rstest]
    fn test_parse_trade_tick_from_array() {
        let json = load_test_json("http_trades.json");
        let wrapper: serde_json::Value = serde_json::from_str(&json).unwrap();
        let result = wrapper.get("result").unwrap();
        let trades_map = result.as_object().unwrap();

        // Get first pair's trades
        let (_pair, trades_value) = trades_map.iter().find(|(k, _)| *k != "last").unwrap();
        let trades = trades_value.as_array().unwrap();
        let trade_array = trades[0].as_array().unwrap();

        // Create a mock instrument for testing
        let instrument_id = InstrumentId::new(Symbol::new("BTC/USD"), *KRAKEN_VENUE);
        let instrument = InstrumentAny::CurrencyPair(CurrencyPair::new(
            instrument_id,
            Symbol::new("XBTUSDT"),
            Currency::BTC(),
            Currency::USDT(),
            1, // price_precision
            8, // size_precision
            Price::from("0.1"),
            Quantity::from("0.00000001"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            TS,
            TS,
        ));

        let trade_tick = parse_trade_tick_from_array(trade_array, &instrument, TS).unwrap();

        assert_eq!(trade_tick.instrument_id, instrument_id);
        assert!(trade_tick.price.as_f64() > 0.0);
        assert!(trade_tick.size.as_f64() > 0.0);
    }

    #[rstest]
    fn test_parse_bar() {
        let json = load_test_json("http_ohlc.json");
        let wrapper: serde_json::Value = serde_json::from_str(&json).unwrap();
        let result = wrapper.get("result").unwrap();
        let ohlc_map = result.as_object().unwrap();

        // Get first pair's OHLC data
        let (_pair, ohlc_value) = ohlc_map.iter().find(|(k, _)| *k != "last").unwrap();
        let ohlcs = ohlc_value.as_array().unwrap();

        // Parse first OHLC array into OhlcData
        let ohlc_array = ohlcs[0].as_array().unwrap();
        let ohlc = OhlcData {
            time: ohlc_array[0].as_i64().unwrap(),
            open: ohlc_array[1].as_str().unwrap().to_string(),
            high: ohlc_array[2].as_str().unwrap().to_string(),
            low: ohlc_array[3].as_str().unwrap().to_string(),
            close: ohlc_array[4].as_str().unwrap().to_string(),
            vwap: ohlc_array[5].as_str().unwrap().to_string(),
            volume: ohlc_array[6].as_str().unwrap().to_string(),
            count: ohlc_array[7].as_i64().unwrap(),
        };

        // Create a mock instrument
        let instrument_id = InstrumentId::new(Symbol::new("BTC/USD"), *KRAKEN_VENUE);
        let instrument = InstrumentAny::CurrencyPair(CurrencyPair::new(
            instrument_id,
            Symbol::new("XBTUSDT"),
            Currency::BTC(),
            Currency::USDT(),
            1, // price_precision
            8, // size_precision
            Price::from("0.1"),
            Quantity::from("0.00000001"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            TS,
            TS,
        ));

        let bar_type = BarType::new(
            instrument_id,
            BarSpecification::new(1, BarAggregation::Minute, PriceType::Last),
            AggregationSource::External,
        );

        let bar = parse_bar(&ohlc, &instrument, bar_type, TS).unwrap();

        assert_eq!(bar.bar_type, bar_type);
        assert!(bar.open.as_f64() > 0.0);
        assert!(bar.high.as_f64() > 0.0);
        assert!(bar.low.as_f64() > 0.0);
        assert!(bar.close.as_f64() > 0.0);
        assert!(bar.volume.as_f64() >= 0.0);
    }

    #[rstest]
    fn test_parse_millis_timestamp() {
        let timestamp = 1762795433.9717445;
        let result = parse_millis_timestamp(timestamp, "test").unwrap();
        assert!(result.as_u64() > 0);
    }
}
