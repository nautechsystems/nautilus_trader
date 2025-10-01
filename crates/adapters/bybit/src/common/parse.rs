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
    data::{Bar, BarType, TradeTick},
    enums::{AggressorSide, AssetClass, CurrencyType, OptionKind},
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

pub(crate) fn parse_price_with_precision(value: &str, precision: u8, field: &str) -> Result<Price> {
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
) -> Result<Quantity> {
    let parsed = value
        .parse::<f64>()
        .with_context(|| format!("Failed to parse {field}='{value}' as f64"))?;
    Quantity::new_checked(parsed, precision).with_context(|| {
        format!("Failed to construct Quantity for {field} with precision {precision}")
    })
}

pub(crate) fn parse_price(value: &str, field: &str) -> Result<Price> {
    Price::from_str(value).map_err(|err| anyhow!("Failed to parse {field}='{value}': {err}"))
}

pub(crate) fn parse_quantity(value: &str, field: &str) -> Result<Quantity> {
    Quantity::from_str(value).map_err(|err| anyhow!("Failed to parse {field}='{value}': {err}"))
}

pub(crate) fn parse_decimal(value: &str, field: &str) -> Result<Decimal> {
    Decimal::from_str(value)
        .map_err(|err| anyhow!("Failed to parse {field}='{value}' as Decimal: {err}"))
}

pub(crate) fn parse_millis_timestamp(value: &str, field: &str) -> Result<UnixNanos> {
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

/// Parses a Bybit order into a Nautilus OrderStatusReport.
pub fn parse_order_status_report(
    order: &crate::http::models::BybitOrder,
    instrument: &InstrumentAny,
    account_id: nautilus_model::identifiers::AccountId,
    ts_init: UnixNanos,
) -> Result<nautilus_model::reports::OrderStatusReport> {
    use nautilus_model::{
        enums::{OrderSide, OrderStatus, OrderType, TimeInForce},
        identifiers::{ClientOrderId, VenueOrderId},
        reports::OrderStatusReport,
    };

    use crate::common::enums::{BybitOrderStatus, BybitOrderType, BybitTimeInForce};

    let instrument_id = instrument.id();
    let venue_order_id = VenueOrderId::new(order.order_id);

    let order_side: OrderSide = order.side.into();

    let order_type: OrderType = match order.order_type {
        BybitOrderType::Market => OrderType::Market,
        BybitOrderType::Limit => OrderType::Limit,
        BybitOrderType::Unknown => OrderType::Limit,
    };

    let time_in_force: TimeInForce = match order.time_in_force {
        BybitTimeInForce::Gtc => TimeInForce::Gtc,
        BybitTimeInForce::Ioc => TimeInForce::Ioc,
        BybitTimeInForce::Fok => TimeInForce::Fok,
        BybitTimeInForce::PostOnly => TimeInForce::Gtc,
    };

    let order_status: OrderStatus = match order.order_status {
        BybitOrderStatus::Created | BybitOrderStatus::New | BybitOrderStatus::Untriggered => {
            OrderStatus::Accepted
        }
        BybitOrderStatus::Rejected => OrderStatus::Rejected,
        BybitOrderStatus::PartiallyFilled => OrderStatus::PartiallyFilled,
        BybitOrderStatus::Filled => OrderStatus::Filled,
        BybitOrderStatus::Canceled | BybitOrderStatus::PartiallyFilledCanceled => {
            OrderStatus::Canceled
        }
        BybitOrderStatus::Triggered => OrderStatus::Triggered,
        BybitOrderStatus::Deactivated => OrderStatus::Canceled,
    };

    let quantity =
        parse_quantity_with_precision(&order.qty, instrument.size_precision(), "order.qty")?;

    let filled_qty = parse_quantity_with_precision(
        &order.cum_exec_qty,
        instrument.size_precision(),
        "order.cumExecQty",
    )?;

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
        Some(nautilus_core::uuid::UUID4::new()),
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
        report = report.with_avg_px(avg_px);
    }

    if !order.trigger_price.is_empty() && order.trigger_price != "0" {
        let trigger_price = parse_price_with_precision(
            &order.trigger_price,
            instrument.price_precision(),
            "order.triggerPrice",
        )?;
        report = report.with_trigger_price(trigger_price);
    }

    Ok(report)
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
