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

use std::str::FromStr;

use nautilus_core::{
    UUID4,
    datetime::{NANOSECONDS_IN_MILLISECOND, millis_to_nanos},
    nanos::UnixNanos,
};
use nautilus_model::{
    currencies::CURRENCY_MAP,
    data::{
        Bar, BarSpecification, BarType, Data, IndexPriceUpdate, MarkPriceUpdate, TradeTick,
        bar::{
            BAR_SPEC_1_DAY_LAST, BAR_SPEC_1_HOUR_LAST, BAR_SPEC_1_MINUTE_LAST,
            BAR_SPEC_1_MONTH_LAST, BAR_SPEC_1_SECOND_LAST, BAR_SPEC_1_WEEK_LAST,
            BAR_SPEC_2_DAY_LAST, BAR_SPEC_2_HOUR_LAST, BAR_SPEC_3_DAY_LAST, BAR_SPEC_3_MINUTE_LAST,
            BAR_SPEC_3_MONTH_LAST, BAR_SPEC_4_HOUR_LAST, BAR_SPEC_5_DAY_LAST,
            BAR_SPEC_5_MINUTE_LAST, BAR_SPEC_6_HOUR_LAST, BAR_SPEC_6_MONTH_LAST,
            BAR_SPEC_12_HOUR_LAST, BAR_SPEC_12_MONTH_LAST, BAR_SPEC_15_MINUTE_LAST,
            BAR_SPEC_30_MINUTE_LAST,
        },
    },
    enums::{
        AccountType, AggressorSide, AssetClass, CurrencyType, LiquiditySide, OptionKind, OrderSide,
        OrderStatus, OrderType, PositionSide, TimeInForce,
    },
    events::AccountState,
    identifiers::{AccountId, ClientOrderId, InstrumentId, Symbol, TradeId, VenueOrderId},
    instruments::{CryptoFuture, CryptoPerpetual, CurrencyPair, InstrumentAny, OptionContract},
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, de::DeserializeOwned};
use ustr::Ustr;

use super::enums::OKXContractType;
use crate::{
    common::{
        consts::OKX_VENUE,
        enums::{OKXExecType, OKXInstrumentType, OKXOrderStatus, OKXOrderType, OKXSide},
        models::OKXInstrument,
    },
    http::models::{
        OKXAccount, OKXCandlestick, OKXOrderHistory, OKXPosition, OKXTrade, OKXTransactionDetail,
    },
    websocket::enums::OKXWsChannel,
};

/// Deserializes empty strings as None for optional fields.
pub fn deserialize_empty_string_as_none<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    Ok(opt.filter(|s| !s.is_empty()))
}

/// Deserializes empty Ustr values as None for optional fields.
pub fn deserialize_empty_ustr_as_none<'de, D>(deserializer: D) -> Result<Option<Ustr>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt = Option::<Ustr>::deserialize(deserializer)?;
    Ok(opt.filter(|s| !s.is_empty()))
}

/// Deserializes string values to u64 integers.
pub fn deserialize_string_to_u64<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    if s.is_empty() {
        Ok(0)
    } else {
        s.parse::<u64>().map_err(serde::de::Error::custom)
    }
}

/// Deserializes optional string values to u64 integers.
pub fn deserialize_optional_string_to_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: Option<String> = Option::deserialize(deserializer)?;
    match s {
        Some(s) if s.is_empty() => Ok(None),
        Some(s) => s.parse().map(Some).map_err(serde::de::Error::custom),
        None => Ok(None),
    }
}

/// Returns the currency either from the internal currency map or creates a default crypto.
fn get_currency(code: &str) -> Currency {
    CURRENCY_MAP
        .lock()
        .unwrap()
        .get(code)
        .copied()
        .unwrap_or(Currency::new(code, 8, 0, code, CurrencyType::Crypto))
}

/// Gets the OKX instrument type for the given instrument.
pub fn okx_instrument_type(instrument: &InstrumentAny) -> anyhow::Result<OKXInstrumentType> {
    match instrument {
        InstrumentAny::CurrencyPair(_) => Ok(OKXInstrumentType::Spot),
        InstrumentAny::CryptoPerpetual(_) => Ok(OKXInstrumentType::Swap),
        InstrumentAny::CryptoFuture(_) => Ok(OKXInstrumentType::Futures),
        InstrumentAny::CryptoOption(_) => Ok(OKXInstrumentType::Option),
        _ => anyhow::bail!("Invalid instrument type for OKX: {instrument:?}"),
    }
}

/// Parses a Nautilus instrument ID from the given OKX `symbol` value.
#[must_use]
pub fn parse_instrument_id(symbol: Ustr) -> InstrumentId {
    InstrumentId::new(Symbol::from_ustr_unchecked(symbol), *OKX_VENUE)
}

/// Parses a Nautilus client order ID from the given OKX `clOrdId` value.
#[must_use]
pub fn parse_client_order_id(value: &str) -> Option<ClientOrderId> {
    if value.is_empty() {
        None
    } else {
        Some(ClientOrderId::new(value))
    }
}

pub fn parse_millisecond_timestamp(timestamp_ms: u64) -> UnixNanos {
    UnixNanos::from(timestamp_ms * NANOSECONDS_IN_MILLISECOND)
}

pub fn parse_rfc3339_timestamp(timestamp: &str) -> anyhow::Result<UnixNanos> {
    let dt = chrono::DateTime::parse_from_rfc3339(timestamp)?;
    let nanos = dt.timestamp_nanos_opt().ok_or_else(|| {
        anyhow::anyhow!("Failed to extract nanoseconds from timestamp: {timestamp}")
    })?;
    Ok(UnixNanos::from(nanos as u64))
}

pub fn parse_price(value: &str, precision: u8) -> anyhow::Result<Price> {
    Price::new_checked(value.parse::<f64>()?, precision)
}

pub fn parse_quantity(value: &str, precision: u8) -> anyhow::Result<Quantity> {
    Quantity::new_checked(value.parse::<f64>()?, precision)
}

pub fn parse_fee(value: Option<&str>, currency: Currency) -> anyhow::Result<Money> {
    // OKX report positive fees with negative signs (i.e., fee charged)
    let fee_f64 = value.unwrap_or("0").parse::<f64>()?;
    Money::new_checked(-fee_f64, currency)
}

/// Parses OKX side to Nautilus aggressor side.
pub fn parse_aggressor_side(side: &Option<OKXSide>) -> AggressorSide {
    match side {
        Some(OKXSide::Buy) => nautilus_model::enums::AggressorSide::Buyer,
        Some(OKXSide::Sell) => nautilus_model::enums::AggressorSide::Seller,
        None => nautilus_model::enums::AggressorSide::NoAggressor,
    }
}

/// Parses OKX execution type to Nautilus liquidity side.
pub fn parse_execution_type(liquidity: &Option<OKXExecType>) -> LiquiditySide {
    match liquidity {
        Some(OKXExecType::Maker) => nautilus_model::enums::LiquiditySide::Maker,
        Some(OKXExecType::Taker) => nautilus_model::enums::LiquiditySide::Taker,
        _ => nautilus_model::enums::LiquiditySide::NoLiquiditySide,
    }
}

/// Parses quantity to Nautilus position side.
pub fn parse_position_side(current_qty: Option<i64>) -> PositionSide {
    match current_qty {
        Some(qty) if qty > 0 => PositionSide::Long,
        Some(qty) if qty < 0 => PositionSide::Short,
        _ => PositionSide::Flat,
    }
}

