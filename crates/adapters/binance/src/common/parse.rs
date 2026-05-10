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

//! Parsing utilities for Binance API responses.
//!
//! Provides conversion functions to transform raw Binance exchange data
//! into Nautilus domain objects such as instruments and market data.

use std::str::FromStr;

use anyhow::Context;
use nautilus_core::nanos::UnixNanos;
use nautilus_model::{
    data::{Bar, BarSpecification, BarType, TradeTick},
    enums::{
        AggressorSide, BarAggregation, LiquiditySide, OrderSide, OrderStatus, OrderType,
        TimeInForce, TriggerType,
    },
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, OrderListId, Symbol, TradeId, Venue, VenueOrderId,
    },
    instruments::{
        Instrument, any::InstrumentAny, crypto_perpetual::CryptoPerpetual,
        currency_pair::CurrencyPair,
    },
    reports::{FillReport, OrderStatusReport},
    types::{Currency, Money, Price, Quantity},
};
use rust_decimal::{Decimal, prelude::ToPrimitive};
use serde_json::Value;

use crate::{
    common::{
        consts::BINANCE,
        encoder::decode_broker_id,
        enums::{BinanceContractStatus, BinanceKlineInterval, BinanceTradingStatus},
    },
    futures::http::models::{BinanceFuturesCoinSymbol, BinanceFuturesUsdSymbol},
    spot::{
        http::models::{
            BinanceAccountTrade, BinanceKlines, BinanceLotSizeFilterSbe, BinanceNewOrderResponse,
            BinanceOrderResponse, BinancePriceFilterSbe, BinanceSymbolSbe, BinanceTrades,
        },
        sbe::spot::{
            order_side::OrderSide as SbeOrderSide, order_status::OrderStatus as SbeOrderStatus,
            order_type::OrderType as SbeOrderType, time_in_force::TimeInForce as SbeTimeInForce,
        },
    },
};
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

    if symbol.status != BinanceTradingStatus::Trading {
        anyhow::bail!(
            "Symbol '{}' is not trading (status: {:?})",
            symbol.symbol,
            symbol.status
        );
    }

    let base_currency = get_currency(symbol.base_asset.as_str());
    let quote_currency = get_currency(symbol.quote_asset.as_str());
    let settlement_currency = get_currency(symbol.margin_asset.as_str());

    let instrument_id = InstrumentId::new(
        Symbol::from_str_unchecked(format!("{}-PERP", symbol.symbol)),
        Venue::new(BINANCE),
    );
    let raw_symbol = Symbol::new(symbol.symbol.as_str());

    let price_filter = get_filter(&symbol.filters, "PRICE_FILTER")
        .context("Missing PRICE_FILTER in symbol filters")?;

    let tick_size = parse_filter_price(price_filter, "tickSize")?;
    if tick_size.is_zero() {
        anyhow::bail!(
            "Invalid tickSize of 0 for symbol '{}', cannot create instrument",
            symbol.symbol,
        );
    }
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
        None, // info
        ts_event,
        ts_init,
    );

    Ok(InstrumentAny::CryptoPerpetual(instrument))
}

/// Parses a COIN-M Futures symbol definition into a Nautilus CryptoPerpetual instrument.
///
/// COIN-M perpetuals are inverse contracts settled in base currency (e.g., BTC).
///
/// # Errors
///
/// Returns an error if:
/// - Required filter values are missing (PRICE_FILTER, LOT_SIZE).
/// - Price or quantity values cannot be parsed.
/// - The contract type is not PERPETUAL.
/// - The contract is not in TRADING status.
pub fn parse_coinm_instrument(
    symbol: &BinanceFuturesCoinSymbol,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    if symbol.contract_type != CONTRACT_TYPE_PERPETUAL {
        anyhow::bail!(
            "Unsupported contract type '{}' for symbol '{}', expected '{}'",
            symbol.contract_type,
            symbol.symbol,
            CONTRACT_TYPE_PERPETUAL
        );
    }

    if symbol.contract_status != Some(BinanceContractStatus::Trading) {
        anyhow::bail!(
            "Symbol '{}' is not trading (status: {:?})",
            symbol.symbol,
            symbol.contract_status
        );
    }

    let base_currency = get_currency(symbol.base_asset.as_str());
    let quote_currency = get_currency(symbol.quote_asset.as_str());

    // COIN-M contracts are settled in the base currency (inverse)
    let settlement_currency = get_currency(symbol.margin_asset.as_str());

    let instrument_id = InstrumentId::new(
        Symbol::from_str_unchecked(format!("{}-PERP", symbol.symbol)),
        Venue::new(BINANCE),
    );
    let raw_symbol = Symbol::new(symbol.symbol.as_str());

    let price_filter = get_filter(&symbol.filters, "PRICE_FILTER")
        .context("Missing PRICE_FILTER in symbol filters")?;

    let tick_size = parse_filter_price(price_filter, "tickSize")?;
    if tick_size.is_zero() {
        anyhow::bail!(
            "Invalid tickSize of 0 for symbol '{}', cannot create instrument",
            symbol.symbol,
        );
    }
    let max_price = parse_filter_price(price_filter, "maxPrice").ok();
    let min_price = parse_filter_price(price_filter, "minPrice").ok();

    let lot_filter =
        get_filter(&symbol.filters, "LOT_SIZE").context("Missing LOT_SIZE in symbol filters")?;

    let step_size = parse_filter_quantity(lot_filter, "stepSize")?;
    let max_quantity = parse_filter_quantity(lot_filter, "maxQty").ok();
    let min_quantity = parse_filter_quantity(lot_filter, "minQty").ok();

    // COIN-M has contract_size as the multiplier
    let multiplier = Quantity::new(symbol.contract_size as f64, 0);

    // Default margin (0.1 = 10x leverage)
    let default_margin = Decimal::new(1, 1);

    let instrument = CryptoPerpetual::new(
        instrument_id,
        raw_symbol,
        base_currency,
        quote_currency,
        settlement_currency,
        true, // is_inverse (COIN-M contracts are inverse)
        tick_size.precision,
        step_size.precision,
        tick_size,
        step_size,
        Some(multiplier),
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
        None, // info
        ts_event,
        ts_init,
    );

    Ok(InstrumentAny::CryptoPerpetual(instrument))
}

