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
use chrono::DateTime;
use nautilus_core::{datetime::NANOSECONDS_IN_MILLISECOND, nanos::UnixNanos, uuid::UUID4};
use nautilus_model::{
    data::{Bar, BarType, TradeTick},
    enums::{
        AggressorSide, ContingencyType, LiquiditySide, OrderStatus, PositionSideSpecified,
        TimeInForce, TrailingOffsetType,
    },
    identifiers::{AccountId, InstrumentId, Symbol, TradeId, VenueOrderId},
    instruments::{
        Instrument, any::InstrumentAny, crypto_perpetual::CryptoPerpetual,
        currency_pair::CurrencyPair,
    },
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{Currency, Money, Price, Quantity},
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::{
    common::{
        consts::KRAKEN_VENUE,
        enums::{KrakenFillType, KrakenPositionSide},
    },
    http::models::{
        AssetPairInfo, FuturesFill, FuturesInstrument, FuturesOpenOrder, FuturesOrderEvent,
        FuturesPosition, FuturesPublicExecution, OhlcData, SpotOrder, SpotTrade,
    },
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

/// Parse an RFC3339 timestamp string into UnixNanos.
fn parse_rfc3339_timestamp(value: &str, field: &str) -> anyhow::Result<UnixNanos> {
    let dt = DateTime::parse_from_rfc3339(value)
        .with_context(|| format!("Failed to parse {field}='{value}' as RFC3339 timestamp"))?;

    let nanos = dt.timestamp_nanos_opt().ok_or_else(|| {
        anyhow::anyhow!("{field} timestamp overflowed when converting to nanoseconds")
    })?;

    Ok(UnixNanos::from(nanos as u64))
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

/// Parses a Kraken futures instrument definition into a Nautilus crypto perpetual instrument.
///
/// # Errors
///
/// Returns an error if:
/// - Tick size cannot be parsed as a valid price.
/// - Contract size cannot be parsed as a valid quantity.
/// - Currency codes are invalid.
pub fn parse_futures_instrument(
    instrument: &FuturesInstrument,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let instrument_id = InstrumentId::new(Symbol::new(&instrument.symbol), *KRAKEN_VENUE);
    let raw_symbol = Symbol::new(&instrument.symbol);

    let base_currency = get_currency(&instrument.base);
    let quote_currency = get_currency(&instrument.quote);

    let is_inverse = instrument.instrument_type.contains("inverse");
    let settlement_currency = if is_inverse {
        base_currency
    } else {
        quote_currency
    };

    let price_increment = Price::from(instrument.tick_size.to_string());

    // Contract size precision: Kraken futures typically use integer contracts
    let size_precision = if instrument.contract_size.fract() == 0.0 {
        0
    } else {
        instrument
            .contract_size
            .to_string()
            .split('.')
            .nth(1)
            .map_or(0, |s| s.len() as u8)
    };
    let size_increment = Quantity::new(instrument.contract_size, size_precision);

    let multiplier = Some(Quantity::new(instrument.contract_size, size_precision));

    // Use first margin level if available
    let (margin_init, margin_maint) = instrument
        .margin_levels
        .first()
        .and_then(|level| {
            let init = Decimal::try_from(level.initial_margin).ok()?;
            let maint = Decimal::try_from(level.maintenance_margin).ok()?;
            Some((Some(init), Some(maint)))
        })
        .unwrap_or((None, None));

    let instrument = CryptoPerpetual::new(
        instrument_id,
        raw_symbol,
        base_currency,
        quote_currency,
        settlement_currency,
        is_inverse,
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        multiplier,
        None, // lot_size
        None, // max_quantity
        None, // min_quantity
        None, // max_notional
        None, // min_notional
        None, // max_price
        None, // min_price
        margin_init,
        margin_maint,
        None, // maker_fee
        None, // taker_fee
        ts_event,
        ts_init,
    );

    Ok(InstrumentAny::CryptoPerpetual(instrument))
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

/// Parses a Kraken Futures public execution into a Nautilus trade tick.
///
/// # Errors
///
/// Returns an error if:
/// - Price or quantity cannot be parsed.
/// - Trade ID is invalid.
pub fn parse_futures_public_execution(
    execution: &FuturesPublicExecution,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let price =
        parse_price_with_precision(&execution.price, instrument.price_precision(), "price")?;
    let size = parse_quantity_with_precision(
        &execution.quantity,
        instrument.size_precision(),
        "quantity",
    )?;

    // Timestamp is in milliseconds
    let ts_event = UnixNanos::from((execution.timestamp as u64) * 1_000_000);

    // Aggressor side is determined by the taker's direction
    let aggressor = match execution.taker_order.direction.to_lowercase().as_str() {
        "buy" => AggressorSide::Buyer,
        "sell" => AggressorSide::Seller,
        _ => AggressorSide::NoAggressor,
    };

    let trade_id = TradeId::new_checked(&execution.uid)?;

    TradeTick::new_checked(
        instrument.id(),
        price,
        size,
        aggressor,
        trade_id,
        ts_event,
        ts_init,
    )
    .context("Failed to construct TradeTick from Kraken futures execution")
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

/// Parses a Kraken spot order into a Nautilus OrderStatusReport.
///
/// # Errors
///
/// Returns an error if:
/// - Order ID, quantities, or prices cannot be parsed.
/// - Order status mapping fails.
pub fn parse_order_status_report(
    order_id: &str,
    order: &SpotOrder,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    let instrument_id = instrument.id();
    let venue_order_id = VenueOrderId::new(order_id);

    let order_side = order.descr.order_side.into();
    let order_type = order.descr.ordertype.into();
    let order_status = order.status.into();

    let time_in_force = if order.expiretm.is_some() {
        TimeInForce::Gtd
    } else if order.oflags.contains("ioc") {
        TimeInForce::Ioc
    } else {
        TimeInForce::Gtc
    };

    let quantity =
        parse_quantity_with_precision(&order.vol, instrument.size_precision(), "order.vol")?;

    let filled_qty = parse_quantity_with_precision(
        &order.vol_exec,
        instrument.size_precision(),
        "order.vol_exec",
    )?;

    let ts_accepted = parse_millis_timestamp(order.opentm, "order.opentm")?;

    let ts_last = order
        .closetm
        .map(|t| parse_millis_timestamp(t, "order.closetm"))
        .transpose()?
        .unwrap_or(ts_accepted);

    let price = if !order.price.is_empty() && order.price != "0" {
        Some(parse_price_with_precision(
            &order.price,
            instrument.price_precision(),
            "order.price",
        )?)
    } else {
        None
    };

    let trigger_price = order
        .stopprice
        .as_ref()
        .and_then(|p| {
            if !p.is_empty() && p != "0" {
                Some(parse_price_with_precision(
                    p,
                    instrument.price_precision(),
                    "order.stopprice",
                ))
            } else {
                None
            }
        })
        .transpose()?;

    let expire_time = order
        .expiretm
        .map(|t| parse_millis_timestamp(t, "order.expiretm"))
        .transpose()?;

    Ok(OrderStatusReport {
        account_id,
        instrument_id,
        client_order_id: None,
        venue_order_id,
        order_side,
        order_type,
        time_in_force,
        order_status,
        quantity,
        filled_qty,
        report_id: UUID4::new(),
        ts_accepted,
        ts_last,
        ts_init,
        order_list_id: None,
        venue_position_id: None,
        linked_order_ids: None,
        parent_order_id: None,
        contingency_type: ContingencyType::NoContingency,
        expire_time,
        price,
        trigger_price,
        trigger_type: None,
        limit_offset: None,
        trailing_offset: None,
        trailing_offset_type: TrailingOffsetType::NoTrailingOffset,
        display_qty: None,
        avg_px: if !order.cost.is_empty() && !order.vol_exec.is_empty() && order.vol_exec != "0" {
            let cost = parse_decimal(&order.cost)?;
            let vol_exec = parse_decimal(&order.vol_exec)?;
            if vol_exec > dec!(0) {
                Some(cost / vol_exec)
            } else {
                None
            }
        } else {
            None
        },
        post_only: order.oflags.contains("post"),
        reduce_only: false,
        cancel_reason: order.reason.clone(),
        ts_triggered: None,
    })
}

/// Parses a Kraken spot trade into a Nautilus FillReport.
///
/// # Errors
///
/// Returns an error if:
/// - Trade ID, quantities, or prices cannot be parsed.
pub fn parse_fill_report(
    trade_id: &str,
    trade: &SpotTrade,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<FillReport> {
    let instrument_id = instrument.id();
    let venue_order_id = VenueOrderId::new(&trade.ordertxid);
    let trade_id_obj = TradeId::new(trade_id);

    let order_side = trade.trade_type.into();

    let last_qty =
        parse_quantity_with_precision(&trade.vol, instrument.size_precision(), "trade.vol")?;

    let last_px =
        parse_price_with_precision(&trade.price, instrument.price_precision(), "trade.price")?;

    let fee_decimal = parse_decimal(&trade.fee)?;
    let quote_currency = match instrument {
        InstrumentAny::CurrencyPair(pair) => pair.quote_currency,
        InstrumentAny::CryptoPerpetual(perp) => perp.quote_currency,
        _ => anyhow::bail!("Unsupported instrument type for fill report"),
    };

    let fee_f64 = fee_decimal
        .try_into()
        .context("Failed to convert fee to f64")?;
    let commission = Money::new(fee_f64, quote_currency);

    let liquidity_side = match trade.maker {
        Some(true) => LiquiditySide::Maker,
        Some(false) => LiquiditySide::Taker,
        None => LiquiditySide::NoLiquiditySide,
    };

    let ts_event = parse_millis_timestamp(trade.time, "trade.time")?;

    Ok(FillReport {
        account_id,
        instrument_id,
        venue_order_id,
        trade_id: trade_id_obj,
        order_side,
        last_qty,
        last_px,
        commission,
        liquidity_side,
        report_id: UUID4::new(),
        ts_event,
        ts_init,
        client_order_id: None,
        venue_position_id: None,
    })
}

/// Parses a Kraken futures open order into a Nautilus OrderStatusReport.
///
/// # Errors
///
/// Returns an error if order ID, quantities, or prices cannot be parsed.
pub fn parse_futures_order_status_report(
    order: &FuturesOpenOrder,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    let instrument_id = instrument.id();
    let venue_order_id = VenueOrderId::new(&order.order_id);

    let order_side = order.side.into();
    let order_type = order.order_type.into();
    let order_status = order.status.into();

    let quantity = Quantity::new(
        order.unfilled_size + order.filled_size,
        instrument.size_precision(),
    );

    let filled_qty = Quantity::new(order.filled_size, instrument.size_precision());

    let ts_accepted = parse_rfc3339_timestamp(&order.received_time, "order.received_time")?;
    let ts_last = parse_rfc3339_timestamp(&order.last_update_time, "order.last_update_time")?;

    let price = order
        .limit_price
        .map(|p| Price::new(p, instrument.price_precision()));

    let trigger_price = order
        .stop_price
        .map(|p| Price::new(p, instrument.price_precision()));

    Ok(OrderStatusReport {
        account_id,
        instrument_id,
        client_order_id: order.cli_ord_id.as_ref().map(|s| s.as_str().into()),
        venue_order_id,
        order_side,
        order_type,
        time_in_force: TimeInForce::Gtc,
        order_status,
        quantity,
        filled_qty,
        report_id: UUID4::new(),
        ts_accepted,
        ts_last,
        ts_init,
        order_list_id: None,
        venue_position_id: None,
        linked_order_ids: None,
        parent_order_id: None,
        contingency_type: ContingencyType::NoContingency,
        expire_time: None,
        price,
        trigger_price,
        trigger_type: None,
        limit_offset: None,
        trailing_offset: None,
        trailing_offset_type: TrailingOffsetType::NoTrailingOffset,
        display_qty: None,
        avg_px: None,
        post_only: false,
        reduce_only: order.reduce_only.unwrap_or(false),
        cancel_reason: None,
        ts_triggered: None,
    })
}

/// Parses a Kraken futures order event (historical order) into a Nautilus OrderStatusReport.
///
/// # Errors
///
/// Returns an error if order ID, quantities, or prices cannot be parsed.
pub fn parse_futures_order_event_status_report(
    event: &FuturesOrderEvent,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    let instrument_id = instrument.id();
    let venue_order_id = VenueOrderId::new(&event.order_id);

    let order_side = event.side.into();
    let order_type = event.order_type.into();

    // Infer status from filled quantity since historical events don't include explicit status
    let order_status = if event.filled >= event.quantity {
        OrderStatus::Filled
    } else if event.filled > 0.0 {
        OrderStatus::PartiallyFilled
    } else {
        OrderStatus::Canceled
    };

    let quantity = Quantity::new(event.quantity, instrument.size_precision());
    let filled_qty = Quantity::new(event.filled, instrument.size_precision());

    let ts_accepted = parse_rfc3339_timestamp(&event.timestamp, "event.timestamp")?;
    let ts_last =
        parse_rfc3339_timestamp(&event.last_update_timestamp, "event.last_update_timestamp")?;

    let price = event
        .limit_price
        .map(|p| Price::new(p, instrument.price_precision()));

    let trigger_price = event
        .stop_price
        .map(|p| Price::new(p, instrument.price_precision()));

    Ok(OrderStatusReport {
        account_id,
        instrument_id,
        client_order_id: event.cli_ord_id.as_ref().map(|s| s.as_str().into()),
        venue_order_id,
        order_side,
        order_type,
        time_in_force: TimeInForce::Gtc,
        order_status,
        quantity,
        filled_qty,
        report_id: UUID4::new(),
        ts_accepted,
        ts_last,
        ts_init,
        order_list_id: None,
        venue_position_id: None,
        linked_order_ids: None,
        parent_order_id: None,
        contingency_type: ContingencyType::NoContingency,
        expire_time: None,
        price,
        trigger_price,
        trigger_type: None,
        limit_offset: None,
        trailing_offset: None,
        trailing_offset_type: TrailingOffsetType::NoTrailingOffset,
        display_qty: None,
        avg_px: None,
        post_only: false,
        reduce_only: event.reduce_only,
        cancel_reason: None,
        ts_triggered: None,
    })
}

/// Parses a Kraken futures fill into a Nautilus FillReport.
///
/// # Errors
///
/// Returns an error if fill ID, quantities, or prices cannot be parsed.
pub fn parse_futures_fill_report(
    fill: &FuturesFill,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<FillReport> {
    let instrument_id = instrument.id();
    let venue_order_id = VenueOrderId::new(&fill.order_id);
    let trade_id = TradeId::new(&fill.fill_id);

    let order_side = fill.side.into();

    let last_qty = Quantity::new(fill.size, instrument.size_precision());
    let last_px = Price::new(fill.price, instrument.price_precision());

    let quote_currency = match instrument {
        InstrumentAny::CryptoPerpetual(perp) => perp.quote_currency,
        InstrumentAny::CryptoFuture(future) => future.quote_currency,
        _ => anyhow::bail!("Unsupported instrument type for futures fill report"),
    };

    let fee_f64 = fill.fee_paid.unwrap_or(0.0);
    let commission = Money::new(fee_f64, quote_currency);

    let liquidity_side = match fill.fill_type {
        KrakenFillType::Maker => LiquiditySide::Maker,
        KrakenFillType::Taker => LiquiditySide::Taker,
    };

    let ts_event = parse_rfc3339_timestamp(&fill.fill_time, "fill.fill_time")?;

    Ok(FillReport {
        account_id,
        instrument_id,
        venue_order_id,
        trade_id,
        order_side,
        last_qty,
        last_px,
        commission,
        liquidity_side,
        report_id: UUID4::new(),
        ts_event,
        ts_init,
        client_order_id: fill.cli_ord_id.as_ref().map(|s| s.as_str().into()),
        venue_position_id: None,
    })
}

/// Parses a Kraken futures position into a Nautilus PositionStatusReport.
///
/// # Errors
///
/// Returns an error if position quantities or prices cannot be parsed.
pub fn parse_futures_position_status_report(
    position: &FuturesPosition,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<PositionStatusReport> {
    let instrument_id = instrument.id();

    let position_side = match position.side {
        KrakenPositionSide::Long => PositionSideSpecified::Long,
        KrakenPositionSide::Short => PositionSideSpecified::Short,
    };

    let quantity = Quantity::new(position.size, instrument.size_precision());
    let signed_decimal_qty = match position_side {
        PositionSideSpecified::Long => Decimal::from_f64_retain(position.size).unwrap_or(dec!(0)),
        PositionSideSpecified::Short => -Decimal::from_f64_retain(position.size).unwrap_or(dec!(0)),
        PositionSideSpecified::Flat => dec!(0),
    };

    let avg_px_open = Some(Decimal::from_f64_retain(position.price).unwrap_or(dec!(0)));

    Ok(PositionStatusReport {
        account_id,
        instrument_id,
        position_side,
        quantity,
        signed_decimal_qty,
        report_id: UUID4::new(),
        ts_last: ts_init,
        ts_init,
        venue_position_id: None,
        avg_px_open,
    })
}

/// Converts a Nautilus BarType to Kraken Spot API interval (in minutes).
///
/// # Errors
///
/// Returns an error if:
/// - Bar aggregation type is not supported (only Minute, Hour, Day are valid).
/// - Bar step is not supported for the aggregation type.
pub fn bar_type_to_spot_interval(bar_type: BarType) -> anyhow::Result<u32> {
    let step = bar_type.spec().step.get() as u32;
    let base_interval = match bar_type.spec().aggregation {
        nautilus_model::enums::BarAggregation::Minute => 1,
        nautilus_model::enums::BarAggregation::Hour => 60,
        nautilus_model::enums::BarAggregation::Day => 1440,
        other => {
            anyhow::bail!("Unsupported bar aggregation for Kraken Spot: {other:?}");
        }
    };
    Ok(base_interval * step)
}

/// Converts a Nautilus BarType to Kraken Futures API resolution string.
///
/// Supported resolutions: 1m, 5m, 15m, 1h, 4h, 12h, 1d, 1w
///
/// # Errors
///
/// Returns an error if:
/// - Bar aggregation type is not supported.
/// - Bar step is not supported for the aggregation type.
pub fn bar_type_to_futures_resolution(bar_type: BarType) -> anyhow::Result<&'static str> {
    let step = bar_type.spec().step.get() as u32;
    match bar_type.spec().aggregation {
        nautilus_model::enums::BarAggregation::Minute => match step {
            1 => Ok("1m"),
            5 => Ok("5m"),
            15 => Ok("15m"),
            _ => anyhow::bail!("Unsupported minute step for Kraken Futures: {step}"),
        },
        nautilus_model::enums::BarAggregation::Hour => match step {
            1 => Ok("1h"),
            4 => Ok("4h"),
            12 => Ok("12h"),
            _ => anyhow::bail!("Unsupported hour step for Kraken Futures: {step}"),
        },
        nautilus_model::enums::BarAggregation::Day => {
            if step == 1 {
                Ok("1d")
            } else {
                anyhow::bail!("Unsupported day step for Kraken Futures: {step}")
            }
        }
        nautilus_model::enums::BarAggregation::Week => {
            if step == 1 {
                Ok("1w")
            } else {
                anyhow::bail!("Unsupported week step for Kraken Futures: {step}")
            }
        }
        other => {
            anyhow::bail!("Unsupported bar aggregation for Kraken Futures: {other:?}");
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;
    use nautilus_model::{
        data::BarSpecification,
        enums::{AggregationSource, BarAggregation, OrderStatus, PriceType},
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
    fn test_parse_futures_instrument() {
        let json = load_test_json("http_futures_instruments.json");
        let response: crate::http::models::FuturesInstrumentsResponse =
            serde_json::from_str(&json).unwrap();

        let fut_instrument = &response.instruments[0];

        let instrument = parse_futures_instrument(fut_instrument, TS, TS).unwrap();

        match instrument {
            InstrumentAny::CryptoPerpetual(perp) => {
                assert_eq!(perp.id.venue.as_str(), "KRAKEN");
                assert_eq!(perp.id.symbol.as_str(), "PI_XBTUSD");
                assert_eq!(perp.raw_symbol.as_str(), "PI_XBTUSD");
                assert_eq!(perp.base_currency.code.as_str(), "BTC");
                assert_eq!(perp.quote_currency.code.as_str(), "USD");
                assert_eq!(perp.settlement_currency.code.as_str(), "BTC");
                assert!(perp.is_inverse);
                assert_eq!(perp.price_increment.as_f64(), 0.5);
                assert_eq!(perp.size_increment.as_f64(), 1.0);
                assert_eq!(perp.margin_init, dec!(0.02));
                assert_eq!(perp.margin_maint, dec!(0.01));
            }
            _ => panic!("Expected CryptoPerpetual"),
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

    #[rstest]
    #[case(1, BarAggregation::Minute, 1)]
    #[case(5, BarAggregation::Minute, 5)]
    #[case(15, BarAggregation::Minute, 15)]
    #[case(1, BarAggregation::Hour, 60)]
    #[case(4, BarAggregation::Hour, 240)]
    #[case(1, BarAggregation::Day, 1440)]
    fn test_bar_type_to_spot_interval(
        #[case] step: usize,
        #[case] aggregation: BarAggregation,
        #[case] expected: u32,
    ) {
        let instrument_id = InstrumentId::new(Symbol::new("BTC/USD"), *KRAKEN_VENUE);
        let bar_type = BarType::new(
            instrument_id,
            BarSpecification::new(step, aggregation, PriceType::Last),
            AggregationSource::External,
        );

        let result = bar_type_to_spot_interval(bar_type).unwrap();
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_bar_type_to_spot_interval_unsupported() {
        let instrument_id = InstrumentId::new(Symbol::new("BTC/USD"), *KRAKEN_VENUE);
        let bar_type = BarType::new(
            instrument_id,
            BarSpecification::new(1, BarAggregation::Second, PriceType::Last),
            AggregationSource::External,
        );

        let result = bar_type_to_spot_interval(bar_type);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unsupported"));
    }

    #[rstest]
    #[case(1, BarAggregation::Minute, "1m")]
    #[case(5, BarAggregation::Minute, "5m")]
    #[case(15, BarAggregation::Minute, "15m")]
    #[case(1, BarAggregation::Hour, "1h")]
    #[case(4, BarAggregation::Hour, "4h")]
    #[case(12, BarAggregation::Hour, "12h")]
    #[case(1, BarAggregation::Day, "1d")]
    #[case(1, BarAggregation::Week, "1w")]
    fn test_bar_type_to_futures_resolution(
        #[case] step: usize,
        #[case] aggregation: BarAggregation,
        #[case] expected: &str,
    ) {
        let instrument_id = InstrumentId::new(Symbol::new("PI_XBTUSD"), *KRAKEN_VENUE);
        let bar_type = BarType::new(
            instrument_id,
            BarSpecification::new(step, aggregation, PriceType::Last),
            AggregationSource::External,
        );

        let result = bar_type_to_futures_resolution(bar_type).unwrap();
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(30, BarAggregation::Minute)] // Unsupported minute step
    #[case(2, BarAggregation::Hour)] // Unsupported hour step
    #[case(2, BarAggregation::Day)] // Unsupported day step
    #[case(1, BarAggregation::Second)] // Unsupported aggregation
    fn test_bar_type_to_futures_resolution_unsupported(
        #[case] step: usize,
        #[case] aggregation: BarAggregation,
    ) {
        let instrument_id = InstrumentId::new(Symbol::new("PI_XBTUSD"), *KRAKEN_VENUE);
        let bar_type = BarType::new(
            instrument_id,
            BarSpecification::new(step, aggregation, PriceType::Last),
            AggregationSource::External,
        );

        let result = bar_type_to_futures_resolution(bar_type);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unsupported"));
    }

    #[rstest]
    fn test_parse_order_status_report() {
        let json = load_test_json("http_open_orders.json");
        let wrapper: serde_json::Value = serde_json::from_str(&json).unwrap();
        let result = wrapper.get("result").unwrap();
        let open_map = result.get("open").unwrap();
        let orders: IndexMap<String, SpotOrder> = serde_json::from_value(open_map.clone()).unwrap();

        let account_id = AccountId::new("KRAKEN-001");
        let instrument_id = InstrumentId::new(Symbol::new("BTC/USDT"), *KRAKEN_VENUE);
        let instrument = InstrumentAny::CurrencyPair(CurrencyPair::new(
            instrument_id,
            Symbol::new("XBTUSDT"),
            Currency::BTC(),
            Currency::USDT(),
            2,
            8,
            Price::from("0.01"),
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

        let (order_id, order) = orders.iter().next().unwrap();

        let report =
            parse_order_status_report(order_id, order, &instrument, account_id, TS).unwrap();

        assert_eq!(report.account_id, account_id);
        assert_eq!(report.instrument_id, instrument_id);
        assert_eq!(report.venue_order_id.as_str(), order_id);
        assert_eq!(report.order_status, OrderStatus::Accepted);
        assert!(report.quantity.as_f64() > 0.0);
    }

    #[rstest]
    fn test_parse_fill_report() {
        let json = load_test_json("http_trades_history.json");
        let wrapper: serde_json::Value = serde_json::from_str(&json).unwrap();
        let result = wrapper.get("result").unwrap();
        let trades_map = result.get("trades").unwrap();
        let trades: IndexMap<String, SpotTrade> =
            serde_json::from_value(trades_map.clone()).unwrap();

        let account_id = AccountId::new("KRAKEN-001");
        let instrument_id = InstrumentId::new(Symbol::new("BTC/USDT"), *KRAKEN_VENUE);
        let instrument = InstrumentAny::CurrencyPair(CurrencyPair::new(
            instrument_id,
            Symbol::new("XBTUSDT"),
            Currency::BTC(),
            Currency::USDT(),
            2,
            8,
            Price::from("0.01"),
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

        let (trade_id, trade) = trades.iter().next().unwrap();

        let report = parse_fill_report(trade_id, trade, &instrument, account_id, TS).unwrap();

        assert_eq!(report.account_id, account_id);
        assert_eq!(report.instrument_id, instrument_id);
        assert_eq!(report.trade_id.to_string(), *trade_id);
        assert!(report.last_qty.as_f64() > 0.0);
        assert!(report.last_px.as_f64() > 0.0);
        assert!(report.commission.as_f64() > 0.0);
    }
}
