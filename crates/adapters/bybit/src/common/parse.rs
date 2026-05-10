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

//! Conversion functions that translate Bybit API schemas into Nautilus instruments.

use std::{convert::TryFrom, str::FromStr};

use anyhow::Context;
pub use nautilus_core::serialization::{
    deserialize_decimal_or_zero, deserialize_optional_decimal_or_zero,
    deserialize_optional_decimal_str, deserialize_string_to_u8,
};

/// Serde helper for Bybit `ON`/`OFF` string fields that represent booleans.
///
/// Use as `#[serde(with = "on_off_bool")]`. Unknown values deserialize as an
/// error rather than silently coercing, so field renames surface rather than
/// decoding to the wrong value.
pub mod on_off_bool {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(value: &bool, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(if *value { "ON" } else { "OFF" })
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
        let raw = String::deserialize(d)?;
        match raw.as_str() {
            "ON" => Ok(true),
            "OFF" => Ok(false),
            other => Err(D::Error::custom(format!(
                "expected 'ON' or 'OFF', received {other:?}"
            ))),
        }
    }
}

/// Serde helper that accepts `readOnly` as either a bool or `0`/`1` integer.
///
/// Bybit returns `readOnly` as a bool on `/v5/user/list-sub-apikeys` and as an
/// integer on `/v5/user/query-api` and the two update endpoints. Deserializing
/// through this module keeps the Rust field a plain `bool` across all DTOs.
pub mod bool_or_int {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(value: &bool, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bool(*value)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum BoolOrInt {
            Bool(bool),
            Int(i64),
        }

        match BoolOrInt::deserialize(d)? {
            BoolOrInt::Bool(b) => Ok(b),
            BoolOrInt::Int(0) => Ok(false),
            BoolOrInt::Int(1) => Ok(true),
            BoolOrInt::Int(n) => Err(D::Error::custom(format!(
                "expected bool or 0/1, received {n}"
            ))),
        }
    }
}

/// Round-trips `Option<bool>` as `0`/`1` integers for Bybit request bodies
/// that advertise `readOnly` as an integer on the wire.
pub mod opt_bool_as_int {
    use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

    pub fn serialize<S: Serializer>(value: &Option<bool>, s: S) -> Result<S::Ok, S::Error> {
        value.map(i32::from).serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
        match Option::<i32>::deserialize(d)? {
            None => Ok(None),
            Some(0) => Ok(Some(false)),
            Some(1) => Ok(Some(true)),
            Some(n) => Err(D::Error::custom(format!("expected 0 or 1, received {n}"))),
        }
    }
}

/// Serde helper that treats the masked secret literal (`"******"`) and empty
/// strings as `None`, preserving real values as `Some`.
///
/// Bybit responses never expose a usable secret: `list-sub-apikeys` returns
/// `"******"`, while the update endpoints return `""`. Surfacing `Option<String>`
/// keeps callers from accidentally treating the sentinel as a real credential.
pub mod masked_secret {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(value: &Option<String>, s: S) -> Result<S::Ok, S::Error> {
        match value {
            Some(v) => v.serialize(s),
            None => "".serialize(s),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
        let raw = Option::<String>::deserialize(d)?;
        Ok(match raw.as_deref() {
            None | Some("" | "******") => None,
            Some(_) => raw,
        })
    }
}
use nautilus_core::{
    Params, UUID4,
    datetime::{NANOSECONDS_IN_MILLISECOND, nanos_to_millis as nanos_to_millis_u64},
    nanos::UnixNanos,
};
use nautilus_model::{
    data::{
        Bar, BarType, BookOrder, FundingRateUpdate, OrderBookDelta, OrderBookDeltas, TradeTick,
    },
    enums::{
        AccountType, AggressorSide, BarAggregation, BookAction, LiquiditySide, OptionKind,
        OrderSide, OrderStatus, OrderType, PositionSideSpecified, RecordFlag, TimeInForce,
        TriggerType,
    },
    events::account::state::AccountState,
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, PositionId, Symbol, TradeId, VenueOrderId,
    },
    instruments::{
        Instrument, any::InstrumentAny, crypto_future::CryptoFuture, crypto_option::CryptoOption,
        crypto_perpetual::CryptoPerpetual, currency_pair::CurrencyPair,
    },
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, MarginBalance, Money, Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::{
    common::{
        enums::{
            BybitContractType, BybitKlineInterval, BybitMarketUnit, BybitOptionType,
            BybitOrderSide, BybitOrderStatus, BybitOrderType, BybitPositionIdx, BybitPositionSide,
            BybitProductType, BybitStopOrderType, BybitTimeInForce, BybitTriggerDirection,
            BybitTriggerType,
        },
        symbol::BybitSymbol,
    },
    http::models::{
        BybitExecution, BybitFeeRate, BybitFunding, BybitInstrumentInverse, BybitInstrumentLinear,
        BybitInstrumentOption, BybitInstrumentSpot, BybitKline, BybitOrderbookResult,
        BybitPosition, BybitTrade, BybitWalletBalance,
    },
    websocket::parse::parse_millis_i64,
};

const BYBIT_HOUR_INTERVALS: &[u64] = &[1, 2, 4, 6, 12];

/// Extracts the raw symbol from a Bybit symbol by removing the product type suffix.
#[must_use]
pub fn extract_raw_symbol(symbol: &str) -> &str {
    symbol.rsplit_once('-').map_or(symbol, |(prefix, _)| prefix)
}

/// Extracts the base coin from a Bybit option symbol.
///
/// For example, `"BTC-27MAR26-70000-P"` returns `"BTC"`.
#[must_use]
pub fn extract_base_coin(symbol: &str) -> &str {
    symbol.split_once('-').map_or(symbol, |(base, _)| base)
}

/// Constructs a full Bybit symbol from a raw symbol and product type.
///
/// Returns a `Ustr` for efficient string interning and comparisons.
#[must_use]
pub fn make_bybit_symbol<S: AsRef<str>>(raw_symbol: S, product_type: BybitProductType) -> Ustr {
    let raw = raw_symbol.as_ref();
    Ustr::from(&format!("{raw}{}", product_type.suffix()))
}

/// Converts a Bybit kline interval string to a Nautilus bar aggregation and step.
///
/// Bybit interval strings: 1, 3, 5, 15, 30, 60, 120, 240, 360, 720 (minutes/hours), D, W, M
#[must_use]
pub fn bybit_interval_to_bar_spec(interval: &str) -> Option<(usize, BarAggregation)> {
    match interval {
        "1" => Some((1, BarAggregation::Minute)),
        "3" => Some((3, BarAggregation::Minute)),
        "5" => Some((5, BarAggregation::Minute)),
        "15" => Some((15, BarAggregation::Minute)),
        "30" => Some((30, BarAggregation::Minute)),
        "60" => Some((1, BarAggregation::Hour)),
        "120" => Some((2, BarAggregation::Hour)),
        "240" => Some((4, BarAggregation::Hour)),
        "360" => Some((6, BarAggregation::Hour)),
        "720" => Some((12, BarAggregation::Hour)),
        "D" => Some((1, BarAggregation::Day)),
        "W" => Some((1, BarAggregation::Week)),
        "M" => Some((1, BarAggregation::Month)),
        _ => None,
    }
}

/// Converts a Nautilus bar aggregation and step to a Bybit kline interval.
///
/// Bybit supported intervals: 1, 3, 5, 15, 30, 60, 120, 240, 360, 720 (minutes), D, W, M
///
/// # Errors
///
/// Returns an error if the aggregation type or step is not supported by Bybit.
pub fn bar_spec_to_bybit_interval(
    aggregation: BarAggregation,
    step: u64,
) -> anyhow::Result<BybitKlineInterval> {
    match aggregation {
        BarAggregation::Minute => match step {
            1 => Ok(BybitKlineInterval::Minute1),
            3 => Ok(BybitKlineInterval::Minute3),
            5 => Ok(BybitKlineInterval::Minute5),
            15 => Ok(BybitKlineInterval::Minute15),
            30 => Ok(BybitKlineInterval::Minute30),
            _ => anyhow::bail!(
                "Bybit only supports minute intervals 1, 3, 5, 15, 30 (use HOUR for >= 60)"
            ),
        },
        BarAggregation::Hour => match step {
            1 => Ok(BybitKlineInterval::Hour1),
            2 => Ok(BybitKlineInterval::Hour2),
            4 => Ok(BybitKlineInterval::Hour4),
            6 => Ok(BybitKlineInterval::Hour6),
            12 => Ok(BybitKlineInterval::Hour12),
            _ => anyhow::bail!(
                "Bybit only supports the following hour intervals: {BYBIT_HOUR_INTERVALS:?}"
            ),
        },
        BarAggregation::Day => {
            if step != 1 {
                anyhow::bail!("Bybit only supports 1 DAY interval bars");
            }
            Ok(BybitKlineInterval::Day1)
        }
        BarAggregation::Week => {
            if step != 1 {
                anyhow::bail!("Bybit only supports 1 WEEK interval bars");
            }
            Ok(BybitKlineInterval::Week1)
        }
        BarAggregation::Month => {
            if step != 1 {
                anyhow::bail!("Bybit only supports 1 MONTH interval bars");
            }
            Ok(BybitKlineInterval::Month1)
        }
        _ => {
            anyhow::bail!("Bybit does not support {aggregation:?} bars");
        }
    }
}

fn default_margin() -> Decimal {
    Decimal::new(1, 1)
}

/// Parses a spot instrument definition returned by Bybit into a Nautilus currency pair.
pub fn parse_spot_instrument(
    definition: &BybitInstrumentSpot,
    fee_rate: &BybitFeeRate,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let base_currency = get_currency(definition.base_coin.as_str());
    let quote_currency = get_currency(definition.quote_coin.as_str());

    let symbol = BybitSymbol::new(format!("{}-SPOT", definition.symbol))?;
    let instrument_id = symbol.to_instrument_id();
    let raw_symbol = Symbol::new(symbol.raw_symbol());

    let price_increment = parse_price(&definition.price_filter.tick_size, "priceFilter.tickSize")?;
    let size_increment = parse_quantity(
        &definition.lot_size_filter.base_precision,
        "lotSizeFilter.basePrecision",
    )?;
    let lot_size = Some(size_increment);
    let max_quantity = Some(parse_quantity(
        &definition.lot_size_filter.max_order_qty,
        "lotSizeFilter.maxOrderQty",
    )?);
    let min_quantity = Some(parse_quantity(
        &definition.lot_size_filter.min_order_qty,
        "lotSizeFilter.minOrderQty",
    )?);

    let maker_fee = parse_decimal(&fee_rate.maker_fee_rate, "makerFeeRate")?;
    let taker_fee = parse_decimal(&fee_rate.taker_fee_rate, "takerFeeRate")?;

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
        lot_size,
        max_quantity,
        min_quantity,
        None,
        None,
        None,
        None,
        Some(default_margin()),
        Some(default_margin()),
        Some(maker_fee),
        Some(taker_fee),
        None,
        ts_event,
        ts_init,
    );

    Ok(InstrumentAny::CurrencyPair(instrument))
}