/// SBE status value for Trading.
const SBE_STATUS_TRADING: u8 = 0;

/// Derives the number of significant decimal places from an SBE mantissa/exponent pair.
///
/// Binance SBE encodes values as `mantissa * 10^exponent` where `exponent` is a global
/// fixed-point encoding parameter (typically -8), not the instrument's trading precision.
/// The actual precision is determined by how many trailing zeros the mantissa carries.
///
/// # Examples
///
/// - ETHUSDC tick_size: mantissa=1_000_000, exp=-8 → 0.01 → precision=2
/// - DOGEUSDT tick_size: mantissa=1_000, exp=-8 → 0.00001 → precision=5
/// - SHIBUSDT tick_size: mantissa=1, exp=-8 → 0.00000001 → precision=8
/// - BTCTRY tick_size: mantissa=100_000_000, exp=-8 → 1.0 → precision=0
fn sbe_mantissa_precision(mantissa: i64, exponent: i8) -> u8 {
    if mantissa == 0 {
        return 0;
    }
    let mut m = mantissa.abs();
    let mut trailing_zeros: i8 = 0;

    while m > 0 && m % 10 == 0 {
        m /= 10;
        trailing_zeros += 1;
    }
    (-exponent - trailing_zeros).max(0) as u8
}

/// Parses an SBE price filter into tick_size, max_price, min_price.
fn parse_sbe_price_filter(filter: &BinancePriceFilterSbe) -> (Price, Option<Price>, Option<Price>) {
    let precision = sbe_mantissa_precision(filter.tick_size, filter.price_exponent);

    let tick_size =
        Price::from_mantissa_exponent(filter.tick_size, filter.price_exponent, precision);

    let max_price = if filter.max_price != 0 {
        Some(Price::from_mantissa_exponent(
            filter.max_price,
            filter.price_exponent,
            precision,
        ))
    } else {
        None
    };

    let min_price = if filter.min_price != 0 {
        Some(Price::from_mantissa_exponent(
            filter.min_price,
            filter.price_exponent,
            precision,
        ))
    } else {
        None
    };

    (tick_size, max_price, min_price)
}

/// Parses an SBE lot size filter into step_size, max_qty, min_qty.
fn parse_sbe_lot_size_filter(
    filter: &BinanceLotSizeFilterSbe,
) -> (Quantity, Option<Quantity>, Option<Quantity>) {
    let precision = sbe_mantissa_precision(filter.step_size, filter.qty_exponent);

    let step_size =
        Quantity::from_mantissa_exponent(filter.step_size as u64, filter.qty_exponent, precision);

    let max_qty = if filter.max_qty != 0 {
        Some(Quantity::from_mantissa_exponent(
            filter.max_qty as u64,
            filter.qty_exponent,
            precision,
        ))
    } else {
        None
    };

    let min_qty = if filter.min_qty != 0 {
        Some(Quantity::from_mantissa_exponent(
            filter.min_qty as u64,
            filter.qty_exponent,
            precision,
        ))
    } else {
        None
    };

    (step_size, max_qty, min_qty)
}

/// Parses a Binance Spot SBE symbol into a Nautilus CurrencyPair instrument.
///
/// # Errors
///
/// Returns an error if:
/// - Required filter values are missing (PRICE_FILTER, LOT_SIZE).
/// - Price or quantity values cannot be parsed.
/// - The symbol is not actively trading.
pub fn parse_spot_instrument_sbe(
    symbol: &BinanceSymbolSbe,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    if symbol.status != SBE_STATUS_TRADING {
        anyhow::bail!(
            "Symbol '{}' is not trading (status: {})",
            symbol.symbol,
            symbol.status
        );
    }

    let base_currency = get_currency(&symbol.base_asset);
    let quote_currency = get_currency(&symbol.quote_asset);

    let instrument_id = InstrumentId::new(
        Symbol::from_str_unchecked(&symbol.symbol),
        Venue::new(BINANCE),
    );
    let raw_symbol = Symbol::new(&symbol.symbol);

    let price_filter = symbol
        .filters
        .price_filter
        .as_ref()
        .context("Missing PRICE_FILTER in symbol filters")?;

    let (tick_size, max_price, min_price) = parse_sbe_price_filter(price_filter);

    let lot_filter = symbol
        .filters
        .lot_size_filter
        .as_ref()
        .context("Missing LOT_SIZE in symbol filters")?;

    let (step_size, max_quantity, min_quantity) = parse_sbe_lot_size_filter(lot_filter);

    // Spot has no leverage, use 1.0 margin
    let default_margin = Decimal::new(1, 0);

    let instrument = CurrencyPair::new(
        instrument_id,
        raw_symbol,
        base_currency,
        quote_currency,
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
        None, // info
        ts_event,
        ts_init,
    );

    Ok(InstrumentAny::CurrencyPair(instrument))
}

/// Parses Binance SBE trades into Nautilus TradeTick objects.
///
/// Uses mantissa/exponent encoding from SBE to construct proper Price and Quantity.
///
/// # Errors
///
/// Returns an error if any trade cannot be parsed.
pub fn parse_spot_trades_sbe(
    trades: &BinanceTrades,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<Vec<TradeTick>> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let mut result = Vec::with_capacity(trades.trades.len());

    for trade in &trades.trades {
        let price = Price::from_mantissa_exponent(
            trade.price_mantissa,
            trades.price_exponent,
            price_precision,
        );
        let size = Quantity::from_mantissa_exponent(
            trade.qty_mantissa as u64,
            trades.qty_exponent,
            size_precision,
        );

        // is_buyer_maker means the buyer was the maker, so the aggressor was selling
        let aggressor_side = if trade.is_buyer_maker {
            AggressorSide::Seller
        } else {
            AggressorSide::Buyer
        };

        // SBE trade timestamps are in microseconds
        let ts_event = UnixNanos::from(trade.time as u64 * 1_000);

        let tick = TradeTick::new(
            instrument_id,
            price,
            size,
            aggressor_side,
            TradeId::new(trade.id.to_string()),
            ts_event,
            ts_init,
        );

        result.push(tick);
    }

    Ok(result)
}

