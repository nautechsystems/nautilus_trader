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

//! Conversion helpers that translate Bybit API schemas into Nautilus instruments.

use std::{convert::TryFrom, str::FromStr};

use anyhow::{Context, Result, anyhow};
use nautilus_core::{datetime::NANOSECONDS_IN_MILLISECOND, nanos::UnixNanos};
use nautilus_model::{
    data::{Bar, BarType, BookOrder, OrderBookDelta, OrderBookDeltas, QuoteTick, TradeTick},
    enums::{
        AggressorSide, AssetClass, BookAction, CurrencyType, OptionKind, OrderSide, RecordFlag,
    },
    identifiers::{Symbol, TradeId},
    instruments::{
        Instrument, any::InstrumentAny, crypto_future::CryptoFuture,
        crypto_perpetual::CryptoPerpetual, currency_pair::CurrencyPair,
        option_contract::OptionContract,
    },
    types::{Currency, Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::{
    common::{
        enums::{BybitContractType, BybitOptionType},
        symbol::BybitSymbol,
    },
    http::models::{
        BybitFeeRate, BybitInstrumentInverse, BybitInstrumentLinear, BybitInstrumentOption,
        BybitInstrumentSpot, BybitKline, BybitTrade,
    },
    websocket::messages::{BybitWsOrderbookDepthMsg, BybitWsTrade},
};

fn default_margin() -> Decimal {
    Decimal::new(1, 1)
}

/// Parses a spot instrument definition returned by Bybit into a Nautilus currency pair.
pub fn parse_spot_instrument(
    definition: &BybitInstrumentSpot,
    fee_rate: &BybitFeeRate,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> Result<InstrumentAny> {
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
) -> Result<InstrumentAny> {
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
                ts_event,
                ts_init,
            );
            Ok(InstrumentAny::CryptoFuture(instrument))
        }
        other => Err(anyhow!("unsupported linear contract variant: {other:?}")),
    }
}

/// Parses an inverse contract definition into a Nautilus instrument.
pub fn parse_inverse_instrument(
    definition: &BybitInstrumentInverse,
    fee_rate: &BybitFeeRate,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> Result<InstrumentAny> {
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
                ts_event,
                ts_init,
            );
            Ok(InstrumentAny::CryptoFuture(instrument))
        }
        other => Err(anyhow!("unsupported inverse contract variant: {other:?}")),
    }
}