/// Parses a linear contract definition (perpetual or dated future) into a Nautilus instrument.
pub fn parse_linear_instrument(
    definition: &BybitInstrumentLinear,
    fee_rate: &BybitFeeRate,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    // Validate required fields
    anyhow::ensure!(
        !definition.base_coin.is_empty(),
        "base_coin is empty for symbol '{}'",
        definition.symbol
    );
    anyhow::ensure!(
        !definition.quote_coin.is_empty(),
        "quote_coin is empty for symbol '{}'",
        definition.symbol
    );

    let base_currency = get_currency(definition.base_coin.as_str());
    let quote_currency = get_currency(definition.quote_coin.as_str());
    let settlement_currency = resolve_settlement_currency(
        definition.settle_coin.as_str(),
        base_currency,
        quote_currency,
    )?;

    let symbol = BybitSymbol::new(format!("{}-LINEAR", definition.symbol))?;
    let instrument_id = symbol.to_instrument_id();
    let raw_symbol = Symbol::new(symbol.raw_symbol());

    let price_increment = parse_price(&definition.price_filter.tick_size, "priceFilter.tickSize")?;
    let size_increment = parse_quantity(
        &definition.lot_size_filter.qty_step,
        "lotSizeFilter.qtyStep",
    )?;
    let lot_size = Some(size_increment);
    let max_quantity = Some(parse_quantity(
        &definition.lot_size_filter.max_order_qty,
        "lotSizeFilter.maxOrderQty",
    )?);
    let min_quantity = Some(parse_quantity(
        &definition.lot_size_filter.min_order_qty,
        "lotSizeFilter.minOrderQty",
    )?);
    let max_price = Some(parse_price(
        &definition.price_filter.max_price,
        "priceFilter.maxPrice",
    )?);
    let min_price = Some(parse_price(
        &definition.price_filter.min_price,
        "priceFilter.minPrice",
    )?);

    let maker_fee = parse_decimal(&fee_rate.maker_fee_rate, "makerFeeRate")?;
    let taker_fee = parse_decimal(&fee_rate.taker_fee_rate, "takerFeeRate")?;

    match definition.contract_type {
        BybitContractType::LinearPerpetual => {
            let instrument = CryptoPerpetual::new(
                instrument_id,
                raw_symbol,
                base_currency,
                quote_currency,
                settlement_currency,
                false,
                price_increment.precision,
                size_increment.precision,
                price_increment,
                size_increment,
                None,
                lot_size,
                max_quantity,
                min_quantity,
                None,
                None,
                max_price,
                min_price,
                Some(default_margin()),
                Some(default_margin()),
                Some(maker_fee),
                Some(taker_fee),
                None,
                ts_event,
                ts_init,
            );
            Ok(InstrumentAny::CryptoPerpetual(instrument))
        }
        BybitContractType::LinearFutures => {
            let activation_ns = parse_millis_timestamp(&definition.launch_time, "launchTime")?;
            let expiration_ns = parse_millis_timestamp(&definition.delivery_time, "deliveryTime")?;
            let instrument = CryptoFuture::new(
                instrument_id,
                raw_symbol,
                base_currency,
                quote_currency,
                settlement_currency,
                false,
                activation_ns,
                expiration_ns,
                price_increment.precision,
                size_increment.precision,
                price_increment,
                size_increment,
                None,
                lot_size,
                max_quantity,
                min_quantity,
                None,
                None,
                max_price,
                min_price,
                Some(default_margin()),
                Some(default_margin()),
                Some(maker_fee),
                Some(taker_fee),
                None,
                ts_event,
                ts_init,
            );
            Ok(InstrumentAny::CryptoFuture(instrument))
        }
        other => Err(anyhow::anyhow!(
            "unsupported linear contract variant: {other:?}"
        )),
    }
}

/// Parses an inverse contract definition into a Nautilus instrument.
pub fn parse_inverse_instrument(
    definition: &BybitInstrumentInverse,
    fee_rate: &BybitFeeRate,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    // Validate required fields
    anyhow::ensure!(
        !definition.base_coin.is_empty(),
        "base_coin is empty for symbol '{}'",
        definition.symbol
    );
    anyhow::ensure!(
        !definition.quote_coin.is_empty(),
        "quote_coin is empty for symbol '{}'",
        definition.symbol
    );

    let base_currency = get_currency(definition.base_coin.as_str());
    let quote_currency = get_currency(definition.quote_coin.as_str());
    let settlement_currency = resolve_settlement_currency(
        definition.settle_coin.as_str(),
        base_currency,
        quote_currency,
    )?;

    let symbol = BybitSymbol::new(format!("{}-INVERSE", definition.symbol))?;
    let instrument_id = symbol.to_instrument_id();
    let raw_symbol = Symbol::new(symbol.raw_symbol());

    let price_increment = parse_price(&definition.price_filter.tick_size, "priceFilter.tickSize")?;
    let size_increment = parse_quantity(
        &definition.lot_size_filter.qty_step,
        "lotSizeFilter.qtyStep",
    )?;
    let lot_size = Some(size_increment);
    let max_quantity = Some(parse_quantity(
        &definition.lot_size_filter.max_order_qty,
        "lotSizeFilter.maxOrderQty",
    )?);
    let min_quantity = Some(parse_quantity(
        &definition.lot_size_filter.min_order_qty,
        "lotSizeFilter.minOrderQty",
    )?);
    let max_price = Some(parse_price(
        &definition.price_filter.max_price,
        "priceFilter.maxPrice",
    )?);
    let min_price = Some(parse_price(
        &definition.price_filter.min_price,
        "priceFilter.minPrice",
    )?);

    let maker_fee = parse_decimal(&fee_rate.maker_fee_rate, "makerFeeRate")?;
    let taker_fee = parse_decimal(&fee_rate.taker_fee_rate, "takerFeeRate")?;

    match definition.contract_type {
        BybitContractType::InversePerpetual => {
            let instrument = CryptoPerpetual::new(
                instrument_id,
                raw_symbol,
                base_currency,
                quote_currency,
                settlement_currency,
                true,
                price_increment.precision,
                size_increment.precision,
                price_increment,
                size_increment,
                None,
                lot_size,
                max_quantity,
                min_quantity,
                None,
                None,
                max_price,
                min_price,
                Some(default_margin()),
                Some(default_margin()),
                Some(maker_fee),
                Some(taker_fee),
                None,
                ts_event,
                ts_init,
            );
            Ok(InstrumentAny::CryptoPerpetual(instrument))
        }
        BybitContractType::InverseFutures => {
            let activation_ns = parse_millis_timestamp(&definition.launch_time, "launchTime")?;
            let expiration_ns = parse_millis_timestamp(&definition.delivery_time, "deliveryTime")?;
            let instrument = CryptoFuture::new(
                instrument_id,
                raw_symbol,
                base_currency,
                quote_currency,
                settlement_currency,
                true,
                activation_ns,
                expiration_ns,
                price_increment.precision,
                size_increment.precision,
                price_increment,
                size_increment,
                None,
                lot_size,
                max_quantity,
                min_quantity,
                None,
                None,
                max_price,
                min_price,
                Some(default_margin()),
                Some(default_margin()),
                Some(maker_fee),
                Some(taker_fee),
                None,
                ts_event,
                ts_init,
            );
            Ok(InstrumentAny::CryptoFuture(instrument))
        }
        other => Err(anyhow::anyhow!(
            "unsupported inverse contract variant: {other:?}"
        )),
    }
}

/// Parses a Bybit option contract definition into a Nautilus [`CryptoOption`].
pub fn parse_option_instrument(
    definition: &BybitInstrumentOption,
    fee_rate: Option<&BybitFeeRate>,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let symbol = BybitSymbol::new(format!("{}-OPTION", definition.symbol))?;
    let instrument_id = symbol.to_instrument_id();
    let raw_symbol = Symbol::new(symbol.raw_symbol());
    let underlying = get_currency(definition.base_coin.as_str());
    let quote_currency = get_currency(definition.quote_coin.as_str());
    let settlement_currency = get_currency(definition.settle_coin.as_str());
    // Bybit Options are linear contracts — they are margined and settled in stablecoins
    let is_inverse = false;

    let price_increment = parse_price(&definition.price_filter.tick_size, "priceFilter.tickSize")?;
    let max_price = Some(parse_price(
        &definition.price_filter.max_price,
        "priceFilter.maxPrice",
    )?);
    let min_price = Some(parse_price(
        &definition.price_filter.min_price,
        "priceFilter.minPrice",
    )?);
    let lot_size = parse_quantity(
        &definition.lot_size_filter.qty_step,
        "lotSizeFilter.qtyStep",
    )?;
    let max_quantity = Some(parse_quantity(
        &definition.lot_size_filter.max_order_qty,
        "lotSizeFilter.maxOrderQty",
    )?);
    let min_quantity = Some(parse_quantity(
        &definition.lot_size_filter.min_order_qty,
        "lotSizeFilter.minOrderQty",
    )?);

    let option_kind = match definition.options_type {
        BybitOptionType::Call => OptionKind::Call,
        BybitOptionType::Put => OptionKind::Put,
    };

    let strike_price = extract_strike_from_symbol(&definition.symbol)?;
    let activation_ns = parse_millis_timestamp(&definition.launch_time, "launchTime")?;
    let expiration_ns = parse_millis_timestamp(&definition.delivery_time, "deliveryTime")?;

    let (maker_fee, taker_fee) = match fee_rate {
        Some(fee) => (
            Some(
                fee.maker_fee_rate
                    .parse::<Decimal>()
                    .unwrap_or(Decimal::ZERO),
            ),
            Some(
                fee.taker_fee_rate
                    .parse::<Decimal>()
                    .unwrap_or(Decimal::ZERO),
            ),
        ),
        None => (Some(Decimal::ZERO), Some(Decimal::ZERO)),
    };

    let instrument = CryptoOption::new(
        instrument_id,
        raw_symbol,
        underlying,
        quote_currency,
        settlement_currency,
        is_inverse,
        option_kind,
        strike_price,
        activation_ns,
        expiration_ns,
        price_increment.precision,
        lot_size.precision,
        price_increment,
        lot_size,                    // Lot size represents size increment.
        Some(Quantity::from(1_u32)), // multiplier
        Some(lot_size),
        max_quantity,
        min_quantity,
        None,
        None,
        max_price,
        min_price,
        None, // margin_init
        None, // margin_maint
        maker_fee,
        taker_fee,
        None,
        ts_event,
        ts_init,
    );

    Ok(InstrumentAny::CryptoOption(instrument))
}

/// Parses a REST trade payload into a [`TradeTick`].
pub fn parse_trade_tick(
    trade: &BybitTrade,
    instrument: &InstrumentAny,
    ts_init: Option<UnixNanos>,
) -> anyhow::Result<TradeTick> {
    let price =
        parse_price_with_precision(&trade.price, instrument.price_precision(), "trade.price")?;
    let size =
        parse_quantity_with_precision(&trade.size, instrument.size_precision(), "trade.size")?;
    let aggressor: AggressorSide = trade.side.into();
    let trade_id = TradeId::new_checked(trade.exec_id.as_str())
        .context("invalid exec_id in Bybit trade payload")?;
    let ts_event = parse_millis_timestamp(&trade.time, "trade.time")?;
    let ts_init = ts_init.unwrap_or(ts_event);

    TradeTick::new_checked(
        instrument.id(),
        price,
        size,
        aggressor,
        trade_id,
        ts_event,
        ts_init,
    )
    .context("failed to construct TradeTick from Bybit trade payload")
}

