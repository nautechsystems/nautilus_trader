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

//! Validation utilities for Hyperliquid outcome (prediction) markets.
//!
//! This module provides validation functions for outcome market orders,
//! including price range checks and order type restrictions.

use nautilus_model::{
    enums::OrderType,
    identifiers::InstrumentId,
    orders::{Order, OrderAny},
    types::Price,
};

/// Minimum valid price for outcome markets (0.1% probability).
pub const OUTCOME_MIN_PRICE: &str = "0.001";
/// Maximum valid price for outcome markets (99.9% probability).
pub const OUTCOME_MAX_PRICE: &str = "0.999";

/// Validates an order for outcome market constraints.
///
/// # Checks performed
///
/// 1. **Price range**: Must be within [0.001, 0.999] for outcome instruments
/// 2. **Order type**: Only LIMIT orders are recommended for MVP
///
/// # Arguments
///
/// * `order` - The order to validate
/// * `instrument_id` - The instrument ID (used to detect outcome markets)
///
/// # Returns
///
/// * `Ok(())` if the order passes all validations
/// * `Err(String)` with a descriptive error message if validation fails
///
/// # Examples
///
/// ```rust
/// use nautilus_hyperliquid::http::outcome_validation::validate_outcome_order;
/// use nautilus_model::{identifiers::InstrumentId, types::Price};
///
/// let instrument_id = InstrumentId::from("OUTCOME-2-YES-OUTCOME.HYPERLIQUID");
/// // Validate your order here...
/// ```
pub fn validate_outcome_order(
    order: &OrderAny,
    instrument_id: &InstrumentId,
) -> Result<(), String> {
    // Only validate outcome market instruments
    if !is_outcome_instrument(instrument_id) {
        return Ok(());
    }

    // Validate price range
    if let Some(price) = order.price() {
        validate_price_range(&price)?;
    }

    // Validate order type (MVP: only LIMIT recommended)
    match order.order_type() {
        OrderType::Limit => {} // OK
        _ => {
            // For MVP, we warn but don't block other order types
            log::debug!(
                "Outcome market order uses non-limit type: {:?} for instrument {}",
                order.order_type(),
                instrument_id
            );
        }
    }

    Ok(())
}

/// Checks if an instrument ID represents an outcome market.
///
/// Outcome instruments have the format: `OUTCOME-{id}-{YES|NO}-OUTCOME.{VENUE}`
pub fn is_outcome_instrument(instrument_id: &InstrumentId) -> bool {
    let symbol = instrument_id.symbol.as_str();
    symbol.starts_with("OUTCOME-") && symbol.contains("-OUTCOME")
}

/// Validates that a price is within the valid outcome market range [0.001, 0.999].
///
/// # Arguments
///
/// * `price` - The price to validate
///
/// # Returns
///
/// * `Ok(())` if the price is valid
/// * `Err(String)` if the price is outside the valid range
///
/// # Examples
///
/// ```rust
/// use nautilus_hyperliquid::http::outcome_validation::validate_price_range;
/// use nautilus_model::types::Price;
///
/// let valid_price = Price::from("0.650");
/// assert!(validate_price_range(&valid_price).is_ok());
///
/// let invalid_price = Price::from("1.500");
/// assert!(validate_price_range(&invalid_price).is_err());
/// ```
pub fn validate_price_range(price: &Price) -> Result<(), String> {
    let min = Price::from(OUTCOME_MIN_PRICE);
    let max = Price::from(OUTCOME_MAX_PRICE);

    if price < &min {
        return Err(format!(
            "Outcome market price {} is below minimum {}, market may be settled",
            price, min
        ));
    }

    if price > &max {
        return Err(format!(
            "Outcome market price {} is above maximum {}, market may be settled",
            price, max
        ));
    }

    Ok(())
}

/// Returns the valid price range for outcome markets.
///
/// # Returns
///
/// A tuple of (min_price, max_price) for outcome markets.
pub fn get_outcome_price_range() -> (Price, Price) {
    (Price::from(OUTCOME_MIN_PRICE), Price::from(OUTCOME_MAX_PRICE))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case("OUTCOME-2-YES-OUTCOME.HYPERLIQUID", true)]
    #[case("OUTCOME-10-NO-OUTCOME.HYPERLIQUID", true)]
    #[case("BTC-USD-PERP.HYPERLIQUID", false)]
    #[case("ETH-USDC-SPOT.HYPERLIQUID", false)]
    fn test_is_outcome_instrument(#[case] symbol: &str, #[case] expected: bool) {
        let instrument_id = InstrumentId::from(symbol);
        assert_eq!(is_outcome_instrument(&instrument_id), expected);
    }

    #[rstest]
    #[case("0.001", true)]  // Minimum valid
    #[case("0.500", true)]  // Middle
    #[case("0.999", true)]  // Maximum valid
    #[case("0.000", false)] // Below minimum
    #[case("1.000", false)] // At 1.0 (settled)
    #[case("1.500", false)] // Above maximum
    fn test_validate_price_range(#[case] price_str: &str, #[case] should_pass: bool) {
        let price = Price::from(price_str);
        let result = validate_price_range(&price);

        if should_pass {
            assert!(result.is_ok(), "Price {} should be valid", price_str);
        } else {
            assert!(result.is_err(), "Price {} should be invalid", price_str);
        }
    }

    #[test]
    fn test_get_outcome_price_range() {
        let (min, max) = get_outcome_price_range();
        assert_eq!(min.to_string(), "0.001");
        assert_eq!(max.to_string(), "0.999");
    }
}