/// Parses a Bybit option contract definition into a Nautilus option instrument.
pub fn parse_option_instrument(
    definition: &BybitInstrumentOption,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> Result<InstrumentAny> {
    let quote_currency = get_currency(definition.quote_coin.as_str());

    let symbol = BybitSymbol::new(format!("{}-OPTION", definition.symbol))?;
    let instrument_id = symbol.to_instrument_id();
    let raw_symbol = Symbol::new(symbol.raw_symbol());

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

    let instrument = OptionContract::new(
        instrument_id,
        raw_symbol,
        AssetClass::Cryptocurrency,
        None,
        Ustr::from(definition.base_coin.as_str()),
        option_kind,
        strike_price,
        quote_currency,
        activation_ns,
        expiration_ns,
        price_increment.precision,
        price_increment,
        Quantity::from(1_u32),
        lot_size,
        max_quantity,
        min_quantity,
        max_price,
        min_price,
        Some(Decimal::ZERO),
        Some(Decimal::ZERO),
        Some(Decimal::ZERO),
        Some(Decimal::ZERO),
        ts_event,
        ts_init,
    );

    Ok(InstrumentAny::OptionContract(instrument))
}

/// Parses a REST trade payload into a [`TradeTick`].
pub fn parse_trade_tick(
    trade: &BybitTrade,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> Result<TradeTick> {
    let price =
        parse_price_with_precision(&trade.price, instrument.price_precision(), "trade.price")?;
    let size =
        parse_quantity_with_precision(&trade.size, instrument.size_precision(), "trade.size")?;
    let aggressor: AggressorSide = trade.side.into();
    let trade_id = TradeId::new_checked(trade.exec_id.as_str())
        .context("invalid exec_id in Bybit trade payload")?;
    let ts_event = parse_millis_timestamp(&trade.time, "trade.time")?;

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

/// Parses a WebSocket trade frame into a [`TradeTick`].
pub fn parse_ws_trade_tick(
    trade: &BybitWsTrade,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> Result<TradeTick> {
    let price = parse_price_with_precision(&trade.p, instrument.price_precision(), "trade.p")?;
    let size = parse_quantity_with_precision(&trade.v, instrument.size_precision(), "trade.v")?;
    let aggressor: AggressorSide = trade.taker_side.into();
    let trade_id = TradeId::new_checked(trade.i.as_str())
        .context("invalid trade identifier in Bybit trade message")?;
    let ts_event = parse_millis_i64(trade.t, "trade.T")?;

    TradeTick::new_checked(
        instrument.id(),
        price,
        size,
        aggressor,
        trade_id,
        ts_event,
        ts_init,
    )
    .context("failed to construct TradeTick from Bybit trade message")
}

/// Parses an order book depth message into [`OrderBookDeltas`].
pub fn parse_orderbook_deltas(
    msg: &BybitWsOrderbookDepthMsg,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> Result<OrderBookDeltas> {
    let is_snapshot = msg.msg_type.eq_ignore_ascii_case("snapshot");
    let ts_event = parse_millis_i64(msg.ts, "orderbook.ts")?;
    let ts_init = if ts_init.is_zero() { ts_event } else { ts_init };

    let depth = &msg.data;
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();
    let update_id = u64::try_from(depth.u)
        .context("received negative update id in Bybit order book message")?;
    let sequence = u64::try_from(depth.seq)
        .context("received negative sequence in Bybit order book message")?;

    let mut deltas = Vec::new();

    if is_snapshot {
        deltas.push(OrderBookDelta::clear(
            instrument_id,
            sequence,
            ts_event,
            ts_init,
        ));
    }

    let total_levels = depth.b.len() + depth.a.len();
    let mut processed = 0_usize;

    let mut push_level = |values: &[String], side: OrderSide| -> Result<()> {
        let (price, size) = parse_book_level(values, price_precision, size_precision, "orderbook")?;
        let action = if size.is_zero() {
            BookAction::Delete
        } else if is_snapshot {
            BookAction::Add
        } else {
            BookAction::Update
        };

        processed += 1;
        let mut flags = RecordFlag::F_MBP as u8;
        if processed == total_levels {
            flags |= RecordFlag::F_LAST as u8;
        }

        let order = BookOrder::new(side, price, size, update_id);
        let delta = OrderBookDelta::new_checked(
            instrument_id,
            action,
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

    for level in &depth.b {
        push_level(level, OrderSide::Buy)?;
    }
    for level in &depth.a {
        push_level(level, OrderSide::Sell)?;
    }

    if total_levels == 0
        && let Some(last) = deltas.last_mut()
    {
        last.flags |= RecordFlag::F_LAST as u8;
    }

    OrderBookDeltas::new_checked(instrument_id, deltas)
        .context("failed to assemble OrderBookDeltas from Bybit message")
}

/// Parses an order book snapshot or delta into a [`QuoteTick`].
pub fn parse_orderbook_quote(
    msg: &BybitWsOrderbookDepthMsg,
    instrument: &InstrumentAny,
    last_quote: Option<&QuoteTick>,
    ts_init: UnixNanos,
) -> Result<QuoteTick> {
    let ts_event = parse_millis_i64(msg.ts, "orderbook.ts")?;
    let ts_init = if ts_init.is_zero() { ts_event } else { ts_init };
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let get_best = |levels: &[Vec<String>], label: &str| -> Result<Option<(Price, Quantity)>> {
        if let Some(values) = levels.first() {
            parse_book_level(values, price_precision, size_precision, label).map(Some)
        } else {
            Ok(None)
        }
    };

    let bids = get_best(&msg.data.b, "bid")?;
    let asks = get_best(&msg.data.a, "ask")?;

    let (bid_price, bid_size) = match (bids, last_quote) {
        (Some(level), _) => level,
        (None, Some(prev)) => (prev.bid_price, prev.bid_size),
        (None, None) => {
            return Err(anyhow!(
                "Bybit order book update missing bid levels and no previous quote provided"
            ));
        }
    };

    let (ask_price, ask_size) = match (asks, last_quote) {
        (Some(level), _) => level,
        (None, Some(prev)) => (prev.ask_price, prev.ask_size),
        (None, None) => {
            return Err(anyhow!(
                "Bybit order book update missing ask levels and no previous quote provided"
            ));
        }
    };

    QuoteTick::new_checked(
        instrument.id(),
        bid_price,
        ask_price,
        bid_size,
        ask_size,
        ts_event,
        ts_init,
    )
    .context("failed to construct QuoteTick from Bybit order book message")
}

/// Parses a kline entry into a [`Bar`].
pub fn parse_kline_bar(
    kline: &BybitKline,
    instrument: &InstrumentAny,
    bar_type: BarType,
    timestamp_on_close: bool,
    ts_init: UnixNanos,
) -> Result<Bar> {
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
    let ts_init = if ts_init.is_zero() { ts_event } else { ts_init };

    Bar::new_checked(bar_type, open, high, low, close, volume, ts_event, ts_init)
        .context("failed to construct Bar from Bybit kline entry")
}

fn parse_price_with_precision(value: &str, precision: u8, field: &str) -> Result<Price> {
    let parsed = value
        .parse::<f64>()
        .with_context(|| format!("failed to parse {field}='{value}' as f64"))?;
    Price::new_checked(parsed, precision).with_context(|| {
        format!("failed to construct Price for {field} with precision {precision}")
    })
}

fn parse_quantity_with_precision(value: &str, precision: u8, field: &str) -> Result<Quantity> {
    let parsed = value
        .parse::<f64>()
        .with_context(|| format!("failed to parse {field}='{value}' as f64"))?;
    Quantity::new_checked(parsed, precision).with_context(|| {
        format!("failed to construct Quantity for {field} with precision {precision}")
    })
}

fn parse_millis_i64(value: i64, field: &str) -> Result<UnixNanos> {
    if value < 0 {
        Err(anyhow!("{field} must be non-negative, was {value}"))
    } else {
        parse_millis_timestamp(&value.to_string(), field)
    }
}

fn parse_book_level(
    level: &[String],
    price_precision: u8,
    size_precision: u8,
    label: &str,
) -> Result<(Price, Quantity)> {
    let price_str = level
        .first()
        .ok_or_else(|| anyhow!("missing price component in {label} level"))?;
    let size_str = level
        .get(1)
        .ok_or_else(|| anyhow!("missing size component in {label} level"))?;
    let price = parse_price_with_precision(price_str, price_precision, label)?;
    let size = parse_quantity_with_precision(size_str, size_precision, label)?;
    Ok((price, size))
}

fn parse_price(value: &str, field: &str) -> Result<Price> {
    Price::from_str(value).map_err(|err| anyhow!("failed to parse {field}='{value}': {err}"))
}

fn parse_quantity(value: &str, field: &str) -> Result<Quantity> {
    Quantity::from_str(value).map_err(|err| anyhow!("failed to parse {field}='{value}': {err}"))
}

fn parse_decimal(value: &str, field: &str) -> Result<Decimal> {
    Decimal::from_str(value)
        .map_err(|err| anyhow!("failed to parse {field}='{value}' as Decimal: {err}"))
}

fn parse_millis_timestamp(value: &str, field: &str) -> Result<UnixNanos> {
    let millis: u64 = value
        .parse()
        .with_context(|| format!("failed to parse {field}='{value}' as u64 millis"))?;
    let nanos = millis
        .checked_mul(NANOSECONDS_IN_MILLISECOND)
        .context("millisecond timestamp overflowed when converting to nanoseconds")?;
    Ok(UnixNanos::from(nanos))
}

fn resolve_settlement_currency(
    settle_coin: &str,
    base_currency: Currency,
    quote_currency: Currency,
) -> Result<Currency> {
    if settle_coin.eq_ignore_ascii_case(base_currency.code.as_str()) {
        Ok(base_currency)
    } else if settle_coin.eq_ignore_ascii_case(quote_currency.code.as_str()) {
        Ok(quote_currency)
    } else {
        Err(anyhow!("unrecognised settlement currency '{settle_coin}'"))
    }
}

fn get_currency(code: &str) -> Currency {
    Currency::try_from_str(code)
        .unwrap_or_else(|| Currency::new(code, 8, 0, code, CurrencyType::Crypto))
}

fn extract_strike_from_symbol(symbol: &str) -> Result<Price> {
    let parts: Vec<&str> = symbol.split('-').collect();
    let strike = parts
        .get(2)
        .ok_or_else(|| anyhow!("invalid option symbol '{symbol}'"))?;
    parse_price(strike, "option strike")
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::{
        common::testing::load_test_json,
        http::models::{
            BybitInstrumentInverseResponse, BybitInstrumentLinearResponse,
            BybitInstrumentOptionResponse, BybitInstrumentSpotResponse, BybitKlinesResponse,
            BybitTradesResponse,
        },
        websocket::messages::{BybitWsOrderbookDepthMsg, BybitWsTradeMsg},
    };

    const TS: UnixNanos = UnixNanos::new(1_700_000_000_000_000_000);

    use nautilus_model::{
        data::BarSpecification,
        enums::{AggregationSource, BarAggregation, PriceType},
    };

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
    fn parse_option_instrument_builds_option_contract() {
        let json = load_test_json("http_get_instruments_option.json");
        let response: BybitInstrumentOptionResponse = serde_json::from_str(&json).unwrap();
        let instrument = &response.result.list[0];

        let parsed = parse_option_instrument(instrument, TS, TS).unwrap();
        match parsed {
            InstrumentAny::OptionContract(option) => {
                assert_eq!(option.id.to_string(), "ETH-26JUN26-16000-P-OPTION.BYBIT");
                assert_eq!(option.option_kind, OptionKind::Put);
                assert_eq!(option.price_increment, Price::from_str("0.1").unwrap());
                assert_eq!(option.lot_size, Quantity::from_str("1").unwrap());
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

        let tick = parse_trade_tick(trade, &instrument, TS).unwrap();

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
    fn parse_ws_trade_into_trade_tick() {
        let instrument = linear_instrument();
        let json = load_test_json("ws_public_trade.json");
        let msg: BybitWsTradeMsg = serde_json::from_str(&json).unwrap();
        let trade = &msg.data[0];

        let tick = parse_ws_trade_tick(trade, &instrument, TS).unwrap();

        assert_eq!(tick.instrument_id, instrument.id());
        assert_eq!(tick.price, instrument.make_price(27451.00));
        assert_eq!(tick.size, instrument.make_qty(0.010, None));
        assert_eq!(tick.aggressor_side, AggressorSide::Buyer);
        assert_eq!(
            tick.trade_id.to_string(),
            "9dc75fca-4bdd-4773-9f78-6f5d7ab2a110"
        );
        assert_eq!(tick.ts_event, UnixNanos::new(1_709_891_679_000_000_000));
    }

    #[rstest]
    fn parse_orderbook_snapshot_into_deltas() {
        let instrument = linear_instrument();
        let json = load_test_json("ws_orderbook_snapshot.json");
        let msg: BybitWsOrderbookDepthMsg = serde_json::from_str(&json).unwrap();

        let deltas = parse_orderbook_deltas(&msg, &instrument, TS).unwrap();

        assert_eq!(deltas.instrument_id, instrument.id());
        assert_eq!(deltas.deltas.len(), 5);
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);
        assert_eq!(
            deltas.deltas[1].order.price,
            instrument.make_price(27450.00)
        );
        assert_eq!(
            deltas.deltas[1].order.size,
            instrument.make_qty(0.500, None)
        );
        let last = deltas.deltas.last().unwrap();
        assert_eq!(last.order.side, OrderSide::Sell);
        assert_eq!(last.order.price, instrument.make_price(27451.50));
        assert_eq!(
            last.flags & RecordFlag::F_LAST as u8,
            RecordFlag::F_LAST as u8
        );
    }

    #[rstest]
    fn parse_orderbook_delta_marks_actions() {
        let instrument = linear_instrument();
        let json = load_test_json("ws_orderbook_delta.json");
        let msg: BybitWsOrderbookDepthMsg = serde_json::from_str(&json).unwrap();

        let deltas = parse_orderbook_deltas(&msg, &instrument, TS).unwrap();

        assert_eq!(deltas.deltas.len(), 2);
        let bid = &deltas.deltas[0];
        assert_eq!(bid.action, BookAction::Update);
        assert_eq!(bid.order.side, OrderSide::Buy);
        assert_eq!(bid.order.size, instrument.make_qty(0.400, None));

        let ask = &deltas.deltas[1];
        assert_eq!(ask.action, BookAction::Delete);
        assert_eq!(ask.order.side, OrderSide::Sell);
        assert_eq!(ask.order.size, instrument.make_qty(0.0, None));
        assert_eq!(
            ask.flags & RecordFlag::F_LAST as u8,
            RecordFlag::F_LAST as u8
        );
    }

    #[rstest]
    fn parse_orderbook_quote_produces_top_of_book() {
        let instrument = linear_instrument();
        let json = load_test_json("ws_orderbook_snapshot.json");
        let msg: BybitWsOrderbookDepthMsg = serde_json::from_str(&json).unwrap();

        let quote = parse_orderbook_quote(&msg, &instrument, None, TS).unwrap();

        assert_eq!(quote.instrument_id, instrument.id());
        assert_eq!(quote.bid_price, instrument.make_price(27450.00));
        assert_eq!(quote.bid_size, instrument.make_qty(0.500, None));
        assert_eq!(quote.ask_price, instrument.make_price(27451.00));
        assert_eq!(quote.ask_size, instrument.make_qty(0.750, None));
    }

    #[rstest]
    fn parse_orderbook_quote_with_delta_updates_sizes() {
        let instrument = linear_instrument();
        let snapshot: BybitWsOrderbookDepthMsg =
            serde_json::from_str(&load_test_json("ws_orderbook_snapshot.json")).unwrap();
        let base_quote = parse_orderbook_quote(&snapshot, &instrument, None, TS).unwrap();

        let delta: BybitWsOrderbookDepthMsg =
            serde_json::from_str(&load_test_json("ws_orderbook_delta.json")).unwrap();
        let updated = parse_orderbook_quote(&delta, &instrument, Some(&base_quote), TS).unwrap();

        assert_eq!(updated.bid_price, instrument.make_price(27450.00));
        assert_eq!(updated.bid_size, instrument.make_qty(0.400, None));
        assert_eq!(updated.ask_price, instrument.make_price(27451.00));
        assert_eq!(updated.ask_size, instrument.make_qty(0.0, None));
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

        let bar = parse_kline_bar(kline, &instrument, bar_type, false, TS).unwrap();

        assert_eq!(bar.bar_type.to_string(), bar_type.to_string());
        assert_eq!(bar.open, instrument.make_price(27450.0));
        assert_eq!(bar.high, instrument.make_price(27460.0));
        assert_eq!(bar.low, instrument.make_price(27440.0));
        assert_eq!(bar.close, instrument.make_price(27455.0));
        assert_eq!(bar.volume, instrument.make_qty(123.45, None));
        assert_eq!(bar.ts_event, UnixNanos::new(1_709_891_679_000_000_000));
    }
}