/// Parses a REST funding payload into a [`FundingRateUpdate`].
pub fn parse_funding_rate(
    funding: &BybitFunding,
    instrument: &InstrumentAny,
    interval_millis: Option<i64>,
) -> anyhow::Result<FundingRateUpdate> {
    let rate = parse_decimal(&funding.funding_rate, "funding.rate")?;
    let ts_event = parse_millis_timestamp(&funding.funding_rate_timestamp, "funding.timestamp")?;
    let interval = interval_millis
        .map(|ms| u16::try_from(ms / 60_000).context("interval milliseconds out of bounds"))
        .transpose()?;

    Ok(FundingRateUpdate::new(
        instrument.id(),
        rate,
        interval,
        None, // next_funding_ns not provided with historical funding rates
        ts_event,
        ts_event,
    ))
}

/// Parses an order book response into [`OrderBookDeltas`].
pub fn parse_orderbook(
    result: &BybitOrderbookResult,
    instrument: &InstrumentAny,
    ts_init: Option<UnixNanos>,
) -> anyhow::Result<OrderBookDeltas> {
    let ts_event = parse_millis_i64(result.ts, "orderbook.timestamp")?;
    let ts_init = ts_init.unwrap_or(ts_event);

    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();
    let update_id = u64::try_from(result.u)
        .context("received negative update id in Bybit order book message")?;
    let sequence = u64::try_from(result.seq)
        .context("received negative sequence in Bybit order book message")?;

    let total_levels = result.b.len() + result.a.len();
    let mut deltas = Vec::with_capacity(total_levels + 1);

    let mut clear = OrderBookDelta::clear(instrument_id, sequence, ts_event, ts_init);

    if total_levels == 0 {
        clear.flags |= RecordFlag::F_LAST as u8;
    }
    deltas.push(clear);

    let mut processed = 0_usize;

    let mut push_level = |values: &[String], side: OrderSide| -> anyhow::Result<()> {
        let (price, size) = parse_book_level(values, price_precision, size_precision, "orderbook")?;

        processed += 1;
        let mut flags = RecordFlag::F_MBP as u8;

        if processed == total_levels {
            flags |= RecordFlag::F_LAST as u8;
        }

        let order = BookOrder::new(side, price, size, update_id);
        let delta = OrderBookDelta::new_checked(
            instrument_id,
            BookAction::Add,
            order,
            flags,
            sequence,
            ts_event,
            ts_init,
        )
        .context("failed to construct OrderBookDelta from Bybit book level")?;
        deltas.push(delta);
        Ok(())
    };

    for level in &result.b {
        push_level(level, OrderSide::Buy)?;
    }

    for level in &result.a {
        push_level(level, OrderSide::Sell)?;
    }

    OrderBookDeltas::new_checked(instrument_id, deltas)
        .context("failed to assemble OrderBookDeltas from Bybit message")
}

pub fn parse_book_level(
    level: &[String],
    price_precision: u8,
    size_precision: u8,
    label: &str,
) -> anyhow::Result<(Price, Quantity)> {
    let price_str = level
        .first()
        .ok_or_else(|| anyhow::anyhow!("missing price component in {label} level"))?;
    let size_str = level
        .get(1)
        .ok_or_else(|| anyhow::anyhow!("missing size component in {label} level"))?;
    let price = parse_price_with_precision(price_str, price_precision, label)?;
    let size = parse_quantity_with_precision(size_str, size_precision, label)?;
    Ok((price, size))
}

/// Parses a kline entry into a [`Bar`].
pub fn parse_kline_bar(
    kline: &BybitKline,
    instrument: &InstrumentAny,
    bar_type: BarType,
    timestamp_on_close: bool,
    ts_init: Option<UnixNanos>,
) -> anyhow::Result<Bar> {
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let open = parse_price_with_precision(&kline.open, price_precision, "kline.open")?;
    let high = parse_price_with_precision(&kline.high, price_precision, "kline.high")?;
    let low = parse_price_with_precision(&kline.low, price_precision, "kline.low")?;
    let close = parse_price_with_precision(&kline.close, price_precision, "kline.close")?;
    let volume = parse_quantity_with_precision(&kline.volume, size_precision, "kline.volume")?;

    let mut ts_event = parse_millis_timestamp(&kline.start, "kline.start")?;

    if timestamp_on_close {
        let interval_ns = bar_type
            .spec()
            .timedelta()
            .num_nanoseconds()
            .context("bar specification produced non-integer interval")?;
        let interval_ns = u64::try_from(interval_ns)
            .context("bar interval overflowed the u64 range for nanoseconds")?;
        let updated = ts_event
            .as_u64()
            .checked_add(interval_ns)
            .context("bar timestamp overflowed when adjusting to close time")?;
        ts_event = UnixNanos::from(updated);
    }
    let ts_init = ts_init.unwrap_or(ts_event);

    Bar::new_checked(bar_type, open, high, low, close, volume, ts_event, ts_init)
        .context("failed to construct Bar from Bybit kline entry")
}

/// Constructs a venue position ID from an instrument and Bybit position index.
///
/// Position index values: 0 = one-way mode, 1 = buy-side hedge, 2 = sell-side hedge.
///
/// Not currently wired into reports because Bybit defaults to netting mode where
/// non-None `venue_position_id` overrides the computed netting position ID.
/// Ready to activate when hedge-mode support is added.
#[must_use]
pub fn make_venue_position_id(instrument_id: InstrumentId, position_idx: i32) -> PositionId {
    let side = match position_idx {
        0 => "ONEWAY",
        1 => "LONG",
        2 => "SHORT",
        _ => "UNKNOWN",
    };
    PositionId::new(format!("{instrument_id}-{side}"))
}