/// Maps Binance SBE order status to Nautilus order status.
#[must_use]
pub const fn map_order_status_sbe(status: SbeOrderStatus) -> OrderStatus {
    match status {
        SbeOrderStatus::New => OrderStatus::Accepted,
        SbeOrderStatus::PendingNew => OrderStatus::Submitted,
        SbeOrderStatus::PartiallyFilled => OrderStatus::PartiallyFilled,
        SbeOrderStatus::Filled => OrderStatus::Filled,
        SbeOrderStatus::Canceled => OrderStatus::Canceled,
        SbeOrderStatus::PendingCancel => OrderStatus::PendingCancel,
        SbeOrderStatus::Rejected => OrderStatus::Rejected,
        SbeOrderStatus::Expired | SbeOrderStatus::ExpiredInMatch => OrderStatus::Expired,
        SbeOrderStatus::Unknown | SbeOrderStatus::NonRepresentable | SbeOrderStatus::NullVal => {
            OrderStatus::Initialized
        }
    }
}

/// Maps Binance SBE order type to Nautilus order type.
#[must_use]
pub const fn map_order_type_sbe(order_type: SbeOrderType) -> OrderType {
    match order_type {
        SbeOrderType::Market => OrderType::Market,
        SbeOrderType::Limit | SbeOrderType::LimitMaker => OrderType::Limit,
        SbeOrderType::StopLoss | SbeOrderType::TakeProfit => OrderType::StopMarket,
        SbeOrderType::StopLossLimit | SbeOrderType::TakeProfitLimit => OrderType::StopLimit,
        SbeOrderType::NonRepresentable | SbeOrderType::NullVal => OrderType::Market,
    }
}

/// Maps Binance SBE order side to Nautilus order side.
#[must_use]
pub const fn map_order_side_sbe(side: SbeOrderSide) -> OrderSide {
    match side {
        SbeOrderSide::Buy => OrderSide::Buy,
        SbeOrderSide::Sell => OrderSide::Sell,
        SbeOrderSide::NonRepresentable | SbeOrderSide::NullVal => OrderSide::NoOrderSide,
    }
}

/// Maps Binance SBE time in force to Nautilus time in force.
#[must_use]
pub const fn map_time_in_force_sbe(tif: SbeTimeInForce) -> TimeInForce {
    match tif {
        SbeTimeInForce::Gtc => TimeInForce::Gtc,
        SbeTimeInForce::Ioc => TimeInForce::Ioc,
        SbeTimeInForce::Fok => TimeInForce::Fok,
        SbeTimeInForce::NonRepresentable | SbeTimeInForce::NullVal => TimeInForce::Gtc,
    }
}

/// Parses a Binance SBE order response into a Nautilus `OrderStatusReport`.
///
/// # Errors
///
/// Returns an error if any field cannot be parsed.
pub fn parse_order_status_report_sbe(
    order: &BinanceOrderResponse,
    account_id: AccountId,
    instrument: &InstrumentAny,
    broker_id: &str,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let price = if order.price_mantissa != 0 {
        Some(Price::from_mantissa_exponent(
            order.price_mantissa,
            order.price_exponent,
            price_precision,
        ))
    } else {
        None
    };

    let quantity = Quantity::from_mantissa_exponent(
        order.orig_qty_mantissa as u64,
        order.qty_exponent,
        size_precision,
    );
    let filled_qty = Quantity::from_mantissa_exponent(
        order.executed_qty_mantissa as u64,
        order.qty_exponent,
        size_precision,
    );

    // Calculate average price from cumulative quote qty / executed qty
    // This requires decimal arithmetic since we're dividing two mantissas
    let avg_px = if order.executed_qty_mantissa > 0 {
        let quote_exp = (order.price_exponent as i32) + (order.qty_exponent as i32);
        let cum_quote_dec = Decimal::new(order.cummulative_quote_qty_mantissa, (-quote_exp) as u32);
        let filled_dec = Decimal::new(
            order.executed_qty_mantissa,
            (-order.qty_exponent as i32) as u32,
        );
        let avg_dec = cum_quote_dec / filled_dec;
        Some(
            Price::from_decimal_dp(avg_dec, price_precision)
                .unwrap_or(Price::zero(price_precision)),
        )
    } else {
        None
    };

    // Parse trigger price for stop orders
    let trigger_price = order.stop_price_mantissa.and_then(|mantissa| {
        if mantissa != 0 {
            Some(Price::from_mantissa_exponent(
                mantissa,
                order.price_exponent,
                price_precision,
            ))
        } else {
            None
        }
    });

    // Map enums
    let order_status = map_order_status_sbe(order.status);
    let order_type = map_order_type_sbe(order.order_type);
    let order_side = map_order_side_sbe(order.side);
    let time_in_force = map_time_in_force_sbe(order.time_in_force);

    // Determine trigger type for stop orders
    let trigger_type = if trigger_price.is_some() {
        Some(TriggerType::LastPrice)
    } else {
        None
    };

    // Parse timestamps (SBE uses microseconds)
    let ts_event = UnixNanos::from_micros(order.update_time as u64);

    // Build order list ID if present
    let order_list_id = order.order_list_id.and_then(|id| {
        if id > 0 {
            Some(OrderListId::new(id.to_string()))
        } else {
            None
        }
    });

    // Determine post-only (limit maker orders are post-only)
    let post_only = order.order_type == SbeOrderType::LimitMaker;

    // Parse order creation time (SBE uses microseconds)
    let ts_accepted = UnixNanos::from_micros(order.time as u64);

    let mut report = OrderStatusReport::new(
        account_id,
        instrument_id,
        Some(ClientOrderId::new(decode_broker_id(
            &order.client_order_id,
            broker_id,
        ))),
        VenueOrderId::new(order.order_id.to_string()),
        order_side,
        order_type,
        time_in_force,
        order_status,
        quantity,
        filled_qty,
        ts_accepted,
        ts_event,
        ts_init,
        None, // report_id (auto-generated)
    );

    // Apply optional fields using builder methods
    if let Some(p) = price {
        report = report.with_price(p);
    }

    if let Some(ap) = avg_px {
        report = report.with_avg_px(ap.as_f64())?;
    }

    if let Some(tp) = trigger_price {
        report = report.with_trigger_price(tp);
    }

    if let Some(tt) = trigger_type {
        report = report.with_trigger_type(tt);
    }

    if let Some(oli) = order_list_id {
        report = report.with_order_list_id(oli);
    }

    if post_only {
        report = report.with_post_only(true);
    }

    Ok(report)
}

