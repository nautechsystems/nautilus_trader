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

//! Value conversions between Nautilus domain types and Binance Futures venue types.

use nautilus_model::enums::OrderSide;
use rust_decimal::Decimal;

use crate::common::enums::BinancePositionSide;

/// Determines the Binance `positionSide` for hedge mode from the Nautilus order side.
///
/// Returns `None` when not in hedge mode (one-way mode orders omit `positionSide`).
/// In hedge mode, `reduce_only` flips the mapping so that Buy closes Short and
/// Sell closes Long.
#[must_use]
pub(crate) fn determine_position_side(
    is_hedge_mode: bool,
    order_side: OrderSide,
    reduce_only: bool,
) -> Option<BinancePositionSide> {
    if !is_hedge_mode {
        return None;
    }

    Some(if reduce_only {
        match order_side {
            OrderSide::Buy => BinancePositionSide::Short,
            OrderSide::Sell => BinancePositionSide::Long,
            _ => BinancePositionSide::Both,
        }
    } else {
        match order_side {
            OrderSide::Buy => BinancePositionSide::Long,
            OrderSide::Sell => BinancePositionSide::Short,
            _ => BinancePositionSide::Both,
        }
    })
}

/// Converts a Nautilus trailing offset (percent) into a Binance `callbackRate` decimal.
///
/// # Errors
///
/// Returns an error if the computed rate is outside the Binance accepted range
/// `[0.1%, 10.0%]`.
pub(crate) fn trailing_offset_to_callback_rate(offset: Decimal) -> anyhow::Result<Decimal> {
    let rate = offset / rust_decimal::Decimal::ONE_HUNDRED;
    let min_rate = rust_decimal::Decimal::new(1, 1);
    let max_rate = rust_decimal::Decimal::new(100, 1);

    if rate < min_rate || rate > max_rate {
        anyhow::bail!("callbackRate {rate}% out of Binance range [{min_rate}, {max_rate}]");
    }

    Ok(rate)
}

/// Converts a Nautilus trailing offset (percent) into a Binance `callbackRate` string.
///
/// # Errors
///
/// Returns an error if the computed rate is outside the Binance accepted range.
pub(crate) fn trailing_offset_to_callback_rate_string(offset: Decimal) -> anyhow::Result<String> {
    let rate = trailing_offset_to_callback_rate(offset)?;
    Ok(format_callback_rate(rate))
}

/// Formats a `callbackRate` decimal for Binance request params.
///
/// Whole percents are rendered with a trailing `.0` to match Binance examples.
#[must_use]
pub(crate) fn format_callback_rate(rate: Decimal) -> String {
    let normalized = rate.normalize();

    if normalized.scale() == 0 {
        format!("{normalized}.0")
    } else {
        normalized.to_string()
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_trailing_offset_to_callback_rate_preserves_precision() {
        let rate = trailing_offset_to_callback_rate(Decimal::from(25)).unwrap();
        assert_eq!(rate, Decimal::new(25, 2));
    }

    #[rstest]
    fn test_trailing_offset_to_callback_rate_string_formats_whole_percent() {
        let rate = trailing_offset_to_callback_rate_string(Decimal::from(100)).unwrap();
        assert_eq!(rate, "1.0");
    }

    #[rstest]
    fn test_trailing_offset_to_callback_rate_rejects_out_of_range_values() {
        let error = trailing_offset_to_callback_rate(Decimal::from(5)).unwrap_err();
        assert_eq!(
            error.to_string(),
            "callbackRate 0.05% out of Binance range [0.1, 10.0]"
        );
    }

    #[rstest]
    #[case::one_way_buy(false, OrderSide::Buy, false, None)]
    #[case::one_way_sell(false, OrderSide::Sell, false, None)]
    #[case::one_way_buy_reduce(false, OrderSide::Buy, true, None)]
    #[case::hedge_open_buy(true, OrderSide::Buy, false, Some(BinancePositionSide::Long))]
    #[case::hedge_open_sell(true, OrderSide::Sell, false, Some(BinancePositionSide::Short))]
    #[case::hedge_close_buy(true, OrderSide::Buy, true, Some(BinancePositionSide::Short))]
    #[case::hedge_close_sell(true, OrderSide::Sell, true, Some(BinancePositionSide::Long))]
    #[case::hedge_no_side(true, OrderSide::NoOrderSide, false, Some(BinancePositionSide::Both))]
    fn test_determine_position_side(
        #[case] is_hedge_mode: bool,
        #[case] order_side: OrderSide,
        #[case] reduce_only: bool,
        #[case] expected: Option<BinancePositionSide>,
    ) {
        assert_eq!(
            determine_position_side(is_hedge_mode, order_side, reduce_only),
            expected,
        );
    }
}
