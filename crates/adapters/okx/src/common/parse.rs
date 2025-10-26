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

//! Parsing utilities that convert OKX payloads into Nautilus domain models.

use std::str::FromStr;

use nautilus_core::{
    UUID4,
    datetime::{NANOSECONDS_IN_MILLISECOND, millis_to_nanos},
    nanos::UnixNanos,
};
use nautilus_model::{
    currencies::CURRENCY_MAP,
    data::{
        Bar, BarSpecification, BarType, Data, FundingRateUpdate, IndexPriceUpdate, MarkPriceUpdate,
        TradeTick,
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
        AccountType, AggregationSource, AggressorSide, LiquiditySide, OptionKind, OrderSide,
        OrderStatus, OrderType, PositionSide, TimeInForce,
    },
    events::AccountState,
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, PositionId, Symbol, TradeId, Venue, VenueOrderId,
    },
    instruments::{CryptoFuture, CryptoOption, CryptoPerpetual, CurrencyPair, InstrumentAny},
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, MarginBalance, Money, Price, Quantity},
};
use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, de::DeserializeOwned};
use ustr::Ustr;

use super::enums::OKXContractType;
use crate::{
    common::{
        consts::OKX_VENUE,
        enums::{
            OKXExecType, OKXInstrumentType, OKXOrderStatus, OKXOrderType, OKXPositionSide, OKXSide,
            OKXTargetCurrency, OKXVipLevel,
        },
        models::OKXInstrument,
    },
    http::models::{
        OKXAccount, OKXCandlestick, OKXIndexTicker, OKXMarkPrice, OKXOrderHistory, OKXPosition,
        OKXTrade, OKXTransactionDetail,
    },
    websocket::{enums::OKXWsChannel, messages::OKXFundingRateMsg},
};

/// Deserializes an empty string into [`None`].
///
/// OKX frequently represents *null* string fields as an empty string (`""`).
/// When such a payload is mapped onto `Option<String>` the default behaviour
/// would yield `Some("")`, which is semantically different from the intended
/// absence of a value.  Applying this helper via
///
/// ```rust
/// #[serde(deserialize_with = "crate::common::parse::deserialize_empty_string_as_none")]
/// pub cl_ord_id: Option<String>,
/// ```
///
/// ensures that empty strings are normalised to `None` during deserialization.
///
/// # Errors
///
/// Returns an error if the JSON value cannot be deserialised into a string.
pub fn deserialize_empty_string_as_none<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    Ok(opt.filter(|s| !s.is_empty()))
}

/// Deserializes an empty [`Ustr`] into [`None`].
///
/// # Errors
///
/// Returns an error if the JSON value cannot be deserialised into a string.
pub fn deserialize_empty_ustr_as_none<'de, D>(deserializer: D) -> Result<Option<Ustr>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt = Option::<Ustr>::deserialize(deserializer)?;
    Ok(opt.filter(|s| !s.is_empty()))
}

/// Deserializes a string into `Option<OKXTargetCurrency>`, treating empty strings as `None`.
///
/// # Errors
///
/// Returns an error if the string cannot be parsed into an `OKXTargetCurrency`.
pub fn deserialize_target_currency_as_none<'de, D>(
    deserializer: D,
) -> Result<Option<crate::common::enums::OKXTargetCurrency>, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    if s.is_empty() {
        Ok(None)
    } else {
        s.parse().map(Some).map_err(serde::de::Error::custom)
    }
}

/// Deserializes a numeric string into a `u64`.
///
/// # Errors
///
/// Returns an error if the string cannot be parsed into a `u64`.
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

/// Deserializes an optional numeric string into `Option<u64>`.
///
/// # Errors
///
/// Returns an error under the same cases as [`deserialize_string_to_u64`].
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

/// Deserializes an OKX VIP level string into [`OKXVipLevel`].
///
/// OKX returns VIP levels in multiple formats:
/// - "VIP0", "VIP1", ..., "VIP9" (VIP tier format)
/// - "Lv0", "Lv1", ..., "Lv9" (Level format)
/// - "0", "1", ..., "9" (bare numeric)
/// - "" (empty string, defaults to VIP0)
///
/// This function handles all formats by stripping any prefix and parsing the numeric value.
///
/// # Errors
///
/// Returns an error if the string cannot be parsed into a valid VIP level.
pub fn deserialize_vip_level<'de, D>(deserializer: D) -> Result<OKXVipLevel, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;

    if s.is_empty() {
        return Ok(OKXVipLevel::Vip0);
    }

    let s_lower = s.to_lowercase();
    let level_str = s_lower
        .strip_prefix("vip")
        .or_else(|| s_lower.strip_prefix("lv"))
        .unwrap_or(&s_lower);

    let level_num = level_str
        .parse::<u8>()
        .map_err(|e| serde::de::Error::custom(format!("Invalid VIP level '{s}': {e}")))?;

    Ok(OKXVipLevel::from(level_num))
}

/// Returns the currency either from the internal currency map or creates a default crypto.
///
/// If the code is empty, logs a warning with context and returns USDT as fallback.
/// For unknown but valid codes, creates a new Currency (preserves newly listed OKX assets).
fn get_currency_with_context(code: &str, context: Option<&str>) -> Currency {
    let trimmed = code.trim();
    let ctx = context.unwrap_or("unknown");

    if trimmed.is_empty() {
        tracing::warn!(
            "get_currency called with empty code (context: {ctx}), using USDT as fallback"
        );
        return Currency::USDT();
    }

    CURRENCY_MAP
        .lock()
        .unwrap()
        .get(trimmed)
        .copied()
        .unwrap_or_else(|| {
            // For unknown codes, create a new currency (8 decimals, crypto type)
            // This preserves newly listed OKX assets that aren't in CURRENCY_MAP yet
            use nautilus_model::enums::CurrencyType;
            Currency::new(trimmed, 8, 0, trimmed, CurrencyType::Crypto)
        })
}

/// Returns the [`OKXInstrumentType`] that corresponds to the supplied
/// [`InstrumentAny`].
///
/// # Errors
///
/// Returns an error if the instrument variant is not supported by OKX.
pub fn okx_instrument_type(instrument: &InstrumentAny) -> anyhow::Result<OKXInstrumentType> {
    match instrument {
        InstrumentAny::CurrencyPair(_) => Ok(OKXInstrumentType::Spot),
        InstrumentAny::CryptoPerpetual(_) => Ok(OKXInstrumentType::Swap),
        InstrumentAny::CryptoFuture(_) => Ok(OKXInstrumentType::Futures),
        InstrumentAny::CryptoOption(_) => Ok(OKXInstrumentType::Option),
        _ => anyhow::bail!("Invalid instrument type for OKX: {instrument:?}"),
    }
}

