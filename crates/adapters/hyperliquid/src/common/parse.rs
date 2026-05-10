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

//! Parsing utilities that convert Hyperliquid payloads into Nautilus domain models.
//!
//! # Conditional Order Support
//!
//! This module implements conditional order support for Hyperliquid,
//! following patterns established in the OKX, Bybit, and BitMEX adapters.
//!
//! ## Supported Order Types
//!
//! ### Standard Orders
//! - **Market**: Implemented as IOC (Immediate-or-Cancel) limit orders.
//! - **Limit**: Standard limit orders with GTC/IOC/ALO time-in-force.
//!
//! ### Conditional/Trigger Orders
//! - **StopMarket**: Protective stop that triggers at specified price and executes at market.
//! - **StopLimit**: Protective stop that triggers at specified price and executes at limit.
//! - **MarketIfTouched**: Profit-taking/entry order that triggers and executes at market.
//! - **LimitIfTouched**: Profit-taking/entry order that triggers and executes at limit.
//!
//! ## Order Semantics
//!
//! ### Stop Orders (StopMarket/StopLimit)
//! - Used for protective stops and risk management.
//! - Mapped to Hyperliquid's trigger orders with `tpsl: Sl`.
//! - Trigger when price reaches the stop level.
//! - Execute immediately (market) or at limit price.
//!
//! ### If Touched Orders (MarketIfTouched/LimitIfTouched)
//! - Used for profit-taking or entry orders.
//! - Mapped to Hyperliquid's trigger orders with `tpsl: Tp`.
//! - Trigger when price reaches the target level.
//! - Execute immediately (market) or at limit price.
//!
//! ## Trigger Price Logic
//!
//! The `tpsl` field (Take Profit / Stop Loss) is determined by:
//! 1. **Order Type**: Stop orders → SL, If Touched orders → TP
//! 2. **Price Relationship** (if available):
//!    - For BUY orders: trigger above market → SL, below → TP
//!    - For SELL orders: trigger below market → SL, above → TP
//!
//! ## Trigger Type Support
//!
//! Hyperliquid uses **mark price** for all trigger evaluations (TP/SL orders).

use anyhow::Context;
use nautilus_core::UnixNanos;
pub use nautilus_core::serialization::{
    deserialize_decimal_from_str, deserialize_optional_decimal_from_str,
    deserialize_vec_decimal_from_str, serialize_decimal_as_str, serialize_optional_decimal_as_str,
    serialize_vec_decimal_as_str,
};
use nautilus_model::{
    data::{bar::BarType, quote::QuoteTick},
    enums::{
        AggregationSource, BarAggregation, ContingencyType, OrderSide, OrderStatus, OrderType,
        TimeInForce,
    },
    identifiers::{ClientOrderId, TradeId},
    orders::{Order, any::OrderAny},
    types::{AccountBalance, Currency, MarginBalance, Money},
};
use rust_decimal::Decimal;

use crate::{
    common::enums::{
        HyperliquidBarInterval::{self, *},
        HyperliquidOrderStatus, HyperliquidTpSl,
    },
    http::models::{
        ClearinghouseState, Cloid, HyperliquidExchangeResponse,
        HyperliquidExecCancelByCloidRequest, HyperliquidExecCancelStatus, HyperliquidExecGrouping,
        HyperliquidExecLimitParams, HyperliquidExecModifyStatus, HyperliquidExecOrderKind,
        HyperliquidExecOrderStatus, HyperliquidExecPlaceOrderRequest, HyperliquidExecResponseData,
        HyperliquidExecTif, HyperliquidExecTpSl, HyperliquidExecTriggerParams, RESPONSE_STATUS_OK,
        SpotClearinghouseState,
    },
    websocket::messages::TrailingOffsetType,
};

/// Creates a deterministic [`TradeId`] from fill fields common to both WS and HTTP responses.
///
/// Uses FNV-1a hash of `(hash, oid, px, sz, time, start_position)` to produce a unique
/// identifier consistent across both data sources for the same physical fill.
/// Includes `start_position` (running position before each fill) to disambiguate
/// multiple partial fills within the same transaction at the same price/size.
/// Format: `{fnv_hex}-{oid_hex}` (exactly 33 chars, within 36-char limit).
pub fn make_fill_trade_id(
    hash: &str,
    oid: u64,
    px: &str,
    sz: &str,
    time: u64,
    start_position: &str,
) -> TradeId {
    // FNV-1a with fixed seed for deterministic output
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in hash.as_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }

    for b in oid.to_le_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }

    for &b in px.as_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }

    for &b in sz.as_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }

    for b in time.to_le_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }

    for &b in start_position.as_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    TradeId::new(format!("{h:016x}-{oid:016x}"))
}

/// Round price down to the nearest valid tick size.
#[inline]
pub fn round_down_to_tick(price: Decimal, tick_size: Decimal) -> Decimal {
    if tick_size.is_zero() {
        return price;
    }
    (price / tick_size).floor() * tick_size
}

/// Round quantity down to the nearest valid step size.
#[inline]
pub fn round_down_to_step(qty: Decimal, step_size: Decimal) -> Decimal {
    if step_size.is_zero() {
        return qty;
    }
    (qty / step_size).floor() * step_size
}

/// Ensure the notional value meets minimum requirements.
#[inline]
pub fn ensure_min_notional(
    price: Decimal,
    qty: Decimal,
    min_notional: Decimal,
) -> Result<(), String> {
    let notional = price * qty;
    if notional < min_notional {
        Err(format!(
            "Notional value {notional} is less than minimum required {min_notional}"
        ))
    } else {
        Ok(())
    }
}

/// Round a decimal to at most N significant figures.
/// Hyperliquid requires prices to have at most 5 significant figures.
pub fn round_to_sig_figs(value: Decimal, sig_figs: u32) -> Decimal {
    if value.is_zero() {
        return Decimal::ZERO;
    }

    // Find order of magnitude using log10
    let abs_val = value.abs();
    let float_val: f64 = abs_val.to_string().parse().unwrap_or(0.0);
    let magnitude = float_val.log10().floor() as i32;

    // Calculate shift to round to sig_figs
    let shift = sig_figs as i32 - 1 - magnitude;
    let factor = Decimal::from(10_i64.pow(shift.unsigned_abs()));

    if shift >= 0 {
        (value * factor).round() / factor
    } else {
        (value / factor).round() * factor
    }
}

/// Normalize price to the specified number of decimal places.
pub fn normalize_price(price: Decimal, decimals: u8) -> Decimal {
    // First round to 5 significant figures (Hyperliquid requirement)
    let sig_fig_price = round_to_sig_figs(price, 5);
    // Then truncate to max decimal places
    let scale = Decimal::from(10_u64.pow(decimals as u32));
    (sig_fig_price * scale).floor() / scale
}

/// Normalize quantity to the specified number of decimal places.
pub fn normalize_quantity(qty: Decimal, decimals: u8) -> Decimal {
    let scale = Decimal::from(10_u64.pow(decimals as u32));
    (qty * scale).floor() / scale
}

/// Complete normalization for an order including price, quantity, and notional validation
pub fn normalize_order(
    price: Decimal,
    qty: Decimal,
    tick_size: Decimal,
    step_size: Decimal,
    min_notional: Decimal,
    price_decimals: u8,
    size_decimals: u8,
) -> Result<(Decimal, Decimal), String> {
    // Normalize to decimal places first
    let normalized_price = normalize_price(price, price_decimals);
    let normalized_qty = normalize_quantity(qty, size_decimals);

    // Round down to tick/step sizes
    let final_price = round_down_to_tick(normalized_price, tick_size);
    let final_qty = round_down_to_step(normalized_qty, step_size);

    // Validate minimum notional
    ensure_min_notional(final_price, final_qty, min_notional)?;

    Ok((final_price, final_qty))
}

/// Converts millisecond timestamp to [`UnixNanos`].
#[inline]
pub fn millis_to_nanos(millis: u64) -> anyhow::Result<UnixNanos> {
    let value = nautilus_core::datetime::millis_to_nanos(millis as f64)?;
    Ok(UnixNanos::from(value))
}

/// Converts a Nautilus `TimeInForce` to Hyperliquid TIF.
///
/// # Errors
///
/// Returns an error if the time in force is not supported.
pub fn time_in_force_to_hyperliquid_tif(
    tif: TimeInForce,
    is_post_only: bool,
) -> anyhow::Result<HyperliquidExecTif> {
    match (tif, is_post_only) {
        (_, true) => Ok(HyperliquidExecTif::Alo), // Always use ALO for post-only orders
        (TimeInForce::Gtc, false) => Ok(HyperliquidExecTif::Gtc),
        (TimeInForce::Ioc, false) => Ok(HyperliquidExecTif::Ioc),
        (TimeInForce::Fok, false) => {
            anyhow::bail!("FOK time in force is not supported by Hyperliquid")
        }
        _ => anyhow::bail!("Unsupported time in force for Hyperliquid: {tif:?}"),
    }
}

