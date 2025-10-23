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

//! Parsing utilities that convert Hyperliquid payloads into Nautilus domain models.
//!
//! # Conditional Order Support
//!
//! This module implements comprehensive conditional order support for Hyperliquid,
//! following patterns established in the OKX, Bybit, and BitMEX adapters.
//!
//! ## Supported Order Types
//!
//! ### Standard Orders
//! - **Market**: Implemented as IOC (Immediate-or-Cancel) limit orders
//! - **Limit**: Standard limit orders with GTC/IOC/ALO time-in-force
//!
//! ### Conditional/Trigger Orders
//! - **StopMarket**: Protective stop that triggers at specified price and executes at market
//! - **StopLimit**: Protective stop that triggers at specified price and executes at limit
//! - **MarketIfTouched**: Profit-taking/entry order that triggers and executes at market
//! - **LimitIfTouched**: Profit-taking/entry order that triggers and executes at limit
//!
//! ## Order Semantics
//!
//! ### Stop Orders (StopMarket/StopLimit)
//! - Used for protective stops and risk management
//! - Mapped to Hyperliquid's trigger orders with `tpsl: Sl`
//! - Trigger when price reaches the stop level
//! - Execute immediately (market) or at limit price
//!
//! ### If Touched Orders (MarketIfTouched/LimitIfTouched)
//! - Used for profit-taking or entry orders
//! - Mapped to Hyperliquid's trigger orders with `tpsl: Tp`
//! - Trigger when price reaches the target level
//! - Execute immediately (market) or at limit price
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
//! Currently, Hyperliquid uses **last traded price** for all trigger evaluations.
//!
//! Future enhancement: Add support for mark/index price triggers if Hyperliquid API adds this feature.
//! See OKX's `OKXTriggerType` and Bybit's `BybitTriggerType` for reference implementations.
//!
//! ## Examples
//!
//! ### Stop Loss Order
//! ```ignore
//! // Long position at $100, stop loss at $95
//! let order = StopMarket {
//!     side: Sell,
//!     trigger_price: $95,
//!     // ... other fields
//! };
//! // Maps to: Trigger { is_market: true, trigger_px: $95, tpsl: Sl }
//! ```
//!
//! ### Take Profit Order
//! ```ignore
//! // Long position at $100, take profit at $110
//! let order = MarketIfTouched {
//!     side: Sell,
//!     trigger_price: $110,
//!     // ... other fields
//! };
//! // Maps to: Trigger { is_market: true, trigger_px: $110, tpsl: Tp }
//! ```
//!
//! ## Integration with Other Adapters
//!
//! This implementation reuses patterns from:
//! - **OKX**: Conditional order types and algo order API structure
//! - **Bybit**: TP/SL mode detection and trigger direction logic
//! - **BitMEX**: Stop order handling and trigger price validation
//!
//! See:
//! - `crates/adapters/okx/src/common/consts.rs` - OKX_CONDITIONAL_ORDER_TYPES
//! - `crates/adapters/bybit/src/common/enums.rs` - BybitStopOrderType, BybitTriggerType
//! - `crates/adapters/bitmex/src/execution/mod.rs` - trigger_price handling

use std::str::FromStr;

use anyhow::Context;
use nautilus_model::{
    enums::{OrderSide, OrderStatus, OrderType, TimeInForce},
    identifiers::{InstrumentId, Symbol, Venue},
    orders::{Order, any::OrderAny},
    types::{AccountBalance, Currency, MarginBalance, Money},
};
use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, Serializer};
use serde_json::Value;

use crate::http::models::{
    AssetId, Cloid, CrossMarginSummary, HyperliquidExchangeResponse,
    HyperliquidExecCancelByCloidRequest, HyperliquidExecLimitParams, HyperliquidExecOrderKind,
    HyperliquidExecPlaceOrderRequest, HyperliquidExecTif, HyperliquidExecTpSl,
    HyperliquidExecTriggerParams,
};

/// Serializes decimal as string (lossless, no scientific notation).
pub fn serialize_decimal_as_str<S>(decimal: &Decimal, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&decimal.normalize().to_string())
}

/// Deserializes decimal from string only (reject numbers to avoid precision loss).
pub fn deserialize_decimal_from_str<'de, D>(deserializer: D) -> Result<Decimal, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    Decimal::from_str(&s).map_err(serde::de::Error::custom)
}