/// Parses a Binance new order response (SBE) into a Nautilus `OrderStatusReport`.
///
/// # Errors
///
/// Returns an error if any field cannot be parsed.
pub fn parse_new_order_response_sbe(
    response: &BinanceNewOrderResponse,
    account_id: AccountId,
    instrument: &InstrumentAny,
    broker_id: &str,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let price = if response.price_mantissa != 0 {
        Some(Price::from_mantissa_exponent(
            response.price_mantissa,
            response.price_exponent,
            price_precision,
        ))
    } else {
        None
    };

    let quantity = Quantity::from_mantissa_exponent(
        response.orig_qty_mantissa as u64,
        response.qty_exponent,
        size_precision,
    );
    let filled_qty = Quantity::from_mantissa_exponent(
        response.executed_qty_mantissa as u64,
        response.qty_exponent,
        size_precision,
    );

    // Calculate average price from cumulative quote qty / executed qty
    // This requires decimal arithmetic since we're dividing two mantissas
    let avg_px = if response.executed_qty_mantissa > 0 {
        let quote_exp = (response.price_exponent as i32) + (response.qty_exponent as i32);
        let cum_quote_dec =
            Decimal::new(response.cummulative_quote_qty_mantissa, (-quote_exp) as u32);
        let filled_dec = Decimal::new(
            response.executed_qty_mantissa,
            (-response.qty_exponent as i32) as u32,
        );
        let avg_dec = cum_quote_dec / filled_dec;
        Some(
            Price::from_decimal_dp(avg_dec, price_precision)
                .unwrap_or(Price::zero(price_precision)),
        )
    } else {
        None
    };

    let trigger_price = response.stop_price_mantissa.and_then(|mantissa| {
        if mantissa != 0 {
            Some(Price::from_mantissa_exponent(
                mantissa,
                response.price_exponent,
                price_precision,
            ))
        } else {
            None
        }
    });

    let order_status = map_order_status_sbe(response.status);
    let order_type = map_order_type_sbe(response.order_type);
    let order_side = map_order_side_sbe(response.side);
    let time_in_force = map_time_in_force_sbe(response.time_in_force);

    let trigger_type = if trigger_price.is_some() {
        Some(TriggerType::LastPrice)
    } else {
        None
    };

    // SBE uses microseconds; for new orders transact_time is both creation and event time
    let ts_event = UnixNanos::from_micros(response.transact_time as u64);
    let ts_accepted = ts_event;

    let order_list_id = response.order_list_id.and_then(|id| {
        if id > 0 {
            Some(OrderListId::new(id.to_string()))
        } else {
            None
        }
    });

    // Limit maker orders are post-only
    let post_only = response.order_type == SbeOrderType::LimitMaker;

    let mut report = OrderStatusReport::new(
        account_id,
        instrument_id,
        Some(ClientOrderId::new(decode_broker_id(
            &response.client_order_id,
            broker_id,
        ))),
        VenueOrderId::new(response.order_id.to_string()),
        order_side,
        order_type,
        time_in_force,
        order_status,
        quantity,
        filled_qty,
        ts_accepted,
        ts_event,
        ts_init,
        None,
    );

    if let Some(p) = price {
        report = report.with_price(p);
    }

    if let Some(ap) = avg_px {
        report = report.with_avg_px(ap.as_f64())?;
    }

    if let Some(tp) = trigger_price {
        report = report.with_trigger_price(tp);
    }

    if let Some(tt) = trigger_type {
        report = report.with_trigger_type(tt);
    }

    if let Some(oli) = order_list_id {
        report = report.with_order_list_id(oli);
    }

    if post_only {
        report = report.with_post_only(true);
    }

    Ok(report)
}

/// Parses a Binance SBE account trade into a Nautilus `FillReport`.
///
/// # Errors
///
/// Returns an error if any field cannot be parsed.
pub fn parse_fill_report_sbe(
    trade: &BinanceAccountTrade,
    account_id: AccountId,
    instrument: &InstrumentAny,
    commission_currency: Currency,
    ts_init: UnixNanos,
) -> anyhow::Result<FillReport> {
    let instrument_id = instrument.id();
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let last_px =
        Price::from_mantissa_exponent(trade.price_mantissa, trade.price_exponent, price_precision);
    let last_qty = Quantity::from_mantissa_exponent(
        trade.qty_mantissa as u64,
        trade.qty_exponent,
        size_precision,
    );

    // Commission still uses Decimal → f64 since Money::new takes f64
    let comm_exp = trade.commission_exponent as i32;
    let comm_dec = Decimal::new(trade.commission_mantissa, (-comm_exp) as u32);
    let commission = Money::new(comm_dec.to_f64().unwrap_or(0.0), commission_currency);

    // Determine order side from is_buyer
    let order_side = if trade.is_buyer {
        OrderSide::Buy
    } else {
        OrderSide::Sell
    };

    // Determine liquidity side from is_maker
    let liquidity_side = if trade.is_maker {
        LiquiditySide::Maker
    } else {
        LiquiditySide::Taker
    };

    // Parse timestamp (SBE uses microseconds)
    let ts_event = UnixNanos::from_micros(trade.time as u64);

    Ok(FillReport::new(
        account_id,
        instrument_id,
        VenueOrderId::new(trade.order_id.to_string()),
        TradeId::new(trade.id.to_string()),
        order_side,
        last_qty,
        last_px,
        commission,
        liquidity_side,
        None, // client_order_id (not in account trades response)
        None, // venue_position_id
        ts_event,
        ts_init,
        None, // report_id
    ))
}

