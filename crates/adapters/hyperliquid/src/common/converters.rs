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

//! Order type conversion utilities for Hyperliquid adapter.
//!
//! This module provides conversion functions between Nautilus core order types
//! and Hyperliquid-specific order type representations.

use nautilus_model::enums::{OrderType, TimeInForce};
use rust_decimal::Decimal;

use super::enums::{
    HyperliquidConditionalOrderType, HyperliquidOrderType, HyperliquidTimeInForce, HyperliquidTpSl,
};

/// Converts a Nautilus `OrderType` to a Hyperliquid order type configuration.
///
/// # Arguments
///
/// * `order_type` - The Nautilus order type to convert
/// * `time_in_force` - The time in force for limit orders
/// * `trigger_price` - Optional trigger price for conditional orders
///
/// # Returns
///
/// A `HyperliquidOrderType` variant configured for the specified order type.
///
/// # Panics
///
/// Panics if a conditional order is specified without a trigger price.
pub fn nautilus_order_type_to_hyperliquid(
    order_type: OrderType,
    time_in_force: Option<TimeInForce>,
    trigger_price: Option<Decimal>,
) -> HyperliquidOrderType {
    match order_type {
        // Regular limit order
        OrderType::Limit => {
            let tif = time_in_force.map_or(
                HyperliquidTimeInForce::Gtc,
                nautilus_time_in_force_to_hyperliquid,
            );
            HyperliquidOrderType::Limit { tif }
        }

        // Stop market order (stop loss)
        OrderType::StopMarket => {
            let trigger_px = trigger_price
                .expect("Trigger price required for StopMarket order")
                .to_string();
            HyperliquidOrderType::Trigger {
                is_market: true,
                trigger_px,
                tpsl: HyperliquidTpSl::Sl,
            }
        }

        // Stop limit order (stop loss with limit)
        OrderType::StopLimit => {
            let trigger_px = trigger_price
                .expect("Trigger price required for StopLimit order")
                .to_string();
            HyperliquidOrderType::Trigger {
                is_market: false,
                trigger_px,
                tpsl: HyperliquidTpSl::Sl,
            }
        }

        // Market if touched (take profit market)
        OrderType::MarketIfTouched => {
            let trigger_px = trigger_price
                .expect("Trigger price required for MarketIfTouched order")
                .to_string();
            HyperliquidOrderType::Trigger {
                is_market: true,
                trigger_px,
                tpsl: HyperliquidTpSl::Tp,
            }
        }

        // Limit if touched (take profit limit)
        OrderType::LimitIfTouched => {
            let trigger_px = trigger_price
                .expect("Trigger price required for LimitIfTouched order")
                .to_string();
            HyperliquidOrderType::Trigger {
                is_market: false,
                trigger_px,
                tpsl: HyperliquidTpSl::Tp,
            }
        }

        // Trailing stop market (requires special handling)
        OrderType::TrailingStopMarket => {
            let trigger_px = trigger_price
                .expect("Trigger price required for TrailingStopMarket order")
                .to_string();
            HyperliquidOrderType::Trigger {
                is_market: true,
                trigger_px,
                tpsl: HyperliquidTpSl::Sl,
            }
        }

        // Trailing stop limit (requires special handling)
        OrderType::TrailingStopLimit => {
            let trigger_px = trigger_price
                .expect("Trigger price required for TrailingStopLimit order")
                .to_string();
            HyperliquidOrderType::Trigger {
                is_market: false,
                trigger_px,
                tpsl: HyperliquidTpSl::Sl,
            }
        }

        // Market orders are handled elsewhere (not represented in HyperliquidOrderType)
        OrderType::Market => {
            panic!("Market orders should be handled separately via immediate execution")
        }

        // Unsupported order types
        _ => panic!("Unsupported order type: {order_type:?}"),
    }
}

/// Converts a Hyperliquid order type to a Nautilus `OrderType`.
///
/// # Arguments
///
/// * `hl_order_type` - The Hyperliquid order type to convert
///
/// # Returns
///
/// The corresponding Nautilus `OrderType`.
pub fn hyperliquid_order_type_to_nautilus(hl_order_type: &HyperliquidOrderType) -> OrderType {
    match hl_order_type {
        HyperliquidOrderType::Limit { .. } => OrderType::Limit,
        HyperliquidOrderType::Trigger {
            is_market, tpsl, ..
        } => match (is_market, tpsl) {
            (true, HyperliquidTpSl::Sl) => OrderType::StopMarket,
            (false, HyperliquidTpSl::Sl) => OrderType::StopLimit,
            (true, HyperliquidTpSl::Tp) => OrderType::MarketIfTouched,
            (false, HyperliquidTpSl::Tp) => OrderType::LimitIfTouched,
        },
    }
}