fn determine_tpsl_type(
    order_type: OrderType,
    order_side: OrderSide,
    trigger_price: Decimal,
    current_price: Option<Decimal>,
) -> HyperliquidExecTpSl {
    match order_type {
        // Stop orders are protective - always SL
        OrderType::StopMarket | OrderType::StopLimit => HyperliquidExecTpSl::Sl,

        // If Touched orders are profit-taking or entry orders - always TP
        OrderType::MarketIfTouched | OrderType::LimitIfTouched => HyperliquidExecTpSl::Tp,

        // For other trigger types, try to infer from price relationship if available
        _ => {
            if let Some(current) = current_price {
                match order_side {
                    OrderSide::Buy => {
                        // Buy order: trigger above market = stop loss, below = take profit
                        if trigger_price > current {
                            HyperliquidExecTpSl::Sl
                        } else {
                            HyperliquidExecTpSl::Tp
                        }
                    }
                    OrderSide::Sell => {
                        // Sell order: trigger below market = stop loss, above = take profit
                        if trigger_price < current {
                            HyperliquidExecTpSl::Sl
                        } else {
                            HyperliquidExecTpSl::Tp
                        }
                    }
                    _ => HyperliquidExecTpSl::Sl, // Default to SL for safety
                }
            } else {
                // No market price available, default to SL for safety
                HyperliquidExecTpSl::Sl
            }
        }
    }
}

/// Converts a Nautilus `BarType` to a Hyperliquid bar interval.
///
/// # Errors
///
/// Returns an error if the bar type uses an unsupported aggregation or step value.
pub fn bar_type_to_interval(bar_type: &BarType) -> anyhow::Result<HyperliquidBarInterval> {
    let spec = bar_type.spec();
    let step = spec.step.get();

    anyhow::ensure!(
        bar_type.aggregation_source() == AggregationSource::External,
        "Only EXTERNAL aggregation is supported"
    );

    let interval = match spec.aggregation {
        BarAggregation::Minute => match step {
            1 => OneMinute,
            3 => ThreeMinutes,
            5 => FiveMinutes,
            15 => FifteenMinutes,
            30 => ThirtyMinutes,
            _ => anyhow::bail!("Unsupported minute step: {step}"),
        },
        BarAggregation::Hour => match step {
            1 => OneHour,
            2 => TwoHours,
            4 => FourHours,
            8 => EightHours,
            12 => TwelveHours,
            _ => anyhow::bail!("Unsupported hour step: {step}"),
        },
        BarAggregation::Day => match step {
            1 => OneDay,
            3 => ThreeDays,
            _ => anyhow::bail!("Unsupported day step: {step}"),
        },
        BarAggregation::Week if step == 1 => OneWeek,
        BarAggregation::Month if step == 1 => OneMonth,
        a => anyhow::bail!("Hyperliquid does not support {a:?} aggregation"),
    };

    Ok(interval)
}

/// Converts a Nautilus order to Hyperliquid request using a pre-resolved asset index.
///
/// This variant is used when the caller has already resolved the asset index
/// from the instrument cache (e.g., for SPOT instruments where the index
/// cannot be derived from the symbol alone). `slippage_bps` controls the
/// buffer applied when deriving a limit from a stop trigger price.
pub fn order_to_hyperliquid_request_with_asset(
    order: &OrderAny,
    asset: u32,
    price_decimals: u8,
    should_normalize_prices: bool,
    slippage_bps: u32,
) -> anyhow::Result<HyperliquidExecPlaceOrderRequest> {
    let is_buy = matches!(order.order_side(), OrderSide::Buy);
    let reduce_only = order.is_reduce_only();
    let order_side = order.order_side();
    let order_type = order.order_type();

    // Normalize decimals to strip trailing zeros, matching the server's
    // canonical form used for EIP-712 signing hash verification.
    let price_decimal = if let Some(price) = order.price() {
        let raw = price.as_decimal();

        if should_normalize_prices {
            normalize_price(raw, price_decimals).normalize()
        } else {
            raw.normalize()
        }
    } else if matches!(order_type, OrderType::Market) {
        Decimal::ZERO
    } else if matches!(
        order_type,
        OrderType::StopMarket | OrderType::MarketIfTouched
    ) {
        match order.trigger_price() {
            Some(tp) => {
                let base = tp.as_decimal().normalize();
                let derived = derive_limit_from_trigger(base, is_buy, slippage_bps);
                let sig_rounded = round_to_sig_figs(derived, 5);
                clamp_price_to_precision(sig_rounded, price_decimals, is_buy).normalize()
            }
            None => Decimal::ZERO,
        }
    } else {
        anyhow::bail!("Limit orders require a price")
    };

    let size_decimal = order.quantity().as_decimal().normalize();

    // Determine order kind based on order type
    let kind = match order_type {
        OrderType::Market => HyperliquidExecOrderKind::Limit {
            limit: HyperliquidExecLimitParams {
                tif: HyperliquidExecTif::Ioc,
            },
        },
        OrderType::Limit => {
            let tif =
                time_in_force_to_hyperliquid_tif(order.time_in_force(), order.is_post_only())?;
            HyperliquidExecOrderKind::Limit {
                limit: HyperliquidExecLimitParams { tif },
            }
        }
        OrderType::StopMarket => {
            if let Some(trigger_price) = order.trigger_price() {
                let raw = trigger_price.as_decimal();
                let trigger_price_decimal = if should_normalize_prices {
                    normalize_price(raw, price_decimals).normalize()
                } else {
                    raw.normalize()
                };
                let tpsl = determine_tpsl_type(order_type, order_side, trigger_price_decimal, None);
                HyperliquidExecOrderKind::Trigger {
                    trigger: HyperliquidExecTriggerParams {
                        is_market: true,
                        trigger_px: trigger_price_decimal,
                        tpsl,
                    },
                }
            } else {
                anyhow::bail!("Stop market orders require a trigger price")
            }
        }
        OrderType::StopLimit => {
            if let Some(trigger_price) = order.trigger_price() {
                let raw = trigger_price.as_decimal();
                let trigger_price_decimal = if should_normalize_prices {
                    normalize_price(raw, price_decimals).normalize()
                } else {
                    raw.normalize()
                };
                let tpsl = determine_tpsl_type(order_type, order_side, trigger_price_decimal, None);
                HyperliquidExecOrderKind::Trigger {
                    trigger: HyperliquidExecTriggerParams {
                        is_market: false,
                        trigger_px: trigger_price_decimal,
                        tpsl,
                    },
                }
            } else {
                anyhow::bail!("Stop limit orders require a trigger price")
            }
        }
        OrderType::MarketIfTouched => {
            if let Some(trigger_price) = order.trigger_price() {
                let raw = trigger_price.as_decimal();
                let trigger_price_decimal = if should_normalize_prices {
                    normalize_price(raw, price_decimals).normalize()
                } else {
                    raw.normalize()
                };
                HyperliquidExecOrderKind::Trigger {
                    trigger: HyperliquidExecTriggerParams {
                        is_market: true,
                        trigger_px: trigger_price_decimal,
                        tpsl: HyperliquidExecTpSl::Tp,
                    },
                }
            } else {
                anyhow::bail!("Market-if-touched orders require a trigger price")
            }
        }
        OrderType::LimitIfTouched => {
            if let Some(trigger_price) = order.trigger_price() {
                let raw = trigger_price.as_decimal();
                let trigger_price_decimal = if should_normalize_prices {
                    normalize_price(raw, price_decimals).normalize()
                } else {
                    raw.normalize()
                };
                HyperliquidExecOrderKind::Trigger {
                    trigger: HyperliquidExecTriggerParams {
                        is_market: false,
                        trigger_px: trigger_price_decimal,
                        tpsl: HyperliquidExecTpSl::Tp,
                    },
                }
            } else {
                anyhow::bail!("Limit-if-touched orders require a trigger price")
            }
        }
        _ => anyhow::bail!("Unsupported order type for Hyperliquid: {order_type:?}"),
    };

    let cloid = Some(Cloid::from_client_order_id(order.client_order_id()));

    Ok(HyperliquidExecPlaceOrderRequest {
        asset,
        is_buy,
        price: price_decimal,
        size: size_decimal,
        reduce_only,
        kind,
        cloid,
    })
}