/// Parses `OKXInstrumentType` from an instrument symbol.
///
/// OKX instrument symbol formats:
/// - SPOT: {BASE}-{QUOTE} (e.g., BTC-USDT)
/// - MARGIN: {BASE}-{QUOTE} (same as SPOT, determined by trade mode)
/// - SWAP: {BASE}-{QUOTE}-SWAP (e.g., BTC-USDT-SWAP)
/// - FUTURES: {BASE}-{QUOTE}-{YYMMDD} (e.g., BTC-USDT-250328)
/// - OPTION: {BASE}-{QUOTE}-{YYMMDD}-{STRIKE}-{C/P} (e.g., BTC-USD-250328-50000-C)
pub fn okx_instrument_type_from_symbol(symbol: &str) -> OKXInstrumentType {
    // TODO: Improve efficiency of this
    let parts: Vec<&str> = symbol.split('-').collect();

    match parts.len() {
        2 => OKXInstrumentType::Spot,
        3 => {
            let suffix = parts[2];
            if suffix == "SWAP" {
                OKXInstrumentType::Swap
            } else if suffix.len() == 6 && suffix.chars().all(|c| c.is_ascii_digit()) {
                // Date format YYMMDD
                OKXInstrumentType::Futures
            } else {
                OKXInstrumentType::Spot
            }
        }
        5 => OKXInstrumentType::Option,
        _ => OKXInstrumentType::Spot, // Default fallback
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

/// Converts a millisecond-based timestamp (as returned by OKX) into
/// [`UnixNanos`].
#[must_use]
pub fn parse_millisecond_timestamp(timestamp_ms: u64) -> UnixNanos {
    UnixNanos::from(timestamp_ms * NANOSECONDS_IN_MILLISECOND)
}

/// Parses an RFC 3339 timestamp string into [`UnixNanos`].
///
/// # Errors
///
/// Returns an error if the string is not a valid RFC 3339 datetime or if the
/// timestamp cannot be represented in nanoseconds.
pub fn parse_rfc3339_timestamp(timestamp: &str) -> anyhow::Result<UnixNanos> {
    let dt = chrono::DateTime::parse_from_rfc3339(timestamp)?;
    let nanos = dt.timestamp_nanos_opt().ok_or_else(|| {
        anyhow::anyhow!("Failed to extract nanoseconds from timestamp: {timestamp}")
    })?;
    Ok(UnixNanos::from(nanos as u64))
}

/// Converts a textual price to a [`Price`] using the given precision.
///
/// # Errors
///
/// Returns an error if the string fails to parse into `f64` or if the number
/// of decimal places exceeds `precision`.
pub fn parse_price(value: &str, precision: u8) -> anyhow::Result<Price> {
    Price::new_checked(value.parse::<f64>()?, precision)
}

/// Converts a textual quantity to a [`Quantity`].
///
/// # Errors
///
/// Returns an error for the same reasons as [`parse_price`] – parsing failure or invalid
/// precision.
pub fn parse_quantity(value: &str, precision: u8) -> anyhow::Result<Quantity> {
    Quantity::new_checked(value.parse::<f64>()?, precision)
}

/// Converts a textual fee amount into a [`Money`] value.
///
/// OKX represents *charges* as positive numbers but they reduce the account
/// balance, hence the value is negated.
///
/// # Errors
///
/// Returns an error if the fee cannot be parsed into `f64` or fails internal
/// validation in [`Money::new_checked`].
pub fn parse_fee(value: Option<&str>, currency: Currency) -> anyhow::Result<Money> {
    // OKX report positive fees with negative signs (i.e., fee charged)
    let fee_f64 = value.unwrap_or("0").parse::<f64>()?;
    Money::new_checked(-fee_f64, currency)
}

/// Parses OKX fee currency code, handling empty strings.
///
/// OKX sometimes returns empty fee currency codes.
/// When the fee currency is empty, defaults to USDT and logs a warning for non-zero fees.
pub fn parse_fee_currency(
    fee_ccy: &str,
    fee_amount: f64,
    context: impl FnOnce() -> String,
) -> Currency {
    let trimmed = fee_ccy.trim();
    if trimmed.is_empty() {
        if fee_amount != 0.0 {
            let ctx = context();
            tracing::warn!(
                "Empty fee_ccy in {ctx} with non-zero fee={fee_amount}, using USDT as fallback"
            );
        }
        return Currency::USDT();
    }

    get_currency_with_context(trimmed, Some(&context()))
}

/// Parses OKX side to Nautilus aggressor side.
pub fn parse_aggressor_side(side: &Option<OKXSide>) -> AggressorSide {
    match side {
        Some(OKXSide::Buy) => AggressorSide::Buyer,
        Some(OKXSide::Sell) => AggressorSide::Seller,
        None => AggressorSide::NoAggressor,
    }
}

/// Parses OKX execution type to Nautilus liquidity side.
pub fn parse_execution_type(liquidity: &Option<OKXExecType>) -> LiquiditySide {
    match liquidity {
        Some(OKXExecType::Maker) => LiquiditySide::Maker,
        Some(OKXExecType::Taker) => LiquiditySide::Taker,
        _ => LiquiditySide::NoLiquiditySide,
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

/// Parses an OKX mark price record into a Nautilus [`MarkPriceUpdate`].
///
/// # Errors
///
/// Returns an error if `raw.mark_px` cannot be parsed into a [`Price`] with
/// the specified precision.
pub fn parse_mark_price_update(
    raw: &OKXMarkPrice,
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
///
/// # Errors
///
/// Returns an error if `raw.idx_px` cannot be parsed into a [`Price`] with the
/// specified precision.
pub fn parse_index_price_update(
    raw: &OKXIndexTicker,
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

/// Parses an [`OKXFundingRateMsg`] into a [`FundingRateUpdate`].
///
/// # Errors
///
/// Returns an error if the `funding_rate` or `next_funding_rate` fields fail
/// to parse into Decimal values.
pub fn parse_funding_rate_msg(
    msg: &OKXFundingRateMsg,
    instrument_id: InstrumentId,
    ts_init: UnixNanos,
) -> anyhow::Result<FundingRateUpdate> {
    let funding_rate = msg
        .funding_rate
        .as_str()
        .parse::<Decimal>()
        .map_err(|e| anyhow::anyhow!("Invalid funding_rate value: {e}"))?
        .normalize();

    let funding_time = Some(parse_millisecond_timestamp(msg.funding_time));
    let ts_event = parse_millisecond_timestamp(msg.ts);

    Ok(FundingRateUpdate::new(
        instrument_id,
        funding_rate,
        funding_time,
        ts_event,
        ts_init,
    ))
}

/// Parses an OKX trade record into a Nautilus [`TradeTick`].
///
/// # Errors
///
/// Returns an error if the price or quantity strings cannot be parsed, or if
/// [`TradeTick::new_checked`] validation fails.
pub fn parse_trade_tick(
    raw: &OKXTrade,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let ts_event = parse_millisecond_timestamp(raw.ts);
    let price = parse_price(&raw.px, price_precision)?;
    let size = parse_quantity(&raw.sz, size_precision)?;
    let aggressor: AggressorSide = raw.side.into();
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
///
/// # Errors
///
/// Returns an error if any of the price or volume strings cannot be parsed or
/// if [`Bar::new`] validation fails.
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
    order: &OKXOrderHistory,
    account_id: AccountId,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> OrderStatusReport {
    // Parse quantities based on target currency
    // OKX always returns acc_fill_sz in base currency, but sz depends on tgt_ccy

    // Determine if this is a quote-quantity order
    // Method 1: Explicit tgt_ccy field set to QuoteCcy
    let is_quote_qty_explicit = order.tgt_ccy == Some(OKXTargetCurrency::QuoteCcy);

    // Method 2: Use OKX defaults when tgt_ccy is None (old orders or missing field)
    // OKX API defaults for SPOT market orders: BUY orders use quote_ccy, SELL orders use base_ccy
    // Note: tgtCcy only applies to SPOT market orders (not limit orders)
    // For limit orders, sz is always in base currency regardless of side
    let is_quote_qty_heuristic = order.tgt_ccy.is_none()
        && (order.inst_type == OKXInstrumentType::Spot
            || order.inst_type == OKXInstrumentType::Margin)
        && order.side == OKXSide::Buy
        && order.ord_type == OKXOrderType::Market;

    let (quantity, filled_qty) = if is_quote_qty_explicit || is_quote_qty_heuristic {
        // Quote-quantity order: sz is in quote currency, need to convert to base
        let sz_quote = order.sz.parse::<f64>().unwrap_or(0.0);

        // Determine the price to use for conversion
        // Priority: 1) limit price (px) for limit orders, 2) avg_px for market orders
        let conversion_price = if !order.px.is_empty() && order.px != "0" {
            // Limit order: use the limit price (order.px)
            order.px.parse::<f64>().unwrap_or(0.0)
        } else if !order.avg_px.is_empty() && order.avg_px != "0" {
            // Market order with fills: use average fill price
            order.avg_px.parse::<f64>().unwrap_or(0.0)
        } else {
            log::warn!(
                "No price available for conversion: ord_id={}, px='{}', avg_px='{}'",
                order.ord_id.as_str(),
                order.px,
                order.avg_px
            );
            0.0
        };

        // Convert quote quantity to base: quantity_base = sz_quote / price
        let quantity_base = if conversion_price > 0.0 {
            Quantity::new(sz_quote / conversion_price, size_precision)
        } else {
            // No price available, can't convert - use sz as-is temporarily
            log::warn!(
                "Cannot convert, using sz as-is: ord_id={}, sz={}",
                order.ord_id.as_str(),
                order.sz
            );
            order
                .sz
                .parse::<f64>()
                .ok()
                .map(|v| Quantity::new(v, size_precision))
                .unwrap_or_default()
        };

        let filled_qty = order
            .acc_fill_sz
            .parse::<f64>()
            .ok()
            .map(|v| Quantity::new(v, size_precision))
            .unwrap_or_default();

        (quantity_base, filled_qty)
    } else {
        // Base-quantity order: both sz and acc_fill_sz are in base currency
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

        (quantity, filled_qty)
    };

    // For quote-quantity orders marked as FILLED, adjust quantity to match filled_qty
    // to avoid precision mismatches from quote-to-base conversion
    let (quantity, filled_qty) = if (is_quote_qty_explicit || is_quote_qty_heuristic)
        && order.state == OKXOrderStatus::Filled
        && filled_qty.is_positive()
    {
        (filled_qty, filled_qty)
    } else {
        (quantity, filled_qty)
    };

    let order_side: OrderSide = order.side.into();
    let okx_status: OKXOrderStatus = order.state;
    let order_status: OrderStatus = okx_status.into();
    let okx_ord_type: OKXOrderType = order.ord_type;
    let order_type: OrderType = okx_ord_type.into();
    // Note: OKX uses ordType for type and liquidity instructions; time-in-force not explicitly represented here
    let time_in_force = TimeInForce::Gtc;

    // Build report
    let mut client_order_id = if order.cl_ord_id.is_empty() {
        None
    } else {
        Some(ClientOrderId::new(order.cl_ord_id.as_str()))
    };

    let mut linked_ids = Vec::new();

    if let Some(algo_cl_ord_id) = order
        .algo_cl_ord_id
        .as_ref()
        .filter(|value| !value.as_str().is_empty())
    {
        let algo_client_id = ClientOrderId::new(algo_cl_ord_id.as_str());
        match &client_order_id {
            Some(existing) if existing == &algo_client_id => {}
            Some(_) => linked_ids.push(algo_client_id),
            None => client_order_id = Some(algo_client_id),
        }
    }

    let venue_order_id = if order.ord_id.is_empty() {
        if let Some(algo_id) = order
            .algo_id
            .as_ref()
            .filter(|value| !value.as_str().is_empty())
        {
            VenueOrderId::new(algo_id.as_str())
        } else if !order.cl_ord_id.is_empty() {
            VenueOrderId::new(order.cl_ord_id.as_str())
        } else {
            let synthetic_id = format!("{}:{}", account_id, order.c_time);
            VenueOrderId::new(&synthetic_id)
        }
    } else {
        VenueOrderId::new(order.ord_id.as_str())
    };

    let ts_accepted = parse_millisecond_timestamp(order.c_time);
    let ts_last = UnixNanos::from(order.u_time * NANOSECONDS_IN_MILLISECOND);

    let mut report = OrderStatusReport::new(
        account_id,
        instrument_id,
        client_order_id,
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
        None,
    );

    // Optional fields
    if !order.px.is_empty()
        && let Ok(p) = order.px.parse::<f64>()
    {
        report = report.with_price(Price::new(p, price_precision));
    }
    if !order.avg_px.is_empty()
        && let Ok(avg) = order.avg_px.parse::<f64>()
    {
        report = report.with_avg_px(avg);
    }
    if order.ord_type == OKXOrderType::PostOnly {
        report = report.with_post_only(true);
    }
    if order.reduce_only == "true" {
        report = report.with_reduce_only(true);
    }

    if !linked_ids.is_empty() {
        report = report.with_linked_order_ids(linked_ids);
    }

    report
}

/// Parses an OKX position into a Nautilus [`PositionStatusReport`].
///
/// # Position Mode Handling
///
/// OKX returns position data differently based on the account's position mode:
///
/// - **Net mode** (`posSide="net"`): The `pos` field uses signed quantities where
///   positive = long, negative = short. Position side is derived from the sign.
///
/// - **Long/Short mode** (`posSide="long"` or `"short"`): The `pos` field is always
///   positive regardless of side. Position side is determined from the `posSide` field.
///   Position IDs are suffixed with `-LONG` or `-SHORT` for uniqueness.
///
/// See: <https://www.okx.com/docs-v5/en/#trading-account-rest-api-get-positions>
///
/// # Errors
///
/// Returns an error if any numeric fields cannot be parsed into their target types.
///
/// # Panics
///
/// Panics if position quantity is invalid and cannot be parsed.
#[allow(clippy::too_many_lines)]
pub fn parse_position_status_report(
    position: OKXPosition,
    account_id: AccountId,
    instrument_id: InstrumentId,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<PositionStatusReport> {
    let pos_value = position.pos.parse::<f64>().unwrap_or_else(|e| {
        panic!(
            "Failed to parse position quantity '{}' for instrument {}: {:?}",
            position.pos, instrument_id, e
        )
    });

    // Determine position side based on OKX position mode:
    // - Net mode: posSide="net", uses signed quantities (positive=long, negative=short)
    // - Long/Short mode: posSide="long"/"short", quantities are always positive, side from field
    let position_side = match position.pos_side {
        OKXPositionSide::Net => {
            // Net mode: derive side from signed quantity
            if pos_value > 0.0 {
                PositionSide::Long
            } else if pos_value < 0.0 {
                PositionSide::Short
            } else {
                PositionSide::Flat
            }
        }
        OKXPositionSide::Long => {
            // Long/Short mode: trust the pos_side field
            PositionSide::Long
        }
        OKXPositionSide::Short => {
            // Long/Short mode: trust the pos_side field
            PositionSide::Short
        }
        OKXPositionSide::None => {
            // Fallback: use signed quantity (same as Net mode logic)
            if pos_value > 0.0 {
                PositionSide::Long
            } else if pos_value < 0.0 {
                PositionSide::Short
            } else {
                PositionSide::Flat
            }
        }
    }
    .as_specified();

    // Convert to absolute quantity (positions are always positive in Nautilus)
    let quantity = Quantity::new(pos_value.abs(), size_precision);

    // Generate venue position ID only for Long/Short mode (hedging)
    // In Net mode, venue_position_id must be None to signal NETTING OMS behavior
    let venue_position_id = match position.pos_side {
        OKXPositionSide::Long => {
            // Long/Short mode - Long leg: append "-LONG"
            position
                .pos_id
                .map(|pos_id| PositionId::new(format!("{pos_id}-LONG")))
        }
        OKXPositionSide::Short => {
            // Long/Short mode - Short leg: append "-SHORT"
            position
                .pos_id
                .map(|pos_id| PositionId::new(format!("{pos_id}-SHORT")))
        }
        OKXPositionSide::Net | OKXPositionSide::None => {
            // Net mode: None signals NETTING OMS (Nautilus uses its own position IDs)
            None
        }
    };

    let avg_px_open = if position.avg_px.is_empty() {
        None
    } else {
        Some(Decimal::from_str(&position.avg_px)?)
    };
    let ts_last = parse_millisecond_timestamp(position.u_time);

    Ok(PositionStatusReport::new(
        account_id,
        instrument_id,
        position_side,
        quantity,
        ts_last,
        ts_init,
        None, // Will generate a UUID4
        venue_position_id,
        avg_px_open,
    ))
}

/// Parses an OKX transaction detail into a Nautilus `FillReport`.
///
/// # Errors
///
/// Returns an error if the OKX transaction detail cannot be parsed.
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
    let order_side: OrderSide = detail.side.into();
    let last_px = parse_price(&detail.fill_px, price_precision)?;
    let last_qty = parse_quantity(&detail.fill_sz, size_precision)?;
    let fee_f64 = detail.fee.as_deref().unwrap_or("0").parse::<f64>()?;
    let fee_currency = parse_fee_currency(&detail.fee_ccy, fee_f64, || {
        format!("fill report for instrument_id={}", instrument_id)
    });
    let commission = Money::new(-fee_f64, fee_currency);
    let liquidity_side: LiquiditySide = detail.exec_type.into();
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
///
/// # Errors
///
/// Returns an error if the payload is not an array or if individual messages
/// cannot be parsed.
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
    let items = match data {
        serde_json::Value::Array(items) => items,
        other => {
            let raw = serde_json::to_string(&other).unwrap_or_else(|_| other.to_string());
            let mut snippet: String = raw.chars().take(512).collect();
            if raw.len() > snippet.len() {
                snippet.push_str("...");
            }
            anyhow::bail!("Expected array payload, received {snippet}");
        }
    };

    let mut results = Vec::with_capacity(items.len());

    for item in items {
        let message: T = serde_json::from_value(item)?;
        let parsed = parser(&message)?;
        results.push(wrapper(parsed));
    }

    Ok(results)
}

/// Converts a Nautilus bar specification into the matching OKX candle channel.
///
/// # Errors
///
/// Returns an error if the provided bar specification does not have a matching
/// OKX websocket channel.
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
///
/// # Errors
///
/// Returns an error if the bar specification does not map to a mark price
/// channel.
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
///
/// # Errors
///
/// Returns an error if the bar specification does not have a corresponding
/// OKX timeframe value.
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
///
/// # Errors
///
/// Returns an error if the timeframe string is not recognized.
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

/// Constructs a properly formatted BarType from OKX instrument ID and timeframe string.
/// This ensures the BarType uses canonical Nautilus format instead of raw OKX strings.
///
/// # Errors
///
/// Returns an error if the timeframe cannot be converted into a
/// `BarSpecification`.
pub fn okx_bar_type_from_timeframe(
    instrument_id: InstrumentId,
    timeframe: &str,
) -> anyhow::Result<BarType> {
    let bar_spec = okx_timeframe_as_bar_spec(timeframe)?;
    Ok(BarType::new(
        instrument_id,
        bar_spec,
        AggregationSource::External,
    ))
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
///
/// # Errors
///
/// Returns an error if the instrument definition cannot be parsed.
pub fn parse_instrument_any(
    instrument: &OKXInstrument,
    margin_init: Option<Decimal>,
    margin_maint: Option<Decimal>,
    maker_fee: Option<Decimal>,
    taker_fee: Option<Decimal>,
    ts_init: UnixNanos,
) -> anyhow::Result<Option<InstrumentAny>> {
    match instrument.inst_type {
        OKXInstrumentType::Spot => parse_spot_instrument(
            instrument,
            margin_init,
            margin_maint,
            maker_fee,
            taker_fee,
            ts_init,
        )
        .map(Some),
        OKXInstrumentType::Margin => parse_spot_instrument(
            instrument,
            margin_init,
            margin_maint,
            maker_fee,
            taker_fee,
            ts_init,
        )
        .map(Some),
        OKXInstrumentType::Swap => parse_swap_instrument(
            instrument,
            margin_init,
            margin_maint,
            maker_fee,
            taker_fee,
            ts_init,
        )
        .map(Some),
        OKXInstrumentType::Futures => parse_futures_instrument(
            instrument,
            margin_init,
            margin_maint,
            maker_fee,
            taker_fee,
            ts_init,
        )
        .map(Some),
        OKXInstrumentType::Option => parse_option_instrument(
            instrument,
            margin_init,
            margin_maint,
            maker_fee,
            taker_fee,
            ts_init,
        )
        .map(Some),
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

/// Parses the multiplier as the product of ct_mult and ct_val.
///
/// For SPOT instruments where both fields are empty, returns None.
/// For derivatives, multiplies the two fields to get the final multiplier.
fn parse_multiplier_product(definition: &OKXInstrument) -> anyhow::Result<Option<Quantity>> {
    if definition.ct_mult.is_empty() && definition.ct_val.is_empty() {
        return Ok(None);
    }

    let mult_value = if definition.ct_mult.is_empty() {
        Decimal::ONE
    } else {
        Decimal::from_str(&definition.ct_mult).map_err(|e| {
            anyhow::anyhow!(
                "Failed to parse `ct_mult` '{}' for {}: {e}",
                definition.ct_mult,
                definition.inst_id
            )
        })?
    };

    let val_value = if definition.ct_val.is_empty() {
        Decimal::ONE
    } else {
        Decimal::from_str(&definition.ct_val).map_err(|e| {
            anyhow::anyhow!(
                "Failed to parse `ct_val` '{}' for {}: {e}",
                definition.ct_val,
                definition.inst_id
            )
        })?
    };

    let product = mult_value * val_value;
    Ok(Some(Quantity::from(product.to_string().as_str())))
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

    if definition.tick_sz.is_empty() {
        anyhow::bail!("`tick_sz` is empty for {}", definition.inst_id);
    }

    let price_increment = Price::from_str(&definition.tick_sz).map_err(|e| {
        anyhow::anyhow!(
            "Failed to parse `tick_sz` '{}' into Price for {}: {e}",
            definition.tick_sz,
            definition.inst_id,
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
        let context = format!("{} instrument {}", definition.inst_type, definition.inst_id);
        let base_currency = get_currency_with_context(&definition.base_ccy, Some(&context));
        let quote_currency = get_currency_with_context(&definition.quote_ccy, Some(&context));

        // Parse multiplier as product of ct_mult and ct_val
        let multiplier = parse_multiplier_product(definition)?;

        let instrument = CurrencyPair::new(
            common.instrument_id,
            common.raw_symbol,
            base_currency,
            quote_currency,
            common.price_increment.precision,
            common.size_increment.precision,
            common.price_increment,
            common.size_increment,
            multiplier,
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
///
/// # Errors
///
/// Returns an error if the instrument definition cannot be parsed.
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
///
/// # Errors
///
/// Returns an error if the instrument definition cannot be parsed.
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
    let context = format!("SWAP instrument {}", definition.inst_id);
    let (base_currency, quote_currency) = definition.uly.split_once('-').ok_or_else(|| {
        anyhow::anyhow!(
            "Invalid underlying '{}' for {}: expected format 'BASE-QUOTE'",
            definition.uly,
            definition.inst_id
        )
    })?;
    let base_currency = get_currency_with_context(base_currency, Some(&context));
    let quote_currency = get_currency_with_context(quote_currency, Some(&context));
    let settlement_currency = get_currency_with_context(&definition.settle_ccy, Some(&context));
    let is_inverse = match definition.ct_type {
        OKXContractType::Linear => false,
        OKXContractType::Inverse => true,
        OKXContractType::None => {
            anyhow::bail!(
                "Invalid contract type '{}' for {}: expected 'linear' or 'inverse'",
                definition.ct_type,
                definition.inst_id
            )
        }
    };

    if definition.tick_sz.is_empty() {
        anyhow::bail!("`tick_sz` is empty for {}", definition.inst_id);
    }

    let price_increment = Price::from_str(&definition.tick_sz).map_err(|e| {
        anyhow::anyhow!(
            "Failed to parse `tick_sz` '{}' into Price for {}: {e}",
            definition.tick_sz,
            definition.inst_id
        )
    })?;
    let size_increment = Quantity::from(&definition.lot_sz);
    let multiplier = parse_multiplier_product(definition)?;
    let lot_size = Some(Quantity::from(&definition.lot_sz));
    let max_quantity = Some(Quantity::from(&definition.max_mkt_sz));
    let min_quantity = Some(Quantity::from(&definition.min_sz));
    let max_notional: Option<Money> = None;
    let min_notional: Option<Money> = None;
    let max_price = None; // TBD
    let min_price = None; // TBD

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
///
/// # Errors
///
/// Returns an error if the instrument definition cannot be parsed.
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
    let context = format!("FUTURES instrument {}", definition.inst_id);
    let underlying = get_currency_with_context(&definition.uly, Some(&context));
    let (_, quote_currency) = definition.uly.split_once('-').ok_or_else(|| {
        anyhow::anyhow!(
            "Invalid underlying '{}' for {}: expected format 'BASE-QUOTE'",
            definition.uly,
            definition.inst_id
        )
    })?;
    let quote_currency = get_currency_with_context(quote_currency, Some(&context));
    let settlement_currency = get_currency_with_context(&definition.settle_ccy, Some(&context));
    let is_inverse = match definition.ct_type {
        OKXContractType::Linear => false,
        OKXContractType::Inverse => true,
        OKXContractType::None => {
            anyhow::bail!(
                "Invalid contract type '{}' for {}: expected 'linear' or 'inverse'",
                definition.ct_type,
                definition.inst_id
            )
        }
    };
    let listing_time = definition
        .list_time
        .ok_or_else(|| anyhow::anyhow!("`list_time` is required for {}", definition.inst_id))?;
    let expiry_time = definition
        .exp_time
        .ok_or_else(|| anyhow::anyhow!("`exp_time` is required for {}", definition.inst_id))?;
    let activation_ns = UnixNanos::from(millis_to_nanos(listing_time as f64));
    let expiration_ns = UnixNanos::from(millis_to_nanos(expiry_time as f64));

    if definition.tick_sz.is_empty() {
        anyhow::bail!("`tick_sz` is empty for {}", definition.inst_id);
    }

    let price_increment = Price::from(definition.tick_sz.clone());
    let size_increment = Quantity::from(&definition.lot_sz);
    let multiplier = parse_multiplier_product(definition)?;
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
///
/// # Errors
///
/// Returns an error if the instrument definition cannot be parsed.
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
    let option_kind: OptionKind = definition.opt_type.into();
    let strike_price = Price::from(&definition.stk);
    let context = format!("OPTION instrument {}", definition.inst_id);

    let (underlying_str, quote_ccy_str) = definition.uly.split_once('-').ok_or_else(|| {
        anyhow::anyhow!(
            "Invalid underlying '{}' for {}: expected format 'BASE-QUOTE'",
            definition.uly,
            definition.inst_id
        )
    })?;

    let underlying = get_currency_with_context(underlying_str, Some(&context));
    let quote_currency = get_currency_with_context(quote_ccy_str, Some(&context));
    let settlement_currency = get_currency_with_context(&definition.settle_ccy, Some(&context));

    let is_inverse = if definition.ct_type == OKXContractType::None {
        settlement_currency == underlying
    } else {
        matches!(definition.ct_type, OKXContractType::Inverse)
    };

    let listing_time = definition
        .list_time
        .ok_or_else(|| anyhow::anyhow!("`list_time` is required for {}", definition.inst_id))?;
    let expiry_time = definition
        .exp_time
        .ok_or_else(|| anyhow::anyhow!("`exp_time` is required for {}", definition.inst_id))?;
    let activation_ns = UnixNanos::from(millis_to_nanos(listing_time as f64));
    let expiration_ns = UnixNanos::from(millis_to_nanos(expiry_time as f64));

    if definition.tick_sz.is_empty() {
        anyhow::bail!("`tick_sz` is empty for {}", definition.inst_id);
    }

    let price_increment = Price::from(definition.tick_sz.clone());
    let size_increment = Quantity::from(&definition.lot_sz);
    let multiplier = parse_multiplier_product(definition)?;
    let lot_size = Quantity::from(&definition.lot_sz);
    let max_quantity = Some(Quantity::from(&definition.max_mkt_sz));
    let min_quantity = Some(Quantity::from(&definition.min_sz));
    let max_notional = None;
    let min_notional = None;
    let max_price = None;
    let min_price = None;

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
        size_increment.precision,
        price_increment,
        size_increment,
        multiplier,
        Some(lot_size),
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
        ts_init,
        ts_init,
    );

    Ok(InstrumentAny::CryptoOption(instrument))
}

/// Parses an OKX account into a Nautilus account state.
///
fn parse_balance_field(
    value_str: &str,
    field_name: &str,
    currency: Currency,
    ccy_str: &str,
) -> Option<Money> {
    match value_str.parse::<f64>() {
        Ok(v) => Some(Money::new(v, currency)),
        Err(e) => {
            tracing::warn!(
                "Skipping balance detail for {ccy_str} with invalid {field_name} '{value_str}': {e}"
            );
            None
        }
    }
}

/// # Errors
///
/// Returns an error if the data cannot be parsed.
pub fn parse_account_state(
    okx_account: &OKXAccount,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<AccountState> {
    let mut balances = Vec::new();
    for b in &okx_account.details {
        // Skip balances with empty or whitespace-only currency codes
        let ccy_str = b.ccy.as_str().trim();
        if ccy_str.is_empty() {
            tracing::debug!(
                "Skipping balance detail with empty currency code | raw_data={:?}",
                b
            );
            continue;
        }

        // Get or create currency (consistent with instrument parsing)
        let currency = get_currency_with_context(ccy_str, Some("balance detail"));

        // Parse balance values, skip if invalid
        let Some(total) = parse_balance_field(&b.cash_bal, "cash_bal", currency, ccy_str) else {
            continue;
        };

        let Some(free) = parse_balance_field(&b.avail_bal, "avail_bal", currency, ccy_str) else {
            continue;
        };

        let locked = total - free;
        let balance = AccountBalance::new(total, locked, free);
        balances.push(balance);
    }

    // Ensure at least one balance exists (Nautilus requires non-empty balances)
    // OKX may return empty details for certain account configurations
    if balances.is_empty() {
        let zero_currency = Currency::USD();
        let zero_money = Money::new(0.0, zero_currency);
        let zero_balance = AccountBalance::new(zero_money, zero_money, zero_money);
        balances.push(zero_balance);
    }

    let mut margins = Vec::new();

    // OKX provides account-level margin requirements (not per instrument)
    if !okx_account.imr.is_empty() && !okx_account.mmr.is_empty() {
        match (
            okx_account.imr.parse::<f64>(),
            okx_account.mmr.parse::<f64>(),
        ) {
            (Ok(imr_value), Ok(mmr_value)) => {
                if imr_value > 0.0 || mmr_value > 0.0 {
                    let margin_currency = Currency::USD();
                    let margin_instrument_id =
                        InstrumentId::new(Symbol::new("ACCOUNT"), Venue::new("OKX"));

                    let initial_margin = Money::new(imr_value, margin_currency);
                    let maintenance_margin = Money::new(mmr_value, margin_currency);

                    let margin_balance = MarginBalance::new(
                        initial_margin,
                        maintenance_margin,
                        margin_instrument_id,
                    );

                    margins.push(margin_balance);
                }
            }
            (Err(e1), _) => {
                tracing::warn!(
                    "Failed to parse initial margin requirement '{}': {}",
                    okx_account.imr,
                    e1
                );
            }
            (_, Err(e2)) => {
                tracing::warn!(
                    "Failed to parse maintenance margin requirement '{}': {}",
                    okx_account.mmr,
                    e2
                );
            }
        }
    }

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
    use nautilus_model::{identifiers::PositionId, instruments::Instrument};
    use rstest::rstest;

    use super::*;
    use crate::{
        OKXPositionSide,
        common::{enums::OKXMarginMode, testing::load_test_json},
        http::{
            client::OKXResponse,
            models::{
                OKXAccount, OKXBalanceDetail, OKXCandlestick, OKXIndexTicker, OKXMarkPrice,
                OKXOrderHistory, OKXPlaceOrderResponse, OKXPosition, OKXPositionHistory,
                OKXPositionTier, OKXTrade, OKXTransactionDetail,
            },
        },
    };

    #[rstest]
    fn test_parse_fee_currency_with_zero_fee_empty_string() {
        let result = parse_fee_currency("", 0.0, || "test context".to_string());
        assert_eq!(result, Currency::USDT());
    }

    #[rstest]
    fn test_parse_fee_currency_with_zero_fee_valid_currency() {
        let result = parse_fee_currency("BTC", 0.0, || "test context".to_string());
        assert_eq!(result, Currency::BTC());
    }

    #[rstest]
    fn test_parse_fee_currency_with_valid_currency() {
        let result = parse_fee_currency("BTC", 0.001, || "test context".to_string());
        assert_eq!(result, Currency::BTC());
    }

    #[rstest]
    fn test_parse_fee_currency_with_empty_string_nonzero_fee() {
        let result = parse_fee_currency("", 0.5, || "test context".to_string());
        assert_eq!(result, Currency::USDT());
    }

    #[rstest]
    fn test_parse_fee_currency_with_whitespace() {
        let result = parse_fee_currency("  ETH  ", 0.002, || "test context".to_string());
        assert_eq!(result, Currency::ETH());
    }

    #[rstest]
    fn test_parse_fee_currency_with_unknown_code() {
        // Unknown currency code should create a new Currency (8 decimals, crypto)
        let result = parse_fee_currency("NEWTOKEN", 0.5, || "test context".to_string());
        assert_eq!(result.code.as_str(), "NEWTOKEN");
        assert_eq!(result.precision, 8);
    }

    #[rstest]
    fn test_get_currency_with_context_valid() {
        let result = get_currency_with_context("BTC", Some("test context"));
        assert_eq!(result, Currency::BTC());
    }

    #[rstest]
    fn test_get_currency_with_context_empty() {
        let result = get_currency_with_context("", Some("test context"));
        assert_eq!(result, Currency::USDT());
    }

    #[rstest]
    fn test_get_currency_with_context_whitespace() {
        let result = get_currency_with_context("  ", Some("test context"));
        assert_eq!(result, Currency::USDT());
    }

    #[rstest]
    fn test_get_currency_with_context_unknown() {
        // Unknown codes should create a new Currency, preserving newly listed assets
        let result = get_currency_with_context("NEWCOIN", Some("test context"));
        assert_eq!(result.code.as_str(), "NEWCOIN");
        assert_eq!(result.precision, 8);
    }

    #[rstest]
    fn test_parse_balance_field_valid() {
        let result = parse_balance_field("100.5", "test_field", Currency::BTC(), "BTC");
        assert!(result.is_some());
        assert_eq!(result.unwrap().as_f64(), 100.5);
    }

    #[rstest]
    fn test_parse_balance_field_invalid_numeric() {
        let result = parse_balance_field("not_a_number", "test_field", Currency::BTC(), "BTC");
        assert!(result.is_none());
    }

    #[rstest]
    fn test_parse_balance_field_empty() {
        let result = parse_balance_field("", "test_field", Currency::BTC(), "BTC");
        assert!(result.is_none());
    }

    // Note: Tests for parse_account_state with edge cases (empty currency codes, invalid values)
    // are covered by the existing tests using test data files (e.g., http_get_account_balance.json)

    #[rstest]
    fn test_parse_trades() {
        let json_data = load_test_json("http_get_trades.json");
        let parsed: OKXResponse<OKXTrade> = serde_json::from_str(&json_data).unwrap();

        // Basic response envelope
        assert_eq!(parsed.code, "0");
        assert_eq!(parsed.msg, "");
        assert_eq!(parsed.data.len(), 2);

        // Inspect first record
        let trade0 = &parsed.data[0];
        assert_eq!(trade0.inst_id, "BTC-USDT");
        assert_eq!(trade0.px, "102537.9");
        assert_eq!(trade0.sz, "0.00013669");
        assert_eq!(trade0.side, OKXSide::Sell);
        assert_eq!(trade0.trade_id, "734864333");
        assert_eq!(trade0.ts, 1747087163557);

        // Inspect second record
        let trade1 = &parsed.data[1];
        assert_eq!(trade1.inst_id, "BTC-USDT");
        assert_eq!(trade1.px, "102537.9");
        assert_eq!(trade1.sz, "0.0000125");
        assert_eq!(trade1.side, OKXSide::Buy);
        assert_eq!(trade1.trade_id, "734864332");
        assert_eq!(trade1.ts, 1747087161666);
    }

    #[rstest]
    fn test_parse_candlesticks() {
        let json_data = load_test_json("http_get_candlesticks.json");
        let parsed: OKXResponse<OKXCandlestick> = serde_json::from_str(&json_data).unwrap();

        // Basic response envelope
        assert_eq!(parsed.code, "0");
        assert_eq!(parsed.msg, "");
        assert_eq!(parsed.data.len(), 2);

        let bar0 = &parsed.data[0];
        assert_eq!(bar0.0, "1625097600000");
        assert_eq!(bar0.1, "33528.6");
        assert_eq!(bar0.2, "33870.0");
        assert_eq!(bar0.3, "33528.6");
        assert_eq!(bar0.4, "33783.9");
        assert_eq!(bar0.5, "778.838");

        let bar1 = &parsed.data[1];
        assert_eq!(bar1.0, "1625097660000");
        assert_eq!(bar1.1, "33783.9");
        assert_eq!(bar1.2, "33783.9");
        assert_eq!(bar1.3, "33782.1");
        assert_eq!(bar1.4, "33782.1");
        assert_eq!(bar1.5, "0.123");
    }

    #[rstest]
    fn test_parse_candlesticks_full() {
        let json_data = load_test_json("http_get_candlesticks_full.json");
        let parsed: OKXResponse<OKXCandlestick> = serde_json::from_str(&json_data).unwrap();

        // Basic response envelope
        assert_eq!(parsed.code, "0");
        assert_eq!(parsed.msg, "");
        assert_eq!(parsed.data.len(), 2);

        // Inspect first record
        let bar0 = &parsed.data[0];
        assert_eq!(bar0.0, "1747094040000");
        assert_eq!(bar0.1, "102806.1");
        assert_eq!(bar0.2, "102820.4");
        assert_eq!(bar0.3, "102806.1");
        assert_eq!(bar0.4, "102820.4");
        assert_eq!(bar0.5, "1040.37");
        assert_eq!(bar0.6, "10.4037");
        assert_eq!(bar0.7, "1069603.34883");
        assert_eq!(bar0.8, "1");

        // Inspect second record
        let bar1 = &parsed.data[1];
        assert_eq!(bar1.0, "1747093980000");
        assert_eq!(bar1.5, "7164.04");
        assert_eq!(bar1.6, "71.6404");
        assert_eq!(bar1.7, "7364701.57952");
        assert_eq!(bar1.8, "1");
    }

    #[rstest]
    fn test_parse_mark_price() {
        let json_data = load_test_json("http_get_mark_price.json");
        let parsed: OKXResponse<OKXMarkPrice> = serde_json::from_str(&json_data).unwrap();

        // Basic response envelope
        assert_eq!(parsed.code, "0");
        assert_eq!(parsed.msg, "");
        assert_eq!(parsed.data.len(), 1);

        // Inspect first record
        let mark_price = &parsed.data[0];

        assert_eq!(mark_price.inst_id, "BTC-USDT-SWAP");
        assert_eq!(mark_price.mark_px, "84660.1");
        assert_eq!(mark_price.ts, 1744590349506);
    }

    #[rstest]
    fn test_parse_index_price() {
        let json_data = load_test_json("http_get_index_price.json");
        let parsed: OKXResponse<OKXIndexTicker> = serde_json::from_str(&json_data).unwrap();

        // Basic response envelope
        assert_eq!(parsed.code, "0");
        assert_eq!(parsed.msg, "");
        assert_eq!(parsed.data.len(), 1);

        // Inspect first record
        let index_price = &parsed.data[0];

        assert_eq!(index_price.inst_id, "BTC-USDT");
        assert_eq!(index_price.idx_px, "103895");
        assert_eq!(index_price.ts, 1746942707815);
    }

    #[rstest]
    fn test_parse_account() {
        let json_data = load_test_json("http_get_account_balance.json");
        let parsed: OKXResponse<OKXAccount> = serde_json::from_str(&json_data).unwrap();

        // Basic response envelope
        assert_eq!(parsed.code, "0");
        assert_eq!(parsed.msg, "");
        assert_eq!(parsed.data.len(), 1);

        // Inspect first record
        let account = &parsed.data[0];
        assert_eq!(account.adj_eq, "");
        assert_eq!(account.borrow_froz, "");
        assert_eq!(account.imr, "");
        assert_eq!(account.iso_eq, "5.4682385526666675");
        assert_eq!(account.mgn_ratio, "");
        assert_eq!(account.mmr, "");
        assert_eq!(account.notional_usd, "");
        assert_eq!(account.notional_usd_for_borrow, "");
        assert_eq!(account.notional_usd_for_futures, "");
        assert_eq!(account.notional_usd_for_option, "");
        assert_eq!(account.notional_usd_for_swap, "");
        assert_eq!(account.ord_froz, "");
        assert_eq!(account.total_eq, "99.88870288820581");
        assert_eq!(account.upl, "");
        assert_eq!(account.u_time, 1744499648556);
        assert_eq!(account.details.len(), 1);

        let detail = &account.details[0];
        assert_eq!(detail.ccy, "USDT");
        assert_eq!(detail.avail_bal, "94.42612990333333");
        assert_eq!(detail.avail_eq, "94.42612990333333");
        assert_eq!(detail.cash_bal, "94.42612990333333");
        assert_eq!(detail.dis_eq, "5.4682385526666675");
        assert_eq!(detail.eq, "99.89469657000001");
        assert_eq!(detail.eq_usd, "99.88870288820581");
        assert_eq!(detail.fixed_bal, "0");
        assert_eq!(detail.frozen_bal, "5.468566666666667");
        assert_eq!(detail.imr, "0");
        assert_eq!(detail.iso_eq, "5.468566666666667");
        assert_eq!(detail.iso_upl, "-0.0273000000000002");
        assert_eq!(detail.mmr, "0");
        assert_eq!(detail.notional_lever, "0");
        assert_eq!(detail.ord_frozen, "0");
        assert_eq!(detail.reward_bal, "0");
        assert_eq!(detail.smt_sync_eq, "0");
        assert_eq!(detail.spot_copy_trading_eq, "0");
        assert_eq!(detail.spot_iso_bal, "0");
        assert_eq!(detail.stgy_eq, "0");
        assert_eq!(detail.twap, "0");
        assert_eq!(detail.upl, "-0.0273000000000002");
        assert_eq!(detail.u_time, 1744498994783);
    }

    #[rstest]
    fn test_parse_order_history() {
        let json_data = load_test_json("http_get_orders_history.json");
        let parsed: OKXResponse<OKXOrderHistory> = serde_json::from_str(&json_data).unwrap();

        // Basic response envelope
        assert_eq!(parsed.code, "0");
        assert_eq!(parsed.msg, "");
        assert_eq!(parsed.data.len(), 1);

        // Inspect first record
        let order = &parsed.data[0];
        assert_eq!(order.ord_id, "2497956918703120384");
        assert_eq!(order.fill_sz, "0.03");
        assert_eq!(order.acc_fill_sz, "0.03");
        assert_eq!(order.state, OKXOrderStatus::Filled);
        assert!(order.fill_fee.is_none());
    }

    #[rstest]
    fn test_parse_position() {
        let json_data = load_test_json("http_get_positions.json");
        let parsed: OKXResponse<OKXPosition> = serde_json::from_str(&json_data).unwrap();

        // Basic response envelope
        assert_eq!(parsed.code, "0");
        assert_eq!(parsed.msg, "");
        assert_eq!(parsed.data.len(), 1);

        // Inspect first record
        let pos = &parsed.data[0];
        assert_eq!(pos.inst_id, "BTC-USDT-SWAP");
        assert_eq!(pos.pos_side, OKXPositionSide::Long);
        assert_eq!(pos.pos, "0.5");
        assert_eq!(pos.base_bal, "0.5");
        assert_eq!(pos.quote_bal, "5000");
        assert_eq!(pos.u_time, 1622559930237);
    }

    #[rstest]
    fn test_parse_position_history() {
        let json_data = load_test_json("http_get_account_positions-history.json");
        let parsed: OKXResponse<OKXPositionHistory> = serde_json::from_str(&json_data).unwrap();

        // Basic response envelope
        assert_eq!(parsed.code, "0");
        assert_eq!(parsed.msg, "");
        assert_eq!(parsed.data.len(), 1);

        // Inspect first record
        let hist = &parsed.data[0];
        assert_eq!(hist.inst_id, "ETH-USDT-SWAP");
        assert_eq!(hist.inst_type, OKXInstrumentType::Swap);
        assert_eq!(hist.mgn_mode, OKXMarginMode::Isolated);
        assert_eq!(hist.pos_side, OKXPositionSide::Long);
        assert_eq!(hist.lever, "3.0");
        assert_eq!(hist.open_avg_px, "3226.93");
        assert_eq!(hist.close_avg_px.as_deref(), Some("3224.8"));
        assert_eq!(hist.pnl.as_deref(), Some("-0.0213"));
        assert!(!hist.c_time.is_empty());
        assert!(hist.u_time > 0);
    }

    #[rstest]
    fn test_parse_position_tiers() {
        let json_data = load_test_json("http_get_position_tiers.json");
        let parsed: OKXResponse<OKXPositionTier> = serde_json::from_str(&json_data).unwrap();

        // Basic response envelope
        assert_eq!(parsed.code, "0");
        assert_eq!(parsed.msg, "");
        assert_eq!(parsed.data.len(), 1);

        // Inspect first tier record
        let tier = &parsed.data[0];
        assert_eq!(tier.inst_id, "BTC-USDT");
        assert_eq!(tier.tier, "1");
        assert_eq!(tier.min_sz, "0");
        assert_eq!(tier.max_sz, "50");
        assert_eq!(tier.imr, "0.1");
        assert_eq!(tier.mmr, "0.03");
    }

    #[rstest]
    fn test_parse_account_field_name_compatibility() {
        // Test with new field names (with Amt suffix)
        let json_new = load_test_json("http_balance_detail_new_fields.json");
        let detail_new: OKXBalanceDetail = serde_json::from_str(&json_new).unwrap();
        assert_eq!(detail_new.max_spot_in_use_amt, "50.0");
        assert_eq!(detail_new.spot_in_use_amt, "30.0");
        assert_eq!(detail_new.cl_spot_in_use_amt, "25.0");

        // Test with old field names (without Amt suffix) - for backward compatibility
        let json_old = load_test_json("http_balance_detail_old_fields.json");
        let detail_old: OKXBalanceDetail = serde_json::from_str(&json_old).unwrap();
        assert_eq!(detail_old.max_spot_in_use_amt, "75.0");
        assert_eq!(detail_old.spot_in_use_amt, "40.0");
        assert_eq!(detail_old.cl_spot_in_use_amt, "35.0");
    }

    #[rstest]
    fn test_parse_place_order_response() {
        let json_data = load_test_json("http_place_order_response.json");
        let parsed: OKXPlaceOrderResponse = serde_json::from_str(&json_data).unwrap();
        assert_eq!(
            parsed.ord_id,
            Some(ustr::Ustr::from("12345678901234567890"))
        );
        assert_eq!(parsed.cl_ord_id, Some(ustr::Ustr::from("client_order_123")));
        assert_eq!(parsed.tag, Some("".to_string()));
    }

    #[rstest]
    fn test_parse_transaction_details() {
        let json_data = load_test_json("http_transaction_detail.json");
        let parsed: OKXTransactionDetail = serde_json::from_str(&json_data).unwrap();
        assert_eq!(parsed.inst_type, OKXInstrumentType::Spot);
        assert_eq!(parsed.inst_id, Ustr::from("BTC-USDT"));
        assert_eq!(parsed.trade_id, Ustr::from("123456789"));
        assert_eq!(parsed.ord_id, Ustr::from("987654321"));
        assert_eq!(parsed.cl_ord_id, Ustr::from("client_123"));
        assert_eq!(parsed.bill_id, Ustr::from("bill_456"));
        assert_eq!(parsed.fill_px, "42000.5");
        assert_eq!(parsed.fill_sz, "0.001");
        assert_eq!(parsed.side, OKXSide::Buy);
        assert_eq!(parsed.exec_type, OKXExecType::Taker);
        assert_eq!(parsed.fee_ccy, "USDT");
        assert_eq!(parsed.fee, Some("0.042".to_string()));
        assert_eq!(parsed.ts, 1625097600000);
    }

    #[rstest]
    fn test_parse_empty_fee_field() {
        let json_data = load_test_json("http_transaction_detail_empty_fee.json");
        let parsed: OKXTransactionDetail = serde_json::from_str(&json_data).unwrap();
        assert_eq!(parsed.fee, None);
    }

    #[rstest]
    fn test_parse_optional_string_to_u64() {
        use serde::Deserialize;

        #[derive(Deserialize)]
        struct TestStruct {
            #[serde(deserialize_with = "crate::common::parse::deserialize_optional_string_to_u64")]
            value: Option<u64>,
        }

        let json_cases = load_test_json("common_optional_string_to_u64.json");
        let cases: Vec<TestStruct> = serde_json::from_str(&json_cases).unwrap();

        assert_eq!(cases[0].value, Some(12345));
        assert_eq!(cases[1].value, None);
        assert_eq!(cases[2].value, None);
    }

    #[rstest]
    fn test_parse_error_handling() {
        // Test error handling with invalid price string
        let invalid_price = "invalid-price";
        let result = crate::common::parse::parse_price(invalid_price, 2);
        assert!(result.is_err());

        // Test error handling with invalid quantity string
        let invalid_quantity = "invalid-quantity";
        let result = crate::common::parse::parse_quantity(invalid_quantity, 8);
        assert!(result.is_err());
    }

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
        assert_eq!(instrument.settlement_currency(), Currency::USD());
        assert_eq!(instrument.price_precision(), 1);
        assert_eq!(instrument.size_precision(), 8);
        assert_eq!(instrument.price_increment(), Price::from("0.1"));
        assert_eq!(instrument.size_increment(), Quantity::from("0.00000001"));
        assert_eq!(instrument.multiplier(), Quantity::from(1));
        assert_eq!(instrument.lot_size(), Some(Quantity::from("0.00000001")));
        assert_eq!(instrument.max_quantity(), Some(Quantity::from(1000000)));
        assert_eq!(instrument.min_quantity(), Some(Quantity::from("0.00001")));
        assert_eq!(instrument.max_notional(), None);
        assert_eq!(instrument.min_notional(), None);
        assert_eq!(instrument.max_price(), None);
        assert_eq!(instrument.min_price(), None);
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
        assert_eq!(instrument.settlement_currency(), Currency::USDT());
        assert_eq!(instrument.price_precision(), 1);
        assert_eq!(instrument.size_precision(), 8);
        assert_eq!(instrument.price_increment(), Price::from("0.1"));
        assert_eq!(instrument.size_increment(), Quantity::from("0.00000001"));
        assert_eq!(instrument.multiplier(), Quantity::from(1));
        assert_eq!(instrument.lot_size(), Some(Quantity::from("0.00000001")));
        assert_eq!(instrument.max_quantity(), Some(Quantity::from(1000000)));
        assert_eq!(instrument.min_quantity(), Some(Quantity::from("0.00001")));
        assert_eq!(instrument.max_notional(), None);
        assert_eq!(instrument.min_notional(), None);
        assert_eq!(instrument.max_price(), None);
        assert_eq!(instrument.min_price(), None);
    }

    #[rstest]
    fn test_parse_spot_instrument_with_valid_ct_mult() {
        let json_data = load_test_json("http_get_instruments_spot.json");
        let mut response: OKXResponse<OKXInstrument> = serde_json::from_str(&json_data).unwrap();

        // Modify ctMult to have a valid multiplier value (ctVal is empty, defaults to 1)
        if let Some(inst) = response.data.first_mut() {
            inst.ct_mult = "0.01".to_string();
        }

        let okx_inst = response.data.first().unwrap();
        let instrument =
            parse_spot_instrument(okx_inst, None, None, None, None, UnixNanos::default()).unwrap();

        // Should parse the multiplier as product of ctMult * ctVal (0.01 * 1 = 0.01)
        if let InstrumentAny::CurrencyPair(pair) = instrument {
            assert_eq!(pair.multiplier, Quantity::from("0.01"));
        } else {
            panic!("Expected CurrencyPair instrument");
        }
    }

    #[rstest]
    fn test_parse_spot_instrument_with_invalid_ct_mult() {
        let json_data = load_test_json("http_get_instruments_spot.json");
        let mut response: OKXResponse<OKXInstrument> = serde_json::from_str(&json_data).unwrap();

        // Modify ctMult to be invalid
        if let Some(inst) = response.data.first_mut() {
            inst.ct_mult = "invalid_number".to_string();
        }

        let okx_inst = response.data.first().unwrap();
        let result = parse_spot_instrument(okx_inst, None, None, None, None, UnixNanos::default());

        // Should error instead of silently defaulting to 1.0
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Failed to parse `ct_mult`")
        );
    }

    #[rstest]
    fn test_parse_spot_instrument_with_fees() {
        let json_data = load_test_json("http_get_instruments_spot.json");
        let response: OKXResponse<OKXInstrument> = serde_json::from_str(&json_data).unwrap();
        let okx_inst = response.data.first().unwrap();

        let maker_fee = Some(Decimal::new(8, 4)); // 0.0008
        let taker_fee = Some(Decimal::new(10, 4)); // 0.0010

        let instrument = parse_spot_instrument(
            okx_inst,
            None,
            None,
            maker_fee,
            taker_fee,
            UnixNanos::default(),
        )
        .unwrap();

        // Should apply the provided fees to the instrument
        if let InstrumentAny::CurrencyPair(pair) = instrument {
            assert_eq!(pair.maker_fee, Decimal::new(8, 4));
            assert_eq!(pair.taker_fee, Decimal::new(10, 4));
        } else {
            panic!("Expected CurrencyPair instrument");
        }
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
        assert_eq!(instrument.settlement_currency(), Currency::BTC());
        assert!(instrument.is_inverse());
        assert_eq!(instrument.price_precision(), 1);
        assert_eq!(instrument.size_precision(), 0);
        assert_eq!(instrument.price_increment(), Price::from("0.1"));
        assert_eq!(instrument.size_increment(), Quantity::from(1));
        assert_eq!(instrument.multiplier(), Quantity::from(100));
        assert_eq!(instrument.lot_size(), Some(Quantity::from(1)));
        assert_eq!(instrument.max_quantity(), Some(Quantity::from(30000)));
        assert_eq!(instrument.min_quantity(), Some(Quantity::from(1)));
        assert_eq!(instrument.max_notional(), None);
        assert_eq!(instrument.min_notional(), None);
        assert_eq!(instrument.max_price(), None);
        assert_eq!(instrument.min_price(), None);
    }

    #[rstest]
    fn test_parse_linear_swap_instrument() {
        let json_data = load_test_json("http_get_instruments_swap.json");
        let response: OKXResponse<OKXInstrument> = serde_json::from_str(&json_data).unwrap();

        let okx_inst = response
            .data
            .iter()
            .find(|i| i.inst_id == "ETH-USDT-SWAP")
            .expect("ETH-USDT-SWAP must be in test data");

        let instrument =
            parse_swap_instrument(okx_inst, None, None, None, None, UnixNanos::default()).unwrap();

        assert_eq!(instrument.id(), InstrumentId::from("ETH-USDT-SWAP.OKX"));
        assert_eq!(instrument.raw_symbol(), Symbol::from("ETH-USDT-SWAP"));
        assert_eq!(instrument.base_currency(), Some(Currency::ETH()));
        assert_eq!(instrument.quote_currency(), Currency::USDT());
        assert_eq!(instrument.settlement_currency(), Currency::USDT());
        assert!(!instrument.is_inverse());
        assert_eq!(instrument.multiplier(), Quantity::from("0.1"));
        assert_eq!(instrument.price_precision(), 2);
        assert_eq!(instrument.size_precision(), 2);
        assert_eq!(instrument.price_increment(), Price::from("0.01"));
        assert_eq!(instrument.size_increment(), Quantity::from("0.01"));
        assert_eq!(instrument.lot_size(), Some(Quantity::from("0.01")));
        assert_eq!(instrument.min_quantity(), Some(Quantity::from("0.01")));
        assert_eq!(instrument.max_quantity(), Some(Quantity::from(20000)));
    }

    #[rstest]
    fn test_fee_field_selection_for_contract_types() {
        use rust_decimal::Decimal;

        // Mock OKXFeeRate with different values for crypto vs USDT-margined
        let maker_crypto = "0.0002"; // Crypto-margined maker fee
        let taker_crypto = "0.0005"; // Crypto-margined taker fee
        let maker_usdt = "0.0008"; // USDT-margined maker fee
        let taker_usdt = "0.0010"; // USDT-margined taker fee

        // Test Linear (USDT-margined) - should use maker_u/taker_u
        let is_usdt_margined = true;
        let (maker_str, taker_str) = if is_usdt_margined {
            (maker_usdt, taker_usdt)
        } else {
            (maker_crypto, taker_crypto)
        };

        assert_eq!(maker_str, "0.0008");
        assert_eq!(taker_str, "0.0010");

        let maker_fee = Decimal::from_str(maker_str).unwrap();
        let taker_fee = Decimal::from_str(taker_str).unwrap();

        assert_eq!(maker_fee, Decimal::new(8, 4));
        assert_eq!(taker_fee, Decimal::new(10, 4));

        // Test Inverse (crypto-margined) - should use maker/taker
        let is_usdt_margined = false;
        let (maker_str, taker_str) = if is_usdt_margined {
            (maker_usdt, taker_usdt)
        } else {
            (maker_crypto, taker_crypto)
        };

        assert_eq!(maker_str, "0.0002");
        assert_eq!(taker_str, "0.0005");

        let maker_fee = Decimal::from_str(maker_str).unwrap();
        let taker_fee = Decimal::from_str(taker_str).unwrap();

        assert_eq!(maker_fee, Decimal::new(2, 4));
        assert_eq!(taker_fee, Decimal::new(5, 4));
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
        assert_eq!(instrument.settlement_currency(), Currency::BTC());
        assert!(instrument.is_inverse());
        assert_eq!(instrument.price_precision(), 1);
        assert_eq!(instrument.size_precision(), 0);
        assert_eq!(instrument.price_increment(), Price::from("0.1"));
        assert_eq!(instrument.size_increment(), Quantity::from(1));
        assert_eq!(instrument.multiplier(), Quantity::from(100));
        assert_eq!(instrument.lot_size(), Some(Quantity::from(1)));
        assert_eq!(instrument.min_quantity(), Some(Quantity::from(1)));
        assert_eq!(instrument.max_quantity(), Some(Quantity::from(10000)));
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
        assert_eq!(instrument.base_currency(), Some(Currency::BTC()));
        assert_eq!(instrument.quote_currency(), Currency::USD());
        assert_eq!(instrument.settlement_currency(), Currency::BTC());
        assert!(instrument.is_inverse());
        assert_eq!(instrument.price_precision(), 4);
        assert_eq!(instrument.size_precision(), 0);
        assert_eq!(instrument.price_increment(), Price::from("0.0001"));
        assert_eq!(instrument.size_increment(), Quantity::from(1));
        assert_eq!(instrument.multiplier(), Quantity::from("0.01"));
        assert_eq!(instrument.lot_size(), Some(Quantity::from(1)));
        assert_eq!(instrument.min_quantity(), Some(Quantity::from(1)));
        assert_eq!(instrument.max_quantity(), Some(Quantity::from(5000)));
        assert_eq!(instrument.max_notional(), None);
        assert_eq!(instrument.min_notional(), None);
        assert_eq!(instrument.max_price(), None);
        assert_eq!(instrument.min_price(), None);
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
        assert_eq!(account_state.margins.len(), 0); // No margins in this test data (spot account)
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
    fn test_parse_account_state_with_margins() {
        // Create test data with margin requirements
        let account_json = r#"{
            "adjEq": "10000.0",
            "borrowFroz": "0",
            "details": [{
                "accAvgPx": "",
                "availBal": "8000.0",
                "availEq": "8000.0",
                "borrowFroz": "0",
                "cashBal": "10000.0",
                "ccy": "USDT",
                "clSpotInUseAmt": "0",
                "coinUsdPrice": "1.0",
                "colBorrAutoConversion": "0",
                "collateralEnabled": false,
                "collateralRestrict": false,
                "crossLiab": "0",
                "disEq": "10000.0",
                "eq": "10000.0",
                "eqUsd": "10000.0",
                "fixedBal": "0",
                "frozenBal": "2000.0",
                "imr": "0",
                "interest": "0",
                "isoEq": "0",
                "isoLiab": "0",
                "isoUpl": "0",
                "liab": "0",
                "maxLoan": "0",
                "mgnRatio": "0",
                "maxSpotInUseAmt": "0",
                "mmr": "0",
                "notionalLever": "0",
                "openAvgPx": "",
                "ordFrozen": "2000.0",
                "rewardBal": "0",
                "smtSyncEq": "0",
                "spotBal": "0",
                "spotCopyTradingEq": "0",
                "spotInUseAmt": "0",
                "spotIsoBal": "0",
                "spotUpl": "0",
                "spotUplRatio": "0",
                "stgyEq": "0",
                "totalPnl": "0",
                "totalPnlRatio": "0",
                "twap": "0",
                "uTime": "1704067200000",
                "upl": "0",
                "uplLiab": "0"
            }],
            "imr": "500.25",
            "isoEq": "0",
            "mgnRatio": "20.5",
            "mmr": "250.75",
            "notionalUsd": "5000.0",
            "notionalUsdForBorrow": "0",
            "notionalUsdForFutures": "0",
            "notionalUsdForOption": "0",
            "notionalUsdForSwap": "5000.0",
            "ordFroz": "2000.0",
            "totalEq": "10000.0",
            "uTime": "1704067200000",
            "upl": "0"
        }"#;

        let okx_account: OKXAccount = serde_json::from_str(account_json).unwrap();
        let account_id = AccountId::new("OKX-001");
        let account_state =
            parse_account_state(&okx_account, account_id, UnixNanos::default()).unwrap();

        // Verify account details
        assert_eq!(account_state.account_id, account_id);
        assert_eq!(account_state.account_type, AccountType::Margin);
        assert_eq!(account_state.balances.len(), 1);

        // Verify margin information was parsed
        assert_eq!(account_state.margins.len(), 1);
        let margin = &account_state.margins[0];

        // Check margin values
        assert_eq!(margin.initial, Money::new(500.25, Currency::USD()));
        assert_eq!(margin.maintenance, Money::new(250.75, Currency::USD()));
        assert_eq!(margin.currency, Currency::USD());
        assert_eq!(margin.instrument_id.symbol.as_str(), "ACCOUNT");
        assert_eq!(margin.instrument_id.venue.as_str(), "OKX");

        // Check the USDT balance details
        let usdt_balance = &account_state.balances[0];
        assert_eq!(usdt_balance.total, Money::new(10000.0, Currency::USDT()));
        assert_eq!(usdt_balance.free, Money::new(8000.0, Currency::USDT()));
        assert_eq!(usdt_balance.locked, Money::new(2000.0, Currency::USDT()));
    }

    #[rstest]
    fn test_parse_account_state_empty_margins() {
        // Create test data with empty margin strings (common for spot accounts)
        let account_json = r#"{
            "adjEq": "",
            "borrowFroz": "",
            "details": [{
                "accAvgPx": "",
                "availBal": "1000.0",
                "availEq": "1000.0",
                "borrowFroz": "0",
                "cashBal": "1000.0",
                "ccy": "BTC",
                "clSpotInUseAmt": "0",
                "coinUsdPrice": "50000.0",
                "colBorrAutoConversion": "0",
                "collateralEnabled": false,
                "collateralRestrict": false,
                "crossLiab": "0",
                "disEq": "50000.0",
                "eq": "1000.0",
                "eqUsd": "50000.0",
                "fixedBal": "0",
                "frozenBal": "0",
                "imr": "0",
                "interest": "0",
                "isoEq": "0",
                "isoLiab": "0",
                "isoUpl": "0",
                "liab": "0",
                "maxLoan": "0",
                "mgnRatio": "0",
                "maxSpotInUseAmt": "0",
                "mmr": "0",
                "notionalLever": "0",
                "openAvgPx": "",
                "ordFrozen": "0",
                "rewardBal": "0",
                "smtSyncEq": "0",
                "spotBal": "0",
                "spotCopyTradingEq": "0",
                "spotInUseAmt": "0",
                "spotIsoBal": "0",
                "spotUpl": "0",
                "spotUplRatio": "0",
                "stgyEq": "0",
                "totalPnl": "0",
                "totalPnlRatio": "0",
                "twap": "0",
                "uTime": "1704067200000",
                "upl": "0",
                "uplLiab": "0"
            }],
            "imr": "",
            "isoEq": "0",
            "mgnRatio": "",
            "mmr": "",
            "notionalUsd": "",
            "notionalUsdForBorrow": "",
            "notionalUsdForFutures": "",
            "notionalUsdForOption": "",
            "notionalUsdForSwap": "",
            "ordFroz": "",
            "totalEq": "50000.0",
            "uTime": "1704067200000",
            "upl": "0"
        }"#;

        let okx_account: OKXAccount = serde_json::from_str(account_json).unwrap();
        let account_id = AccountId::new("OKX-SPOT");
        let account_state =
            parse_account_state(&okx_account, account_id, UnixNanos::default()).unwrap();

        // Verify no margins are created when fields are empty
        assert_eq!(account_state.margins.len(), 0);
        assert_eq!(account_state.balances.len(), 1);

        // Check the BTC balance
        let btc_balance = &account_state.balances[0];
        assert_eq!(btc_balance.total, Money::new(1000.0, Currency::BTC()));
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
            &okx_order,
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
        )
        .unwrap();

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

    #[rstest]
    fn test_bar_type_identity_preserved_through_parse() {
        use std::str::FromStr;

        use crate::http::models::OKXCandlestick;

        // Create a BarType
        let bar_type = BarType::from_str("ETH-USDT-SWAP.OKX-1-MINUTE-LAST-EXTERNAL").unwrap();

        // Create sample candlestick data
        let raw_candlestick = OKXCandlestick(
            "1721807460000".to_string(), // timestamp
            "3177.9".to_string(),        // open
            "3177.9".to_string(),        // high
            "3177.7".to_string(),        // low
            "3177.8".to_string(),        // close
            "18.603".to_string(),        // volume
            "59054.8231".to_string(),    // turnover
            "18.603".to_string(),        // base_volume
            "1".to_string(),             // count
        );

        // Parse the candlestick
        let bar =
            parse_candlestick(&raw_candlestick, bar_type, 1, 3, UnixNanos::default()).unwrap();

        // Verify that the BarType is preserved exactly
        assert_eq!(
            bar.bar_type, bar_type,
            "BarType must be preserved exactly through parsing"
        );
    }

    #[rstest]
    fn test_deserialize_vip_level_all_formats() {
        use serde::Deserialize;
        use serde_json;

        #[derive(Deserialize)]
        struct TestFeeRate {
            #[serde(deserialize_with = "crate::common::parse::deserialize_vip_level")]
            level: OKXVipLevel,
        }

        // Test VIP prefix format
        let json = r#"{"level":"VIP4"}"#;
        let result: TestFeeRate = serde_json::from_str(json).unwrap();
        assert_eq!(result.level, OKXVipLevel::Vip4);

        let json = r#"{"level":"VIP5"}"#;
        let result: TestFeeRate = serde_json::from_str(json).unwrap();
        assert_eq!(result.level, OKXVipLevel::Vip5);

        // Test Lv prefix format
        let json = r#"{"level":"Lv1"}"#;
        let result: TestFeeRate = serde_json::from_str(json).unwrap();
        assert_eq!(result.level, OKXVipLevel::Vip1);

        let json = r#"{"level":"Lv0"}"#;
        let result: TestFeeRate = serde_json::from_str(json).unwrap();
        assert_eq!(result.level, OKXVipLevel::Vip0);

        let json = r#"{"level":"Lv9"}"#;
        let result: TestFeeRate = serde_json::from_str(json).unwrap();
        assert_eq!(result.level, OKXVipLevel::Vip9);
    }

    #[rstest]
    fn test_deserialize_vip_level_empty_string() {
        use serde::Deserialize;
        use serde_json;

        #[derive(Deserialize)]
        struct TestFeeRate {
            #[serde(deserialize_with = "crate::common::parse::deserialize_vip_level")]
            level: OKXVipLevel,
        }

        // Empty string should default to VIP0
        let json = r#"{"level":""}"#;
        let result: TestFeeRate = serde_json::from_str(json).unwrap();
        assert_eq!(result.level, OKXVipLevel::Vip0);
    }

    #[rstest]
    fn test_deserialize_vip_level_without_prefix() {
        use serde::Deserialize;
        use serde_json;

        #[derive(Deserialize)]
        struct TestFeeRate {
            #[serde(deserialize_with = "crate::common::parse::deserialize_vip_level")]
            level: OKXVipLevel,
        }

        let json = r#"{"level":"5"}"#;
        let result: TestFeeRate = serde_json::from_str(json).unwrap();
        assert_eq!(result.level, OKXVipLevel::Vip5);
    }

    #[rstest]
    fn test_parse_position_status_report_net_mode_long() {
        // Test Net mode: positive quantity = Long position
        let position = OKXPosition {
            inst_id: Ustr::from("BTC-USDT-SWAP"),
            inst_type: OKXInstrumentType::Swap,
            mgn_mode: OKXMarginMode::Cross,
            pos_id: Some(Ustr::from("12345")),
            pos_side: OKXPositionSide::Net, // Net mode
            pos: "1.5".to_string(),         // Positive = Long
            base_bal: "1.5".to_string(),
            ccy: "BTC".to_string(),
            fee: "0.01".to_string(),
            lever: "10.0".to_string(),
            last: "50000".to_string(),
            mark_px: "50000".to_string(),
            liq_px: "45000".to_string(),
            mmr: "0.1".to_string(),
            interest: "0".to_string(),
            trade_id: Ustr::from("111"),
            notional_usd: "75000".to_string(),
            avg_px: "50000".to_string(),
            upl: "0".to_string(),
            upl_ratio: "0".to_string(),
            u_time: 1622559930237,
            margin: "0.5".to_string(),
            mgn_ratio: "0.01".to_string(),
            adl: "0".to_string(),
            c_time: "1622559930237".to_string(),
            realized_pnl: "0".to_string(),
            upl_last_px: "0".to_string(),
            upl_ratio_last_px: "0".to_string(),
            avail_pos: "1.5".to_string(),
            be_px: "0".to_string(),
            funding_fee: "0".to_string(),
            idx_px: "0".to_string(),
            liq_penalty: "0".to_string(),
            opt_val: "0".to_string(),
            pending_close_ord_liab_val: "0".to_string(),
            pnl: "0".to_string(),
            pos_ccy: "BTC".to_string(),
            quote_bal: "75000".to_string(),
            quote_borrowed: "0".to_string(),
            quote_interest: "0".to_string(),
            spot_in_use_amt: "0".to_string(),
            spot_in_use_ccy: "BTC".to_string(),
            usd_px: "50000".to_string(),
        };

        let account_id = AccountId::new("OKX-001");
        let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");
        let report = parse_position_status_report(
            position,
            account_id,
            instrument_id,
            8,
            UnixNanos::default(),
        )
        .unwrap();

        assert_eq!(report.account_id, account_id);
        assert_eq!(report.instrument_id, instrument_id);
        assert_eq!(report.position_side, PositionSide::Long.as_specified());
        assert_eq!(report.quantity, Quantity::from("1.5"));
        // Net mode: venue_position_id is None (signals NETTING OMS)
        assert_eq!(report.venue_position_id, None);
    }

    #[rstest]
    fn test_parse_position_status_report_net_mode_short() {
        // Test Net mode: negative quantity = Short position
        let position = OKXPosition {
            inst_id: Ustr::from("BTC-USDT-SWAP"),
            inst_type: OKXInstrumentType::Swap,
            mgn_mode: OKXMarginMode::Isolated,
            pos_id: Some(Ustr::from("67890")),
            pos_side: OKXPositionSide::Net, // Net mode
            pos: "-2.3".to_string(),        // Negative = Short
            base_bal: "2.3".to_string(),
            ccy: "BTC".to_string(),
            fee: "0.02".to_string(),
            lever: "5.0".to_string(),
            last: "50000".to_string(),
            mark_px: "50000".to_string(),
            liq_px: "55000".to_string(),
            mmr: "0.2".to_string(),
            interest: "0".to_string(),
            trade_id: Ustr::from("222"),
            notional_usd: "115000".to_string(),
            avg_px: "50000".to_string(),
            upl: "0".to_string(),
            upl_ratio: "0".to_string(),
            u_time: 1622559930237,
            margin: "1.0".to_string(),
            mgn_ratio: "0.02".to_string(),
            adl: "0".to_string(),
            c_time: "1622559930237".to_string(),
            realized_pnl: "0".to_string(),
            upl_last_px: "0".to_string(),
            upl_ratio_last_px: "0".to_string(),
            avail_pos: "2.3".to_string(),
            be_px: "0".to_string(),
            funding_fee: "0".to_string(),
            idx_px: "0".to_string(),
            liq_penalty: "0".to_string(),
            opt_val: "0".to_string(),
            pending_close_ord_liab_val: "0".to_string(),
            pnl: "0".to_string(),
            pos_ccy: "BTC".to_string(),
            quote_bal: "115000".to_string(),
            quote_borrowed: "0".to_string(),
            quote_interest: "0".to_string(),
            spot_in_use_amt: "0".to_string(),
            spot_in_use_ccy: "BTC".to_string(),
            usd_px: "50000".to_string(),
        };

        let account_id = AccountId::new("OKX-001");
        let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");
        let report = parse_position_status_report(
            position,
            account_id,
            instrument_id,
            8,
            UnixNanos::default(),
        )
        .unwrap();

        assert_eq!(report.account_id, account_id);
        assert_eq!(report.instrument_id, instrument_id);
        assert_eq!(report.position_side, PositionSide::Short.as_specified());
        assert_eq!(report.quantity, Quantity::from("2.3")); // Absolute value
        // Net mode: venue_position_id is None (signals NETTING OMS)
        assert_eq!(report.venue_position_id, None);
    }

    #[rstest]
    fn test_parse_position_status_report_net_mode_flat() {
        // Test Net mode: zero quantity = Flat position
        let position = OKXPosition {
            inst_id: Ustr::from("ETH-USDT-SWAP"),
            inst_type: OKXInstrumentType::Swap,
            mgn_mode: OKXMarginMode::Cross,
            pos_id: Some(Ustr::from("99999")),
            pos_side: OKXPositionSide::Net, // Net mode
            pos: "0".to_string(),           // Zero = Flat
            base_bal: "0".to_string(),
            ccy: "ETH".to_string(),
            fee: "0".to_string(),
            lever: "10.0".to_string(),
            last: "3000".to_string(),
            mark_px: "3000".to_string(),
            liq_px: "0".to_string(),
            mmr: "0".to_string(),
            interest: "0".to_string(),
            trade_id: Ustr::from("333"),
            notional_usd: "0".to_string(),
            avg_px: "".to_string(),
            upl: "0".to_string(),
            upl_ratio: "0".to_string(),
            u_time: 1622559930237,
            margin: "0".to_string(),
            mgn_ratio: "0".to_string(),
            adl: "0".to_string(),
            c_time: "1622559930237".to_string(),
            realized_pnl: "0".to_string(),
            upl_last_px: "0".to_string(),
            upl_ratio_last_px: "0".to_string(),
            avail_pos: "0".to_string(),
            be_px: "0".to_string(),
            funding_fee: "0".to_string(),
            idx_px: "0".to_string(),
            liq_penalty: "0".to_string(),
            opt_val: "0".to_string(),
            pending_close_ord_liab_val: "0".to_string(),
            pnl: "0".to_string(),
            pos_ccy: "ETH".to_string(),
            quote_bal: "0".to_string(),
            quote_borrowed: "0".to_string(),
            quote_interest: "0".to_string(),
            spot_in_use_amt: "0".to_string(),
            spot_in_use_ccy: "ETH".to_string(),
            usd_px: "3000".to_string(),
        };

        let account_id = AccountId::new("OKX-001");
        let instrument_id = InstrumentId::from("ETH-USDT-SWAP.OKX");
        let report = parse_position_status_report(
            position,
            account_id,
            instrument_id,
            8,
            UnixNanos::default(),
        )
        .unwrap();

        assert_eq!(report.account_id, account_id);
        assert_eq!(report.instrument_id, instrument_id);
        assert_eq!(report.position_side, PositionSide::Flat.as_specified());
        assert_eq!(report.quantity, Quantity::from("0"));
        // Net mode: venue_position_id is None (signals NETTING OMS)
        assert_eq!(report.venue_position_id, None);
    }

    #[rstest]
    fn test_parse_position_status_report_long_short_mode_long() {
        // Test Long/Short mode: posSide="long" with positive quantity
        let position = OKXPosition {
            inst_id: Ustr::from("BTC-USDT-SWAP"),
            inst_type: OKXInstrumentType::Swap,
            mgn_mode: OKXMarginMode::Cross,
            pos_id: Some(Ustr::from("11111")),
            pos_side: OKXPositionSide::Long, // Long/Short mode - Long leg
            pos: "3.2".to_string(),          // Positive quantity (always positive in this mode)
            base_bal: "3.2".to_string(),
            ccy: "BTC".to_string(),
            fee: "0.01".to_string(),
            lever: "10.0".to_string(),
            last: "50000".to_string(),
            mark_px: "50000".to_string(),
            liq_px: "45000".to_string(),
            mmr: "0.1".to_string(),
            interest: "0".to_string(),
            trade_id: Ustr::from("444"),
            notional_usd: "160000".to_string(),
            avg_px: "50000".to_string(),
            upl: "0".to_string(),
            upl_ratio: "0".to_string(),
            u_time: 1622559930237,
            margin: "1.6".to_string(),
            mgn_ratio: "0.01".to_string(),
            adl: "0".to_string(),
            c_time: "1622559930237".to_string(),
            realized_pnl: "0".to_string(),
            upl_last_px: "0".to_string(),
            upl_ratio_last_px: "0".to_string(),
            avail_pos: "3.2".to_string(),
            be_px: "0".to_string(),
            funding_fee: "0".to_string(),
            idx_px: "0".to_string(),
            liq_penalty: "0".to_string(),
            opt_val: "0".to_string(),
            pending_close_ord_liab_val: "0".to_string(),
            pnl: "0".to_string(),
            pos_ccy: "BTC".to_string(),
            quote_bal: "160000".to_string(),
            quote_borrowed: "0".to_string(),
            quote_interest: "0".to_string(),
            spot_in_use_amt: "0".to_string(),
            spot_in_use_ccy: "BTC".to_string(),
            usd_px: "50000".to_string(),
        };

        let account_id = AccountId::new("OKX-001");
        let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");
        let report = parse_position_status_report(
            position,
            account_id,
            instrument_id,
            8,
            UnixNanos::default(),
        )
        .unwrap();

        assert_eq!(report.account_id, account_id);
        assert_eq!(report.instrument_id, instrument_id);
        assert_eq!(report.position_side, PositionSide::Long.as_specified());
        assert_eq!(report.quantity, Quantity::from("3.2"));
        // Long/Short mode - Long leg: "-LONG" suffix
        assert_eq!(
            report.venue_position_id,
            Some(PositionId::new("11111-LONG"))
        );
    }

    #[rstest]
    fn test_parse_position_status_report_long_short_mode_short() {
        // Test Long/Short mode: posSide="short" with positive quantity
        // This is the critical test - positive quantity but SHORT side!
        let position = OKXPosition {
            inst_id: Ustr::from("BTC-USDT-SWAP"),
            inst_type: OKXInstrumentType::Swap,
            mgn_mode: OKXMarginMode::Cross,
            pos_id: Some(Ustr::from("22222")),
            pos_side: OKXPositionSide::Short, // Long/Short mode - Short leg
            pos: "1.8".to_string(),           // Positive quantity (always positive in this mode)
            base_bal: "1.8".to_string(),
            ccy: "BTC".to_string(),
            fee: "0.02".to_string(),
            lever: "10.0".to_string(),
            last: "50000".to_string(),
            mark_px: "50000".to_string(),
            liq_px: "55000".to_string(),
            mmr: "0.2".to_string(),
            interest: "0".to_string(),
            trade_id: Ustr::from("555"),
            notional_usd: "90000".to_string(),
            avg_px: "50000".to_string(),
            upl: "0".to_string(),
            upl_ratio: "0".to_string(),
            u_time: 1622559930237,
            margin: "0.9".to_string(),
            mgn_ratio: "0.02".to_string(),
            adl: "0".to_string(),
            c_time: "1622559930237".to_string(),
            realized_pnl: "0".to_string(),
            upl_last_px: "0".to_string(),
            upl_ratio_last_px: "0".to_string(),
            avail_pos: "1.8".to_string(),
            be_px: "0".to_string(),
            funding_fee: "0".to_string(),
            idx_px: "0".to_string(),
            liq_penalty: "0".to_string(),
            opt_val: "0".to_string(),
            pending_close_ord_liab_val: "0".to_string(),
            pnl: "0".to_string(),
            pos_ccy: "BTC".to_string(),
            quote_bal: "90000".to_string(),
            quote_borrowed: "0".to_string(),
            quote_interest: "0".to_string(),
            spot_in_use_amt: "0".to_string(),
            spot_in_use_ccy: "BTC".to_string(),
            usd_px: "50000".to_string(),
        };

        let account_id = AccountId::new("OKX-001");
        let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");
        let report = parse_position_status_report(
            position,
            account_id,
            instrument_id,
            8,
            UnixNanos::default(),
        )
        .unwrap();

        assert_eq!(report.account_id, account_id);
        assert_eq!(report.instrument_id, instrument_id);
        // This is the critical assertion: positive quantity but SHORT side
        assert_eq!(report.position_side, PositionSide::Short.as_specified());
        assert_eq!(report.quantity, Quantity::from("1.8"));
        // Long/Short mode - Short leg: "-SHORT" suffix
        assert_eq!(
            report.venue_position_id,
            Some(PositionId::new("22222-SHORT"))
        );
    }
}