/// Converts a Hyperliquid conditional order type to a Nautilus `OrderType`.
///
/// # Arguments
///
/// * `conditional_type` - The Hyperliquid conditional order type
///
/// # Returns
///
/// The corresponding Nautilus `OrderType`.
pub fn hyperliquid_conditional_to_nautilus(
    conditional_type: HyperliquidConditionalOrderType,
) -> OrderType {
    OrderType::from(conditional_type)
}

/// Converts a Nautilus `OrderType` to a Hyperliquid conditional order type.
///
/// # Arguments
///
/// * `order_type` - The Nautilus order type
///
/// # Returns
///
/// The corresponding Hyperliquid conditional order type.
///
/// # Panics
///
/// Panics if the order type is not a conditional order type.
pub fn nautilus_to_hyperliquid_conditional(
    order_type: OrderType,
) -> HyperliquidConditionalOrderType {
    HyperliquidConditionalOrderType::from(order_type)
}

/// Converts a Nautilus `TimeInForce` to a Hyperliquid time in force.
///
/// # Arguments
///
/// * `tif` - The Nautilus time in force
///
/// # Returns
///
/// The corresponding Hyperliquid time in force.
pub fn nautilus_time_in_force_to_hyperliquid(tif: TimeInForce) -> HyperliquidTimeInForce {
    match tif {
        TimeInForce::Gtc => HyperliquidTimeInForce::Gtc,
        TimeInForce::Ioc => HyperliquidTimeInForce::Ioc,
        TimeInForce::Fok => HyperliquidTimeInForce::Ioc, // FOK maps to IOC in Hyperliquid
        TimeInForce::Gtd => HyperliquidTimeInForce::Gtc, // GTD maps to GTC
        TimeInForce::Day => HyperliquidTimeInForce::Gtc, // DAY maps to GTC
        TimeInForce::AtTheOpen => HyperliquidTimeInForce::Gtc, // ATO maps to GTC
        TimeInForce::AtTheClose => HyperliquidTimeInForce::Gtc, // ATC maps to GTC
    }
}

/// Converts a Hyperliquid time in force to a Nautilus `TimeInForce`.
///
/// # Arguments
///
/// * `hl_tif` - The Hyperliquid time in force
///
/// # Returns
///
/// The corresponding Nautilus time in force.
pub fn hyperliquid_time_in_force_to_nautilus(hl_tif: HyperliquidTimeInForce) -> TimeInForce {
    match hl_tif {
        HyperliquidTimeInForce::Gtc => TimeInForce::Gtc,
        HyperliquidTimeInForce::Ioc => TimeInForce::Ioc,
        HyperliquidTimeInForce::Alo => TimeInForce::Gtc, // ALO (post-only) maps to GTC
    }
}