/// Serialize optional decimal as string
pub fn serialize_optional_decimal_as_str<S>(
    decimal: &Option<Decimal>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match decimal {
        Some(d) => serializer.serialize_str(&d.normalize().to_string()),
        None => serializer.serialize_none(),
    }
}

/// Deserialize optional decimal from string
pub fn deserialize_optional_decimal_from_str<'de, D>(
    deserializer: D,
) -> Result<Option<Decimal>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    match opt {
        Some(s) => {
            let decimal = Decimal::from_str(&s).map_err(serde::de::Error::custom)?;
            Ok(Some(decimal))
        }
        None => Ok(None),
    }
}

/// Serialize vector of decimals as strings
pub fn serialize_vec_decimal_as_str<S>(
    decimals: &Vec<Decimal>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    use serde::ser::SerializeSeq;
    let mut seq = serializer.serialize_seq(Some(decimals.len()))?;
    for decimal in decimals {
        seq.serialize_element(&decimal.normalize().to_string())?;
    }
    seq.end()
}

/// Deserialize vector of decimals from strings
pub fn deserialize_vec_decimal_from_str<'de, D>(deserializer: D) -> Result<Vec<Decimal>, D::Error>
where
    D: Deserializer<'de>,
{
    let strings = Vec::<String>::deserialize(deserializer)?;
    strings
        .into_iter()
        .map(|s| Decimal::from_str(&s).map_err(serde::de::Error::custom))
        .collect()
}

////////////////////////////////////////////////////////////////////////////////
// Normalization and Validation Functions
////////////////////////////////////////////////////////////////////////////////

/// Round price down to the nearest valid tick size
#[inline]
pub fn round_down_to_tick(price: Decimal, tick_size: Decimal) -> Decimal {
    if tick_size.is_zero() {
        return price;
    }
    (price / tick_size).floor() * tick_size
}

/// Round quantity down to the nearest valid step size
#[inline]
pub fn round_down_to_step(qty: Decimal, step_size: Decimal) -> Decimal {
    if step_size.is_zero() {
        return qty;
    }
    (qty / step_size).floor() * step_size
}

/// Ensure the notional value meets minimum requirements
#[inline]
pub fn ensure_min_notional(
    price: Decimal,
    qty: Decimal,
    min_notional: Decimal,
) -> Result<(), String> {
    let notional = price * qty;
    if notional < min_notional {
        Err(format!(
            "Notional value {} is less than minimum required {}",
            notional, min_notional
        ))
    } else {
        Ok(())
    }
}

/// Normalize price to the specified number of decimal places
pub fn normalize_price(price: Decimal, decimals: u8) -> Decimal {
    let scale = Decimal::from(10_u64.pow(decimals as u32));
    (price * scale).floor() / scale
}

/// Normalize quantity to the specified number of decimal places
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

// ================================================================================================
// Order Conversion Functions
// ================================================================================================

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
        (TimeInForce::Fok, false) => Ok(HyperliquidExecTif::Ioc), // FOK maps to IOC in Hyperliquid
        _ => anyhow::bail!("Unsupported time in force for Hyperliquid: {tif:?}"),
    }
}