/// Parses OKX side to Nautilus order side.
pub fn parse_order_side(order_side: &Option<OKXSide>) -> OrderSide {
    match order_side {
        Some(OKXSide::Buy) => OrderSide::Buy,
        Some(OKXSide::Sell) => OrderSide::Sell,
        None => OrderSide::NoOrderSide,
    }
}

/// Parses an OKX mark price record into a Nautilus [`MarkPriceUpdate`].
pub fn parse_mark_price_update(
    raw: &crate::http::models::OKXMarkPrice,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<MarkPriceUpdate> {
    let ts_event = parse_millisecond_timestamp(raw.ts);
    let price = parse_price(&raw.mark_px, price_precision)?;
    Ok(MarkPriceUpdate::new(
        instrument_id,
        price,
        ts_event,
        ts_init,
    ))
}

/// Parses an OKX index ticker record into a Nautilus [`IndexPriceUpdate`].
pub fn parse_index_price_update(
    raw: &crate::http::models::OKXIndexTicker,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<IndexPriceUpdate> {
    let ts_event = parse_millisecond_timestamp(raw.ts);
    let price = parse_price(&raw.idx_px, price_precision)?;
    Ok(IndexPriceUpdate::new(
        instrument_id,
        price,
        ts_event,
        ts_init,
    ))
}

/// Parses an OKX trade record into a Nautilus [`TradeTick`].
pub fn parse_trade_tick(
    raw: &OKXTrade,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    // Parse event timestamp
    let ts_event = parse_millisecond_timestamp(raw.ts);
    let price = parse_price(&raw.px, price_precision)?;
    let size = parse_quantity(&raw.sz, size_precision)?;
    let aggressor = AggressorSide::from(raw.side.clone());
    let trade_id = TradeId::new(raw.trade_id);

    TradeTick::new_checked(
        instrument_id,
        price,
        size,
        aggressor,
        trade_id,
        ts_event,
        ts_init,
    )
}

/// Parses an OKX historical candlestick record into a Nautilus [`Bar`].
pub fn parse_candlestick(
    raw: &OKXCandlestick,
    bar_type: BarType,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<Bar> {
    let ts_event = parse_millisecond_timestamp(raw.0.parse()?);
    let open = parse_price(&raw.1, price_precision)?;
    let high = parse_price(&raw.2, price_precision)?;
    let low = parse_price(&raw.3, price_precision)?;
    let close = parse_price(&raw.4, price_precision)?;
    let volume = parse_quantity(&raw.5, size_precision)?;

    Ok(Bar::new(
        bar_type, open, high, low, close, volume, ts_event, ts_init,
    ))
}

/// Parses an OKX order history record into a Nautilus [`OrderStatusReport`].
#[allow(clippy::too_many_lines)]
pub fn parse_order_status_report(
    order: OKXOrderHistory,
    account_id: AccountId,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> OrderStatusReport {
    let quantity = order
        .sz
        .parse::<f64>()
        .ok()
        .map(|v| Quantity::new(v, size_precision))
        .unwrap_or_default();
    let filled_qty = order
        .acc_fill_sz
        .parse::<f64>()
        .ok()
        .map(|v| Quantity::new(v, size_precision))
        .unwrap_or_default();
    let order_side: OrderSide = order.side.into();
    let okx_status: OKXOrderStatus = match order.state.as_str() {
        "live" => OKXOrderStatus::Live,
        "partially_filled" => OKXOrderStatus::PartiallyFilled,
        "filled" => OKXOrderStatus::Filled,
        "canceled" => OKXOrderStatus::Canceled,
        "mmp_canceled" => OKXOrderStatus::MmpCanceled,
        _ => OKXOrderStatus::Live, // Default fallback
    };
    let order_status: OrderStatus = okx_status.into();
    let okx_ord_type: OKXOrderType = match order.ord_type.as_str() {
        "market" => OKXOrderType::Market,
        "limit" => OKXOrderType::Limit,
        "post_only" => OKXOrderType::PostOnly,
        "fok" => OKXOrderType::Fok,
        "ioc" => OKXOrderType::Ioc,
        "optimal_limit_ioc" => OKXOrderType::OptimalLimitIoc,
        "mmp" => OKXOrderType::Mmp,
        "mmp_and_post_only" => OKXOrderType::MmpAndPostOnly,
        _ => OKXOrderType::Limit, // Default fallback
    };
    let order_type: OrderType = okx_ord_type.into();
    // Note: OKX uses ordType for type and liquidity instructions; time-in-force not explicitly represented here
    let time_in_force = TimeInForce::Gtc;

    // Build report
    let client_ord = if order.cl_ord_id.is_empty() {
        None
    } else {
        Some(ClientOrderId::new(order.cl_ord_id))
    };

    let ts_accepted = parse_millisecond_timestamp(order.c_time);
    let ts_last = UnixNanos::from(order.u_time * NANOSECONDS_IN_MILLISECOND);

    let mut report = OrderStatusReport::new(
        account_id,
        instrument_id,
        client_ord,
        VenueOrderId::new(order.ord_id),
        order_side,
        order_type,
        time_in_force,
        order_status,
        quantity,
        filled_qty,
        ts_accepted,
        ts_last,
        ts_init,
        None,
    );

    // Optional fields
    if !order.px.is_empty() {
        if let Ok(p) = order.px.parse::<f64>() {
            report = report.with_price(Price::new(p, price_precision));
        }
    }
    if !order.avg_px.is_empty() {
        if let Ok(avg) = order.avg_px.parse::<f64>() {
            report = report.with_avg_px(avg);
        }
    }
    if order.ord_type == "post_only" {
        report = report.with_post_only(true);
    }
    if order.reduce_only == "true" {
        report = report.with_reduce_only(true);
    }
    report
}

/// Parses an OKX position into a Nautilus [`PositionStatusReport`].
#[allow(clippy::too_many_lines)]
pub fn parse_position_status_report(
    position: OKXPosition,
    account_id: AccountId,
    instrument_id: InstrumentId,
    size_precision: u8,
    ts_init: UnixNanos,
) -> PositionStatusReport {
    let position_side: PositionSide = position.pos_side.into();
    let quantity = position
        .pos
        .parse::<f64>()
        .ok()
        .map(|v| Quantity::new(v, size_precision))
        .unwrap_or_default();
    let venue_position_id = None; // TODO: Only support netting for now
    // let venue_position_id = Some(PositionId::new(position.pos_id));
    // TODO: Standardize timestamp parsing (deserialize model fields)
    let ts_last = UnixNanos::from(position.u_time * NANOSECONDS_IN_MILLISECOND);

    PositionStatusReport::new(
        account_id,
        instrument_id,
        position_side,
        quantity,
        venue_position_id,
        ts_last,
        ts_init,
        None,
    )
}

/// Parses an OKX transaction detail into a Nautilus `FillReport`.
///
/// # Errors
///
/// This function will return an error if the OKX transaction detail cannot be parsed.
pub fn parse_fill_report(
    detail: OKXTransactionDetail,
    account_id: AccountId,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<FillReport> {
    let client_order_id = if detail.cl_ord_id.is_empty() {
        None
    } else {
        Some(ClientOrderId::new(detail.cl_ord_id))
    };
    let venue_order_id = VenueOrderId::new(detail.ord_id);
    let trade_id = TradeId::new(detail.trade_id);
    let order_side = parse_order_side(&Some(detail.side.clone()));
    let last_px = parse_price(&detail.fill_px, price_precision)?;
    let last_qty = parse_quantity(&detail.fill_sz, size_precision)?;
    let fee_f64 = detail.fee.as_deref().unwrap_or("0").parse::<f64>()?;
    let commission = Money::new(-fee_f64, Currency::from(&detail.fee_ccy));
    let liquidity_side: LiquiditySide = LiquiditySide::from(detail.exec_type.clone());
    let ts_event = parse_millisecond_timestamp(detail.ts);

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
        None, // venue_position_id not provided by OKX fills
        ts_event,
        ts_init,
        None, // Will generate a new UUID4
    ))
}

/// Parses vector messages from OKX WebSocket data.
///
/// Reduces code duplication by providing a common pattern for deserializing JSON arrays,
/// parsing each message, and wrapping results in Nautilus Data enum variants.
pub fn parse_message_vec<T, R, F, W>(
    data: serde_json::Value,
    parser: F,
    wrapper: W,
) -> anyhow::Result<Vec<Data>>
where
    T: DeserializeOwned,
    F: Fn(&T) -> anyhow::Result<R>,
    W: Fn(R) -> Data,
{
    let msgs: Vec<T> = serde_json::from_value(data)?;
    let mut results = Vec::with_capacity(msgs.len());

    for msg in msgs {
        let parsed = parser(&msg)?;
        results.push(wrapper(parsed));
    }

    Ok(results)
}

pub fn bar_spec_as_okx_channel(bar_spec: BarSpecification) -> anyhow::Result<OKXWsChannel> {
    let channel = match bar_spec {
        BAR_SPEC_1_SECOND_LAST => OKXWsChannel::Candle1Second,
        BAR_SPEC_1_MINUTE_LAST => OKXWsChannel::Candle1Minute,
        BAR_SPEC_3_MINUTE_LAST => OKXWsChannel::Candle3Minute,
        BAR_SPEC_5_MINUTE_LAST => OKXWsChannel::Candle5Minute,
        BAR_SPEC_15_MINUTE_LAST => OKXWsChannel::Candle15Minute,
        BAR_SPEC_30_MINUTE_LAST => OKXWsChannel::Candle30Minute,
        BAR_SPEC_1_HOUR_LAST => OKXWsChannel::Candle1Hour,
        BAR_SPEC_2_HOUR_LAST => OKXWsChannel::Candle2Hour,
        BAR_SPEC_4_HOUR_LAST => OKXWsChannel::Candle4Hour,
        BAR_SPEC_6_HOUR_LAST => OKXWsChannel::Candle6Hour,
        BAR_SPEC_12_HOUR_LAST => OKXWsChannel::Candle12Hour,
        BAR_SPEC_1_DAY_LAST => OKXWsChannel::Candle1Day,
        BAR_SPEC_2_DAY_LAST => OKXWsChannel::Candle2Day,
        BAR_SPEC_3_DAY_LAST => OKXWsChannel::Candle3Day,
        BAR_SPEC_5_DAY_LAST => OKXWsChannel::Candle5Day,
        BAR_SPEC_1_WEEK_LAST => OKXWsChannel::Candle1Week,
        BAR_SPEC_1_MONTH_LAST => OKXWsChannel::Candle1Month,
        BAR_SPEC_3_MONTH_LAST => OKXWsChannel::Candle3Month,
        BAR_SPEC_6_MONTH_LAST => OKXWsChannel::Candle6Month,
        BAR_SPEC_12_MONTH_LAST => OKXWsChannel::Candle1Year,
        _ => anyhow::bail!("Invalid `BarSpecification` for channel, was {bar_spec}"),
    };
    Ok(channel)
}

/// Converts Nautilus bar specification to OKX mark price channel.
pub fn bar_spec_as_okx_mark_price_channel(
    bar_spec: BarSpecification,
) -> anyhow::Result<OKXWsChannel> {
    let channel = match bar_spec {
        BAR_SPEC_1_SECOND_LAST => OKXWsChannel::MarkPriceCandle1Second,
        BAR_SPEC_1_MINUTE_LAST => OKXWsChannel::MarkPriceCandle1Minute,
        BAR_SPEC_3_MINUTE_LAST => OKXWsChannel::MarkPriceCandle3Minute,
        BAR_SPEC_5_MINUTE_LAST => OKXWsChannel::MarkPriceCandle5Minute,
        BAR_SPEC_15_MINUTE_LAST => OKXWsChannel::MarkPriceCandle15Minute,
        BAR_SPEC_30_MINUTE_LAST => OKXWsChannel::MarkPriceCandle30Minute,
        BAR_SPEC_1_HOUR_LAST => OKXWsChannel::MarkPriceCandle1Hour,
        BAR_SPEC_2_HOUR_LAST => OKXWsChannel::MarkPriceCandle2Hour,
        BAR_SPEC_4_HOUR_LAST => OKXWsChannel::MarkPriceCandle4Hour,
        BAR_SPEC_6_HOUR_LAST => OKXWsChannel::MarkPriceCandle6Hour,
        BAR_SPEC_12_HOUR_LAST => OKXWsChannel::MarkPriceCandle12Hour,
        BAR_SPEC_1_DAY_LAST => OKXWsChannel::MarkPriceCandle1Day,
        BAR_SPEC_2_DAY_LAST => OKXWsChannel::MarkPriceCandle2Day,
        BAR_SPEC_3_DAY_LAST => OKXWsChannel::MarkPriceCandle3Day,
        BAR_SPEC_5_DAY_LAST => OKXWsChannel::MarkPriceCandle5Day,
        BAR_SPEC_1_WEEK_LAST => OKXWsChannel::MarkPriceCandle1Week,
        BAR_SPEC_1_MONTH_LAST => OKXWsChannel::MarkPriceCandle1Month,
        BAR_SPEC_3_MONTH_LAST => OKXWsChannel::MarkPriceCandle3Month,
        _ => anyhow::bail!("Invalid `BarSpecification` for mark price channel, was {bar_spec}"),
    };
    Ok(channel)
}

/// Converts Nautilus bar specification to OKX timeframe string.
pub fn bar_spec_as_okx_timeframe(bar_spec: BarSpecification) -> anyhow::Result<&'static str> {
    let timeframe = match bar_spec {
        BAR_SPEC_1_SECOND_LAST => "1s",
        BAR_SPEC_1_MINUTE_LAST => "1m",
        BAR_SPEC_3_MINUTE_LAST => "3m",
        BAR_SPEC_5_MINUTE_LAST => "5m",
        BAR_SPEC_15_MINUTE_LAST => "15m",
        BAR_SPEC_30_MINUTE_LAST => "30m",
        BAR_SPEC_1_HOUR_LAST => "1H",
        BAR_SPEC_2_HOUR_LAST => "2H",
        BAR_SPEC_4_HOUR_LAST => "4H",
        BAR_SPEC_6_HOUR_LAST => "6H",
        BAR_SPEC_12_HOUR_LAST => "12H",
        BAR_SPEC_1_DAY_LAST => "1D",
        BAR_SPEC_2_DAY_LAST => "2D",
        BAR_SPEC_3_DAY_LAST => "3D",
        BAR_SPEC_5_DAY_LAST => "5D",
        BAR_SPEC_1_WEEK_LAST => "1W",
        BAR_SPEC_1_MONTH_LAST => "1M",
        BAR_SPEC_3_MONTH_LAST => "3M",
        BAR_SPEC_6_MONTH_LAST => "6M",
        BAR_SPEC_12_MONTH_LAST => "1Y",
        _ => anyhow::bail!("Invalid `BarSpecification` for timeframe, was {bar_spec}"),
    };
    Ok(timeframe)
}

/// Converts OKX timeframe string to Nautilus bar specification.
pub fn okx_timeframe_as_bar_spec(timeframe: &str) -> anyhow::Result<BarSpecification> {
    let bar_spec = match timeframe {
        "1s" => BAR_SPEC_1_SECOND_LAST,
        "1m" => BAR_SPEC_1_MINUTE_LAST,
        "3m" => BAR_SPEC_3_MINUTE_LAST,
        "5m" => BAR_SPEC_5_MINUTE_LAST,
        "15m" => BAR_SPEC_15_MINUTE_LAST,
        "30m" => BAR_SPEC_30_MINUTE_LAST,
        "1H" => BAR_SPEC_1_HOUR_LAST,
        "2H" => BAR_SPEC_2_HOUR_LAST,
        "4H" => BAR_SPEC_4_HOUR_LAST,
        "6H" => BAR_SPEC_6_HOUR_LAST,
        "12H" => BAR_SPEC_12_HOUR_LAST,
        "1D" => BAR_SPEC_1_DAY_LAST,
        "2D" => BAR_SPEC_2_DAY_LAST,
        "3D" => BAR_SPEC_3_DAY_LAST,
        "5D" => BAR_SPEC_5_DAY_LAST,
        "1W" => BAR_SPEC_1_WEEK_LAST,
        "1M" => BAR_SPEC_1_MONTH_LAST,
        "3M" => BAR_SPEC_3_MONTH_LAST,
        "6M" => BAR_SPEC_6_MONTH_LAST,
        "1Y" => BAR_SPEC_12_MONTH_LAST,
        _ => anyhow::bail!("Invalid timeframe for `BarSpecification`, was {timeframe}"),
    };
    Ok(bar_spec)
}

/// Converts OKX WebSocket channel to bar specification if it's a candle channel.
pub fn okx_channel_to_bar_spec(channel: &OKXWsChannel) -> Option<BarSpecification> {
    use OKXWsChannel::*;
    match channel {
        Candle1Second | MarkPriceCandle1Second => Some(BAR_SPEC_1_SECOND_LAST),
        Candle1Minute | MarkPriceCandle1Minute => Some(BAR_SPEC_1_MINUTE_LAST),
        Candle3Minute | MarkPriceCandle3Minute => Some(BAR_SPEC_3_MINUTE_LAST),
        Candle5Minute | MarkPriceCandle5Minute => Some(BAR_SPEC_5_MINUTE_LAST),
        Candle15Minute | MarkPriceCandle15Minute => Some(BAR_SPEC_15_MINUTE_LAST),
        Candle30Minute | MarkPriceCandle30Minute => Some(BAR_SPEC_30_MINUTE_LAST),
        Candle1Hour | MarkPriceCandle1Hour => Some(BAR_SPEC_1_HOUR_LAST),
        Candle2Hour | MarkPriceCandle2Hour => Some(BAR_SPEC_2_HOUR_LAST),
        Candle4Hour | MarkPriceCandle4Hour => Some(BAR_SPEC_4_HOUR_LAST),
        Candle6Hour | MarkPriceCandle6Hour => Some(BAR_SPEC_6_HOUR_LAST),
        Candle12Hour | MarkPriceCandle12Hour => Some(BAR_SPEC_12_HOUR_LAST),
        Candle1Day | MarkPriceCandle1Day => Some(BAR_SPEC_1_DAY_LAST),
        Candle2Day | MarkPriceCandle2Day => Some(BAR_SPEC_2_DAY_LAST),
        Candle3Day | MarkPriceCandle3Day => Some(BAR_SPEC_3_DAY_LAST),
        Candle5Day | MarkPriceCandle5Day => Some(BAR_SPEC_5_DAY_LAST),
        Candle1Week | MarkPriceCandle1Week => Some(BAR_SPEC_1_WEEK_LAST),
        Candle1Month | MarkPriceCandle1Month => Some(BAR_SPEC_1_MONTH_LAST),
        Candle3Month | MarkPriceCandle3Month => Some(BAR_SPEC_3_MONTH_LAST),
        Candle6Month => Some(BAR_SPEC_6_MONTH_LAST),
        Candle1Year => Some(BAR_SPEC_12_MONTH_LAST),
        _ => None,
    }
}

/// Parses an OKX instrument definition into a Nautilus instrument.
pub fn parse_instrument_any(
    instrument: &OKXInstrument,
    ts_init: UnixNanos,
) -> anyhow::Result<Option<InstrumentAny>> {
    match instrument.inst_type {
        OKXInstrumentType::Spot => {
            parse_spot_instrument(instrument, None, None, None, None, ts_init).map(Some)
        }
        OKXInstrumentType::Swap => {
            parse_swap_instrument(instrument, None, None, None, None, ts_init).map(Some)
        }
        OKXInstrumentType::Futures => {
            parse_futures_instrument(instrument, None, None, None, None, ts_init).map(Some)
        }
        OKXInstrumentType::Option => {
            parse_option_instrument(instrument, None, None, None, None, ts_init).map(Some)
        }
        _ => Ok(None),
    }
}

/// Common parsed instrument data extracted from OKX definitions.
#[derive(Debug)]
struct CommonInstrumentData {
    instrument_id: InstrumentId,
    raw_symbol: Symbol,
    price_increment: Price,
    size_increment: Quantity,
    lot_size: Option<Quantity>,
    max_quantity: Option<Quantity>,
    min_quantity: Option<Quantity>,
    max_notional: Option<Money>,
    min_notional: Option<Money>,
    max_price: Option<Price>,
    min_price: Option<Price>,
}

/// Margin and fee configuration for an instrument.
struct MarginAndFees {
    margin_init: Option<Decimal>,
    margin_maint: Option<Decimal>,
    maker_fee: Option<Decimal>,
    taker_fee: Option<Decimal>,
}

/// Trait for instrument-specific parsing logic.
trait InstrumentParser {
    /// Parses instrument-specific fields and creates the final instrument.
    fn parse_specific_fields(
        &self,
        definition: &OKXInstrument,
        common: CommonInstrumentData,
        margin_fees: MarginAndFees,
        ts_init: UnixNanos,
    ) -> anyhow::Result<InstrumentAny>;
}

/// Extracts common fields shared across all instrument types.
fn parse_common_instrument_data(
    definition: &OKXInstrument,
) -> anyhow::Result<CommonInstrumentData> {
    let instrument_id = parse_instrument_id(definition.inst_id);
    let raw_symbol = Symbol::from_ustr_unchecked(definition.inst_id);

    let price_increment = Price::from_str(&definition.tick_sz).map_err(|e| {
        anyhow::anyhow!(
            "Failed to parse tick_sz '{}' into Price: {}",
            definition.tick_sz,
            e
        )
    })?;

    let size_increment = Quantity::from(&definition.lot_sz);
    let lot_size = Some(Quantity::from(&definition.lot_sz));
    let max_quantity = Some(Quantity::from(&definition.max_mkt_sz));
    let min_quantity = Some(Quantity::from(&definition.min_sz));
    let max_notional: Option<Money> = None;
    let min_notional: Option<Money> = None;
    let max_price = None; // TBD
    let min_price = None; // TBD

    Ok(CommonInstrumentData {
        instrument_id,
        raw_symbol,
        price_increment,
        size_increment,
        lot_size,
        max_quantity,
        min_quantity,
        max_notional,
        min_notional,
        max_price,
        min_price,
    })
}

/// Generic instrument parsing function that delegates to type-specific parsers.
fn parse_instrument_with_parser<P: InstrumentParser>(
    definition: &OKXInstrument,
    parser: P,
    margin_init: Option<Decimal>,
    margin_maint: Option<Decimal>,
    maker_fee: Option<Decimal>,
    taker_fee: Option<Decimal>,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let common = parse_common_instrument_data(definition)?;
    parser.parse_specific_fields(
        definition,
        common,
        MarginAndFees {
            margin_init,
            margin_maint,
            maker_fee,
            taker_fee,
        },
        ts_init,
    )
}

/// Parser for spot trading pairs (CurrencyPair).
struct SpotInstrumentParser;

impl InstrumentParser for SpotInstrumentParser {
    fn parse_specific_fields(
        &self,
        definition: &OKXInstrument,
        common: CommonInstrumentData,
        margin_fees: MarginAndFees,
        ts_init: UnixNanos,
    ) -> anyhow::Result<InstrumentAny> {
        let base_currency = get_currency(&definition.base_ccy);
        let quote_currency = get_currency(&definition.quote_ccy);

        let instrument = CurrencyPair::new(
            common.instrument_id,
            common.raw_symbol,
            base_currency,
            quote_currency,
            common.price_increment.precision,
            common.size_increment.precision,
            common.price_increment,
            common.size_increment,
            common.lot_size,
            common.max_quantity,
            common.min_quantity,
            common.max_notional,
            common.min_notional,
            common.max_price,
            common.min_price,
            margin_fees.margin_init,
            margin_fees.margin_maint,
            margin_fees.maker_fee,
            margin_fees.taker_fee,
            ts_init,
            ts_init,
        );

        Ok(InstrumentAny::CurrencyPair(instrument))
    }
}

/// Parses an OKX spot instrument definition into a Nautilus currency pair.
pub fn parse_spot_instrument(
    definition: &OKXInstrument,
    margin_init: Option<Decimal>,
    margin_maint: Option<Decimal>,
    maker_fee: Option<Decimal>,
    taker_fee: Option<Decimal>,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    parse_instrument_with_parser(
        definition,
        SpotInstrumentParser,
        margin_init,
        margin_maint,
        maker_fee,
        taker_fee,
        ts_init,
    )
}

/// Parses an OKX swap instrument definition into a Nautilus crypto perpetual.
pub fn parse_swap_instrument(
    definition: &OKXInstrument,
    margin_init: Option<Decimal>,
    margin_maint: Option<Decimal>,
    maker_fee: Option<Decimal>,
    taker_fee: Option<Decimal>,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let instrument_id = parse_instrument_id(definition.inst_id);
    let raw_symbol = Symbol::from_ustr_unchecked(definition.inst_id);
    let (base_currency, quote_currency) = definition
        .uly
        .split_once('-')
        .ok_or_else(|| anyhow::anyhow!("Invalid underlying for swap: {}", definition.uly))?;
    let base_currency = get_currency(base_currency);
    let quote_currency = get_currency(quote_currency);
    let settlement_currency = get_currency(&definition.settle_ccy);
    let is_inverse = match definition.ct_type {
        OKXContractType::Linear => false,
        OKXContractType::Inverse => true,
        OKXContractType::None => {
            anyhow::bail!("Invalid contract type for swap: {}", definition.ct_type)
        }
    };
    let price_increment = match Price::from_str(&definition.tick_sz) {
        Ok(price) => price,
        Err(e) => {
            anyhow::bail!(
                "Failed to parse tick_size '{}' into Price: {}",
                definition.tick_sz,
                e
            );
        }
    };
    let size_increment = Quantity::from(&definition.lot_sz);
    let multiplier = Some(Quantity::from(&definition.ct_mult));
    let lot_size = Some(Quantity::from(&definition.lot_sz));
    let max_quantity = Some(Quantity::from(&definition.max_mkt_sz));
    let min_quantity = Some(Quantity::from(&definition.min_sz));
    let max_notional: Option<Money> = None;
    let min_notional: Option<Money> = None;
    let max_price = None; // TBD
    let min_price = None; // TBD

    // For linear swaps (USDT-margined), trades are in base currency units (e.g., BTC)
    // For inverse swaps (coin-margined), trades are in contract units
    // The lotSz represents minimum contract size, but actual trade sizes for linear swaps
    // are in fractional base currency amounts requiring higher precision
    let (size_precision, adjusted_size_increment) = if is_inverse {
        // For inverse swaps, use the lot size precision as trades are in contract units
        (size_increment.precision, size_increment)
    } else {
        // For linear swaps, use base currency precision (typically 8 for crypto)
        // and adjust the size increment to match this precision
        let precision = 8u8;
        let adjusted_increment = Quantity::new(1.0, precision); // Minimum trade size of 0.00000001
        (precision, adjusted_increment)
    };

    let instrument = CryptoPerpetual::new(
        instrument_id,
        raw_symbol,
        base_currency,
        quote_currency,
        settlement_currency,
        is_inverse,
        price_increment.precision,
        size_precision,
        price_increment,
        adjusted_size_increment,
        multiplier,
        lot_size,
        max_quantity,
        min_quantity,
        max_notional,
        min_notional,
        max_price,
        min_price,
        margin_init,
        margin_maint,
        maker_fee,
        taker_fee,
        ts_init, // No ts_event for response
        ts_init,
    );

    Ok(InstrumentAny::CryptoPerpetual(instrument))
}

/// Parses an OKX futures instrument definition into a Nautilus crypto future.
pub fn parse_futures_instrument(
    definition: &OKXInstrument,
    margin_init: Option<Decimal>,
    margin_maint: Option<Decimal>,
    maker_fee: Option<Decimal>,
    taker_fee: Option<Decimal>,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let instrument_id = parse_instrument_id(definition.inst_id);
    let raw_symbol = Symbol::from_ustr_unchecked(definition.inst_id);
    let underlying = get_currency(&definition.uly);
    let (_, quote_currency) = definition
        .uly
        .split_once('-')
        .ok_or_else(|| anyhow::anyhow!("Invalid underlying for Swap: {}", definition.uly))?;
    let quote_currency = get_currency(quote_currency);
    let settlement_currency = get_currency(&definition.settle_ccy);
    let is_inverse = match definition.ct_type {
        OKXContractType::Linear => false,
        OKXContractType::Inverse => true,
        OKXContractType::None => {
            anyhow::bail!("Invalid contract type for futures: {}", definition.ct_type)
        }
    };
    let listing_time = definition
        .list_time
        .ok_or_else(|| anyhow::anyhow!("`listing_time` is required to parse Swap instrument"))?;
    let expiry_time = definition
        .exp_time
        .ok_or_else(|| anyhow::anyhow!("`expiry_time` is required to parse Swap instrument"))?;
    let activation_ns = UnixNanos::from(millis_to_nanos(listing_time as f64));
    let expiration_ns = UnixNanos::from(millis_to_nanos(expiry_time as f64));
    let price_increment = Price::from(definition.tick_sz.to_string());
    let size_increment = Quantity::from(&definition.lot_sz);
    let multiplier = Some(Quantity::from(&definition.ct_mult));
    let lot_size = Some(Quantity::from(&definition.lot_sz));
    let max_quantity = Some(Quantity::from(&definition.max_mkt_sz));
    let min_quantity = Some(Quantity::from(&definition.min_sz));
    let max_notional: Option<Money> = None;
    let min_notional: Option<Money> = None;
    let max_price = None; // TBD
    let min_price = None; // TBD

    let instrument = CryptoFuture::new(
        instrument_id,
        raw_symbol,
        underlying,
        quote_currency,
        settlement_currency,
        is_inverse,
        activation_ns,
        expiration_ns,
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        multiplier,
        lot_size,
        max_quantity,
        min_quantity,
        max_notional,
        min_notional,
        max_price,
        min_price,
        margin_init,
        margin_maint,
        maker_fee,
        taker_fee,
        ts_init, // No ts_event for response
        ts_init,
    );

    Ok(InstrumentAny::CryptoFuture(instrument))
}

/// Parses an OKX option instrument definition into a Nautilus option contract.
pub fn parse_option_instrument(
    definition: &OKXInstrument,
    margin_init: Option<Decimal>,
    margin_maint: Option<Decimal>,
    maker_fee: Option<Decimal>,
    taker_fee: Option<Decimal>,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let instrument_id = parse_instrument_id(definition.inst_id);
    let raw_symbol = Symbol::from_ustr_unchecked(definition.inst_id);
    let asset_class = AssetClass::Cryptocurrency;
    let exchange = Some(Ustr::from("OKX"));
    let underlying = Ustr::from(&definition.uly);
    let option_kind: OptionKind = definition.opt_type.clone().into();
    let strike_price = Price::from(&definition.stk);
    let currency = definition
        .uly
        .split_once('-')
        .map(|(_, quote_ccy)| get_currency(quote_ccy))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Invalid underlying for Option instrument: {}",
                definition.uly
            )
        })?;
    let listing_time = definition
        .list_time
        .ok_or_else(|| anyhow::anyhow!("`listing_time` is required to parse Option instrument"))?;
    let expiry_time = definition
        .exp_time
        .ok_or_else(|| anyhow::anyhow!("`expiry_time` is required to parse Option instrument"))?;
    let activation_ns = UnixNanos::from(millis_to_nanos(listing_time as f64));
    let expiration_ns = UnixNanos::from(millis_to_nanos(expiry_time as f64));
    let price_increment = Price::from(definition.tick_sz.to_string());
    let multiplier = Quantity::from(&definition.ct_mult);
    let lot_size = Quantity::from(&definition.lot_sz);
    let max_quantity = Some(Quantity::from(&definition.max_mkt_sz));
    let min_quantity = Some(Quantity::from(&definition.min_sz));
    let max_price = None; // TBD
    let min_price = None; // TBD

    let instrument = OptionContract::new(
        instrument_id,
        raw_symbol,
        asset_class,
        exchange,
        underlying,
        option_kind,
        strike_price,
        currency,
        activation_ns,
        expiration_ns,
        price_increment.precision,
        price_increment,
        multiplier,
        lot_size,
        max_quantity,
        min_quantity,
        max_price,
        min_price,
        margin_init,
        margin_maint,
        maker_fee,
        taker_fee,
        ts_init, // No ts_event for response
        ts_init,
    );

    Ok(InstrumentAny::OptionContract(instrument))
}