/// Determines the TP/SL type based on order type and side.
///
/// # Arguments
///
/// * `order_type` - The Nautilus order type
/// * `is_buy` - Whether this is a buy order
///
/// # Returns
///
/// The appropriate `HyperliquidTpSl` type.
///
/// # Logic
///
/// For buy orders:
/// - Stop orders (trigger below current price) -> Stop Loss
/// - Take profit orders (trigger above current price) -> Take Profit
///
/// For sell orders:
/// - Stop orders (trigger above current price) -> Stop Loss
/// - Take profit orders (trigger below current price) -> Take Profit
pub fn determine_tpsl_type(order_type: OrderType, is_buy: bool) -> HyperliquidTpSl {
    match order_type {
        OrderType::StopMarket
        | OrderType::StopLimit
        | OrderType::TrailingStopMarket
        | OrderType::TrailingStopLimit => HyperliquidTpSl::Sl,
        OrderType::MarketIfTouched | OrderType::LimitIfTouched => HyperliquidTpSl::Tp,
        _ => {
            // Default logic based on side if order type is ambiguous
            if is_buy {
                HyperliquidTpSl::Sl
            } else {
                HyperliquidTpSl::Tp
            }
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_nautilus_to_hyperliquid_limit_order() {
        let result =
            nautilus_order_type_to_hyperliquid(OrderType::Limit, Some(TimeInForce::Gtc), None);

        match result {
            HyperliquidOrderType::Limit { tif } => {
                assert_eq!(tif, HyperliquidTimeInForce::Gtc);
            }
            _ => panic!("Expected Limit order type"),
        }
    }

    #[rstest]
    fn test_nautilus_to_hyperliquid_stop_market() {
        let result = nautilus_order_type_to_hyperliquid(
            OrderType::StopMarket,
            None,
            Some(Decimal::new(49000, 0)),
        );

        match result {
            HyperliquidOrderType::Trigger {
                is_market,
                trigger_px,
                tpsl,
            } => {
                assert!(is_market);
                assert_eq!(trigger_px, "49000");
                assert_eq!(tpsl, HyperliquidTpSl::Sl);
            }
            _ => panic!("Expected Trigger order type"),
        }
    }

    #[rstest]
    fn test_nautilus_to_hyperliquid_stop_limit() {
        let result = nautilus_order_type_to_hyperliquid(
            OrderType::StopLimit,
            None,
            Some(Decimal::new(49000, 0)),
        );

        match result {
            HyperliquidOrderType::Trigger {
                is_market,
                trigger_px,
                tpsl,
            } => {
                assert!(!is_market);
                assert_eq!(trigger_px, "49000");
                assert_eq!(tpsl, HyperliquidTpSl::Sl);
            }
            _ => panic!("Expected Trigger order type"),
        }
    }

    #[rstest]
    fn test_nautilus_to_hyperliquid_take_profit_market() {
        let result = nautilus_order_type_to_hyperliquid(
            OrderType::MarketIfTouched,
            None,
            Some(Decimal::new(51000, 0)),
        );

        match result {
            HyperliquidOrderType::Trigger {
                is_market,
                trigger_px,
                tpsl,
            } => {
                assert!(is_market);
                assert_eq!(trigger_px, "51000");
                assert_eq!(tpsl, HyperliquidTpSl::Tp);
            }
            _ => panic!("Expected Trigger order type"),
        }
    }

    #[rstest]
    fn test_nautilus_to_hyperliquid_take_profit_limit() {
        let result = nautilus_order_type_to_hyperliquid(
            OrderType::LimitIfTouched,
            None,
            Some(Decimal::new(51000, 0)),
        );

        match result {
            HyperliquidOrderType::Trigger {
                is_market,
                trigger_px,
                tpsl,
            } => {
                assert!(!is_market);
                assert_eq!(trigger_px, "51000");
                assert_eq!(tpsl, HyperliquidTpSl::Tp);
            }
            _ => panic!("Expected Trigger order type"),
        }
    }

    #[rstest]
    fn test_hyperliquid_to_nautilus_limit() {
        let hl_order = HyperliquidOrderType::Limit {
            tif: HyperliquidTimeInForce::Gtc,
        };
        assert_eq!(
            hyperliquid_order_type_to_nautilus(&hl_order),
            OrderType::Limit
        );
    }

    #[rstest]
    fn test_hyperliquid_to_nautilus_stop_market() {
        let hl_order = HyperliquidOrderType::Trigger {
            is_market: true,
            trigger_px: "49000".to_string(),
            tpsl: HyperliquidTpSl::Sl,
        };
        assert_eq!(
            hyperliquid_order_type_to_nautilus(&hl_order),
            OrderType::StopMarket
        );
    }

    #[rstest]
    fn test_hyperliquid_to_nautilus_stop_limit() {
        let hl_order = HyperliquidOrderType::Trigger {
            is_market: false,
            trigger_px: "49000".to_string(),
            tpsl: HyperliquidTpSl::Sl,
        };
        assert_eq!(
            hyperliquid_order_type_to_nautilus(&hl_order),
            OrderType::StopLimit
        );
    }

    #[rstest]
    fn test_hyperliquid_to_nautilus_take_profit_market() {
        let hl_order = HyperliquidOrderType::Trigger {
            is_market: true,
            trigger_px: "51000".to_string(),
            tpsl: HyperliquidTpSl::Tp,
        };
        assert_eq!(
            hyperliquid_order_type_to_nautilus(&hl_order),
            OrderType::MarketIfTouched
        );
    }

    #[rstest]
    fn test_hyperliquid_to_nautilus_take_profit_limit() {
        let hl_order = HyperliquidOrderType::Trigger {
            is_market: false,
            trigger_px: "51000".to_string(),
            tpsl: HyperliquidTpSl::Tp,
        };
        assert_eq!(
            hyperliquid_order_type_to_nautilus(&hl_order),
            OrderType::LimitIfTouched
        );
    }

    #[rstest]
    fn test_time_in_force_conversions() {
        // Test Nautilus to Hyperliquid
        assert_eq!(
            nautilus_time_in_force_to_hyperliquid(TimeInForce::Gtc),
            HyperliquidTimeInForce::Gtc
        );
        assert_eq!(
            nautilus_time_in_force_to_hyperliquid(TimeInForce::Ioc),
            HyperliquidTimeInForce::Ioc
        );
        assert_eq!(
            nautilus_time_in_force_to_hyperliquid(TimeInForce::Fok),
            HyperliquidTimeInForce::Ioc
        );

        // Test Hyperliquid to Nautilus
        assert_eq!(
            hyperliquid_time_in_force_to_nautilus(HyperliquidTimeInForce::Gtc),
            TimeInForce::Gtc
        );
        assert_eq!(
            hyperliquid_time_in_force_to_nautilus(HyperliquidTimeInForce::Ioc),
            TimeInForce::Ioc
        );
        assert_eq!(
            hyperliquid_time_in_force_to_nautilus(HyperliquidTimeInForce::Alo),
            TimeInForce::Gtc
        );
    }

    #[rstest]
    fn test_conditional_order_type_conversions() {
        // Test Hyperliquid conditional to Nautilus
        assert_eq!(
            hyperliquid_conditional_to_nautilus(HyperliquidConditionalOrderType::StopMarket),
            OrderType::StopMarket
        );
        assert_eq!(
            hyperliquid_conditional_to_nautilus(HyperliquidConditionalOrderType::StopLimit),
            OrderType::StopLimit
        );
        assert_eq!(
            hyperliquid_conditional_to_nautilus(HyperliquidConditionalOrderType::TakeProfitMarket),
            OrderType::MarketIfTouched
        );
        assert_eq!(
            hyperliquid_conditional_to_nautilus(HyperliquidConditionalOrderType::TakeProfitLimit),
            OrderType::LimitIfTouched
        );

        // Test Nautilus to Hyperliquid conditional
        assert_eq!(
            nautilus_to_hyperliquid_conditional(OrderType::StopMarket),
            HyperliquidConditionalOrderType::StopMarket
        );
        assert_eq!(
            nautilus_to_hyperliquid_conditional(OrderType::StopLimit),
            HyperliquidConditionalOrderType::StopLimit
        );
        assert_eq!(
            nautilus_to_hyperliquid_conditional(OrderType::MarketIfTouched),
            HyperliquidConditionalOrderType::TakeProfitMarket
        );
        assert_eq!(
            nautilus_to_hyperliquid_conditional(OrderType::LimitIfTouched),
            HyperliquidConditionalOrderType::TakeProfitLimit
        );
    }

    #[rstest]
    fn test_determine_tpsl_type() {
        // Stop orders should always be SL
        assert_eq!(
            determine_tpsl_type(OrderType::StopMarket, true),
            HyperliquidTpSl::Sl
        );
        assert_eq!(
            determine_tpsl_type(OrderType::StopLimit, false),
            HyperliquidTpSl::Sl
        );

        // Take profit orders should always be TP
        assert_eq!(
            determine_tpsl_type(OrderType::MarketIfTouched, true),
            HyperliquidTpSl::Tp
        );
        assert_eq!(
            determine_tpsl_type(OrderType::LimitIfTouched, false),
            HyperliquidTpSl::Tp
        );

        // Trailing stops should be SL
        assert_eq!(
            determine_tpsl_type(OrderType::TrailingStopMarket, true),
            HyperliquidTpSl::Sl
        );
        assert_eq!(
            determine_tpsl_type(OrderType::TrailingStopLimit, false),
            HyperliquidTpSl::Sl
        );
    }
}