/// Parses a Bybit execution into a Nautilus FillReport.
///
/// # Errors
///
/// This function returns an error if:
/// - Required price or quantity fields cannot be parsed.
/// - The execution timestamp cannot be parsed.
/// - Numeric conversions fail.
pub fn parse_fill_report(
    execution: &BybitExecution,
    account_id: AccountId,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<FillReport> {
    let instrument_id = instrument.id();
    let venue_order_id = VenueOrderId::new(execution.order_id.as_str());
    let trade_id = TradeId::new_checked(execution.exec_id.as_str())
        .context("invalid execId in Bybit execution payload")?;

    let order_side: OrderSide = execution.side.into();

    let last_px = parse_price_with_precision(
        &execution.exec_price,
        instrument.price_precision(),
        "execution.execPrice",
    )?;

    let last_qty = parse_quantity_with_precision(
        &execution.exec_qty,
        instrument.size_precision(),
        "execution.execQty",
    )?;

    let fee_decimal: Decimal = execution
        .exec_fee
        .parse()
        .with_context(|| format!("Failed to parse execFee='{}'", execution.exec_fee))?;
    let currency = get_currency(&execution.fee_currency);
    let commission = Money::from_decimal(fee_decimal, currency).with_context(|| {
        format!(
            "Failed to create commission from execFee='{}'",
            execution.exec_fee
        )
    })?;

    // Determine liquidity side from is_maker flag
    let liquidity_side = if execution.is_maker {
        LiquiditySide::Maker
    } else {
        LiquiditySide::Taker
    };

    let ts_event = parse_millis_timestamp(&execution.exec_time, "execution.execTime")?;

    // Parse client_order_id if present
    let client_order_id = if execution.order_link_id.is_empty() {
        None
    } else {
        Some(ClientOrderId::new(execution.order_link_id.as_str()))
    };

    Ok(FillReport::new(
        account_id,
        instrument_id,
        venue_order_id,
        trade_id,
        order_side,
        last_qty,
        last_px,
        commission,
        liquidity_side,
        client_order_id,
        None, // venue_position_id: execution data lacks position_idx
        ts_event,
        ts_init,
        None, // Will generate a new UUID4
    ))
}

/// Parses a Bybit position into a Nautilus PositionStatusReport.
///
/// # Errors
///
/// This function returns an error if:
/// - Position quantity or price fields cannot be parsed.
/// - The position timestamp cannot be parsed.
/// - Numeric conversions fail.
pub fn parse_position_status_report(
    position: &BybitPosition,
    account_id: AccountId,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<PositionStatusReport> {
    let instrument_id = instrument.id();

    // Parse position size
    let size_f64 = position
        .size
        .parse::<f64>()
        .with_context(|| format!("Failed to parse position size '{}'", position.size))?;

    // Determine position side and quantity
    let (position_side, quantity) = match position.side {
        BybitPositionSide::Buy => {
            let qty = Quantity::new(size_f64, instrument.size_precision());
            (PositionSideSpecified::Long, qty)
        }
        BybitPositionSide::Sell => {
            let qty = Quantity::new(size_f64, instrument.size_precision());
            (PositionSideSpecified::Short, qty)
        }
        BybitPositionSide::Flat => {
            let qty = Quantity::new(0.0, instrument.size_precision());
            (PositionSideSpecified::Flat, qty)
        }
    };

    // Parse average entry price
    let avg_px_open = if position.avg_price.is_empty() || position.avg_price == "0" {
        None
    } else {
        Some(Decimal::from_str(&position.avg_price)?)
    };

    // Use ts_init if updatedTime is empty (initial/flat positions)
    let ts_last = if position.updated_time.is_empty() {
        ts_init
    } else {
        parse_millis_timestamp(&position.updated_time, "position.updatedTime")?
    };

    // Bybit ranks open positions 1-5 by ADL priority (5 = next to be deleveraged);
    // 0 means the account has no open position or is flat.
    if position.adl_rank_indicator >= 4 {
        log::warn!(
            "Elevated ADL risk: {} position size={} adl_rank={}",
            instrument_id,
            position.size,
            position.adl_rank_indicator,
        );
    }

    Ok(PositionStatusReport::new(
        account_id,
        instrument_id,
        position_side,
        quantity,
        ts_last,
        ts_init,
        None, // Will generate a new UUID4
        None, // venue_position_id omitted: non-None triggers hedge-mode reconciliation
        avg_px_open,
    ))
}

/// Parses a Bybit wallet balance into a Nautilus account state.
///
/// # Errors
///
/// Returns an error if:
/// - Balance data cannot be parsed.
/// - Currency is invalid.
pub fn parse_account_state(
    wallet_balance: &BybitWalletBalance,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<AccountState> {
    let mut balances = Vec::new();

    for coin in &wallet_balance.coin {
        let total_dec = coin.wallet_balance - coin.spot_borrow;
        let locked_dec = coin.locked;

        let currency = get_currency(&coin.coin);
        balances.push(AccountBalance::from_total_and_locked(
            total_dec, locked_dec, currency,
        )?);
    }

    let mut margins = Vec::new();

    for coin in &wallet_balance.coin {
        // Position IM is reserved against open positions; order IM is reserved against
        // pending orders. Sum both so an account that only has open orders still
        // reports a non-zero initial margin.
        let position_im_f64 = match &coin.total_position_im {
            Some(im) if !im.is_empty() => im.parse::<f64>()?,
            _ => 0.0,
        };
        let order_im_f64 = match &coin.total_order_im {
            Some(im) if !im.is_empty() => im.parse::<f64>()?,
            _ => 0.0,
        };
        let initial_margin_f64 = position_im_f64 + order_im_f64;

        let maintenance_margin_f64 = match &coin.total_position_mm {
            Some(mm) if !mm.is_empty() => mm.parse::<f64>()?,
            _ => 0.0,
        };

        if initial_margin_f64 == 0.0 && maintenance_margin_f64 == 0.0 {
            continue;
        }

        let currency = get_currency(&coin.coin);
        let initial_margin = Money::new(initial_margin_f64, currency);
        let maintenance_margin = Money::new(maintenance_margin_f64, currency);

        margins.push(MarginBalance::new(initial_margin, maintenance_margin, None));
    }

    let account_type = AccountType::Margin;
    let is_reported = true;
    let event_id = UUID4::new();

    // Use current time as ts_event since Bybit doesn't provide this in wallet balance
    let ts_event = ts_init;

    Ok(AccountState::new(
        account_id,
        account_type,
        balances,
        margins,
        is_reported,
        event_id,
        ts_event,
        ts_init,
        None,
    ))
}

pub(crate) fn parse_price_with_precision(
    value: &str,
    precision: u8,
    field: &str,
) -> anyhow::Result<Price> {
    let parsed = value
        .parse::<f64>()
        .with_context(|| format!("Failed to parse {field}='{value}' as f64"))?;
    Price::new_checked(parsed, precision).with_context(|| {
        format!("Failed to construct Price for {field} with precision {precision}")
    })
}

pub(crate) fn parse_quantity_with_precision(
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

pub(crate) fn parse_price(value: &str, field: &str) -> anyhow::Result<Price> {
    Price::from_str(value).map_err(|e| anyhow::anyhow!("Failed to parse {field}='{value}': {e}"))
}

pub(crate) fn parse_quantity(value: &str, field: &str) -> anyhow::Result<Quantity> {
    Quantity::from_str(value).map_err(|e| anyhow::anyhow!("Failed to parse {field}='{value}': {e}"))
}

pub(crate) fn parse_decimal(value: &str, field: &str) -> anyhow::Result<Decimal> {
    Decimal::from_str(value)
        .map_err(|e| anyhow::anyhow!("Failed to parse {field}='{value}' as Decimal: {e}"))
}

pub(crate) fn parse_millis_timestamp(value: &str, field: &str) -> anyhow::Result<UnixNanos> {
    let millis: u64 = value
        .parse()
        .with_context(|| format!("Failed to parse {field}='{value}' as u64 millis"))?;
    let nanos = millis
        .checked_mul(NANOSECONDS_IN_MILLISECOND)
        .context("millisecond timestamp overflowed when converting to nanoseconds")?;
    Ok(UnixNanos::from(nanos))
}

fn resolve_settlement_currency(
    settle_coin: &str,
    base_currency: Currency,
    quote_currency: Currency,
) -> anyhow::Result<Currency> {
    if settle_coin.eq_ignore_ascii_case(base_currency.code.as_str()) {
        Ok(base_currency)
    } else if settle_coin.eq_ignore_ascii_case(quote_currency.code.as_str()) {
        Ok(quote_currency)
    } else {
        Err(anyhow::anyhow!(
            "unrecognised settlement currency '{settle_coin}'"
        ))
    }
}

/// Returns a currency from the internal map or creates a new crypto currency.
///
/// Uses [`Currency::get_or_create_crypto`] to handle unknown currency codes,
/// which automatically registers newly listed Bybit assets.
pub fn get_currency(code: &str) -> Currency {
    Currency::get_or_create_crypto(code)
}

fn extract_strike_from_symbol(symbol: &str) -> anyhow::Result<Price> {
    let parts: Vec<&str> = symbol.split('-').collect();
    let strike = parts
        .get(2)
        .ok_or_else(|| anyhow::anyhow!("invalid option symbol '{symbol}'"))?;
    parse_price(strike, "option strike")
}

/// Resolves a Nautilus [`OrderType`] from Bybit order classification fields.
///
/// Bybit represents conditional orders using a combination of `orderType` (Market/Limit),
/// `stopOrderType` (Stop, TakeProfit, StopLoss, etc.), `triggerDirection` (RisesTo/FallsTo),
/// and `side` (Buy/Sell). This function maps all combinations to the appropriate Nautilus
/// conditional order types.
///
/// When `triggerDirection` is `None`, the stop order type is informational only (a parent
/// order with TP/SL metadata attached), so the order is classified as plain Market/Limit.
#[must_use]
pub fn parse_bybit_order_type(
    order_type: BybitOrderType,
    stop_order_type: BybitStopOrderType,
    trigger_direction: BybitTriggerDirection,
    side: BybitOrderSide,
) -> OrderType {
    if matches!(
        stop_order_type,
        BybitStopOrderType::None | BybitStopOrderType::Unknown
    ) {
        return match order_type {
            BybitOrderType::Market => OrderType::Market,
            BybitOrderType::Limit | BybitOrderType::Unknown => OrderType::Limit,
        };
    }

    // No trigger direction means TP/SL metadata on a parent order,
    // not a standalone conditional
    if trigger_direction == BybitTriggerDirection::None {
        return match order_type {
            BybitOrderType::Market => OrderType::Market,
            BybitOrderType::Limit | BybitOrderType::Unknown => OrderType::Limit,
        };
    }

    // TrailingStop maps to StopMarket/StopLimit because Bybit does not
    // provide the trailing offset fields needed for the dedicated types.
    match (order_type, trigger_direction, side) {
        (BybitOrderType::Market, BybitTriggerDirection::RisesTo, BybitOrderSide::Buy) => {
            OrderType::StopMarket
        }
        (BybitOrderType::Market, BybitTriggerDirection::FallsTo, BybitOrderSide::Buy) => {
            OrderType::MarketIfTouched
        }
        (BybitOrderType::Market, BybitTriggerDirection::FallsTo, BybitOrderSide::Sell) => {
            OrderType::StopMarket
        }
        (BybitOrderType::Market, BybitTriggerDirection::RisesTo, BybitOrderSide::Sell) => {
            OrderType::MarketIfTouched
        }
        (BybitOrderType::Limit, BybitTriggerDirection::RisesTo, BybitOrderSide::Buy) => {
            OrderType::StopLimit
        }
        (BybitOrderType::Limit, BybitTriggerDirection::FallsTo, BybitOrderSide::Buy) => {
            OrderType::LimitIfTouched
        }
        (BybitOrderType::Limit, BybitTriggerDirection::FallsTo, BybitOrderSide::Sell) => {
            OrderType::StopLimit
        }
        (BybitOrderType::Limit, BybitTriggerDirection::RisesTo, BybitOrderSide::Sell) => {
            OrderType::LimitIfTouched
        }
        _ => match order_type {
            BybitOrderType::Market => OrderType::Market,
            BybitOrderType::Limit | BybitOrderType::Unknown => OrderType::Limit,
        },
    }
}

/// Parses a Bybit order into a Nautilus OrderStatusReport.
pub fn parse_order_status_report(
    order: &crate::http::models::BybitOrder,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    let instrument_id = instrument.id();
    let venue_order_id = VenueOrderId::new(order.order_id);

    let order_side: OrderSide = order.side.into();

    let order_type = parse_bybit_order_type(
        order.order_type,
        order.stop_order_type,
        order.trigger_direction,
        order.side,
    );

    let time_in_force: TimeInForce = match order.time_in_force {
        BybitTimeInForce::Gtc => TimeInForce::Gtc,
        BybitTimeInForce::Ioc => TimeInForce::Ioc,
        BybitTimeInForce::Fok => TimeInForce::Fok,
        BybitTimeInForce::PostOnly => TimeInForce::Gtc,
    };

    let quantity =
        parse_quantity_with_precision(&order.qty, instrument.size_precision(), "order.qty")?;

    let filled_qty = parse_quantity_with_precision(
        &order.cum_exec_qty,
        instrument.size_precision(),
        "order.cumExecQty",
    )?;

    // Map Bybit order status to Nautilus order status
    // Special case: if Bybit reports "Rejected" but the order has fills, treat it as Canceled.
    // This handles the case where the exchange partially fills an order then rejects the
    // remaining quantity (e.g., due to margin, risk limits, or liquidity constraints).
    // The state machine does not allow PARTIALLY_FILLED -> REJECTED transitions.
    let order_status: OrderStatus = match order.order_status {
        BybitOrderStatus::Created | BybitOrderStatus::New | BybitOrderStatus::Untriggered => {
            OrderStatus::Accepted
        }
        BybitOrderStatus::Rejected => {
            if filled_qty.is_positive() {
                OrderStatus::Canceled
            } else {
                OrderStatus::Rejected
            }
        }
        BybitOrderStatus::PartiallyFilled => OrderStatus::PartiallyFilled,
        BybitOrderStatus::Filled => OrderStatus::Filled,
        BybitOrderStatus::Canceled | BybitOrderStatus::PartiallyFilledCanceled => {
            OrderStatus::Canceled
        }
        BybitOrderStatus::Triggered => OrderStatus::Triggered,
        BybitOrderStatus::Deactivated => OrderStatus::Canceled,
    };

    let ts_accepted = parse_millis_timestamp(&order.created_time, "order.createdTime")?;
    let ts_last = parse_millis_timestamp(&order.updated_time, "order.updatedTime")?;

    let mut report = OrderStatusReport::new(
        account_id,
        instrument_id,
        None,
        venue_order_id,
        order_side,
        order_type,
        time_in_force,
        order_status,
        quantity,
        filled_qty,
        ts_accepted,
        ts_last,
        ts_init,
        Some(UUID4::new()),
    );

    if !order.order_link_id.is_empty() {
        report = report.with_client_order_id(ClientOrderId::new(order.order_link_id.as_str()));
    }

    if !order.price.is_empty() && order.price != "0" {
        let price =
            parse_price_with_precision(&order.price, instrument.price_precision(), "order.price")?;
        report = report.with_price(price);
    }

    if let Some(avg_price) = &order.avg_price
        && !avg_price.is_empty()
        && avg_price != "0"
    {
        let avg_px = avg_price
            .parse::<f64>()
            .with_context(|| format!("Failed to parse avg_price='{avg_price}' as f64"))?;
        report = report.with_avg_px(avg_px)?;
    }

    if !order.trigger_price.is_empty() && order.trigger_price != "0" {
        let trigger_price = parse_price_with_precision(
            &order.trigger_price,
            instrument.price_precision(),
            "order.triggerPrice",
        )?;
        report = report.with_trigger_price(trigger_price);

        // Set trigger_type for conditional orders
        let trigger_type: TriggerType = order.trigger_by.into();
        report = report.with_trigger_type(trigger_type);
    }

    // venue_position_id omitted: in netting mode, non-None values override the
    // computed netting position ID and break position tracking.

    if order.reduce_only {
        report = report.with_reduce_only(true);
    }

    if order.time_in_force == BybitTimeInForce::PostOnly {
        report = report.with_post_only(true);
    }

    Ok(report)
}

/// Returns the `marketUnit` parameter for spot market orders.
#[must_use]
pub fn spot_market_unit(
    product_type: BybitProductType,
    order_type: BybitOrderType,
    is_quote_quantity: bool,
) -> Option<BybitMarketUnit> {
    if product_type == BybitProductType::Spot && order_type == BybitOrderType::Market {
        if is_quote_quantity {
            Some(BybitMarketUnit::QuoteCoin)
        } else {
            Some(BybitMarketUnit::BaseCoin)
        }
    } else {
        None
    }
}

/// Returns the `isLeverage` parameter (spot-only).
#[must_use]
pub fn spot_leverage(product_type: BybitProductType, is_leverage: bool) -> Option<i32> {
    if product_type == BybitProductType::Spot {
        Some(i32::from(is_leverage))
    } else {
        None
    }
}

/// Returns the trigger direction for stop and MIT orders.
#[must_use]
pub fn trigger_direction(
    order_type: OrderType,
    order_side: OrderSide,
    is_stop_order: bool,
) -> Option<BybitTriggerDirection> {
    if !is_stop_order {
        return None;
    }

    match (order_type, order_side) {
        (OrderType::StopMarket | OrderType::StopLimit, OrderSide::Buy) => {
            Some(BybitTriggerDirection::RisesTo)
        }
        (OrderType::StopMarket | OrderType::StopLimit, OrderSide::Sell) => {
            Some(BybitTriggerDirection::FallsTo)
        }
        (OrderType::MarketIfTouched | OrderType::LimitIfTouched, OrderSide::Buy) => {
            Some(BybitTriggerDirection::FallsTo)
        }
        (OrderType::MarketIfTouched | OrderType::LimitIfTouched, OrderSide::Sell) => {
            Some(BybitTriggerDirection::RisesTo)
        }
        _ => None,
    }
}

/// Maps Nautilus time-in-force to Bybit's TIF.
///
/// Returns `Err(tif)` with the unsupported value for caller-specific error wrapping.
pub fn map_time_in_force(
    order_type: BybitOrderType,
    time_in_force: Option<TimeInForce>,
    post_only: Option<bool>,
) -> Result<Option<BybitTimeInForce>, TimeInForce> {
    if order_type == BybitOrderType::Market {
        return Ok(None);
    }

    if post_only == Some(true) {
        return Ok(Some(BybitTimeInForce::PostOnly));
    }

    match time_in_force {
        Some(TimeInForce::Gtc) => Ok(Some(BybitTimeInForce::Gtc)),
        Some(TimeInForce::Ioc) => Ok(Some(BybitTimeInForce::Ioc)),
        Some(TimeInForce::Fok) => Ok(Some(BybitTimeInForce::Fok)),
        Some(tif) => Err(tif),
        None => Ok(None),
    }
}

/// Converts an optional `UnixNanos` timestamp to optional milliseconds.
pub fn nanos_to_millis(value: Option<UnixNanos>) -> Option<i64> {
    value.map(|nanos| nanos_to_millis_u64(nanos.as_u64()) as i64)
}

/// Parsed and validated Bybit TP/SL parameters from a `SubmitOrder.params` map.
#[derive(Debug, Default)]
pub struct BybitTpSlParams {
    pub take_profit: Option<Price>,
    pub stop_loss: Option<Price>,
    pub tp_trigger_by: Option<BybitTriggerType>,
    pub sl_trigger_by: Option<BybitTriggerType>,
    pub tp_order_type: Option<BybitOrderType>,
    pub sl_order_type: Option<BybitOrderType>,
    pub tp_limit_price: Option<String>,
    pub sl_limit_price: Option<String>,
    pub tp_trigger_price: Option<String>,
    pub sl_trigger_price: Option<String>,
    pub close_on_trigger: Option<bool>,
    pub is_leverage: bool,
    pub order_iv: Option<String>,
    pub mmp: Option<bool>,
    pub position_idx: Option<BybitPositionIdx>,
}

impl BybitTpSlParams {
    pub fn has_tp_sl(&self) -> bool {
        self.take_profit.is_some() || self.stop_loss.is_some()
    }
}

/// Extracts a string value from params, accepting both string and numeric JSON values.
pub fn get_price_str(params: &Params, key: &str) -> Option<String> {
    let value = params.get(key)?;
    if let Some(s) = value.as_str() {
        Some(s.to_string())
    } else if let Some(n) = value.as_f64() {
        Some(n.to_string())
    } else if let Some(n) = value.as_i64() {
        Some(n.to_string())
    } else {
        value.as_u64().map(|n| n.to_string())
    }
}

/// Parses Bybit TP/SL parameters from an optional params map.
pub fn parse_bybit_tp_sl_params(params: Option<&Params>) -> anyhow::Result<BybitTpSlParams> {
    let Some(params) = params else {
        return Ok(BybitTpSlParams::default());
    };

    let mut result = BybitTpSlParams {
        is_leverage: params.get_bool("is_leverage").unwrap_or(false),
        ..Default::default()
    };

    if let Some(s) = get_price_str(params, "take_profit") {
        let p =
            Price::from_str(&s).map_err(|e| anyhow::anyhow!("invalid 'take_profit' price: {e}"))?;

        if p.as_f64() < 0.0 {
            anyhow::bail!("invalid 'take_profit' price: '{s}', expected a non-negative value");
        }
        result.take_profit = Some(p);
    }

    if let Some(s) = get_price_str(params, "stop_loss") {
        let p =
            Price::from_str(&s).map_err(|e| anyhow::anyhow!("invalid 'stop_loss' price: {e}"))?;

        if p.as_f64() < 0.0 {
            anyhow::bail!("invalid 'stop_loss' price: '{s}', expected a non-negative value");
        }
        result.stop_loss = Some(p);
    }

    for (key, setter) in [
        (
            "tp_limit_price",
            &mut result.tp_limit_price as &mut Option<String>,
        ),
        ("sl_limit_price", &mut result.sl_limit_price),
        ("tp_trigger_price", &mut result.tp_trigger_price),
        ("sl_trigger_price", &mut result.sl_trigger_price),
    ] {
        if let Some(s) = get_price_str(params, key) {
            let v: f64 = s
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid price for '{key}': '{s}'"))?;

            if !v.is_finite() || v < 0.0 {
                anyhow::bail!(
                    "invalid price for '{key}': '{s}', expected a finite non-negative number"
                );
            }
            *setter = Some(s);
        }
    }

    if let Some(s) = params.get_str("tp_trigger_by") {
        result.tp_trigger_by = Some(parse_trigger_type(s)?);
    }

    if let Some(s) = params.get_str("sl_trigger_by") {
        result.sl_trigger_by = Some(parse_trigger_type(s)?);
    }

    if let Some(s) = params.get_str("tp_order_type") {
        result.tp_order_type = Some(parse_tp_sl_order_type(s)?);
    }

    if let Some(s) = params.get_str("sl_order_type") {
        result.sl_order_type = Some(parse_tp_sl_order_type(s)?);
    }

    let has_tp_fields = result.tp_trigger_by.is_some()
        || result.tp_order_type.is_some()
        || result.tp_limit_price.is_some()
        || result.tp_trigger_price.is_some();

    let has_sl_fields = result.sl_trigger_by.is_some()
        || result.sl_order_type.is_some()
        || result.sl_limit_price.is_some()
        || result.sl_trigger_price.is_some();

    if result.take_profit.is_none() && has_tp_fields {
        anyhow::bail!("TP override fields require 'take_profit' to be set");
    }

    if result.stop_loss.is_none() && has_sl_fields {
        anyhow::bail!("SL override fields require 'stop_loss' to be set");
    }

    if result.tp_order_type == Some(BybitOrderType::Limit) && result.tp_limit_price.is_none() {
        anyhow::bail!("'tp_order_type' is 'Limit' but 'tp_limit_price' was not provided");
    }

    if result.sl_order_type == Some(BybitOrderType::Limit) && result.sl_limit_price.is_none() {
        anyhow::bail!("'sl_order_type' is 'Limit' but 'sl_limit_price' was not provided");
    }

    if result.tp_limit_price.is_some() && result.tp_order_type != Some(BybitOrderType::Limit) {
        anyhow::bail!("'tp_limit_price' requires 'tp_order_type' to be 'Limit'");
    }

    if result.sl_limit_price.is_some() && result.sl_order_type != Some(BybitOrderType::Limit) {
        anyhow::bail!("'sl_limit_price' requires 'sl_order_type' to be 'Limit'");
    }

    result.close_on_trigger = params.get_bool("close_on_trigger");

    if let Some(value) = params.get("order_iv") {
        match get_price_str(params, "order_iv") {
            Some(s) => result.order_iv = Some(s),
            None => {
                anyhow::bail!("invalid type for 'order_iv': {value}, expected string or number")
            }
        }
    }

    if let Some(value) = params.get("mmp") {
        match value.as_bool() {
            Some(b) => result.mmp = Some(b),
            None => anyhow::bail!("invalid type for 'mmp': {value}, expected bool"),
        }
    }

    if let Some(value) = params.get("position_idx") {
        let idx = value.as_i64().ok_or_else(|| {
            anyhow::anyhow!("invalid type for 'position_idx': {value}, expected integer")
        })?;
        result.position_idx = Some(match idx {
            0 => BybitPositionIdx::OneWay,
            1 => BybitPositionIdx::BuyHedge,
            2 => BybitPositionIdx::SellHedge,
            _ => anyhow::bail!("invalid 'position_idx': {idx}, expected 0, 1, or 2"),
        });
    }

    Ok(result)
}

fn parse_trigger_type(s: &str) -> anyhow::Result<BybitTriggerType> {
    match s {
        "LastPrice" => Ok(BybitTriggerType::LastPrice),
        "MarkPrice" => Ok(BybitTriggerType::MarkPrice),
        "IndexPrice" => Ok(BybitTriggerType::IndexPrice),
        _ => anyhow::bail!(
            "invalid Bybit trigger type: '{s}', expected LastPrice, MarkPrice, or IndexPrice"
        ),
    }
}

fn parse_tp_sl_order_type(s: &str) -> anyhow::Result<BybitOrderType> {
    match s {
        "Market" => Ok(BybitOrderType::Market),
        "Limit" => Ok(BybitOrderType::Limit),
        _ => anyhow::bail!("invalid Bybit TP/SL order type: '{s}', expected Market or Limit"),
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        data::BarSpecification,
        enums::{AggregationSource, BarAggregation, PositionSide, PriceType},
    };
    use rstest::rstest;
    use serde_json::json;

    use super::*;
    use crate::{
        common::{
            enums::{BybitOrderSide, BybitOrderType, BybitStopOrderType, BybitTriggerDirection},
            testing::load_test_json,
        },
        http::models::{
            BybitInstrumentInverseResponse, BybitInstrumentLinearResponse,
            BybitInstrumentOptionResponse, BybitInstrumentSpotResponse, BybitKlinesResponse,
            BybitOpenOrdersResponse, BybitTradeHistoryResponse, BybitTradesResponse,
        },
    };

    const TS: UnixNanos = UnixNanos::new(1_700_000_000_000_000_000);

    fn sample_fee_rate(
        symbol: &str,
        taker: &str,
        maker: &str,
        base_coin: Option<&str>,
    ) -> BybitFeeRate {
        BybitFeeRate {
            symbol: Ustr::from(symbol),
            taker_fee_rate: taker.to_string(),
            maker_fee_rate: maker.to_string(),
            base_coin: base_coin.map(Ustr::from),
        }
    }

    fn linear_instrument() -> InstrumentAny {
        let json = load_test_json("http_get_instruments_linear.json");
        let response: BybitInstrumentLinearResponse = serde_json::from_str(&json).unwrap();
        let instrument = &response.result.list[0];
        let fee_rate = sample_fee_rate("BTCUSDT", "0.00055", "0.0001", Some("BTC"));
        parse_linear_instrument(instrument, &fee_rate, TS, TS).unwrap()
    }

    #[rstest]
    fn parse_spot_instrument_builds_currency_pair() {
        let json = load_test_json("http_get_instruments_spot.json");
        let response: BybitInstrumentSpotResponse = serde_json::from_str(&json).unwrap();
        let instrument = &response.result.list[0];
        let fee_rate = sample_fee_rate("BTCUSDT", "0.0006", "0.0001", Some("BTC"));

        let parsed = parse_spot_instrument(instrument, &fee_rate, TS, TS).unwrap();
        match parsed {
            InstrumentAny::CurrencyPair(pair) => {
                assert_eq!(pair.id.to_string(), "BTCUSDT-SPOT.BYBIT");
                assert_eq!(pair.price_increment, Price::from_str("0.1").unwrap());
                assert_eq!(pair.size_increment, Quantity::from_str("0.0001").unwrap());
                assert_eq!(pair.base_currency.code.as_str(), "BTC");
                assert_eq!(pair.quote_currency.code.as_str(), "USDT");
            }
            _ => panic!("expected CurrencyPair"),
        }
    }

    #[rstest]
    fn parse_linear_perpetual_instrument_builds_crypto_perpetual() {
        let json = load_test_json("http_get_instruments_linear.json");
        let response: BybitInstrumentLinearResponse = serde_json::from_str(&json).unwrap();
        let instrument = &response.result.list[0];
        let fee_rate = sample_fee_rate("BTCUSDT", "0.00055", "0.0001", Some("BTC"));

        let parsed = parse_linear_instrument(instrument, &fee_rate, TS, TS).unwrap();
        match parsed {
            InstrumentAny::CryptoPerpetual(perp) => {
                assert_eq!(perp.id.to_string(), "BTCUSDT-LINEAR.BYBIT");
                assert!(!perp.is_inverse);
                assert_eq!(perp.price_increment, Price::from_str("0.5").unwrap());
                assert_eq!(perp.size_increment, Quantity::from_str("0.001").unwrap());
            }
            other => panic!("unexpected instrument variant: {other:?}"),
        }
    }

    #[rstest]
    fn parse_inverse_perpetual_instrument_builds_inverse_perpetual() {
        let json = load_test_json("http_get_instruments_inverse.json");
        let response: BybitInstrumentInverseResponse = serde_json::from_str(&json).unwrap();
        let instrument = &response.result.list[0];
        let fee_rate = sample_fee_rate("BTCUSD", "0.00075", "0.00025", Some("BTC"));

        let parsed = parse_inverse_instrument(instrument, &fee_rate, TS, TS).unwrap();
        match parsed {
            InstrumentAny::CryptoPerpetual(perp) => {
                assert_eq!(perp.id.to_string(), "BTCUSD-INVERSE.BYBIT");
                assert!(perp.is_inverse);
                assert_eq!(perp.price_increment, Price::from_str("0.5").unwrap());
                assert_eq!(perp.size_increment, Quantity::from_str("1").unwrap());
            }
            other => panic!("unexpected instrument variant: {other:?}"),
        }
    }

    #[rstest]
    fn parse_option_instrument_builds_crypto_option() {
        let json = load_test_json("http_get_instruments_option.json");
        let response: BybitInstrumentOptionResponse = serde_json::from_str(&json).unwrap();
        let instrument = &response.result.list[0];

        let parsed = parse_option_instrument(instrument, None, TS, TS).unwrap();
        match parsed {
            InstrumentAny::CryptoOption(option) => {
                assert_eq!(option.id.to_string(), "ETH-26JUN26-16000-P-OPTION.BYBIT");
                assert_eq!(option.underlying.code.as_str(), "ETH");
                assert_eq!(option.quote_currency.code.as_str(), "USDC");
                assert_eq!(option.settlement_currency.code.as_str(), "USDC");
                assert!(!option.is_inverse);
                assert_eq!(option.option_kind, OptionKind::Put);
                assert_eq!(option.price_precision, 1);
                assert_eq!(option.price_increment, Price::from_str("0.1").unwrap());
                assert_eq!(option.size_precision, 0);
                assert_eq!(option.size_increment, Quantity::from_str("1").unwrap());
                assert_eq!(option.lot_size, Quantity::from_str("1").unwrap());
            }
            other => panic!("unexpected instrument variant: {other:?}"),
        }
    }

    #[rstest]
    fn test_extract_base_coin_from_option_symbol() {
        assert_eq!(extract_base_coin("BTC-27MAR26-70000-P"), "BTC");
        assert_eq!(extract_base_coin("ETH-26JUN26-16000-C"), "ETH");
        assert_eq!(extract_base_coin("SOL-30MAR26-200-P-USDT"), "SOL");
        assert_eq!(extract_base_coin("BTC"), "BTC");
    }

    #[rstest]
    fn test_extract_base_coin_from_nautilus_option_symbol() {
        // After extract_raw_symbol strips the "-OPTION" suffix
        let raw = extract_raw_symbol("BTC-27MAR26-70000-P-USDT-OPTION");
        assert_eq!(extract_base_coin(raw), "BTC");
    }

    #[rstest]
    fn parse_option_instrument_with_fee_rate() {
        let json = load_test_json("http_get_instruments_option.json");
        let response: BybitInstrumentOptionResponse = serde_json::from_str(&json).unwrap();
        let instrument = &response.result.list[0];
        let fee = sample_fee_rate("", "0.0006", "0.0001", Some("ETH"));

        let parsed = parse_option_instrument(instrument, Some(&fee), TS, TS).unwrap();
        match parsed {
            InstrumentAny::CryptoOption(option) => {
                assert_eq!(option.taker_fee, Decimal::new(6, 4));
                assert_eq!(option.maker_fee, Decimal::new(1, 4));
                assert_eq!(option.margin_init, Decimal::ZERO);
                assert_eq!(option.margin_maint, Decimal::ZERO);
            }
            other => panic!("unexpected instrument variant: {other:?}"),
        }
    }

    #[rstest]
    fn parse_option_instrument_without_fee_rate_defaults_to_zero() {
        let json = load_test_json("http_get_instruments_option.json");
        let response: BybitInstrumentOptionResponse = serde_json::from_str(&json).unwrap();
        let instrument = &response.result.list[0];

        let parsed = parse_option_instrument(instrument, None, TS, TS).unwrap();
        match parsed {
            InstrumentAny::CryptoOption(option) => {
                assert_eq!(option.taker_fee, Decimal::ZERO);
                assert_eq!(option.maker_fee, Decimal::ZERO);
            }
            other => panic!("unexpected instrument variant: {other:?}"),
        }
    }

    #[rstest]
    fn parse_http_trade_into_trade_tick() {
        let instrument = linear_instrument();
        let json = load_test_json("http_get_trades_recent.json");
        let response: BybitTradesResponse = serde_json::from_str(&json).unwrap();
        let trade = &response.result.list[0];

        let tick = parse_trade_tick(trade, &instrument, Some(TS)).unwrap();

        assert_eq!(tick.instrument_id, instrument.id());
        assert_eq!(tick.price, instrument.make_price(27450.50));
        assert_eq!(tick.size, instrument.make_qty(0.005, None));
        assert_eq!(tick.aggressor_side, AggressorSide::Buyer);
        assert_eq!(
            tick.trade_id.to_string(),
            "a905d5c3-1ed0-4f37-83e4-9c73a2fe2f01"
        );
        assert_eq!(tick.ts_event, UnixNanos::new(1_709_891_679_000_000_000));
    }

    #[rstest]
    fn parse_kline_into_bar() {
        let instrument = linear_instrument();
        let json = load_test_json("http_get_klines_linear.json");
        let response: BybitKlinesResponse = serde_json::from_str(&json).unwrap();
        let kline = &response.result.list[0];

        let bar_type = BarType::new(
            instrument.id(),
            BarSpecification::new(1, BarAggregation::Minute, PriceType::Last),
            AggregationSource::External,
        );

        let bar = parse_kline_bar(kline, &instrument, bar_type, false, Some(TS)).unwrap();

        assert_eq!(bar.bar_type.to_string(), bar_type.to_string());
        assert_eq!(bar.open, instrument.make_price(27450.0));
        assert_eq!(bar.high, instrument.make_price(27460.0));
        assert_eq!(bar.low, instrument.make_price(27440.0));
        assert_eq!(bar.close, instrument.make_price(27455.0));
        assert_eq!(bar.volume, instrument.make_qty(123.45, None));
        assert_eq!(bar.ts_event, UnixNanos::new(1_709_891_679_000_000_000));
    }

    #[rstest]
    fn parse_http_position_short_into_position_status_report() {
        use crate::http::models::BybitPositionListResponse;

        let json = load_test_json("http_get_positions.json");
        let response: BybitPositionListResponse = serde_json::from_str(&json).unwrap();

        // Get the short position (ETHUSDT, side="Sell", size="5.0")
        let short_position = &response.result.list[1];
        assert_eq!(short_position.symbol.as_str(), "ETHUSDT");
        assert_eq!(short_position.side, BybitPositionSide::Sell);

        // Create ETHUSDT instrument for parsing
        let eth_json = load_test_json("http_get_instruments_linear.json");
        let eth_response: BybitInstrumentLinearResponse = serde_json::from_str(&eth_json).unwrap();
        let eth_def = &eth_response.result.list[1]; // ETHUSDT is second in the list
        let fee_rate = sample_fee_rate("ETHUSDT", "0.00055", "0.0001", Some("ETH"));
        let eth_instrument = parse_linear_instrument(eth_def, &fee_rate, TS, TS).unwrap();

        let account_id = AccountId::new("BYBIT-001");
        let report =
            parse_position_status_report(short_position, account_id, &eth_instrument, TS).unwrap();

        // Verify short position is correctly parsed
        assert_eq!(report.account_id, account_id);
        assert_eq!(report.instrument_id.symbol.as_str(), "ETHUSDT-LINEAR");
        assert_eq!(report.position_side.as_position_side(), PositionSide::Short);
        assert_eq!(report.quantity, eth_instrument.make_qty(5.0, None));
        assert_eq!(
            report.avg_px_open,
            Some(Decimal::try_from(3000.00).unwrap())
        );
        assert_eq!(report.ts_last, UnixNanos::new(1_697_673_700_112_000_000));
    }

    #[rstest]
    fn parse_http_order_partially_filled_rejected_maps_to_canceled() {
        use crate::http::models::BybitOrderHistoryResponse;

        let instrument = linear_instrument();
        let json = load_test_json("http_get_order_partially_filled_rejected.json");
        let response: BybitOrderHistoryResponse = serde_json::from_str(&json).unwrap();
        let order = &response.result.list[0];
        let account_id = AccountId::new("BYBIT-001");

        let report = parse_order_status_report(order, &instrument, account_id, TS).unwrap();

        // Verify that Bybit "Rejected" status with fills is mapped to Canceled, not Rejected
        assert_eq!(report.order_status, OrderStatus::Canceled);
        assert_eq!(report.filled_qty, instrument.make_qty(0.005, None));
        assert_eq!(
            report.client_order_id.as_ref().unwrap().to_string(),
            "O-20251001-164609-APEX-000-49"
        );
    }

    #[rstest]
    #[case(BarAggregation::Minute, 1, BybitKlineInterval::Minute1)]
    #[case(BarAggregation::Minute, 3, BybitKlineInterval::Minute3)]
    #[case(BarAggregation::Minute, 5, BybitKlineInterval::Minute5)]
    #[case(BarAggregation::Minute, 15, BybitKlineInterval::Minute15)]
    #[case(BarAggregation::Minute, 30, BybitKlineInterval::Minute30)]
    fn test_bar_spec_to_bybit_interval_minutes(
        #[case] aggregation: BarAggregation,
        #[case] step: u64,
        #[case] expected: BybitKlineInterval,
    ) {
        let result = bar_spec_to_bybit_interval(aggregation, step).unwrap();
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(BarAggregation::Hour, 1, BybitKlineInterval::Hour1)]
    #[case(BarAggregation::Hour, 2, BybitKlineInterval::Hour2)]
    #[case(BarAggregation::Hour, 4, BybitKlineInterval::Hour4)]
    #[case(BarAggregation::Hour, 6, BybitKlineInterval::Hour6)]
    #[case(BarAggregation::Hour, 12, BybitKlineInterval::Hour12)]
    fn test_bar_spec_to_bybit_interval_hours(
        #[case] aggregation: BarAggregation,
        #[case] step: u64,
        #[case] expected: BybitKlineInterval,
    ) {
        let result = bar_spec_to_bybit_interval(aggregation, step).unwrap();
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(BarAggregation::Day, 1, BybitKlineInterval::Day1)]
    #[case(BarAggregation::Week, 1, BybitKlineInterval::Week1)]
    #[case(BarAggregation::Month, 1, BybitKlineInterval::Month1)]
    fn test_bar_spec_to_bybit_interval_day_week_month(
        #[case] aggregation: BarAggregation,
        #[case] step: u64,
        #[case] expected: BybitKlineInterval,
    ) {
        let result = bar_spec_to_bybit_interval(aggregation, step).unwrap();
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(BarAggregation::Minute, 2)]
    #[case(BarAggregation::Minute, 10)]
    #[case(BarAggregation::Hour, 3)]
    #[case(BarAggregation::Hour, 24)]
    #[case(BarAggregation::Day, 2)]
    #[case(BarAggregation::Week, 2)]
    #[case(BarAggregation::Month, 2)]
    fn test_bar_spec_to_bybit_interval_unsupported_steps(
        #[case] aggregation: BarAggregation,
        #[case] step: u64,
    ) {
        let result = bar_spec_to_bybit_interval(aggregation, step);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_bar_spec_to_bybit_interval_unsupported_aggregation() {
        let result = bar_spec_to_bybit_interval(BarAggregation::Second, 1);
        assert!(result.is_err());
    }

    #[rstest]
    #[case("1", 1, BarAggregation::Minute)]
    #[case("3", 3, BarAggregation::Minute)]
    #[case("5", 5, BarAggregation::Minute)]
    #[case("15", 15, BarAggregation::Minute)]
    #[case("30", 30, BarAggregation::Minute)]
    fn test_bybit_interval_to_bar_spec_minutes(
        #[case] interval: &str,
        #[case] expected_step: usize,
        #[case] expected_aggregation: BarAggregation,
    ) {
        let result = bybit_interval_to_bar_spec(interval).unwrap();
        assert_eq!(result, (expected_step, expected_aggregation));
    }

    #[rstest]
    #[case("60", 1, BarAggregation::Hour)]
    #[case("120", 2, BarAggregation::Hour)]
    #[case("240", 4, BarAggregation::Hour)]
    #[case("360", 6, BarAggregation::Hour)]
    #[case("720", 12, BarAggregation::Hour)]
    fn test_bybit_interval_to_bar_spec_hours(
        #[case] interval: &str,
        #[case] expected_step: usize,
        #[case] expected_aggregation: BarAggregation,
    ) {
        let result = bybit_interval_to_bar_spec(interval).unwrap();
        assert_eq!(result, (expected_step, expected_aggregation));
    }

    #[rstest]
    #[case("D", 1, BarAggregation::Day)]
    #[case("W", 1, BarAggregation::Week)]
    #[case("M", 1, BarAggregation::Month)]
    fn test_bybit_interval_to_bar_spec_day_week_month(
        #[case] interval: &str,
        #[case] expected_step: usize,
        #[case] expected_aggregation: BarAggregation,
    ) {
        let result = bybit_interval_to_bar_spec(interval).unwrap();
        assert_eq!(result, (expected_step, expected_aggregation));
    }

    #[rstest]
    #[case("2")]
    #[case("10")]
    #[case("100")]
    #[case("invalid")]
    #[case("")]
    fn test_bybit_interval_to_bar_spec_unsupported(#[case] interval: &str) {
        let result = bybit_interval_to_bar_spec(interval);
        assert!(result.is_none());
    }

    fn params_from(pairs: &[(&str, serde_json::Value)]) -> Params {
        let mut p = Params::new();
        for (k, v) in pairs {
            p.insert(k.to_string(), v.clone());
        }
        p
    }

    #[rstest]
    fn test_parse_tp_sl_params_none_returns_defaults() {
        let result = parse_bybit_tp_sl_params(None).unwrap();
        assert!(!result.is_leverage);
        assert!(!result.has_tp_sl());
        assert!(result.order_iv.is_none());
        assert!(result.mmp.is_none());
    }

    #[rstest]
    fn test_parse_tp_sl_params_empty_returns_defaults() {
        let p = Params::new();
        let result = parse_bybit_tp_sl_params(Some(&p)).unwrap();
        assert!(!result.is_leverage);
        assert!(!result.has_tp_sl());
        assert!(result.order_iv.is_none());
        assert!(result.mmp.is_none());
    }

    #[rstest]
    fn test_parse_tp_sl_params_valid_full() {
        let p = params_from(&[
            ("take_profit", json!("55000.00")),
            ("stop_loss", json!("47000.00")),
            ("tp_trigger_by", json!("MarkPrice")),
            ("sl_trigger_by", json!("IndexPrice")),
            ("tp_order_type", json!("Limit")),
            ("tp_limit_price", json!("54990.00")),
            ("sl_order_type", json!("Market")),
            ("close_on_trigger", json!(true)),
            ("is_leverage", json!(true)),
        ]);
        let result = parse_bybit_tp_sl_params(Some(&p)).unwrap();

        assert!(result.has_tp_sl());
        assert!(result.take_profit.is_some());
        assert!(result.stop_loss.is_some());
        assert_eq!(result.tp_trigger_by, Some(BybitTriggerType::MarkPrice));
        assert_eq!(result.sl_trigger_by, Some(BybitTriggerType::IndexPrice));
        assert_eq!(result.tp_order_type, Some(BybitOrderType::Limit));
        assert_eq!(result.sl_order_type, Some(BybitOrderType::Market));
        assert_eq!(result.tp_limit_price.as_deref(), Some("54990.00"));
        assert_eq!(result.close_on_trigger, Some(true));
        assert!(result.is_leverage);
    }

    #[rstest]
    #[case("abc")]
    #[case("nan")]
    #[case("inf")]
    #[case("-1.0")]
    fn test_parse_tp_sl_params_rejects_invalid_take_profit(#[case] price: &str) {
        let p = params_from(&[("take_profit", json!(price))]);
        assert!(parse_bybit_tp_sl_params(Some(&p)).is_err());
    }

    #[rstest]
    #[case("abc")]
    #[case("nan")]
    #[case("inf")]
    fn test_parse_tp_sl_params_rejects_invalid_stop_loss(#[case] price: &str) {
        let p = params_from(&[("stop_loss", json!(price))]);
        assert!(parse_bybit_tp_sl_params(Some(&p)).is_err());
    }

    #[rstest]
    #[case("nan")]
    #[case("inf")]
    #[case("-5.0")]
    #[case("not_a_number")]
    fn test_parse_tp_sl_params_rejects_invalid_limit_price(#[case] price: &str) {
        let p = params_from(&[
            ("take_profit", json!("55000.00")),
            ("tp_order_type", json!("Limit")),
            ("tp_limit_price", json!(price)),
        ]);
        assert!(parse_bybit_tp_sl_params(Some(&p)).is_err());
    }

    #[rstest]
    fn test_parse_tp_sl_params_rejects_invalid_trigger_type() {
        let p = params_from(&[
            ("take_profit", json!("55000.00")),
            ("tp_trigger_by", json!("InvalidType")),
        ]);
        assert!(parse_bybit_tp_sl_params(Some(&p)).is_err());
    }

    #[rstest]
    fn test_parse_tp_sl_params_rejects_invalid_order_type() {
        let p = params_from(&[
            ("stop_loss", json!("47000.00")),
            ("sl_order_type", json!("Stop")),
        ]);
        assert!(parse_bybit_tp_sl_params(Some(&p)).is_err());
    }

    #[rstest]
    fn test_parse_tp_sl_params_rejects_limit_without_limit_price() {
        let p = params_from(&[
            ("take_profit", json!("55000.00")),
            ("tp_order_type", json!("Limit")),
        ]);
        let err = parse_bybit_tp_sl_params(Some(&p)).unwrap_err();
        assert!(err.to_string().contains("tp_limit_price"));
    }

    #[rstest]
    fn test_parse_tp_sl_params_rejects_limit_price_without_limit_type() {
        let p = params_from(&[
            ("take_profit", json!("55000.00")),
            ("tp_limit_price", json!("54990.00")),
        ]);
        let err = parse_bybit_tp_sl_params(Some(&p)).unwrap_err();
        assert!(err.to_string().contains("tp_order_type"));
    }

    #[rstest]
    fn test_parse_tp_sl_params_rejects_orphaned_tp_fields() {
        let p = params_from(&[("tp_trigger_by", json!("MarkPrice"))]);
        let err = parse_bybit_tp_sl_params(Some(&p)).unwrap_err();
        assert!(err.to_string().contains("TP override fields require"));
    }

    #[rstest]
    fn test_parse_tp_sl_params_accepts_numeric_prices() {
        let p = params_from(&[("take_profit", json!(55000.0)), ("stop_loss", json!(47000))]);
        let result = parse_bybit_tp_sl_params(Some(&p)).unwrap();
        assert!(result.take_profit.is_some());
        assert!(result.stop_loss.is_some());
    }

    #[rstest]
    fn test_parse_tp_sl_params_rejects_orphaned_sl_fields() {
        let p = params_from(&[("sl_trigger_by", json!("IndexPrice"))]);
        let err = parse_bybit_tp_sl_params(Some(&p)).unwrap_err();
        assert!(err.to_string().contains("SL override fields require"));
    }

    #[rstest]
    fn test_parse_tp_sl_params_rejects_bool_order_iv() {
        let p = params_from(&[("order_iv", json!(true))]);
        let err = parse_bybit_tp_sl_params(Some(&p)).unwrap_err();
        assert!(err.to_string().contains("order_iv"));
    }

    #[rstest]
    fn test_parse_tp_sl_params_rejects_string_mmp() {
        let p = params_from(&[("mmp", json!("true"))]);
        let err = parse_bybit_tp_sl_params(Some(&p)).unwrap_err();
        assert!(err.to_string().contains("mmp"));
    }

    #[rstest]
    fn test_parse_tp_sl_params_order_iv_string() {
        let p = params_from(&[("order_iv", json!("0.75"))]);
        let result = parse_bybit_tp_sl_params(Some(&p)).unwrap();
        assert_eq!(result.order_iv.as_deref(), Some("0.75"));
    }

    #[rstest]
    fn test_parse_tp_sl_params_order_iv_numeric() {
        let p = params_from(&[("order_iv", json!(0.75))]);
        let result = parse_bybit_tp_sl_params(Some(&p)).unwrap();
        assert_eq!(result.order_iv.as_deref(), Some("0.75"));
    }

    #[rstest]
    fn test_parse_tp_sl_params_mmp() {
        let p = params_from(&[("mmp", json!(true))]);
        let result = parse_bybit_tp_sl_params(Some(&p)).unwrap();
        assert_eq!(result.mmp, Some(true));
    }

    #[rstest]
    #[case(0, BybitPositionIdx::OneWay)]
    #[case(1, BybitPositionIdx::BuyHedge)]
    #[case(2, BybitPositionIdx::SellHedge)]
    fn test_parse_tp_sl_params_position_idx_valid(
        #[case] idx: i64,
        #[case] expected: BybitPositionIdx,
    ) {
        let p = params_from(&[("position_idx", json!(idx))]);
        let result = parse_bybit_tp_sl_params(Some(&p)).unwrap();
        assert_eq!(result.position_idx, Some(expected));
    }

    #[rstest]
    #[case(json!(3))]
    #[case(json!(-1))]
    #[case(json!("1"))]
    #[case(json!(true))]
    fn test_parse_tp_sl_params_position_idx_invalid(#[case] value: serde_json::Value) {
        let p = params_from(&[("position_idx", value)]);
        let err = parse_bybit_tp_sl_params(Some(&p)).unwrap_err();
        assert!(err.to_string().contains("position_idx"));
    }

    #[rstest]
    #[case(
        BybitOrderType::Market,
        BybitStopOrderType::TakeProfit,
        BybitTriggerDirection::RisesTo,
        BybitOrderSide::Sell,
        OrderType::MarketIfTouched
    )]
    #[case(
        BybitOrderType::Market,
        BybitStopOrderType::StopLoss,
        BybitTriggerDirection::FallsTo,
        BybitOrderSide::Sell,
        OrderType::StopMarket
    )]
    #[case(
        BybitOrderType::Market,
        BybitStopOrderType::TakeProfit,
        BybitTriggerDirection::FallsTo,
        BybitOrderSide::Buy,
        OrderType::MarketIfTouched
    )]
    #[case(
        BybitOrderType::Market,
        BybitStopOrderType::StopLoss,
        BybitTriggerDirection::RisesTo,
        BybitOrderSide::Buy,
        OrderType::StopMarket
    )]
    #[case(
        BybitOrderType::Limit,
        BybitStopOrderType::TakeProfit,
        BybitTriggerDirection::RisesTo,
        BybitOrderSide::Sell,
        OrderType::LimitIfTouched
    )]
    #[case(
        BybitOrderType::Limit,
        BybitStopOrderType::StopLoss,
        BybitTriggerDirection::FallsTo,
        BybitOrderSide::Sell,
        OrderType::StopLimit
    )]
    #[case(
        BybitOrderType::Limit,
        BybitStopOrderType::PartialTakeProfit,
        BybitTriggerDirection::FallsTo,
        BybitOrderSide::Buy,
        OrderType::LimitIfTouched
    )]
    #[case(
        BybitOrderType::Limit,
        BybitStopOrderType::PartialStopLoss,
        BybitTriggerDirection::RisesTo,
        BybitOrderSide::Buy,
        OrderType::StopLimit
    )]
    #[case(
        BybitOrderType::Market,
        BybitStopOrderType::TpslOrder,
        BybitTriggerDirection::FallsTo,
        BybitOrderSide::Sell,
        OrderType::StopMarket
    )]
    #[case(
        BybitOrderType::Market,
        BybitStopOrderType::Stop,
        BybitTriggerDirection::RisesTo,
        BybitOrderSide::Buy,
        OrderType::StopMarket
    )]
    #[case(
        BybitOrderType::Market,
        BybitStopOrderType::Stop,
        BybitTriggerDirection::FallsTo,
        BybitOrderSide::Sell,
        OrderType::StopMarket
    )]
    #[case(
        BybitOrderType::Market,
        BybitStopOrderType::TrailingStop,
        BybitTriggerDirection::FallsTo,
        BybitOrderSide::Sell,
        OrderType::StopMarket
    )]
    #[case(
        BybitOrderType::Limit,
        BybitStopOrderType::TrailingStop,
        BybitTriggerDirection::RisesTo,
        BybitOrderSide::Buy,
        OrderType::StopLimit
    )]
    fn test_parse_bybit_order_type_conditional(
        #[case] order_type: BybitOrderType,
        #[case] stop_order_type: BybitStopOrderType,
        #[case] trigger_direction: BybitTriggerDirection,
        #[case] side: BybitOrderSide,
        #[case] expected: OrderType,
    ) {
        let result = parse_bybit_order_type(order_type, stop_order_type, trigger_direction, side);
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(
        BybitOrderType::Market,
        BybitStopOrderType::None,
        BybitTriggerDirection::None,
        BybitOrderSide::Buy,
        OrderType::Market
    )]
    #[case(
        BybitOrderType::Limit,
        BybitStopOrderType::Unknown,
        BybitTriggerDirection::None,
        BybitOrderSide::Sell,
        OrderType::Limit
    )]
    #[case(
        BybitOrderType::Market,
        BybitStopOrderType::TakeProfit,
        BybitTriggerDirection::None,
        BybitOrderSide::Sell,
        OrderType::Market
    )]
    #[case(
        BybitOrderType::Limit,
        BybitStopOrderType::StopLoss,
        BybitTriggerDirection::None,
        BybitOrderSide::Buy,
        OrderType::Limit
    )]
    fn test_parse_bybit_order_type_plain(
        #[case] order_type: BybitOrderType,
        #[case] stop_order_type: BybitStopOrderType,
        #[case] trigger_direction: BybitTriggerDirection,
        #[case] side: BybitOrderSide,
        #[case] expected: OrderType,
    ) {
        let result = parse_bybit_order_type(order_type, stop_order_type, trigger_direction, side);
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_parse_order_status_report_take_profit() {
        let instrument = linear_instrument();
        let json = load_test_json("http_get_orders_realtime_tp_sl.json");
        let response: BybitOpenOrdersResponse = serde_json::from_str(&json).unwrap();
        let order = &response.result.list[0];
        let account_id = AccountId::new("BYBIT-001");

        let report = parse_order_status_report(order, &instrument, account_id, TS).unwrap();

        assert_eq!(report.order_type, OrderType::MarketIfTouched);
        assert_eq!(report.order_side, OrderSide::Sell);
        assert_eq!(report.order_status, OrderStatus::Accepted);
        assert!(report.trigger_price.is_some());
        assert_eq!(
            report.trigger_price.unwrap(),
            Price::from_str("55000.0").unwrap()
        );
        assert_eq!(report.trigger_type, Some(TriggerType::LastPrice));
        assert!(report.reduce_only);
    }

    #[rstest]
    fn test_parse_order_status_report_stop_loss_limit() {
        let instrument = linear_instrument();
        let json = load_test_json("http_get_orders_realtime_tp_sl.json");
        let response: BybitOpenOrdersResponse = serde_json::from_str(&json).unwrap();
        let order = &response.result.list[1];
        let account_id = AccountId::new("BYBIT-001");

        let report = parse_order_status_report(order, &instrument, account_id, TS).unwrap();

        assert_eq!(report.order_type, OrderType::StopLimit);
        assert_eq!(report.order_side, OrderSide::Sell);
        assert_eq!(report.order_status, OrderStatus::Accepted);
        assert!(report.trigger_price.is_some());
        assert_eq!(
            report.trigger_price.unwrap(),
            Price::from_str("48000.0").unwrap()
        );
        assert!(report.price.is_some());
        assert_eq!(report.price.unwrap(), Price::from_str("47500.0").unwrap());
        assert_eq!(report.trigger_type, Some(TriggerType::LastPrice));
        assert!(report.reduce_only);
    }

    #[rstest]
    #[case::oneway(0, "BTCUSDT-LINEAR.BYBIT-ONEWAY")]
    #[case::long(1, "BTCUSDT-LINEAR.BYBIT-LONG")]
    #[case::short(2, "BTCUSDT-LINEAR.BYBIT-SHORT")]
    #[case::unknown(99, "BTCUSDT-LINEAR.BYBIT-UNKNOWN")]
    fn test_make_venue_position_id(#[case] position_idx: i32, #[case] expected: &str) {
        let instrument_id = InstrumentId::from("BTCUSDT-LINEAR.BYBIT");
        let result = make_venue_position_id(instrument_id, position_idx);
        assert_eq!(result, PositionId::from(expected));
    }

    #[rstest]
    fn test_parse_fill_report_venue_position_id_is_none() {
        let instrument = linear_instrument();
        let json = load_test_json("http_get_executions.json");
        let response: BybitTradeHistoryResponse = serde_json::from_str(&json).unwrap();
        let execution = &response.result.list[0];
        let account_id = AccountId::new("BYBIT-001");

        let report = parse_fill_report(execution, account_id, &instrument, TS).unwrap();

        assert_eq!(report.venue_position_id, None);
    }

    #[rstest]
    fn test_parse_order_status_report_venue_position_id_is_none() {
        let instrument = linear_instrument();
        let json = load_test_json("http_get_orders_realtime_tp_sl.json");
        let response: BybitOpenOrdersResponse = serde_json::from_str(&json).unwrap();
        let order = &response.result.list[0]; // TP order, positionIdx=0
        let account_id = AccountId::new("BYBIT-001");

        let report = parse_order_status_report(order, &instrument, account_id, TS).unwrap();

        assert_eq!(report.venue_position_id, None);
    }
}