/// Extracts asset ID from instrument symbol.
///
/// For Hyperliquid, this typically involves parsing the symbol to get the underlying asset.
/// Currently supports a hardcoded mapping for common assets.
///
/// # Errors
///
/// Returns an error if the symbol format is unsupported or the asset is not found.
pub fn extract_asset_id_from_symbol(symbol: &str) -> anyhow::Result<AssetId> {
    // For perpetuals, remove "-USD-PERP" or "-USD" suffix to get the base asset
    let base = if let Some(base) = symbol.strip_suffix("-PERP") {
        // Remove "-USD-PERP" -> Remove "-USD" from what remains
        base.strip_suffix("-USD")
            .ok_or_else(|| anyhow::anyhow!("Cannot extract asset from symbol: {symbol}"))?
    } else if let Some(base) = symbol.strip_suffix("-USD") {
        // Just "-USD" suffix
        base
    } else {
        anyhow::bail!("Cannot extract asset ID from symbol: {symbol}")
    };

    // Convert symbol like "BTC" to asset index
    // Asset indices from Hyperliquid testnet meta endpoint (as of October 2025)
    // Source: https://api.hyperliquid-testnet.xyz/info
    //
    // NOTE: These indices may change. For production, consider querying the meta endpoint
    // dynamically during initialization to avoid hardcoded mappings.
    Ok(match base {
        "SOL" => 0,    // Solana
        "APT" => 1,    // Aptos
        "ATOM" => 2,   // Cosmos
        "BTC" => 3,    // Bitcoin
        "ETH" => 4,    // Ethereum
        "MATIC" => 5,  // Polygon
        "BNB" => 6,    // Binance Coin
        "AVAX" => 7,   // Avalanche
        "DYDX" => 9,   // dYdX
        "APE" => 10,   // ApeCoin
        "OP" => 11,    // Optimism
        "kPEPE" => 12, // Pepe (1k units)
        "ARB" => 13,   // Arbitrum
        "kSHIB" => 29, // Shiba Inu (1k units)
        "WIF" => 78,   // Dogwifhat
        "DOGE" => 173, // Dogecoin
        _ => {
            // For unknown assets, query the meta endpoint or add to this mapping
            anyhow::bail!("Asset ID mapping not found for symbol: {symbol}")
        }
    })
}

/// Determines if a trigger order should be TP (take profit) or SL (stop loss).
///
/// Logic follows exchange patterns from OKX/Bybit:
/// - For BUY orders: trigger above current price = SL, below = TP
/// - For SELL orders: trigger below current price = SL, above = TP
/// - For Market/Limit If Touched orders: always TP (triggered when price reaches target)
///
/// # Note
///
/// Hyperliquid's trigger logic:
/// - StopMarket/StopLimit: Protective stops (SL)
/// - MarketIfTouched/LimitIfTouched: Profit taking or entry orders (TP)
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