/// Default slippage buffer in basis points for MARKET orders.
pub const DEFAULT_MARKET_SLIPPAGE_BPS: u32 = 50;

/// Derives a market order limit price from a quote with a configurable
/// slippage buffer in basis points, rounded to 5 significant figures and
/// clamped to the instrument's price precision.
pub fn derive_market_order_price(
    quote: &QuoteTick,
    is_buy: bool,
    price_decimals: u8,
    slippage_bps: u32,
) -> Decimal {
    let base = if is_buy {
        quote.ask_price.as_decimal()
    } else {
        quote.bid_price.as_decimal()
    };
    let derived = derive_limit_from_trigger(base, is_buy, slippage_bps);
    let sig_rounded = round_to_sig_figs(derived, 5);
    clamp_price_to_precision(sig_rounded, price_decimals, is_buy).normalize()
}

/// Derives a limit price from a trigger price with a configurable
/// slippage buffer in basis points, widening the limit so BUY satisfies
/// `limit_px >= trigger_px` and SELL satisfies `limit_px <= trigger_px`.
pub fn derive_limit_from_trigger(
    trigger_price: Decimal,
    is_buy: bool,
    slippage_bps: u32,
) -> Decimal {
    // bps -> Decimal: e.g. 50 bps -> 0.005
    let slippage = Decimal::new(slippage_bps as i64, 4);
    let price = if is_buy {
        trigger_price * (Decimal::ONE + slippage)
    } else {
        trigger_price * (Decimal::ONE - slippage)
    };

    // Strip trailing zeros for EIP-712 signing hash verification
    price.normalize()
}

/// Clamp a price to the instrument's decimal precision,
/// rounding in the direction that preserves the slippage buffer.
pub fn clamp_price_to_precision(price: Decimal, decimals: u8, is_buy: bool) -> Decimal {
    let scale = Decimal::from(10_u64.pow(decimals as u32));

    if is_buy {
        (price * scale).ceil() / scale
    } else {
        (price * scale).floor() / scale
    }
}

/// Converts a client order ID to a Hyperliquid cancel request using a pre-resolved asset index.
pub fn client_order_id_to_cancel_request_with_asset(
    client_order_id: &str,
    asset: u32,
) -> HyperliquidExecCancelByCloidRequest {
    let cloid = Cloid::from_client_order_id(ClientOrderId::from(client_order_id));
    HyperliquidExecCancelByCloidRequest { asset, cloid }
}

/// Extracts per-item error from a successful Hyperliquid exchange response.
///
/// When the top-level status is "ok", individual items in the `statuses`
/// array may still contain errors. Returns the first error found, or
/// `None` if all items succeeded or the response cannot be parsed.
pub fn extract_inner_error(response: &HyperliquidExchangeResponse) -> Option<String> {
    let HyperliquidExchangeResponse::Status { response, .. } = response else {
        return None;
    };
    let data: HyperliquidExecResponseData = serde_json::from_value(response.clone()).ok()?;
    match data {
        HyperliquidExecResponseData::Order { data } => {
            for status in &data.statuses {
                if let HyperliquidExecOrderStatus::Error { error } = status {
                    return Some(error.clone());
                }
            }
            None
        }
        HyperliquidExecResponseData::Cancel { data } => {
            for status in &data.statuses {
                if let HyperliquidExecCancelStatus::Error { error } = status {
                    return Some(error.clone());
                }
            }
            None
        }
        HyperliquidExecResponseData::Modify { data } => {
            for status in &data.statuses {
                if let HyperliquidExecModifyStatus::Error { error } = status {
                    return Some(error.clone());
                }
            }
            None
        }
        _ => None,
    }
}

