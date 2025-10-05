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

use std::str::FromStr;

use anyhow::{Context, Result};
use nautilus_model::{
    enums::{OrderSide, OrderType, TimeInForce},
    orders::{Order, any::OrderAny},
};
use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, Serializer};
use serde_json::Value;

use crate::http::models::{
    AssetId, Cloid, HyperliquidExchangeResponse, HyperliquidExecCancelByCloidRequest,
    HyperliquidExecLimitParams, HyperliquidExecOrderKind, HyperliquidExecPlaceOrderRequest,
    HyperliquidExecTif, HyperliquidExecTpSl, HyperliquidExecTriggerParams,
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
) -> Result<HyperliquidExecTif> {
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
pub fn extract_asset_id_from_symbol(symbol: &str) -> Result<AssetId> {
    // For perpetuals, remove "-USD" suffix to get the base asset
    if let Some(base) = symbol.strip_suffix("-USD") {
        // Convert symbol like "BTC-USD" to asset index
        // This is a simplified mapping - in practice you'd need to query the asset registry
        Ok(match base {
            "BTC" => 0,
            "ETH" => 1,
            "DOGE" => 3,
            "SOL" => 4,
            "WIF" => 8,
            "SHIB" => 10,
            "PEPE" => 11,
            _ => {
                // For unknown assets, we'll need to query the meta endpoint
                // For now, return a placeholder that will need to be resolved
                anyhow::bail!("Asset ID mapping not found for symbol: {symbol}")
            }
        })
    } else {
        anyhow::bail!("Cannot extract asset ID from symbol: {symbol}")
    }
}

/// Converts a Nautilus order into a Hyperliquid order request.
pub fn order_to_hyperliquid_request(order: &OrderAny) -> Result<HyperliquidExecPlaceOrderRequest> {
    let instrument_id = order.instrument_id();
    let symbol = instrument_id.symbol.as_str();
    let asset = extract_asset_id_from_symbol(symbol)
        .with_context(|| format!("Failed to extract asset ID from symbol: {}", symbol))?;

    let is_buy = matches!(order.order_side(), OrderSide::Buy);
    let reduce_only = order.is_reduce_only();

    // Convert price to decimal
    let price_decimal = match order.price() {
        Some(price) => Decimal::from_str_exact(&price.to_string())
            .with_context(|| format!("Failed to convert price to decimal: {}", price))?,
        None => {
            // For market orders without price, use 0 as placeholder
            // The actual market price will be determined by the exchange
            if matches!(order.order_type(), OrderType::Market) {
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
    let kind = match order.order_type() {
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

                HyperliquidExecOrderKind::Trigger {
                    trigger: HyperliquidExecTriggerParams {
                        is_market: true,
                        trigger_px: trigger_price_decimal,
                        tpsl: HyperliquidExecTpSl::Sl, // Default to stop loss
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

                HyperliquidExecOrderKind::Trigger {
                    trigger: HyperliquidExecTriggerParams {
                        is_market: false,
                        trigger_px: trigger_price_decimal,
                        tpsl: HyperliquidExecTpSl::Sl, // Default to stop loss
                    },
                }
            } else {
                anyhow::bail!("Stop limit orders require a trigger price")
            }
        }
        _ => anyhow::bail!(
            "Unsupported order type for Hyperliquid: {:?}",
            order.order_type()
        ),
    };

    // Convert client order ID to CLOID
    let cloid = match Cloid::from_hex(order.client_order_id()) {
        Ok(cloid) => Some(cloid),
        Err(err) => {
            anyhow::bail!(
                "Failed to convert client order ID '{}' to CLOID: {}",
                order.client_order_id(),
                err
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
) -> Result<Vec<HyperliquidExecPlaceOrderRequest>> {
    orders
        .iter()
        .map(|order| order_to_hyperliquid_request(order))
        .collect()
}

/// Creates a JSON value representing multiple orders for the Hyperliquid exchange action.
pub fn orders_to_hyperliquid_action_value(orders: &[&OrderAny]) -> Result<Value> {
    let requests = orders_to_hyperliquid_requests(orders)?;
    serde_json::to_value(requests).context("Failed to serialize orders to JSON")
}

/// Converts an OrderAny into a Hyperliquid order request.
pub fn order_any_to_hyperliquid_request(
    order: &OrderAny,
) -> Result<HyperliquidExecPlaceOrderRequest> {
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
) -> Result<HyperliquidExecCancelByCloidRequest> {
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

/// Creates a JSON value representing cancel requests for the Hyperliquid exchange action.
pub fn cancel_requests_to_hyperliquid_action_value(
    requests: &[HyperliquidExecCancelByCloidRequest],
) -> Result<Value> {
    serde_json::to_value(requests).context("Failed to serialize cancel requests to JSON")
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
}