/// Converts a Nautilus order into a Hyperliquid order request.
///
/// # Supported Order Types
///
/// - `Market`: Implemented as IOC limit order
/// - `Limit`: Standard limit order with TIF (GTC/IOC/ALO)
/// - `StopMarket`: Trigger order with market execution (protective stop)
/// - `StopLimit`: Trigger order with limit price (protective stop)
/// - `MarketIfTouched`: Trigger order with market execution (profit taking/entry)
/// - `LimitIfTouched`: Trigger order with limit price (profit taking/entry)
///
/// # Conditional Order Patterns
///
/// Following patterns from OKX and Bybit adapters:
/// - Stop orders (StopMarket/StopLimit) use `tpsl: Sl`
/// - If Touched orders (MIT/LIT) use `tpsl: Tp`
/// - Trigger price determines when order activates
/// - Order side and trigger price relationship determines TP vs SL semantics
///
/// # Trigger Type Support
///
/// Hyperliquid currently uses last traded price for all triggers.
/// Future enhancement: Add support for mark/index price triggers if Hyperliquid API supports it.
pub fn order_to_hyperliquid_request(
    order: &OrderAny,
) -> anyhow::Result<HyperliquidExecPlaceOrderRequest> {
    let instrument_id = order.instrument_id();
    let symbol = instrument_id.symbol.as_str();
    let asset = extract_asset_id_from_symbol(symbol)
        .with_context(|| format!("Failed to extract asset ID from symbol: {}", symbol))?;

    let is_buy = matches!(order.order_side(), OrderSide::Buy);
    let reduce_only = order.is_reduce_only();
    let order_side = order.order_side();
    let order_type = order.order_type();

    // Convert price to decimal
    let price_decimal = match order.price() {
        Some(price) => Decimal::from_str_exact(&price.to_string())
            .with_context(|| format!("Failed to convert price to decimal: {}", price))?,
        None => {
            // For market orders without price, use 0 as placeholder
            // The actual market price will be determined by the exchange
            if matches!(
                order_type,
                OrderType::Market | OrderType::StopMarket | OrderType::MarketIfTouched
            ) {
                Decimal::ZERO
            } else {
                anyhow::bail!("Limit orders require a price")
            }
        }
    };

    // Convert size to decimal
    let size_decimal =
        Decimal::from_str_exact(&order.quantity().to_string()).with_context(|| {
            format!(
                "Failed to convert quantity to decimal: {}",
                order.quantity()
            )
        })?;

    // Determine order kind based on order type
    let kind = match order_type {
        OrderType::Market => {
            // Market orders in Hyperliquid are implemented as limit orders with IOC time-in-force
            HyperliquidExecOrderKind::Limit {
                limit: HyperliquidExecLimitParams {
                    tif: HyperliquidExecTif::Ioc,
                },
            }
        }
        OrderType::Limit => {
            let tif =
                time_in_force_to_hyperliquid_tif(order.time_in_force(), order.is_post_only())?;
            HyperliquidExecOrderKind::Limit {
                limit: HyperliquidExecLimitParams { tif },
            }
        }
        OrderType::StopMarket => {
            if let Some(trigger_price) = order.trigger_price() {
                let trigger_price_decimal = Decimal::from_str_exact(&trigger_price.to_string())
                    .with_context(|| {
                        format!(
                            "Failed to convert trigger price to decimal: {}",
                            trigger_price
                        )
                    })?;

                // Determine TP/SL based on order semantics
                let tpsl = determine_tpsl_type(
                    order_type,
                    order_side,
                    trigger_price_decimal,
                    None, // Current market price not available here
                );

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
                let trigger_price_decimal = Decimal::from_str_exact(&trigger_price.to_string())
                    .with_context(|| {
                        format!(
                            "Failed to convert trigger price to decimal: {}",
                            trigger_price
                        )
                    })?;

                // Determine TP/SL based on order semantics
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
            // MIT orders trigger when price is reached and execute at market
            // These are typically used for profit taking or entry orders
            if let Some(trigger_price) = order.trigger_price() {
                let trigger_price_decimal = Decimal::from_str_exact(&trigger_price.to_string())
                    .with_context(|| {
                        format!(
                            "Failed to convert trigger price to decimal: {}",
                            trigger_price
                        )
                    })?;

                HyperliquidExecOrderKind::Trigger {
                    trigger: HyperliquidExecTriggerParams {
                        is_market: true,
                        trigger_px: trigger_price_decimal,
                        tpsl: HyperliquidExecTpSl::Tp, // MIT is typically for profit taking
                    },
                }
            } else {
                anyhow::bail!("Market-if-touched orders require a trigger price")
            }
        }
        OrderType::LimitIfTouched => {
            // LIT orders trigger when price is reached and execute at limit price
            // These are typically used for profit taking or entry orders with price control
            if let Some(trigger_price) = order.trigger_price() {
                let trigger_price_decimal = Decimal::from_str_exact(&trigger_price.to_string())
                    .with_context(|| {
                        format!(
                            "Failed to convert trigger price to decimal: {}",
                            trigger_price
                        )
                    })?;

                HyperliquidExecOrderKind::Trigger {
                    trigger: HyperliquidExecTriggerParams {
                        is_market: false,
                        trigger_px: trigger_price_decimal,
                        tpsl: HyperliquidExecTpSl::Tp, // LIT is typically for profit taking
                    },
                }
            } else {
                anyhow::bail!("Limit-if-touched orders require a trigger price")
            }
        }
        _ => anyhow::bail!("Unsupported order type for Hyperliquid: {:?}", order_type),
    };

    // Convert client order ID to CLOID
    let cloid = match Cloid::from_hex(order.client_order_id()) {
        Ok(cloid) => Some(cloid),
        Err(e) => {
            anyhow::bail!(
                "Failed to convert client order ID '{}' to CLOID: {}",
                order.client_order_id(),
                e
            )
        }
    };

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

/// Converts a list of Nautilus orders into Hyperliquid order requests.
pub fn orders_to_hyperliquid_requests(
    orders: &[&OrderAny],
) -> anyhow::Result<Vec<HyperliquidExecPlaceOrderRequest>> {
    orders
        .iter()
        .map(|order| order_to_hyperliquid_request(order))
        .collect()
}

/// Creates a JSON value representing multiple orders for the Hyperliquid exchange action.
pub fn orders_to_hyperliquid_action_value(orders: &[&OrderAny]) -> anyhow::Result<Value> {
    let requests = orders_to_hyperliquid_requests(orders)?;
    serde_json::to_value(requests).context("Failed to serialize orders to JSON")
}

/// Converts an OrderAny into a Hyperliquid order request.
pub fn order_any_to_hyperliquid_request(
    order: &OrderAny,
) -> anyhow::Result<HyperliquidExecPlaceOrderRequest> {
    order_to_hyperliquid_request(order)
}

/// Converts a client order ID to a Hyperliquid cancel request.
///
/// # Errors
///
/// Returns an error if the symbol cannot be parsed or the client order ID is invalid.
pub fn client_order_id_to_cancel_request(
    client_order_id: &str,
    symbol: &str,
) -> anyhow::Result<HyperliquidExecCancelByCloidRequest> {
    let asset = extract_asset_id_from_symbol(symbol)
        .with_context(|| format!("Failed to extract asset ID from symbol: {}", symbol))?;

    let cloid = Cloid::from_hex(client_order_id).map_err(|e| {
        anyhow::anyhow!(
            "Failed to convert client order ID '{}' to CLOID: {}",
            client_order_id,
            e
        )
    })?;

    Ok(HyperliquidExecCancelByCloidRequest { asset, cloid })
}

/// Checks if a Hyperliquid exchange response indicates success.
pub fn is_response_successful(response: &HyperliquidExchangeResponse) -> bool {
    matches!(response, HyperliquidExchangeResponse::Status { status, .. } if status == "ok")
}

/// Extracts error message from a Hyperliquid exchange response.
pub fn extract_error_message(response: &HyperliquidExchangeResponse) -> String {
    match response {
        HyperliquidExchangeResponse::Status { status, response } => {
            if status == "ok" {
                "Operation successful".to_string()
            } else {
                // Try to extract error message from response data
                if let Some(error_msg) = response.get("error").and_then(|v| v.as_str()) {
                    error_msg.to_string()
                } else {
                    format!("Request failed with status: {}", status)
                }
            }
        }
        HyperliquidExchangeResponse::Error { error } => error.clone(),
    }
}

/// Determines if an order is a conditional/trigger order based on order data.
///
/// # Arguments
///
/// * `trigger_px` - Optional trigger price
/// * `tpsl` - Optional TP/SL indicator
///
/// # Returns
///
/// `true` if the order is a conditional order, `false` otherwise.
pub fn is_conditional_order_data(trigger_px: Option<&str>, tpsl: Option<&str>) -> bool {
    trigger_px.is_some() && tpsl.is_some()
}

/// Parses trigger order type from Hyperliquid order data.
///
/// # Arguments
///
/// * `is_market` - Whether this is a market trigger order
/// * `tpsl` - TP/SL indicator ("tp" or "sl")
///
/// # Returns
///
/// The corresponding Nautilus `OrderType`.
pub fn parse_trigger_order_type(is_market: bool, tpsl: &str) -> OrderType {
    match (is_market, tpsl) {
        (true, "sl") => OrderType::StopMarket,
        (false, "sl") => OrderType::StopLimit,
        (true, "tp") => OrderType::MarketIfTouched,
        (false, "tp") => OrderType::LimitIfTouched,
        _ => OrderType::StopMarket, // Default fallback
    }
}

/// Extracts order status from WebSocket order data.
///
/// # Arguments
///
/// * `status` - Status string from WebSocket
/// * `trigger_activated` - Whether trigger has been activated (for conditional orders)
///
/// # Returns
///
/// A tuple of (OrderStatus, optional trigger status string).
pub fn parse_order_status_with_trigger(
    status: &str,
    trigger_activated: Option<bool>,
) -> (OrderStatus, Option<String>) {
    use crate::common::enums::hyperliquid_status_to_order_status;

    let base_status = hyperliquid_status_to_order_status(status);

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
///
/// # Arguments
///
/// * `offset` - Trailing offset value
/// * `offset_type` - Type of offset ("price", "percentage", "basisPoints")
/// * `callback_price` - Current callback price
///
/// # Returns
///
/// Human-readable description of trailing stop parameters.
pub fn format_trailing_stop_info(
    offset: &str,
    offset_type: &str,
    callback_price: Option<&str>,
) -> String {
    let offset_desc = match offset_type {
        "percentage" => format!("{}%", offset),
        "basisPoints" => format!("{} bps", offset),
        "price" => offset.to_string(),
        _ => offset.to_string(),
    };

    if let Some(callback) = callback_price {
        format!(
            "Trailing stop: {} offset, callback at {}",
            offset_desc, callback
        )
    } else {
        format!("Trailing stop: {} offset", offset_desc)
    }
}

/// Validates conditional order parameters from WebSocket data.
///
/// # Arguments
///
/// * `trigger_px` - Trigger price
/// * `tpsl` - TP/SL indicator
/// * `is_market` - Market or limit flag
///
/// # Returns
///
/// `Ok(())` if parameters are valid, `Err` with description otherwise.
///
/// # Panics
///
/// This function does not panic - it returns errors instead of panicking.
pub fn validate_conditional_order_params(
    trigger_px: Option<&str>,
    tpsl: Option<&str>,
    is_market: Option<bool>,
) -> anyhow::Result<()> {
    if trigger_px.is_none() {
        anyhow::bail!("Conditional order missing trigger price");
    }

    if tpsl.is_none() {
        anyhow::bail!("Conditional order missing tpsl indicator");
    }

    let tpsl_value = tpsl.expect("tpsl should be Some at this point");
    if tpsl_value != "tp" && tpsl_value != "sl" {
        anyhow::bail!("Invalid tpsl value: {}", tpsl_value);
    }

    if is_market.is_none() {
        anyhow::bail!("Conditional order missing is_market flag");
    }

    Ok(())
}

/// Parses trigger price from string to Decimal.
///
/// # Arguments
///
/// * `trigger_px` - Trigger price as string
///
/// # Returns
///
/// Parsed Decimal value or error.
pub fn parse_trigger_price(trigger_px: &str) -> anyhow::Result<Decimal> {
    Decimal::from_str_exact(trigger_px)
        .with_context(|| format!("Failed to parse trigger price: {}", trigger_px))
}

/// Parses Hyperliquid clearinghouse state into Nautilus account balances and margins.
///
/// # Errors
///
/// Returns an error if the data cannot be parsed.
pub fn parse_account_balances_and_margins(
    cross_margin_summary: &CrossMarginSummary,
) -> anyhow::Result<(Vec<AccountBalance>, Vec<MarginBalance>)> {
    let mut balances = Vec::new();
    let mut margins = Vec::new();

    // Parse balance from cross margin summary
    let currency = Currency::USD(); // Hyperliquid uses USDC/USD

    // Account value represents total collateral
    let total_value = cross_margin_summary
        .account_value
        .to_string()
        .parse::<f64>()?;

    // Withdrawable represents available balance
    let withdrawable = cross_margin_summary
        .withdrawable
        .to_string()
        .parse::<f64>()?;

    // Total margin used is locked in positions
    let margin_used = cross_margin_summary
        .total_margin_used
        .to_string()
        .parse::<f64>()?;

    // Calculate total, locked, and free
    let total = Money::new(total_value, currency);
    let locked = Money::new(margin_used, currency);
    let free = Money::new(withdrawable, currency);

    let balance = AccountBalance::new(total, locked, free);
    balances.push(balance);

    // Create margin balance for the account
    // Initial margin = margin used (locked in positions)
    // Maintenance margin can be approximated from leverage and position values
    // For now, use margin_used as both initial and maintenance (conservative)
    if margin_used > 0.0 {
        let margin_instrument_id =
            InstrumentId::new(Symbol::new("ACCOUNT"), Venue::new("HYPERLIQUID"));

        let initial_margin = Money::new(margin_used, currency);
        let maintenance_margin = Money::new(margin_used, currency);

        let margin_balance =
            MarginBalance::new(initial_margin, maintenance_margin, margin_instrument_id);

        margins.push(margin_balance);
    }

    Ok((balances, margins))
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;
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
        println!("Serialized: {}", json);

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

            assert_eq!(decimal, parsed.value, "Failed for case: {}", case);
            assert_eq!(
                Some(decimal),
                parsed.optional_value,
                "Failed for case: {}",
                case
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
        use rust_decimal_macros::dec;

        assert_eq!(round_down_to_tick(dec!(100.07), dec!(0.05)), dec!(100.05));
        assert_eq!(round_down_to_tick(dec!(100.03), dec!(0.05)), dec!(100.00));
        assert_eq!(round_down_to_tick(dec!(100.05), dec!(0.05)), dec!(100.05));

        // Edge case: zero tick size
        assert_eq!(round_down_to_tick(dec!(100.07), dec!(0)), dec!(100.07));
    }

    #[rstest]
    fn test_round_down_to_step() {
        use rust_decimal_macros::dec;

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
        use rust_decimal_macros::dec;

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
    fn test_normalize_price() {
        use rust_decimal_macros::dec;

        assert_eq!(normalize_price(dec!(100.12345), 2), dec!(100.12));
        assert_eq!(normalize_price(dec!(100.19999), 2), dec!(100.19));
        assert_eq!(normalize_price(dec!(100.999), 0), dec!(100));
        assert_eq!(normalize_price(dec!(100.12345), 4), dec!(100.1234));
    }

    #[rstest]
    fn test_normalize_quantity() {
        use rust_decimal_macros::dec;

        assert_eq!(normalize_quantity(dec!(1.12345), 3), dec!(1.123));
        assert_eq!(normalize_quantity(dec!(1.99999), 3), dec!(1.999));
        assert_eq!(normalize_quantity(dec!(1.999), 0), dec!(1));
        assert_eq!(normalize_quantity(dec!(1.12345), 5), dec!(1.12345));
    }

    #[rstest]
    fn test_normalize_order_complete() {
        use rust_decimal_macros::dec;

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
        use rust_decimal_macros::dec;

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
        use rust_decimal_macros::dec;

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

    // ========================================================================
    // Conditional Order Parsing Tests
    // ========================================================================

    #[rstest]
    fn test_is_conditional_order_data() {
        // Test with trigger price and tpsl (conditional)
        assert!(is_conditional_order_data(Some("50000.0"), Some("sl")));

        // Test with only trigger price (not conditional - needs both)
        assert!(!is_conditional_order_data(Some("50000.0"), None));

        // Test with only tpsl (not conditional - needs both)
        assert!(!is_conditional_order_data(None, Some("tp")));

        // Test with no conditional fields
        assert!(!is_conditional_order_data(None, None));
    }

    #[rstest]
    fn test_parse_trigger_order_type() {
        // Stop Market
        assert_eq!(parse_trigger_order_type(true, "sl"), OrderType::StopMarket);

        // Stop Limit
        assert_eq!(parse_trigger_order_type(false, "sl"), OrderType::StopLimit);

        // Take Profit Market
        assert_eq!(
            parse_trigger_order_type(true, "tp"),
            OrderType::MarketIfTouched
        );

        // Take Profit Limit
        assert_eq!(
            parse_trigger_order_type(false, "tp"),
            OrderType::LimitIfTouched
        );
    }

    #[rstest]
    fn test_parse_order_status_with_trigger() {
        // Test with open status and activated trigger
        let (status, trigger_status) = parse_order_status_with_trigger("open", Some(true));
        assert_eq!(status, OrderStatus::Accepted);
        assert_eq!(trigger_status, Some("activated".to_string()));

        // Test with open status and not activated
        let (status, trigger_status) = parse_order_status_with_trigger("open", Some(false));
        assert_eq!(status, OrderStatus::Accepted);
        assert_eq!(trigger_status, Some("pending".to_string()));

        // Test without trigger info
        let (status, trigger_status) = parse_order_status_with_trigger("open", None);
        assert_eq!(status, OrderStatus::Accepted);
        assert_eq!(trigger_status, None);
    }

    #[rstest]
    fn test_format_trailing_stop_info() {
        // Price offset
        let info = format_trailing_stop_info("100.0", "price", Some("50000.0"));
        assert!(info.contains("100.0"));
        assert!(info.contains("callback at 50000.0"));

        // Percentage offset
        let info = format_trailing_stop_info("5.0", "percentage", None);
        assert!(info.contains("5.0%"));
        assert!(info.contains("Trailing stop"));

        // Basis points offset
        let info = format_trailing_stop_info("250", "basisPoints", Some("49000.0"));
        assert!(info.contains("250 bps"));
        assert!(info.contains("49000.0"));
    }

    #[rstest]
    fn test_parse_trigger_price() {
        use rust_decimal_macros::dec;

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
}