/// Extracts per-item errors from a successful batch response.
///
/// Returns a `Vec` with one `Option<String>` per item in the `statuses`
/// array: `Some(error)` for failed items, `None` for successful ones.
/// Returns an empty vec if the response cannot be parsed.
pub fn extract_inner_errors(response: &HyperliquidExchangeResponse) -> Vec<Option<String>> {
    let HyperliquidExchangeResponse::Status { response, .. } = response else {
        return Vec::new();
    };
    let Ok(data) = serde_json::from_value::<HyperliquidExecResponseData>(response.clone()) else {
        return Vec::new();
    };

    match data {
        HyperliquidExecResponseData::Order { data } => data
            .statuses
            .into_iter()
            .map(|s| match s {
                HyperliquidExecOrderStatus::Error { error } => Some(error),
                _ => None,
            })
            .collect(),
        HyperliquidExecResponseData::Cancel { data } => data
            .statuses
            .into_iter()
            .map(|s| match s {
                HyperliquidExecCancelStatus::Error { error } => Some(error),
                HyperliquidExecCancelStatus::Success(_) => None,
            })
            .collect(),
        HyperliquidExecResponseData::Modify { data } => data
            .statuses
            .into_iter()
            .map(|s| match s {
                HyperliquidExecModifyStatus::Error { error } => Some(error),
                HyperliquidExecModifyStatus::Success(_) => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Extracts error message from a Hyperliquid exchange response.
pub fn extract_error_message(response: &HyperliquidExchangeResponse) -> String {
    match response {
        HyperliquidExchangeResponse::Status { status, response } => {
            if status == RESPONSE_STATUS_OK {
                "Operation successful".to_string()
            } else {
                // Try to extract error message from response data
                if let Some(error_msg) = response.get("error").and_then(|v| v.as_str()) {
                    error_msg.to_string()
                } else {
                    format!("Request failed with status: {status}")
                }
            }
        }
        HyperliquidExchangeResponse::Error { error } => error.clone(),
    }
}

/// Determines if an order is a conditional/trigger order based on order data.
///
/// # Returns
///
/// `true` if the order is a conditional order, `false` otherwise.
pub fn is_conditional_order_data(trigger_px: Option<&str>, tpsl: Option<&HyperliquidTpSl>) -> bool {
    trigger_px.is_some() && tpsl.is_some()
}

/// Parses trigger order type from Hyperliquid order data.
///
/// # Returns
///
/// The corresponding Nautilus `OrderType`.
pub fn parse_trigger_order_type(is_market: bool, tpsl: &HyperliquidTpSl) -> OrderType {
    match (is_market, tpsl) {
        (true, HyperliquidTpSl::Sl) => OrderType::StopMarket,
        (false, HyperliquidTpSl::Sl) => OrderType::StopLimit,
        (true, HyperliquidTpSl::Tp) => OrderType::MarketIfTouched,
        (false, HyperliquidTpSl::Tp) => OrderType::LimitIfTouched,
    }
}

/// Extracts order status from WebSocket order data.
///
/// # Returns
///
/// A tuple of (OrderStatus, optional trigger status string).
pub fn parse_order_status_with_trigger(
    status: HyperliquidOrderStatus,
    trigger_activated: Option<bool>,
) -> (OrderStatus, Option<String>) {
    let base_status = OrderStatus::from(status);

    // For conditional orders, add trigger status information
    if let Some(activated) = trigger_activated {
        let trigger_status = if activated {
            Some("activated".to_string())
        } else {
            Some("pending".to_string())
        };
        (base_status, trigger_status)
    } else {
        (base_status, None)
    }
}

/// Converts WebSocket trailing stop data to description string.
pub fn format_trailing_stop_info(
    offset: &str,
    offset_type: TrailingOffsetType,
    callback_price: Option<&str>,
) -> String {
    let offset_desc = offset_type.format_offset(offset);

    if let Some(callback) = callback_price {
        format!("Trailing stop: {offset_desc} offset, callback at {callback}")
    } else {
        format!("Trailing stop: {offset_desc} offset")
    }
}

/// Validates conditional order parameters from WebSocket data.
///
/// # Returns
///
/// `Ok(())` if parameters are valid, `Err` with description otherwise.
pub fn validate_conditional_order_params(
    trigger_px: Option<&str>,
    tpsl: Option<&HyperliquidTpSl>,
    is_market: Option<bool>,
) -> anyhow::Result<()> {
    if trigger_px.is_none() {
        anyhow::bail!("Conditional order missing trigger price");
    }

    if tpsl.is_none() {
        anyhow::bail!("Conditional order missing tpsl indicator");
    }

    // No need to validate tpsl value - the enum type guarantees it's either Tp or Sl

    if is_market.is_none() {
        anyhow::bail!("Conditional order missing is_market flag");
    }

    Ok(())
}

/// Parses trigger price from string to Decimal.
///
/// # Returns
///
/// Parsed Decimal value or error.
pub fn parse_trigger_price(trigger_px: &str) -> anyhow::Result<Decimal> {
    Decimal::from_str_exact(trigger_px)
        .with_context(|| format!("Failed to parse trigger price: {trigger_px}"))
}

/// Parses Hyperliquid clearinghouse state into Nautilus account balances and margins.
///
/// Uses the same field selection as the HTTP account-state path
/// (`cross_margin_summary.total_raw_usd` for total, top-level `state.withdrawable`
/// for free) so the execution adapter and the HTTP client emit consistent balances
/// for the same clearinghouse snapshot.
///
/// # Errors
///
/// Returns an error if the data cannot be parsed.
pub fn parse_account_balances_and_margins(
    state: &ClearinghouseState,
) -> anyhow::Result<(Vec<AccountBalance>, Vec<MarginBalance>)> {
    let mut balances = Vec::new();
    let mut margins = Vec::new();

    let currency = Currency::USDC();

    let cross_margin_summary = match &state.cross_margin_summary {
        Some(summary) => summary,
        None => return Ok((balances, margins)),
    };

    let mut total_value = cross_margin_summary.total_raw_usd.max(Decimal::ZERO);
    let free_value = state.withdrawable.unwrap_or(total_value).max(Decimal::ZERO);

    // Withdrawable may include spot balances that sit outside the margin account value;
    // raise total so those funds are not silently clamped away. Mirrors the HTTP parser.
    if free_value > total_value {
        total_value = free_value;
    }

    balances.push(AccountBalance::from_total_and_free(
        total_value,
        free_value,
        currency,
    )?);

    let margin_used = cross_margin_summary.total_margin_used;

    if margin_used > Decimal::ZERO {
        // Hyperliquid perps use a single-collateral (USDC) cross-margin model, so the
        // reserved margin is emitted as an account-wide entry keyed by USDC.
        let initial_margin = Money::from_decimal(margin_used, currency)?;
        let maintenance_margin = Money::from_decimal(margin_used, currency)?;
        margins.push(MarginBalance::new(initial_margin, maintenance_margin, None));
    }

    Ok((balances, margins))
}

/// Merges perp clearinghouse balances with spot balances into a unified set.
///
/// The perp parser already reflects combined USDC (its `withdrawable` may include
/// spot buckets). To avoid double-counting, this helper appends only non-USDC
/// spot tokens onto the perp-derived balances. If the perp state has no margin
/// summary, the full spot balance set is used verbatim.
///
/// # Errors
///
/// Returns an error if any balance conversion fails.
pub fn parse_combined_account_balances_and_margins(
    perp_state: &ClearinghouseState,
    spot_state: &SpotClearinghouseState,
) -> anyhow::Result<(Vec<AccountBalance>, Vec<MarginBalance>)> {
    let (mut balances, margins) = parse_account_balances_and_margins(perp_state)?;

    let has_perp_summary = perp_state.cross_margin_summary.is_some();
    let spot_balances = parse_spot_account_balances(spot_state)?;

    for balance in spot_balances {
        let is_usdc = balance.currency.code.as_str() == "USDC";
        if has_perp_summary && is_usdc {
            continue;
        }
        balances.push(balance);
    }

    Ok((balances, margins))
}

/// Parses Hyperliquid spot clearinghouse state into Nautilus account balances.
///
/// Emits one [`AccountBalance`] per non-zero spot token, deriving free from
/// `total - hold`. Tokens unknown to the global currency registry are registered
/// on the fly with 8-decimal precision (matches Hyperliquid's `sz_decimals` cap).
///
/// # Errors
///
/// Returns an error if any balance cannot be converted to a Nautilus `Money`.
pub fn parse_spot_account_balances(
    state: &SpotClearinghouseState,
) -> anyhow::Result<Vec<AccountBalance>> {
    let mut balances = Vec::with_capacity(state.balances.len());

    for balance in &state.balances {
        if balance.total.is_zero() {
            continue;
        }

        let currency = crate::http::parse::get_currency(balance.coin.as_str());

        // Let `from_total_and_locked` do the clamping and derivation at currency
        // precision so the `total == locked + free` invariant holds without
        // bespoke rounding here.
        balances.push(AccountBalance::from_total_and_locked(
            balance.total,
            balance.hold,
            currency,
        )?);
    }

    Ok(balances)
}

/// Determine the Hyperliquid grouping strategy for an order list.
///
/// Contingency type, reduce-only flags, structural shape, and parent/child
/// linkage must all agree to avoid misclassifying generic contingent lists
/// as Hyperliquid TP/SL groups.
///
/// - `NormalTpsl` (OTOCO bracket): entry order is OTO and not reduce-only,
///   all child orders are OCO, reduce-only, and reference the entry as parent.
/// - `PositionTpsl` (OCO pair): every order is OCO, reduce-only, and linked
///   to the same sibling set.
/// - `Na`: everything else (independent batch).
pub(crate) fn determine_order_list_grouping(orders: &[OrderAny]) -> HyperliquidExecGrouping {
    if orders.len() >= 2 {
        let entry = &orders[0];
        let children = &orders[1..];
        let entry_id = entry.client_order_id();
        let entry_is_oto =
            entry.contingency_type() == Some(ContingencyType::Oto) && !entry.is_reduce_only();
        let children_are_linked = children.iter().all(|o| {
            o.contingency_type() == Some(ContingencyType::Oco)
                && o.is_reduce_only()
                && o.parent_order_id() == Some(entry_id)
        });

        if entry_is_oto && children_are_linked {
            return HyperliquidExecGrouping::NormalTpsl;
        }
    }

    let all_oco_linked = orders.len() >= 2
        && orders
            .iter()
            .all(|o| o.contingency_type() == Some(ContingencyType::Oco) && o.is_reduce_only())
        && orders.iter().all(|o| {
            o.linked_order_ids().is_some_and(|ids| {
                ids.iter()
                    .all(|id| orders.iter().any(|other| other.client_order_id() == *id))
            })
        });

    if all_oco_linked {
        HyperliquidExecGrouping::PositionTpsl
    } else {
        HyperliquidExecGrouping::Na
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use nautilus_model::{
        enums::{OrderSide, TimeInForce, TriggerType},
        identifiers::{ClientOrderId, InstrumentId, StrategyId, TraderId},
        orders::{OrderAny, StopMarketOrder},
        types::{Price, Quantity},
    };
    use rstest::rstest;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use serde::{Deserialize, Serialize};

    use super::*;

    #[derive(Serialize, Deserialize)]
    struct TestStruct {
        #[serde(
            serialize_with = "serialize_decimal_as_str",
            deserialize_with = "deserialize_decimal_from_str"
        )]
        value: Decimal,
        #[serde(
            serialize_with = "serialize_optional_decimal_as_str",
            deserialize_with = "deserialize_optional_decimal_from_str"
        )]
        optional_value: Option<Decimal>,
    }

    #[rstest]
    fn test_decimal_serialization_roundtrip() {
        let original = TestStruct {
            value: Decimal::from_str("123.456789012345678901234567890").unwrap(),
            optional_value: Some(Decimal::from_str("0.000000001").unwrap()),
        };

        let json = serde_json::to_string(&original).unwrap();
        println!("Serialized: {json}");

        // Check that it's serialized as strings (rust_decimal may normalize precision)
        assert!(json.contains("\"123.45678901234567890123456789\""));
        assert!(json.contains("\"0.000000001\""));

        let deserialized: TestStruct = serde_json::from_str(&json).unwrap();
        assert_eq!(original.value, deserialized.value);
        assert_eq!(original.optional_value, deserialized.optional_value);
    }

    #[rstest]
    fn test_decimal_precision_preservation() {
        let test_cases = [
            "0",
            "1",
            "0.1",
            "0.01",
            "0.001",
            "123.456789012345678901234567890",
            "999999999999999999.999999999999999999",
        ];

        for case in test_cases {
            let decimal = Decimal::from_str(case).unwrap();
            let test_struct = TestStruct {
                value: decimal,
                optional_value: Some(decimal),
            };

            let json = serde_json::to_string(&test_struct).unwrap();
            let parsed: TestStruct = serde_json::from_str(&json).unwrap();

            assert_eq!(decimal, parsed.value, "Failed for case: {case}");
            assert_eq!(
                Some(decimal),
                parsed.optional_value,
                "Failed for case: {case}"
            );
        }
    }

    #[rstest]
    fn test_optional_none_handling() {
        let test_struct = TestStruct {
            value: Decimal::from_str("42.0").unwrap(),
            optional_value: None,
        };

        let json = serde_json::to_string(&test_struct).unwrap();
        assert!(json.contains("null"));

        let parsed: TestStruct = serde_json::from_str(&json).unwrap();
        assert_eq!(test_struct.value, parsed.value);
        assert_eq!(None, parsed.optional_value);
    }

    #[rstest]
    fn test_round_down_to_tick() {
        assert_eq!(round_down_to_tick(dec!(100.07), dec!(0.05)), dec!(100.05));
        assert_eq!(round_down_to_tick(dec!(100.03), dec!(0.05)), dec!(100.00));
        assert_eq!(round_down_to_tick(dec!(100.05), dec!(0.05)), dec!(100.05));

        // Edge case: zero tick size
        assert_eq!(round_down_to_tick(dec!(100.07), dec!(0)), dec!(100.07));
    }

    #[rstest]
    fn test_round_down_to_step() {
        assert_eq!(
            round_down_to_step(dec!(0.12349), dec!(0.0001)),
            dec!(0.1234)
        );
        assert_eq!(round_down_to_step(dec!(1.5555), dec!(0.1)), dec!(1.5));
        assert_eq!(round_down_to_step(dec!(1.0001), dec!(0.0001)), dec!(1.0001));

        // Edge case: zero step size
        assert_eq!(round_down_to_step(dec!(0.12349), dec!(0)), dec!(0.12349));
    }

    #[rstest]
    fn test_min_notional_validation() {
        // Should pass
        assert!(ensure_min_notional(dec!(100), dec!(0.1), dec!(10)).is_ok());
        assert!(ensure_min_notional(dec!(100), dec!(0.11), dec!(10)).is_ok());

        // Should fail
        assert!(ensure_min_notional(dec!(100), dec!(0.05), dec!(10)).is_err());
        assert!(ensure_min_notional(dec!(1), dec!(5), dec!(10)).is_err());

        // Edge case: exactly at minimum
        assert!(ensure_min_notional(dec!(100), dec!(0.1), dec!(10)).is_ok());
    }

    #[rstest]
    fn test_round_to_sig_figs() {
        // BTC price ~$104,567 needs to round to 5 sig figs
        assert_eq!(round_to_sig_figs(dec!(104567.3), 5), dec!(104570));
        assert_eq!(round_to_sig_figs(dec!(104522.5), 5), dec!(104520));
        assert_eq!(round_to_sig_figs(dec!(99999.9), 5), dec!(100000));

        // Smaller prices should keep decimals
        assert_eq!(round_to_sig_figs(dec!(1234.5), 5), dec!(1234.5));
        assert_eq!(round_to_sig_figs(dec!(0.12345), 5), dec!(0.12345));
        assert_eq!(round_to_sig_figs(dec!(0.123456), 5), dec!(0.12346));

        // Sub-1 values with leading zeros must preserve 5 sig figs
        assert_eq!(round_to_sig_figs(dec!(0.000123456), 5), dec!(0.00012346));
        assert_eq!(round_to_sig_figs(dec!(0.000999999), 5), dec!(0.0010000)); // 6 sig figs -> 5

        // Zero case
        assert_eq!(round_to_sig_figs(dec!(0), 5), dec!(0));
    }

    #[rstest]
    fn test_normalize_price() {
        // Now includes 5 sig fig rounding first
        assert_eq!(normalize_price(dec!(100.12345), 2), dec!(100.12));
        assert_eq!(normalize_price(dec!(100.19999), 2), dec!(100.2)); // Rounded to 5 sig figs first
        assert_eq!(normalize_price(dec!(100.999), 0), dec!(101)); // 100.999 -> 101.00 (5 sig) -> 101
        assert_eq!(normalize_price(dec!(100.12345), 4), dec!(100.12)); // 5 sig figs = 100.12

        // BTC-like prices get rounded to 5 sig figs
        assert_eq!(normalize_price(dec!(104567.3), 1), dec!(104570));
    }

    #[rstest]
    fn test_normalize_quantity() {
        assert_eq!(normalize_quantity(dec!(1.12345), 3), dec!(1.123));
        assert_eq!(normalize_quantity(dec!(1.99999), 3), dec!(1.999));
        assert_eq!(normalize_quantity(dec!(1.999), 0), dec!(1));
        assert_eq!(normalize_quantity(dec!(1.12345), 5), dec!(1.12345));
    }

    #[rstest]
    fn test_normalize_order_complete() {
        let result = normalize_order(
            dec!(100.12345), // price
            dec!(0.123456),  // qty
            dec!(0.01),      // tick_size
            dec!(0.0001),    // step_size
            dec!(10),        // min_notional
            2,               // price_decimals
            4,               // size_decimals
        );

        assert!(result.is_ok());
        let (price, qty) = result.unwrap();
        assert_eq!(price, dec!(100.12)); // normalized and rounded down
        assert_eq!(qty, dec!(0.1234)); // normalized and rounded down
    }

    #[rstest]
    fn test_normalize_order_min_notional_fail() {
        let result = normalize_order(
            dec!(100.12345), // price
            dec!(0.05),      // qty (too small for min notional)
            dec!(0.01),      // tick_size
            dec!(0.0001),    // step_size
            dec!(10),        // min_notional
            2,               // price_decimals
            4,               // size_decimals
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Notional value"));
    }

    #[rstest]
    fn test_edge_cases() {
        // Test with very small numbers
        assert_eq!(
            round_down_to_tick(dec!(0.000001), dec!(0.000001)),
            dec!(0.000001)
        );

        // Test with large numbers
        assert_eq!(round_down_to_tick(dec!(999999.99), dec!(1.0)), dec!(999999));

        // Test rounding edge case
        assert_eq!(
            round_down_to_tick(dec!(100.009999), dec!(0.01)),
            dec!(100.00)
        );
    }

    #[rstest]
    fn test_is_conditional_order_data() {
        // Test with trigger price and tpsl (conditional)
        assert!(is_conditional_order_data(
            Some("50000.0"),
            Some(&HyperliquidTpSl::Sl)
        ));

        // Test with only trigger price (not conditional - needs both)
        assert!(!is_conditional_order_data(Some("50000.0"), None));

        // Test with only tpsl (not conditional - needs both)
        assert!(!is_conditional_order_data(None, Some(&HyperliquidTpSl::Tp)));

        // Test with no conditional fields
        assert!(!is_conditional_order_data(None, None));
    }

    #[rstest]
    fn test_parse_trigger_order_type() {
        // Stop Market
        assert_eq!(
            parse_trigger_order_type(true, &HyperliquidTpSl::Sl),
            OrderType::StopMarket
        );

        // Stop Limit
        assert_eq!(
            parse_trigger_order_type(false, &HyperliquidTpSl::Sl),
            OrderType::StopLimit
        );

        // Take Profit Market
        assert_eq!(
            parse_trigger_order_type(true, &HyperliquidTpSl::Tp),
            OrderType::MarketIfTouched
        );

        // Take Profit Limit
        assert_eq!(
            parse_trigger_order_type(false, &HyperliquidTpSl::Tp),
            OrderType::LimitIfTouched
        );
    }

    #[rstest]
    fn test_parse_order_status_with_trigger() {
        // Test with open status and activated trigger
        let (status, trigger_status) =
            parse_order_status_with_trigger(HyperliquidOrderStatus::Open, Some(true));
        assert_eq!(status, OrderStatus::Accepted);
        assert_eq!(trigger_status, Some("activated".to_string()));

        // Test with open status and not activated
        let (status, trigger_status) =
            parse_order_status_with_trigger(HyperliquidOrderStatus::Open, Some(false));
        assert_eq!(status, OrderStatus::Accepted);
        assert_eq!(trigger_status, Some("pending".to_string()));

        // Test without trigger info
        let (status, trigger_status) =
            parse_order_status_with_trigger(HyperliquidOrderStatus::Open, None);
        assert_eq!(status, OrderStatus::Accepted);
        assert_eq!(trigger_status, None);
    }

    #[rstest]
    fn test_format_trailing_stop_info() {
        // Price offset
        let info = format_trailing_stop_info("100.0", TrailingOffsetType::Price, Some("50000.0"));
        assert!(info.contains("100.0"));
        assert!(info.contains("callback at 50000.0"));

        // Percentage offset
        let info = format_trailing_stop_info("5.0", TrailingOffsetType::Percentage, None);
        assert!(info.contains("5.0%"));
        assert!(info.contains("Trailing stop"));

        // Basis points offset
        let info =
            format_trailing_stop_info("250", TrailingOffsetType::BasisPoints, Some("49000.0"));
        assert!(info.contains("250 bps"));
        assert!(info.contains("49000.0"));
    }

    #[rstest]
    fn test_parse_trigger_price() {
        // Valid price
        let result = parse_trigger_price("50000.0");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), dec!(50000.0));

        // Valid integer price
        let result = parse_trigger_price("49000");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), dec!(49000));

        // Invalid price
        let result = parse_trigger_price("invalid");
        assert!(result.is_err());

        // Empty string
        let result = parse_trigger_price("");
        assert!(result.is_err());
    }

    #[rstest]
    #[case(dec!(0), true, dec!(0))] // Zero
    #[case(dec!(0), false, dec!(0))] // Zero
    #[case(dec!(0.001), true, dec!(0.001005))] // Small price BUY
    #[case(dec!(0.001), false, dec!(0.000995))] // Small price SELL
    #[case(dec!(100), true, dec!(100.5))] // Round price BUY
    #[case(dec!(100), false, dec!(99.5))] // Round price SELL
    #[case(dec!(2470), true, dec!(2482.35))] // ETH-like BUY
    #[case(dec!(2470), false, dec!(2457.65))] // ETH-like SELL
    #[case(dec!(104567.3), true, dec!(105090.1365))] // BTC-like BUY
    #[case(dec!(104567.3), false, dec!(104044.4635))] // BTC-like SELL
    fn test_derive_limit_from_trigger(
        #[case] trigger_price: Decimal,
        #[case] is_buy: bool,
        #[case] expected: Decimal,
    ) {
        let result = derive_limit_from_trigger(trigger_price, is_buy, DEFAULT_MARKET_SLIPPAGE_BPS);
        assert_eq!(result, expected);

        // Verify invariant: BUY limit >= trigger, SELL limit <= trigger
        if is_buy {
            assert!(result >= trigger_price);
        } else {
            assert!(result <= trigger_price);
        }
    }

    #[rstest]
    // BUY rounds up (ceil)
    #[case(dec!(2457.65), 2, true, dec!(2457.65))] // Already at precision
    #[case(dec!(2457.65), 1, true, dec!(2457.7))] // Ceil to 1dp
    #[case(dec!(2457.65), 0, true, dec!(2458))] // Ceil to integer
    // SELL rounds down (floor)
    #[case(dec!(2457.65), 2, false, dec!(2457.65))] // Already at precision
    #[case(dec!(2457.65), 1, false, dec!(2457.6))] // Floor to 1dp
    #[case(dec!(2457.65), 0, false, dec!(2457))] // Floor to integer
    // High precision (no-op)
    #[case(dec!(0.4975), 4, true, dec!(0.4975))]
    #[case(dec!(0.4975), 4, false, dec!(0.4975))]
    // Precision forces clamping on small values
    #[case(dec!(0.4975), 2, true, dec!(0.50))]
    #[case(dec!(0.4975), 2, false, dec!(0.49))]
    fn test_clamp_price_to_precision(
        #[case] price: Decimal,
        #[case] decimals: u8,
        #[case] is_buy: bool,
        #[case] expected: Decimal,
    ) {
        assert_eq!(clamp_price_to_precision(price, decimals, is_buy), expected);
    }

    fn stop_market_order(side: OrderSide, trigger_price: &str) -> OrderAny {
        OrderAny::StopMarket(StopMarketOrder::new(
            TraderId::from("TESTER-001"),
            StrategyId::from("S-001"),
            InstrumentId::from("ETH-USD-PERP.HYPERLIQUID"),
            ClientOrderId::from("O-001"),
            side,
            Quantity::from(1),
            Price::from(trigger_price),
            TriggerType::LastPrice,
            TimeInForce::Gtc,
            None,
            false,
            false,
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
            Default::default(),
            Default::default(),
        ))
    }

    #[rstest]
    // ETH-like (precision=2): clamping is a no-op
    #[case(OrderSide::Sell, "2470.00", 2)]
    #[case(OrderSide::Buy, "2470.00", 2)]
    // BTC-like (precision=1): clamping is a no-op
    #[case(OrderSide::Sell, "104567.3", 1)]
    #[case(OrderSide::Buy, "104567.3", 1)]
    // Low-price token (precision=4): clamping is a no-op
    #[case(OrderSide::Sell, "0.50", 4)]
    #[case(OrderSide::Buy, "0.50", 4)]
    // Clamping materially changes: ETH trigger at precision=1
    // SELL: 2470 * 0.995 = 2457.65 → sig5 = 2457.6 → floor(1dp) = 2457.6
    // BUY:  2470 * 1.005 = 2482.35 → sig5 = 2482.4 → ceil(1dp) = 2482.4
    #[case(OrderSide::Sell, "2470.00", 1)]
    #[case(OrderSide::Buy, "2470.00", 1)]
    // Clamping materially changes: precision=0 forces integer
    // SELL: 2470 * 0.995 = 2457.65 → sig5 = 2457.6 → floor(0dp) = 2457
    // BUY:  2470 * 1.005 = 2482.35 → sig5 = 2482.4 → ceil(0dp) = 2483
    #[case(OrderSide::Sell, "2470.00", 0)]
    #[case(OrderSide::Buy, "2470.00", 0)]
    fn test_order_to_request_stop_market_derives_limit_from_trigger(
        #[case] side: OrderSide,
        #[case] trigger_str: &str,
        #[case] price_decimals: u8,
    ) {
        let order = stop_market_order(side, trigger_str);
        let request = order_to_hyperliquid_request_with_asset(
            &order,
            0,
            price_decimals,
            true,
            DEFAULT_MARKET_SLIPPAGE_BPS,
        )
        .unwrap();
        let trigger = Decimal::from_str(trigger_str).unwrap();
        let is_buy = matches!(side, OrderSide::Buy);

        // Price must satisfy Hyperliquid's directional constraint
        if is_buy {
            assert!(
                request.price >= trigger,
                "BUY limit {} must be >= trigger {trigger}",
                request.price,
            );
            assert!(request.is_buy);
        } else {
            assert!(
                request.price <= trigger,
                "SELL limit {} must be <= trigger {trigger}",
                request.price,
            );
            assert!(!request.is_buy);
        }

        // Price must equal the full pipeline: derive -> sig figs -> clamp -> normalize
        let derived = derive_limit_from_trigger(trigger, is_buy, DEFAULT_MARKET_SLIPPAGE_BPS);
        let sig_rounded = round_to_sig_figs(derived, 5);
        let expected = clamp_price_to_precision(sig_rounded, price_decimals, is_buy).normalize();
        assert_eq!(request.price, expected);

        // Decimal places must not exceed instrument precision
        let price_str = request.price.to_string();
        let actual_decimals = price_str
            .find('.')
            .map_or(0, |dot| price_str.len() - dot - 1);
        assert!(
            actual_decimals <= price_decimals as usize,
            "Price {price_str} has {actual_decimals} decimals, max allowed {price_decimals}",
        );

        // Decimal trailing zeros must be stripped (canonical form)
        if price_str.contains('.') {
            assert!(
                !price_str.ends_with('0'),
                "Price {price_str} has decimal trailing zeros",
            );
        }

        let expected_trigger = normalize_price(trigger, price_decimals).normalize();
        assert_eq!(
            request.kind,
            HyperliquidExecOrderKind::Trigger {
                trigger: HyperliquidExecTriggerParams {
                    is_market: true,
                    trigger_px: expected_trigger,
                    tpsl: HyperliquidExecTpSl::Sl,
                },
            },
        );
    }

    fn ok_response(inner: serde_json::Value) -> HyperliquidExchangeResponse {
        HyperliquidExchangeResponse::Status {
            status: "ok".to_string(),
            response: inner,
        }
    }

    #[rstest]
    fn test_extract_inner_error_order_with_error() {
        let response = ok_response(serde_json::json!({
            "type": "order",
            "data": {"statuses": [{"error": "Order has invalid price."}]}
        }));
        assert_eq!(
            extract_inner_error(&response),
            Some("Order has invalid price.".to_string()),
        );
    }

    #[rstest]
    fn test_extract_inner_error_order_resting() {
        let response = ok_response(serde_json::json!({
            "type": "order",
            "data": {"statuses": [{"resting": {"oid": 12345}}]}
        }));
        assert_eq!(extract_inner_error(&response), None);
    }

    #[rstest]
    fn test_extract_inner_error_order_filled() {
        let response = ok_response(serde_json::json!({
            "type": "order",
            "data": {"statuses": [{"filled": {"totalSz": "0.01", "avgPx": "2470.0", "oid": 99}}]}
        }));
        assert_eq!(extract_inner_error(&response), None);
    }

    #[rstest]
    fn test_extract_inner_error_cancel_error() {
        let response = ok_response(serde_json::json!({
            "type": "cancel",
            "data": {"statuses": [{"error": "Order not found"}]}
        }));
        assert_eq!(
            extract_inner_error(&response),
            Some("Order not found".to_string()),
        );
    }

    #[rstest]
    fn test_extract_inner_error_cancel_success() {
        let response = ok_response(serde_json::json!({
            "type": "cancel",
            "data": {"statuses": ["success"]}
        }));
        assert_eq!(extract_inner_error(&response), None);
    }

    #[rstest]
    fn test_extract_inner_error_modify_error() {
        let response = ok_response(serde_json::json!({
            "type": "modify",
            "data": {"statuses": [{"error": "Invalid modify"}]}
        }));
        assert_eq!(
            extract_inner_error(&response),
            Some("Invalid modify".to_string()),
        );
    }

    #[rstest]
    fn test_extract_inner_error_modify_success() {
        let response = ok_response(serde_json::json!({
            "type": "modify",
            "data": {"statuses": ["success"]}
        }));
        assert_eq!(extract_inner_error(&response), None);
    }

    #[rstest]
    fn test_extract_inner_error_non_status_response() {
        let response = HyperliquidExchangeResponse::Error {
            error: "top-level error".to_string(),
        };
        assert_eq!(extract_inner_error(&response), None);
    }

    #[rstest]
    fn test_extract_inner_error_unparsable_response() {
        let response = ok_response(serde_json::json!({"unknown": "data"}));
        assert_eq!(extract_inner_error(&response), None);
    }

    #[rstest]
    fn test_extract_inner_error_returns_first_error_in_batch() {
        let response = ok_response(serde_json::json!({
            "type": "order",
            "data": {"statuses": [
                {"resting": {"oid": 1}},
                {"error": "Second failed"},
                {"error": "Third failed"},
            ]}
        }));
        assert_eq!(
            extract_inner_error(&response),
            Some("Second failed".to_string()),
        );
    }

    #[rstest]
    fn test_extract_inner_errors_mixed_batch() {
        let response = ok_response(serde_json::json!({
            "type": "order",
            "data": {"statuses": [
                {"resting": {"oid": 1}},
                {"error": "Failed order"},
                {"filled": {"totalSz": "0.01", "avgPx": "100.0", "oid": 2}},
            ]}
        }));
        let errors = extract_inner_errors(&response);
        assert_eq!(errors.len(), 3);
        assert_eq!(errors[0], None);
        assert_eq!(errors[1], Some("Failed order".to_string()));
        assert_eq!(errors[2], None);
    }

    #[rstest]
    fn test_extract_inner_errors_all_success() {
        let response = ok_response(serde_json::json!({
            "type": "order",
            "data": {"statuses": [
                {"resting": {"oid": 1}},
                {"resting": {"oid": 2}},
            ]}
        }));
        let errors = extract_inner_errors(&response);
        assert_eq!(errors.len(), 2);
        assert!(errors.iter().all(|e| e.is_none()));
    }

    #[rstest]
    fn test_extract_inner_errors_cancel_success() {
        let response = ok_response(serde_json::json!({
            "type": "cancel",
            "data": {"statuses": ["success"]}
        }));
        let errors = extract_inner_errors(&response);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].is_none());
    }

    #[rstest]
    fn test_extract_inner_errors_cancel_mixed() {
        let response = ok_response(serde_json::json!({
            "type": "cancel",
            "data": {"statuses": [
                "success",
                {"error": "Order was never placed, already canceled, or filled."},
                "success",
            ]}
        }));
        let errors = extract_inner_errors(&response);
        assert_eq!(errors.len(), 3);
        assert_eq!(errors[0], None);
        assert_eq!(
            errors[1],
            Some("Order was never placed, already canceled, or filled.".to_string())
        );
        assert_eq!(errors[2], None);
    }

    #[rstest]
    fn test_extract_inner_errors_modify_mixed() {
        let response = ok_response(serde_json::json!({
            "type": "modify",
            "data": {"statuses": [
                "success",
                {"error": "Order does not exist"},
            ]}
        }));
        let errors = extract_inner_errors(&response);
        assert_eq!(errors.len(), 2);
        assert_eq!(errors[0], None);
        assert_eq!(errors[1], Some("Order does not exist".to_string()));
    }

    #[rstest]
    fn test_extract_inner_errors_unparsable() {
        let response = ok_response(serde_json::json!({"foo": "bar"}));
        let errors = extract_inner_errors(&response);
        assert!(errors.is_empty());
    }

    fn count_sig_figs(s: &str) -> usize {
        let s = s.trim_start_matches('-');
        if s.contains('.') {
            // Decimal: all digits excluding leading zeros are significant
            let digits: String = s.replace('.', "");
            digits.trim_start_matches('0').len()
        } else {
            // Integer: trailing zeros are place-holders, not significant
            let s = s.trim_start_matches('0');
            s.trim_end_matches('0').len()
        }
    }

    fn make_quote(bid: &str, ask: &str) -> QuoteTick {
        QuoteTick::new(
            InstrumentId::from("ETH-USD-PERP.HYPERLIQUID"),
            Price::from(bid),
            Price::from(ask),
            Quantity::from("1"),
            Quantity::from("1"),
            Default::default(),
            Default::default(),
        )
    }

    #[rstest]
    // BUY uses ask, SELL uses bid
    // Pipeline: base → +/-0.5% slippage → round 5 sig figs → clamp → normalize
    //
    // ETH-like (precision=2)
    // BUY: ask=2470 → 2470*1.005=2482.35 → sig5=2482.4 → clamp(2,ceil)=2482.40 → 2482.4
    #[case("2460.00", "2470.00", true, 2, "2482.4")]
    // SELL: bid=2460 → 2460*0.995=2447.70 → sig5=2447.7 → clamp(2,floor)=2447.70 → 2447.7
    #[case("2460.00", "2470.00", false, 2, "2447.7")]
    //
    // BTC-like (precision=1)
    // BUY: ask=104567.3 → 104567.3*1.005=105090.1365 → sig5=105090 → clamp(1,ceil)=105090 → 105090
    #[case("104500.0", "104567.3", true, 1, "105090")]
    // SELL: bid=104500.0 → 104500*0.995=103977.5 → sig5=103980 → clamp(1,floor)=103980 → 103980
    #[case("104500.0", "104567.3", false, 1, "103980")]
    //
    // Low-price token (precision=4)
    // BUY: ask=0.5000 → 0.5*1.005=0.5025 → sig5=0.50250 → clamp(4,ceil)=0.5025 → 0.5025
    #[case("0.4900", "0.5000", true, 4, "0.5025")]
    // SELL: bid=0.49 → 0.49*0.995=0.48755 → sig5=0.48755 → clamp(4,floor)=0.4875 → 0.4875
    #[case("0.4900", "0.5000", false, 4, "0.4875")]
    //
    // High-price low-precision (precision=0)
    // BUY: ask=50000 → 50000*1.005=50250 → sig5=50250 → clamp(0,ceil)=50250 → 50250
    #[case("49900", "50000", true, 0, "50250")]
    // SELL: bid=49900 → 49900*0.995=49650.5 → sig5=49650 → clamp(0,floor)=49650 → 49650
    #[case("49900", "50000", false, 0, "49650")]
    //
    // Very small price (precision=6)
    // BUY: ask=0.001234 → 0.001234*1.005=0.0012402 → sig5=0.0012402 → clamp(6,ceil)=0.001241
    #[case("0.001200", "0.001234", true, 6, "0.001241")]
    // SELL: bid=0.0012 → 0.0012*0.995=0.001194 → sig5=0.001194 → clamp(6,floor)=0.001194
    #[case("0.001200", "0.001234", false, 6, "0.001194")]
    fn test_derive_market_order_price(
        #[case] bid: &str,
        #[case] ask: &str,
        #[case] is_buy: bool,
        #[case] price_decimals: u8,
        #[case] expected: &str,
    ) {
        let quote = make_quote(bid, ask);
        let result =
            derive_market_order_price(&quote, is_buy, price_decimals, DEFAULT_MARKET_SLIPPAGE_BPS);
        let expected_dec = Decimal::from_str(expected).unwrap();
        assert_eq!(result, expected_dec);

        // Verify the result matches the full pipeline manually
        let base = if is_buy {
            quote.ask_price.as_decimal()
        } else {
            quote.bid_price.as_decimal()
        };
        let derived = derive_limit_from_trigger(base, is_buy, DEFAULT_MARKET_SLIPPAGE_BPS);
        let sig_rounded = round_to_sig_figs(derived, 5);
        let pipeline = clamp_price_to_precision(sig_rounded, price_decimals, is_buy).normalize();
        assert_eq!(result, pipeline);

        // Must not have trailing zeros after decimal point
        let s = result.to_string();
        if s.contains('.') {
            assert!(!s.ends_with('0'), "Price {s} has trailing zeros");
        }

        // Sig figs must not exceed 5
        let sig_count = count_sig_figs(&s);
        assert!(sig_count <= 5, "Price {s} has {sig_count} sig figs, max 5",);

        // Decimal places must not exceed instrument precision
        let actual_decimals = s.find('.').map_or(0, |dot| s.len() - dot - 1);
        assert!(
            actual_decimals <= price_decimals as usize,
            "Price {s} has {actual_decimals} decimals, max {price_decimals}",
        );
    }

    #[rstest]
    #[case(50, dec!(1000), true, dec!(1005))] // default 0.5% BUY
    #[case(50, dec!(1000), false, dec!(995))] // default 0.5% SELL
    #[case(0, dec!(1000), true, dec!(1000))] // 0 bps: no adjustment
    #[case(100, dec!(1000), true, dec!(1010))] // 1% BUY
    #[case(100, dec!(1000), false, dec!(990))] // 1% SELL
    #[case(800, dec!(1000), true, dec!(1080))] // 8% (Hyperliquid SDK default) BUY
    #[case(800, dec!(1000), false, dec!(920))] // 8% SELL
    fn test_derive_limit_from_trigger_respects_bps(
        #[case] slippage_bps: u32,
        #[case] trigger: Decimal,
        #[case] is_buy: bool,
        #[case] expected: Decimal,
    ) {
        let result = derive_limit_from_trigger(trigger, is_buy, slippage_bps);
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_derive_market_order_price_respects_slippage_override() {
        let quote = make_quote("100.00", "100.10");
        let tight = derive_market_order_price(&quote, true, 2, 50);
        let wide = derive_market_order_price(&quote, true, 2, 800);
        assert_eq!(tight, dec!(100.6));
        assert_eq!(wide, dec!(108.11));
        assert!(wide > tight);
    }

    // Locks in the field-selection invariant; diverging from it would silently
    // disagree with the HTTP parser whenever `account_value != total_raw_usd`
    // or the nested and top-level `withdrawable` values differ.
    #[rstest]
    fn test_parse_account_balances_uses_total_raw_usd_and_top_level_withdrawable() {
        let json = r#"{
            "assetPositions": [],
            "crossMarginSummary": {
                "accountValue": "150",
                "totalNtlPos": "0",
                "totalRawUsd": "100",
                "totalMarginUsed": "20",
                "withdrawable": "120"
            },
            "withdrawable": "80",
            "time": 1700000000000
        }"#;

        let state: ClearinghouseState = serde_json::from_str(json).unwrap();
        let (balances, margins) = parse_account_balances_and_margins(&state).unwrap();

        assert_eq!(balances.len(), 1);
        let balance = &balances[0];
        // Total comes from total_raw_usd (100), not account_value (150); free comes
        // from top-level state.withdrawable (80), not the nested summary.withdrawable (120).
        assert_eq!(balance.total.as_decimal(), dec!(100));
        assert_eq!(balance.free.as_decimal(), dec!(80));
        assert_eq!(balance.locked.as_decimal(), dec!(20));

        assert_eq!(margins.len(), 1);
        assert_eq!(margins[0].initial.as_decimal(), dec!(20));
    }

    #[rstest]
    fn test_parse_account_balances_bumps_total_when_withdrawable_exceeds() {
        let json = r#"{
            "assetPositions": [],
            "crossMarginSummary": {
                "accountValue": "100",
                "totalNtlPos": "0",
                "totalRawUsd": "100",
                "totalMarginUsed": "0",
                "withdrawable": "100"
            },
            "withdrawable": "150",
            "time": 1700000000000
        }"#;

        let state: ClearinghouseState = serde_json::from_str(json).unwrap();
        let (balances, _) = parse_account_balances_and_margins(&state).unwrap();

        assert_eq!(balances.len(), 1);
        let balance = &balances[0];
        assert_eq!(balance.total.as_decimal(), dec!(150));
        assert_eq!(balance.free.as_decimal(), dec!(150));
        assert_eq!(balance.locked.as_decimal(), dec!(0));
    }

    #[rstest]
    fn test_parse_account_balances_returns_empty_when_no_cross_margin_summary() {
        let json = r#"{
            "assetPositions": [],
            "withdrawable": "100",
            "time": 1700000000000
        }"#;

        let state: ClearinghouseState = serde_json::from_str(json).unwrap();
        let (balances, margins) = parse_account_balances_and_margins(&state).unwrap();
        assert!(balances.is_empty());
        assert!(margins.is_empty());
    }

    #[rstest]
    fn test_parse_spot_account_balances_emits_one_per_token() {
        let json = r#"{
            "balances": [
                {"coin": "USDC", "token": 0, "total": "100.25", "hold": "10", "entryNtl": "0"},
                {"coin": "PURR", "token": 1, "total": "50", "hold": "0", "entryNtl": "25"},
                {"coin": "DUST", "token": 2, "total": "0", "hold": "0", "entryNtl": "0"}
            ]
        }"#;

        let state: SpotClearinghouseState = serde_json::from_str(json).unwrap();
        let balances = parse_spot_account_balances(&state).unwrap();

        assert_eq!(balances.len(), 2);

        let usdc = &balances[0];
        assert_eq!(usdc.currency.code.as_str(), "USDC");
        assert_eq!(usdc.total.as_decimal(), dec!(100.25));
        assert_eq!(usdc.free.as_decimal(), dec!(90.25));
        assert_eq!(usdc.locked.as_decimal(), dec!(10));

        let purr = &balances[1];
        assert_eq!(purr.currency.code.as_str(), "PURR");
        assert_eq!(purr.total.as_decimal(), dec!(50));
        assert_eq!(purr.free.as_decimal(), dec!(50));
    }

    #[rstest]
    fn test_parse_spot_account_balances_clamps_hold_to_total() {
        let json = r#"{
            "balances": [
                {"coin": "HYPE", "token": 5, "total": "5", "hold": "10", "entryNtl": "0"}
            ]
        }"#;

        let state: SpotClearinghouseState = serde_json::from_str(json).unwrap();
        let balances = parse_spot_account_balances(&state).unwrap();

        assert_eq!(balances.len(), 1);
        let hype = &balances[0];
        assert_eq!(hype.total.as_decimal(), dec!(5));
        assert_eq!(hype.free.as_decimal(), dec!(0));
        assert_eq!(hype.locked.as_decimal(), dec!(5));
    }

    #[rstest]
    fn test_parse_spot_account_balances_empty() {
        let state = SpotClearinghouseState::default();
        let balances = parse_spot_account_balances(&state).unwrap();
        assert!(balances.is_empty());
    }

    #[rstest]
    fn test_parse_combined_deduplicates_usdc_when_perp_summary_present() {
        let perp_json = r#"{
            "assetPositions": [],
            "crossMarginSummary": {
                "accountValue": "500",
                "totalNtlPos": "0",
                "totalRawUsd": "500",
                "totalMarginUsed": "0",
                "withdrawable": "500"
            },
            "withdrawable": "500"
        }"#;
        let perp_state: ClearinghouseState = serde_json::from_str(perp_json).unwrap();

        let spot_json = r#"{
            "balances": [
                {"coin": "USDC", "token": 0, "total": "123", "hold": "0", "entryNtl": "0"},
                {"coin": "PURR", "token": 1, "total": "10", "hold": "0", "entryNtl": "5"}
            ]
        }"#;
        let spot_state: SpotClearinghouseState = serde_json::from_str(spot_json).unwrap();

        let (balances, margins) =
            parse_combined_account_balances_and_margins(&perp_state, &spot_state).unwrap();

        assert!(margins.is_empty());
        assert_eq!(balances.len(), 2);
        assert_eq!(balances[0].currency.code.as_str(), "USDC");
        assert_eq!(balances[0].total.as_decimal(), dec!(500));
        assert_eq!(balances[1].currency.code.as_str(), "PURR");
        assert_eq!(balances[1].total.as_decimal(), dec!(10));
    }

    #[rstest]
    fn test_parse_combined_uses_spot_usdc_when_perp_summary_missing() {
        let perp_json = r#"{"assetPositions": []}"#;
        let perp_state: ClearinghouseState = serde_json::from_str(perp_json).unwrap();

        let spot_json = r#"{
            "balances": [
                {"coin": "USDC", "token": 0, "total": "50", "hold": "0", "entryNtl": "0"}
            ]
        }"#;
        let spot_state: SpotClearinghouseState = serde_json::from_str(spot_json).unwrap();

        let (balances, _) =
            parse_combined_account_balances_and_margins(&perp_state, &spot_state).unwrap();

        assert_eq!(balances.len(), 1);
        assert_eq!(balances[0].currency.code.as_str(), "USDC");
        assert_eq!(balances[0].total.as_decimal(), dec!(50));
    }
}