/// Parses Binance klines (candlesticks) into Nautilus Bar objects.
///
/// # Errors
///
/// Returns an error if any kline cannot be parsed.
pub fn parse_klines_to_bars(
    klines: &BinanceKlines,
    bar_type: BarType,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<Vec<Bar>> {
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let mut bars = Vec::with_capacity(klines.klines.len());

    for kline in &klines.klines {
        let open =
            Price::from_mantissa_exponent(kline.open_price, klines.price_exponent, price_precision);
        let high =
            Price::from_mantissa_exponent(kline.high_price, klines.price_exponent, price_precision);
        let low =
            Price::from_mantissa_exponent(kline.low_price, klines.price_exponent, price_precision);
        let close = Price::from_mantissa_exponent(
            kline.close_price,
            klines.price_exponent,
            price_precision,
        );

        // Volume is 128-bit so we still use Decimal path for now
        let volume_mantissa = i128::from_le_bytes(kline.volume);
        let volume_dec =
            Decimal::from_i128_with_scale(volume_mantissa, (-klines.qty_exponent as i32) as u32);
        let volume = Quantity::new(volume_dec.to_f64().unwrap_or(0.0), size_precision);

        let ts_event = UnixNanos::from_micros(kline.open_time as u64);

        let bar = Bar::new(bar_type, open, high, low, close, volume, ts_event, ts_init);
        bars.push(bar);
    }

    Ok(bars)
}

/// Converts a Nautilus bar specification to a Binance kline interval.
///
/// # Errors
///
/// Returns an error if the bar specification does not map to a supported
/// Binance kline interval.
pub fn bar_spec_to_binance_interval(
    bar_spec: BarSpecification,
) -> anyhow::Result<BinanceKlineInterval> {
    let step = bar_spec.step.get();
    let interval = match bar_spec.aggregation {
        BarAggregation::Second => {
            anyhow::bail!("Binance Spot does not support second-level kline intervals")
        }
        BarAggregation::Minute => match step {
            1 => BinanceKlineInterval::Minute1,
            3 => BinanceKlineInterval::Minute3,
            5 => BinanceKlineInterval::Minute5,
            15 => BinanceKlineInterval::Minute15,
            30 => BinanceKlineInterval::Minute30,
            _ => anyhow::bail!("Unsupported minute interval: {step}m"),
        },
        BarAggregation::Hour => match step {
            1 => BinanceKlineInterval::Hour1,
            2 => BinanceKlineInterval::Hour2,
            4 => BinanceKlineInterval::Hour4,
            6 => BinanceKlineInterval::Hour6,
            8 => BinanceKlineInterval::Hour8,
            12 => BinanceKlineInterval::Hour12,
            _ => anyhow::bail!("Unsupported hour interval: {step}h"),
        },
        BarAggregation::Day => match step {
            1 => BinanceKlineInterval::Day1,
            3 => BinanceKlineInterval::Day3,
            _ => anyhow::bail!("Unsupported day interval: {step}d"),
        },
        BarAggregation::Week => match step {
            1 => BinanceKlineInterval::Week1,
            _ => anyhow::bail!("Unsupported week interval: {step}w"),
        },
        BarAggregation::Month => match step {
            1 => BinanceKlineInterval::Month1,
            _ => anyhow::bail!("Unsupported month interval: {step}M"),
        },
        agg => anyhow::bail!("Unsupported bar aggregation for Binance: {agg:?}"),
    };

    Ok(interval)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json::json;
    use ustr::Ustr;

    use super::*;
    use crate::common::{
        consts::BINANCE_NAUTILUS_SPOT_BROKER_ID,
        enums::{BinanceContractStatus, BinanceTradingStatus},
    };

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

    fn sample_coinm_symbol() -> BinanceFuturesCoinSymbol {
        BinanceFuturesCoinSymbol {
            symbol: Ustr::from("BTCUSD_PERP"),
            pair: Ustr::from("BTCUSD"),
            contract_type: "PERPETUAL".to_string(),
            delivery_date: 4_133_404_800_000,
            onboard_date: 1_569_398_400_000,
            contract_status: Some(BinanceContractStatus::Trading),
            contract_size: 100,
            maint_margin_percent: "2.5000".to_string(),
            required_margin_percent: "5.0000".to_string(),
            base_asset: Ustr::from("BTC"),
            quote_asset: Ustr::from("USD"),
            margin_asset: Ustr::from("BTC"),
            price_precision: 1,
            quantity_precision: 0,
            base_asset_precision: 8,
            quote_precision: 8,
            equal_qty_precision: None,
            trigger_protect: Some("0.0500".to_string()),
            liquidation_fee: Some("0.012500".to_string()),
            market_take_bound: Some("0.05".to_string()),
            order_types: vec!["LIMIT".to_string(), "MARKET".to_string()],
            time_in_force: vec!["GTC".to_string(), "IOC".to_string()],
            filters: vec![
                json!({
                    "filterType": "PRICE_FILTER",
                    "tickSize": "0.10",
                    "maxPrice": "1000000",
                    "minPrice": "0.10"
                }),
                json!({
                    "filterType": "LOT_SIZE",
                    "stepSize": "1",
                    "maxQty": "1000",
                    "minQty": "1"
                }),
            ],
        }
    }

    fn sample_spot_symbol_sbe() -> BinanceSymbolSbe {
        BinanceSymbolSbe {
            symbol: "ETHUSDT".to_string(),
            base_asset: "ETH".to_string(),
            quote_asset: "USDT".to_string(),
            base_asset_precision: 8,
            quote_asset_precision: 8,
            status: SBE_STATUS_TRADING,
            order_types: 0,
            iceberg_allowed: true,
            oco_allowed: true,
            oto_allowed: false,
            quote_order_qty_market_allowed: true,
            allow_trailing_stop: true,
            cancel_replace_allowed: true,
            amend_allowed: true,
            is_spot_trading_allowed: true,
            is_margin_trading_allowed: false,
            filters: crate::spot::http::models::BinanceSymbolFiltersSbe {
                price_filter: Some(BinancePriceFilterSbe {
                    price_exponent: -8,
                    min_price: 1_000_000,
                    max_price: 100_000_000_000_000,
                    tick_size: 1_000_000,
                }),
                lot_size_filter: Some(BinanceLotSizeFilterSbe {
                    qty_exponent: -8,
                    min_qty: 10_000,
                    max_qty: 900_000_000_000,
                    step_size: 10_000,
                }),
            },
            permissions: vec![vec!["SPOT".to_string()]],
        }
    }

    fn sample_spot_instrument() -> InstrumentAny {
        let ts = UnixNanos::from(1_700_000_000_000_000_000u64);
        parse_spot_instrument_sbe(&sample_spot_symbol_sbe(), ts, ts).unwrap()
    }

    fn sample_account_id() -> AccountId {
        AccountId::from("BINANCE-SPOT-001")
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
            other => panic!("Expected CryptoPerpetual, was {other:?}"),
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

    #[rstest]
    fn test_parse_coinm_perpetual() {
        let symbol = sample_coinm_symbol();
        let ts = UnixNanos::from(1_700_000_000_000_000_000u64);

        let result = parse_coinm_instrument(&symbol, ts, ts).unwrap();

        match result {
            InstrumentAny::CryptoPerpetual(perp) => {
                assert_eq!(perp.id.to_string(), "BTCUSD_PERP-PERP.BINANCE");
                assert_eq!(perp.raw_symbol.to_string(), "BTCUSD_PERP");
                assert_eq!(perp.base_currency.code.as_str(), "BTC");
                assert_eq!(perp.quote_currency.code.as_str(), "USD");
                assert_eq!(perp.settlement_currency.code.as_str(), "BTC");
                assert!(perp.is_inverse);
                assert_eq!(perp.price_increment, Price::from_str("0.10").unwrap());
                assert_eq!(perp.size_increment, Quantity::from_str("1").unwrap());
            }
            other => panic!("Expected CryptoPerpetual, was {other:?}"),
        }
    }

    #[rstest]
    fn test_parse_spot_instrument_sbe() {
        let symbol = sample_spot_symbol_sbe();
        let ts = UnixNanos::from(1_700_000_000_000_000_000u64);

        let result = parse_spot_instrument_sbe(&symbol, ts, ts).unwrap();

        match result {
            InstrumentAny::CurrencyPair(pair) => {
                assert_eq!(pair.id.to_string(), "ETHUSDT.BINANCE");
                assert_eq!(pair.raw_symbol.to_string(), "ETHUSDT");
                assert_eq!(pair.base_currency.code.as_str(), "ETH");
                assert_eq!(pair.quote_currency.code.as_str(), "USDT");
                assert_eq!(pair.price_increment, Price::from_str("0.01").unwrap());
                assert_eq!(pair.size_increment, Quantity::from_str("0.0001").unwrap());
            }
            other => panic!("Expected CurrencyPair, was {other:?}"),
        }
    }

    #[rstest]
    fn test_parse_spot_trades_sbe() {
        let instrument = sample_spot_instrument();
        let trades = BinanceTrades {
            price_exponent: -2,
            qty_exponent: -4,
            trades: vec![
                crate::spot::http::models::BinanceTrade {
                    id: 1,
                    price_mantissa: 12_345,
                    qty_mantissa: 25_000,
                    quote_qty_mantissa: 0,
                    time: 1_700_000_000_000_000,
                    is_buyer_maker: false,
                    is_best_match: true,
                },
                crate::spot::http::models::BinanceTrade {
                    id: 2,
                    price_mantissa: 12_340,
                    qty_mantissa: 10_000,
                    quote_qty_mantissa: 0,
                    time: 1_700_000_000_500_000,
                    is_buyer_maker: true,
                    is_best_match: true,
                },
            ],
        };
        let ts_init = UnixNanos::from(1_700_000_001_000_000_000u64);

        let result = parse_spot_trades_sbe(&trades, &instrument, ts_init).unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].instrument_id, instrument.id());
        assert_eq!(result[0].price.as_f64(), 123.45);
        assert_eq!(result[0].size.as_f64(), 2.5);
        assert_eq!(result[0].aggressor_side, AggressorSide::Buyer);
        assert_eq!(result[0].trade_id, TradeId::new("1"));
        assert_eq!(
            result[0].ts_event,
            UnixNanos::from(1_700_000_000_000_000_000u64)
        );
        assert_eq!(result[0].ts_init, ts_init);
        assert_eq!(result[1].aggressor_side, AggressorSide::Seller);
    }

    #[rstest]
    fn test_parse_order_status_report_sbe() {
        let instrument = sample_spot_instrument();
        let order = BinanceOrderResponse {
            price_exponent: -2,
            qty_exponent: -4,
            order_id: 42,
            order_list_id: Some(77),
            price_mantissa: 12_345,
            orig_qty_mantissa: 25_000,
            executed_qty_mantissa: 10_000,
            cummulative_quote_qty_mantissa: 123_450_000,
            status: SbeOrderStatus::PartiallyFilled,
            time_in_force: SbeTimeInForce::Gtc,
            order_type: SbeOrderType::LimitMaker,
            side: SbeOrderSide::Buy,
            stop_price_mantissa: None,
            iceberg_qty_mantissa: None,
            time: 1_700_000_000_000_000,
            update_time: 1_700_000_000_100_000,
            is_working: true,
            working_time: Some(1_700_000_000_050_000),
            orig_quote_order_qty_mantissa: 0,
            self_trade_prevention_mode:
                crate::spot::sbe::spot::self_trade_prevention_mode::SelfTradePreventionMode::None,
            client_order_id: "client-123".to_string(),
            symbol: "ETHUSDT".to_string(),
        };
        let ts_init = UnixNanos::from(1_700_000_001_000_000_000u64);

        let report = parse_order_status_report_sbe(
            &order,
            sample_account_id(),
            &instrument,
            BINANCE_NAUTILUS_SPOT_BROKER_ID,
            ts_init,
        )
        .unwrap();

        assert_eq!(report.account_id, sample_account_id());
        assert_eq!(report.instrument_id, instrument.id());
        assert_eq!(
            report.client_order_id,
            Some(ClientOrderId::new("client-123"))
        );
        assert_eq!(report.venue_order_id, VenueOrderId::new("42"));
        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.order_type, OrderType::Limit);
        assert_eq!(report.order_status, OrderStatus::PartiallyFilled);
        assert_eq!(report.quantity.as_f64(), 2.5);
        assert_eq!(report.filled_qty.as_f64(), 1.0);
        assert_eq!(report.order_list_id, Some(OrderListId::new("77")));
        assert_eq!(report.price, Some(Price::new(123.45, 2)));
        assert_eq!(report.avg_px.unwrap().to_string(), "123.45");
        assert!(report.post_only);
        assert_eq!(
            report.ts_accepted,
            UnixNanos::from(1_700_000_000_000_000_000u64)
        );
        assert_eq!(
            report.ts_last,
            UnixNanos::from(1_700_000_000_100_000_000u64)
        );
        assert_eq!(report.ts_init, ts_init);
    }

    #[rstest]
    fn test_parse_new_order_response_sbe() {
        let instrument = sample_spot_instrument();
        let response = BinanceNewOrderResponse {
            price_exponent: -2,
            qty_exponent: -4,
            order_id: 99,
            order_list_id: Some(7),
            transact_time: 1_700_000_000_000_000,
            price_mantissa: 12_100,
            orig_qty_mantissa: 20_000,
            executed_qty_mantissa: 5_000,
            cummulative_quote_qty_mantissa: 60_500_000,
            status: SbeOrderStatus::New,
            time_in_force: SbeTimeInForce::Gtc,
            order_type: SbeOrderType::StopLossLimit,
            side: SbeOrderSide::Sell,
            stop_price_mantissa: Some(12_000),
            working_time: Some(1_700_000_000_000_000),
            self_trade_prevention_mode:
                crate::spot::sbe::spot::self_trade_prevention_mode::SelfTradePreventionMode::None,
            client_order_id: "client-456".to_string(),
            symbol: "ETHUSDT".to_string(),
            fills: vec![],
        };
        let ts_init = UnixNanos::from(1_700_000_001_000_000_000u64);

        let report = parse_new_order_response_sbe(
            &response,
            sample_account_id(),
            &instrument,
            BINANCE_NAUTILUS_SPOT_BROKER_ID,
            ts_init,
        )
        .unwrap();

        assert_eq!(report.account_id, sample_account_id());
        assert_eq!(report.instrument_id, instrument.id());
        assert_eq!(
            report.client_order_id,
            Some(ClientOrderId::new("client-456"))
        );
        assert_eq!(report.venue_order_id, VenueOrderId::new("99"));
        assert_eq!(report.order_side, OrderSide::Sell);
        assert_eq!(report.order_type, OrderType::StopLimit);
        assert_eq!(report.order_status, OrderStatus::Accepted);
        assert_eq!(report.quantity.as_f64(), 2.0);
        assert_eq!(report.filled_qty.as_f64(), 0.5);
        assert_eq!(report.order_list_id, Some(OrderListId::new("7")));
        assert_eq!(report.price, Some(Price::new(121.0, 2)));
        assert_eq!(report.trigger_price, Some(Price::new(120.0, 2)));
        assert_eq!(report.trigger_type, Some(TriggerType::LastPrice));
        assert_eq!(report.avg_px.unwrap().to_string(), "121");
        assert!(!report.post_only);
        assert_eq!(
            report.ts_accepted,
            UnixNanos::from(1_700_000_000_000_000_000u64)
        );
        assert_eq!(
            report.ts_last,
            UnixNanos::from(1_700_000_000_000_000_000u64)
        );
    }

    #[rstest]
    fn test_parse_fill_report_sbe() {
        let instrument = sample_spot_instrument();
        let trade = BinanceAccountTrade {
            price_exponent: -2,
            qty_exponent: -4,
            commission_exponent: -8,
            id: 123,
            order_id: 456,
            order_list_id: None,
            price_mantissa: 12_345,
            qty_mantissa: 25_000,
            quote_qty_mantissa: 0,
            commission_mantissa: 10_000,
            time: 1_700_000_000_000_000,
            is_buyer: false,
            is_maker: true,
            is_best_match: true,
            symbol: "ETHUSDT".to_string(),
            commission_asset: "USDT".to_string(),
        };
        let ts_init = UnixNanos::from(1_700_000_001_000_000_000u64);

        let report = parse_fill_report_sbe(
            &trade,
            sample_account_id(),
            &instrument,
            Currency::from("USDT"),
            ts_init,
        )
        .unwrap();

        assert_eq!(report.account_id, sample_account_id());
        assert_eq!(report.instrument_id, instrument.id());
        assert_eq!(report.venue_order_id, VenueOrderId::new("456"));
        assert_eq!(report.trade_id, TradeId::new("123"));
        assert_eq!(report.order_side, OrderSide::Sell);
        assert_eq!(report.last_qty.as_f64(), 2.5);
        assert_eq!(report.last_px.as_f64(), 123.45);
        assert_eq!(report.liquidity_side, LiquiditySide::Maker);
        assert_eq!(report.commission.as_f64(), 0.0001);
        assert_eq!(
            report.ts_event,
            UnixNanos::from(1_700_000_000_000_000_000u64)
        );
        assert_eq!(report.ts_init, ts_init);
        assert!(report.client_order_id.is_none());
    }

    #[rstest]
    fn test_parse_klines_to_bars() {
        use nautilus_model::enums::{AggregationSource, PriceType};

        let instrument = sample_spot_instrument();
        let bar_type = BarType::new(
            instrument.id(),
            BarSpecification::new(1, BarAggregation::Minute, PriceType::Last),
            AggregationSource::External,
        );
        let klines = BinanceKlines {
            price_exponent: -2,
            qty_exponent: -4,
            klines: vec![crate::spot::http::models::BinanceKline {
                open_time: 1_700_000_000_000_000,
                open_price: 12_000,
                high_price: 12_500,
                low_price: 11_900,
                close_price: 12_345,
                volume: 1_234_500_i128.to_le_bytes(),
                close_time: 1_700_000_059_999_000,
                quote_volume: 0_i128.to_le_bytes(),
                num_trades: 100,
                taker_buy_base_volume: 0_i128.to_le_bytes(),
                taker_buy_quote_volume: 0_i128.to_le_bytes(),
            }],
        };
        let ts_init = UnixNanos::from(1_700_000_001_000_000_000u64);

        let bars = parse_klines_to_bars(&klines, bar_type, &instrument, ts_init).unwrap();

        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].bar_type, bar_type);
        assert_eq!(bars[0].open, Price::new(120.0, 2));
        assert_eq!(bars[0].high, Price::new(125.0, 2));
        assert_eq!(bars[0].low, Price::new(119.0, 2));
        assert_eq!(bars[0].close, Price::new(123.45, 2));
        assert_eq!(bars[0].volume, Quantity::new(123.45, 4));
        assert_eq!(
            bars[0].ts_event,
            UnixNanos::from(1_700_000_000_000_000_000u64)
        );
        assert_eq!(bars[0].ts_init, ts_init);
    }

    mod bar_spec_tests {
        use std::num::NonZeroUsize;

        use nautilus_model::{
            data::BarSpecification,
            enums::{BarAggregation, PriceType},
        };

        use super::*;
        use crate::common::enums::BinanceKlineInterval;

        fn make_bar_spec(step: usize, aggregation: BarAggregation) -> BarSpecification {
            BarSpecification {
                step: NonZeroUsize::new(step).unwrap(),
                aggregation,
                price_type: PriceType::Last,
            }
        }

        #[rstest]
        #[case(1, BarAggregation::Minute, BinanceKlineInterval::Minute1)]
        #[case(3, BarAggregation::Minute, BinanceKlineInterval::Minute3)]
        #[case(5, BarAggregation::Minute, BinanceKlineInterval::Minute5)]
        #[case(15, BarAggregation::Minute, BinanceKlineInterval::Minute15)]
        #[case(30, BarAggregation::Minute, BinanceKlineInterval::Minute30)]
        #[case(1, BarAggregation::Hour, BinanceKlineInterval::Hour1)]
        #[case(2, BarAggregation::Hour, BinanceKlineInterval::Hour2)]
        #[case(4, BarAggregation::Hour, BinanceKlineInterval::Hour4)]
        #[case(6, BarAggregation::Hour, BinanceKlineInterval::Hour6)]
        #[case(8, BarAggregation::Hour, BinanceKlineInterval::Hour8)]
        #[case(12, BarAggregation::Hour, BinanceKlineInterval::Hour12)]
        #[case(1, BarAggregation::Day, BinanceKlineInterval::Day1)]
        #[case(3, BarAggregation::Day, BinanceKlineInterval::Day3)]
        #[case(1, BarAggregation::Week, BinanceKlineInterval::Week1)]
        #[case(1, BarAggregation::Month, BinanceKlineInterval::Month1)]
        fn test_bar_spec_to_binance_interval(
            #[case] step: usize,
            #[case] aggregation: BarAggregation,
            #[case] expected: BinanceKlineInterval,
        ) {
            let bar_spec = make_bar_spec(step, aggregation);
            let result = bar_spec_to_binance_interval(bar_spec).unwrap();
            assert_eq!(result, expected);
        }

        #[rstest]
        fn test_unsupported_second_interval() {
            let bar_spec = make_bar_spec(1, BarAggregation::Second);
            let result = bar_spec_to_binance_interval(bar_spec);
            assert!(result.is_err());
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("does not support second-level")
            );
        }

        #[rstest]
        fn test_unsupported_minute_interval() {
            let bar_spec = make_bar_spec(7, BarAggregation::Minute);
            let result = bar_spec_to_binance_interval(bar_spec);
            assert!(result.is_err());
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("Unsupported minute interval")
            );
        }

        #[rstest]
        fn test_unsupported_aggregation() {
            let bar_spec = make_bar_spec(100, BarAggregation::Tick);
            let result = bar_spec_to_binance_interval(bar_spec);
            assert!(result.is_err());
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("Unsupported bar aggregation")
            );
        }
    }

    mod sbe_precision_tests {
        use super::*;
        use crate::spot::http::models::{BinanceLotSizeFilterSbe, BinancePriceFilterSbe};

        #[rstest]
        #[case::precision_0(100_000_000, -8, 0)]
        #[case::precision_1(10_000_000, -8, 1)]
        #[case::precision_2(1_000_000, -8, 2)]
        #[case::precision_3(100_000, -8, 3)]
        #[case::precision_4(10_000, -8, 4)]
        #[case::precision_5(1_000, -8, 5)]
        #[case::precision_6(100, -8, 6)]
        #[case::precision_7(10, -8, 7)]
        #[case::precision_8(1, -8, 8)]
        fn test_sbe_mantissa_precision(
            #[case] mantissa: i64,
            #[case] exponent: i8,
            #[case] expected: u8,
        ) {
            let result = sbe_mantissa_precision(mantissa, exponent);
            assert_eq!(
                result, expected,
                "mantissa={mantissa}, exponent={exponent}: expected {expected}, was {result}"
            );
        }

        #[rstest]
        fn test_sbe_mantissa_precision_zero_mantissa() {
            assert_eq!(sbe_mantissa_precision(0, -8), 0);
        }

        #[rstest]
        fn test_sbe_mantissa_precision_positive_exponent() {
            assert_eq!(sbe_mantissa_precision(1, 0), 0);
            assert_eq!(sbe_mantissa_precision(5, 2), 0);
        }

        #[rstest]
        fn test_parse_sbe_price_filter_ethusdc() {
            let filter = BinancePriceFilterSbe {
                price_exponent: -8,
                min_price: 1_000_000,
                max_price: 100_000_000_000_000,
                tick_size: 1_000_000,
            };

            let (tick_size, max_price, min_price) = parse_sbe_price_filter(&filter);

            assert_eq!(tick_size.precision, 2, "tick_size precision");
            assert_eq!(tick_size.as_f64(), 0.01);
            assert_eq!(max_price.unwrap().precision, 2);
            assert_eq!(min_price.unwrap().precision, 2);
        }

        #[rstest]
        fn test_parse_sbe_price_filter_shibusdt() {
            let filter = BinancePriceFilterSbe {
                price_exponent: -8,
                min_price: 1,
                max_price: 100_000_000,
                tick_size: 1,
            };

            let (tick_size, _, _) = parse_sbe_price_filter(&filter);

            assert_eq!(tick_size.precision, 8);
            assert_eq!(tick_size.as_f64(), 0.00000001);
        }

        #[rstest]
        fn test_parse_sbe_lot_size_filter_ethusdc() {
            let filter = BinanceLotSizeFilterSbe {
                qty_exponent: -8,
                min_qty: 10_000,
                max_qty: 900_000_000_000,
                step_size: 10_000,
            };

            let (step_size, max_qty, min_qty) = parse_sbe_lot_size_filter(&filter);

            assert_eq!(step_size.precision, 4, "step_size precision");
            assert_eq!(min_qty.unwrap().precision, 4);
            assert_eq!(max_qty.unwrap().precision, 4);
        }
    }
}