/// Parses an OKX account into a Nautilus account state.
pub fn parse_account_state(
    okx_account: &OKXAccount,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<AccountState> {
    let mut balances = Vec::new();
    for b in &okx_account.details {
        let currency = Currency::from(b.ccy);
        let total = Money::new(b.cash_bal.parse::<f64>()?, currency);
        let free = Money::new(b.avail_bal.parse::<f64>()?, currency);
        let locked = total - free;
        let balance = AccountBalance::new(total, locked, free);
        balances.push(balance);
    }
    let margins = vec![]; // TBD

    let account_type = AccountType::Margin;
    let is_reported = true;
    let event_id = UUID4::new();
    let ts_event = UnixNanos::from(millis_to_nanos(okx_account.u_time as f64));

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

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_model::{
        enums::AggregationSource, identifiers::InstrumentId, instruments::Instrument,
    };
    use rstest::rstest;

    use super::*;
    use crate::{common::testing::load_test_json, http::client::OKXResponse};

    #[rstest]
    fn test_parse_spot_instrument() {
        let json_data = load_test_json("http_get_instruments_spot.json");
        let response: OKXResponse<OKXInstrument> = serde_json::from_str(&json_data).unwrap();
        let okx_inst: &OKXInstrument = response
            .data
            .first()
            .expect("Test data must have an instrument");

        let instrument =
            parse_spot_instrument(okx_inst, None, None, None, None, UnixNanos::default()).unwrap();

        assert_eq!(instrument.id(), InstrumentId::from("BTC-USD.OKX"));
        assert_eq!(instrument.raw_symbol(), Symbol::from("BTC-USD"));
        assert_eq!(instrument.underlying(), None);
        assert_eq!(instrument.base_currency(), Some(Currency::BTC()));
        assert_eq!(instrument.quote_currency(), Currency::USD());
        assert_eq!(instrument.price_precision(), 1);
        assert_eq!(instrument.size_precision(), 8);
        assert_eq!(instrument.price_increment(), Price::from("0.1"));
        assert_eq!(instrument.size_increment(), Quantity::from("0.00000001"));
    }

    #[rstest]
    fn test_parse_margin_instrument() {
        let json_data = load_test_json("http_get_instruments_margin.json");
        let response: OKXResponse<OKXInstrument> = serde_json::from_str(&json_data).unwrap();
        let okx_inst: &OKXInstrument = response
            .data
            .first()
            .expect("Test data must have an instrument");

        let instrument =
            parse_spot_instrument(okx_inst, None, None, None, None, UnixNanos::default()).unwrap();

        assert_eq!(instrument.id(), InstrumentId::from("BTC-USDT.OKX"));
        assert_eq!(instrument.raw_symbol(), Symbol::from("BTC-USDT"));
        assert_eq!(instrument.underlying(), None);
        assert_eq!(instrument.base_currency(), Some(Currency::BTC()));
        assert_eq!(instrument.quote_currency(), Currency::USDT());
        assert_eq!(instrument.price_precision(), 1);
        assert_eq!(instrument.size_precision(), 8);
        assert_eq!(instrument.price_increment(), Price::from("0.1"));
        assert_eq!(instrument.size_increment(), Quantity::from("0.00000001"));
    }

    #[rstest]
    fn test_parse_swap_instrument() {
        let json_data = load_test_json("http_get_instruments_swap.json");
        let response: OKXResponse<OKXInstrument> = serde_json::from_str(&json_data).unwrap();
        let okx_inst: &OKXInstrument = response
            .data
            .first()
            .expect("Test data must have an instrument");

        let instrument =
            parse_swap_instrument(okx_inst, None, None, None, None, UnixNanos::default()).unwrap();

        assert_eq!(instrument.id(), InstrumentId::from("BTC-USD-SWAP.OKX"));
        assert_eq!(instrument.raw_symbol(), Symbol::from("BTC-USD-SWAP"));
        assert_eq!(instrument.underlying(), None);
        assert_eq!(instrument.base_currency(), Some(Currency::BTC()));
        assert_eq!(instrument.quote_currency(), Currency::USD());
        assert_eq!(instrument.price_precision(), 1);
        assert_eq!(instrument.size_precision(), 0);
        assert_eq!(instrument.price_increment(), Price::from("0.1"));
        assert_eq!(instrument.size_increment(), Quantity::from(1));
    }

    #[rstest]
    fn test_parse_futures_instrument() {
        let json_data = load_test_json("http_get_instruments_futures.json");
        let response: OKXResponse<OKXInstrument> = serde_json::from_str(&json_data).unwrap();
        let okx_inst: &OKXInstrument = response
            .data
            .first()
            .expect("Test data must have an instrument");

        let instrument =
            parse_futures_instrument(okx_inst, None, None, None, None, UnixNanos::default())
                .unwrap();

        assert_eq!(instrument.id(), InstrumentId::from("BTC-USD-241220.OKX"));
        assert_eq!(instrument.raw_symbol(), Symbol::from("BTC-USD-241220"));
        assert_eq!(instrument.underlying(), Some(Ustr::from("BTC-USD")));
        assert_eq!(instrument.quote_currency(), Currency::USD());
        assert_eq!(instrument.price_precision(), 1);
        assert_eq!(instrument.size_precision(), 0);
        assert_eq!(instrument.price_increment(), Price::from("0.1"));
        assert_eq!(instrument.size_increment(), Quantity::from(1));
    }

    #[rstest]
    fn test_parse_option_instrument() {
        let json_data = load_test_json("http_get_instruments_option.json");
        let response: OKXResponse<OKXInstrument> = serde_json::from_str(&json_data).unwrap();
        let okx_inst: &OKXInstrument = response
            .data
            .first()
            .expect("Test data must have an instrument");

        let instrument =
            parse_option_instrument(okx_inst, None, None, None, None, UnixNanos::default())
                .unwrap();

        assert_eq!(
            instrument.id(),
            InstrumentId::from("BTC-USD-241217-92000-C.OKX")
        );
        assert_eq!(
            instrument.raw_symbol(),
            Symbol::from("BTC-USD-241217-92000-C")
        );
        assert_eq!(instrument.underlying(), Some(Ustr::from("BTC-USD")));
        assert_eq!(instrument.quote_currency(), Currency::USD());
        assert_eq!(instrument.price_precision(), 4);
        assert_eq!(instrument.size_precision(), 0);
        assert_eq!(instrument.price_increment(), Price::from("0.0001"));
        assert_eq!(instrument.size_increment(), Quantity::from(1));
    }

    #[rstest]
    fn test_parse_account_state() {
        let json_data = load_test_json("http_get_account_balance.json");
        let response: OKXResponse<OKXAccount> = serde_json::from_str(&json_data).unwrap();
        let okx_account = response
            .data
            .first()
            .expect("Test data must have an account");

        let account_id = AccountId::new("OKX-001");
        let account_state =
            parse_account_state(okx_account, account_id, UnixNanos::default()).unwrap();

        assert_eq!(account_state.account_id, account_id);
        assert_eq!(account_state.account_type, AccountType::Margin);
        assert_eq!(account_state.balances.len(), 1);
        assert_eq!(account_state.margins.len(), 0); // TBD in implementation
        assert!(account_state.is_reported);

        // Check the USDT balance details
        let usdt_balance = &account_state.balances[0];
        assert_eq!(
            usdt_balance.total,
            Money::new(94.42612990333333, Currency::USDT())
        );
        assert_eq!(
            usdt_balance.free,
            Money::new(94.42612990333333, Currency::USDT())
        );
        assert_eq!(usdt_balance.locked, Money::new(0.0, Currency::USDT()));
    }

    #[rstest]
    fn test_parse_order_status_report() {
        let json_data = load_test_json("http_get_orders_history.json");
        let response: OKXResponse<OKXOrderHistory> = serde_json::from_str(&json_data).unwrap();
        let okx_order = response
            .data
            .first()
            .expect("Test data must have an order")
            .clone();

        let account_id = AccountId::new("OKX-001");
        let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");
        let order_report = parse_order_status_report(
            okx_order,
            account_id,
            instrument_id,
            2,
            8,
            UnixNanos::default(),
        );

        assert_eq!(order_report.account_id, account_id);
        assert_eq!(order_report.instrument_id, instrument_id);
        assert_eq!(order_report.quantity, Quantity::from("0.03000000"));
        assert_eq!(order_report.filled_qty, Quantity::from("0.03000000"));
        assert_eq!(order_report.order_side, OrderSide::Buy);
        assert_eq!(order_report.order_type, OrderType::Market);
        assert_eq!(order_report.order_status, OrderStatus::Filled);
    }

    #[rstest]
    fn test_parse_position_status_report() {
        let json_data = load_test_json("http_get_positions.json");
        let response: OKXResponse<OKXPosition> = serde_json::from_str(&json_data).unwrap();
        let okx_position = response
            .data
            .first()
            .expect("Test data must have a position")
            .clone();

        let account_id = AccountId::new("OKX-001");
        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let position_report = parse_position_status_report(
            okx_position,
            account_id,
            instrument_id,
            8,
            UnixNanos::default(),
        );

        assert_eq!(position_report.account_id, account_id);
        assert_eq!(position_report.instrument_id, instrument_id);
    }

    #[rstest]
    fn test_parse_trade_tick() {
        let json_data = load_test_json("http_get_trades.json");
        let response: OKXResponse<OKXTrade> = serde_json::from_str(&json_data).unwrap();
        let okx_trade = response.data.first().expect("Test data must have a trade");

        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let trade_tick =
            parse_trade_tick(okx_trade, instrument_id, 2, 8, UnixNanos::default()).unwrap();

        assert_eq!(trade_tick.instrument_id, instrument_id);
        assert_eq!(trade_tick.price, Price::from("102537.90"));
        assert_eq!(trade_tick.size, Quantity::from("0.00013669"));
        assert_eq!(trade_tick.aggressor_side, AggressorSide::Seller);
        assert_eq!(trade_tick.trade_id, TradeId::new("734864333"));
    }

    #[rstest]
    fn test_parse_mark_price_update() {
        let json_data = load_test_json("http_get_mark_price.json");
        let response: OKXResponse<crate::http::models::OKXMarkPrice> =
            serde_json::from_str(&json_data).unwrap();
        let okx_mark_price = response
            .data
            .first()
            .expect("Test data must have a mark price");

        let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");
        let mark_price_update =
            parse_mark_price_update(okx_mark_price, instrument_id, 2, UnixNanos::default())
                .unwrap();

        assert_eq!(mark_price_update.instrument_id, instrument_id);
        assert_eq!(mark_price_update.value, Price::from("84660.10"));
        assert_eq!(
            mark_price_update.ts_event,
            UnixNanos::from(1744590349506000000)
        );
    }

    #[rstest]
    fn test_parse_index_price_update() {
        let json_data = load_test_json("http_get_index_price.json");
        let response: OKXResponse<crate::http::models::OKXIndexTicker> =
            serde_json::from_str(&json_data).unwrap();
        let okx_index_ticker = response
            .data
            .first()
            .expect("Test data must have an index ticker");

        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let index_price_update =
            parse_index_price_update(okx_index_ticker, instrument_id, 2, UnixNanos::default())
                .unwrap();

        assert_eq!(index_price_update.instrument_id, instrument_id);
        assert_eq!(index_price_update.value, Price::from("103895.00"));
        assert_eq!(
            index_price_update.ts_event,
            UnixNanos::from(1746942707815000000)
        );
    }

    #[rstest]
    fn test_parse_candlestick() {
        let json_data = load_test_json("http_get_candlesticks.json");
        let response: OKXResponse<crate::http::models::OKXCandlestick> =
            serde_json::from_str(&json_data).unwrap();
        let okx_candlestick = response
            .data
            .first()
            .expect("Test data must have a candlestick");

        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let bar_type = BarType::new(
            instrument_id,
            BAR_SPEC_1_DAY_LAST,
            AggregationSource::External,
        );
        let bar = parse_candlestick(okx_candlestick, bar_type, 2, 8, UnixNanos::default()).unwrap();

        assert_eq!(bar.bar_type, bar_type);
        assert_eq!(bar.open, Price::from("33528.60"));
        assert_eq!(bar.high, Price::from("33870.00"));
        assert_eq!(bar.low, Price::from("33528.60"));
        assert_eq!(bar.close, Price::from("33783.90"));
        assert_eq!(bar.volume, Quantity::from("778.83800000"));
        assert_eq!(bar.ts_event, UnixNanos::from(1625097600000000000));
    }

    #[rstest]
    fn test_parse_millisecond_timestamp() {
        let timestamp_ms = 1625097600000u64;
        let result = parse_millisecond_timestamp(timestamp_ms);
        assert_eq!(result, UnixNanos::from(1625097600000000000));
    }

    #[rstest]
    fn test_parse_rfc3339_timestamp() {
        let timestamp_str = "2021-07-01T00:00:00.000Z";
        let result = parse_rfc3339_timestamp(timestamp_str).unwrap();
        assert_eq!(result, UnixNanos::from(1625097600000000000));

        // Test with timezone
        let timestamp_str_tz = "2021-07-01T08:00:00.000+08:00";
        let result_tz = parse_rfc3339_timestamp(timestamp_str_tz).unwrap();
        assert_eq!(result_tz, UnixNanos::from(1625097600000000000));

        // Test error case
        let invalid_timestamp = "invalid-timestamp";
        assert!(parse_rfc3339_timestamp(invalid_timestamp).is_err());
    }

    #[rstest]
    fn test_parse_price() {
        let price_str = "42219.5";
        let precision = 2;
        let result = parse_price(price_str, precision).unwrap();
        assert_eq!(result, Price::from("42219.50"));

        // Test error case
        let invalid_price = "invalid-price";
        assert!(parse_price(invalid_price, precision).is_err());
    }

    #[rstest]
    fn test_parse_quantity() {
        let quantity_str = "0.12345678";
        let precision = 8;
        let result = parse_quantity(quantity_str, precision).unwrap();
        assert_eq!(result, Quantity::from("0.12345678"));

        // Test error case
        let invalid_quantity = "invalid-quantity";
        assert!(parse_quantity(invalid_quantity, precision).is_err());
    }

    #[rstest]
    fn test_parse_aggressor_side() {
        assert_eq!(
            parse_aggressor_side(&Some(OKXSide::Buy)),
            AggressorSide::Buyer
        );
        assert_eq!(
            parse_aggressor_side(&Some(OKXSide::Sell)),
            AggressorSide::Seller
        );
        assert_eq!(parse_aggressor_side(&None), AggressorSide::NoAggressor);
    }

    #[rstest]
    fn test_parse_execution_type() {
        assert_eq!(
            parse_execution_type(&Some(OKXExecType::Maker)),
            LiquiditySide::Maker
        );
        assert_eq!(
            parse_execution_type(&Some(OKXExecType::Taker)),
            LiquiditySide::Taker
        );
        assert_eq!(parse_execution_type(&None), LiquiditySide::NoLiquiditySide);
    }

    #[rstest]
    fn test_parse_position_side() {
        assert_eq!(parse_position_side(Some(100)), PositionSide::Long);
        assert_eq!(parse_position_side(Some(-100)), PositionSide::Short);
        assert_eq!(parse_position_side(Some(0)), PositionSide::Flat);
        assert_eq!(parse_position_side(None), PositionSide::Flat);
    }

    #[rstest]
    fn test_parse_order_side() {
        assert_eq!(parse_order_side(&Some(OKXSide::Buy)), OrderSide::Buy);
        assert_eq!(parse_order_side(&Some(OKXSide::Sell)), OrderSide::Sell);
        assert_eq!(parse_order_side(&None), OrderSide::NoOrderSide);
    }

    #[rstest]
    fn test_parse_client_order_id() {
        let valid_id = "client_order_123";
        let result = parse_client_order_id(valid_id);
        assert_eq!(result, Some(ClientOrderId::new(valid_id)));

        let empty_id = "";
        let result_empty = parse_client_order_id(empty_id);
        assert_eq!(result_empty, None);
    }

    #[rstest]
    fn test_deserialize_empty_string_as_none() {
        let json_with_empty = r#""""#;
        let result: Option<String> = serde_json::from_str(json_with_empty).unwrap();
        let processed = result.filter(|s| !s.is_empty());
        assert_eq!(processed, None);

        let json_with_value = r#""test_value""#;
        let result: Option<String> = serde_json::from_str(json_with_value).unwrap();
        let processed = result.filter(|s| !s.is_empty());
        assert_eq!(processed, Some("test_value".to_string()));
    }

    #[rstest]
    fn test_deserialize_string_to_u64() {
        use serde::Deserialize;

        #[derive(Deserialize)]
        struct TestStruct {
            #[serde(deserialize_with = "deserialize_string_to_u64")]
            value: u64,
        }

        let json_value = r#"{"value": "12345"}"#;
        let result: TestStruct = serde_json::from_str(json_value).unwrap();
        assert_eq!(result.value, 12345);

        let json_empty = r#"{"value": ""}"#;
        let result_empty: TestStruct = serde_json::from_str(json_empty).unwrap();
        assert_eq!(result_empty.value, 0);
    }

    #[rstest]
    fn test_fill_report_parsing() {
        // Create a mock transaction detail for testing
        let transaction_detail = crate::http::models::OKXTransactionDetail {
            inst_type: OKXInstrumentType::Spot,
            inst_id: Ustr::from("BTC-USDT"),
            trade_id: Ustr::from("12345"),
            ord_id: Ustr::from("67890"),
            cl_ord_id: Ustr::from("client_123"),
            bill_id: Ustr::from("bill_456"),
            fill_px: "42219.5".to_string(),
            fill_sz: "0.001".to_string(),
            side: OKXSide::Buy,
            exec_type: OKXExecType::Taker,
            fee_ccy: "USDT".to_string(),
            fee: Some("0.042".to_string()),
            ts: 1625097600000,
        };

        let account_id = AccountId::new("OKX-001");
        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let fill_report = parse_fill_report(
            transaction_detail,
            account_id,
            instrument_id,
            2,
            8,
            UnixNanos::default(),
        )
        .unwrap();

        assert_eq!(fill_report.account_id, account_id);
        assert_eq!(fill_report.instrument_id, instrument_id);
        assert_eq!(fill_report.trade_id, TradeId::new("12345"));
        assert_eq!(fill_report.venue_order_id, VenueOrderId::new("67890"));
        assert_eq!(fill_report.order_side, OrderSide::Buy);
        assert_eq!(fill_report.last_px, Price::from("42219.50"));
        assert_eq!(fill_report.last_qty, Quantity::from("0.00100000"));
        assert_eq!(fill_report.liquidity_side, LiquiditySide::Taker);
    }
}
