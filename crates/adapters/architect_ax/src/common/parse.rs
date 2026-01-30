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

//! Conversion functions that translate AX API schemas into Nautilus types.

pub use nautilus_core::serialization::{
    deserialize_decimal_or_zero, deserialize_optional_decimal_from_str,
    deserialize_optional_decimal_or_zero, deserialize_optional_decimal_str, parse_decimal,
    parse_optional_decimal, serialize_decimal_as_str, serialize_optional_decimal_as_str,
};
use nautilus_model::{
    data::BarSpecification,
    enums::BarAggregation,
    types::{Quantity, fixed::FIXED_PRECISION, quantity::QuantityRaw},
};

use super::enums::AxCandleWidth;

/// Maps a Nautilus [`BarSpecification`] to an [`AxCandleWidth`].
///
/// # Errors
///
/// Returns an error if the bar specification is not supported by Ax.
pub fn map_bar_spec_to_candle_width(spec: &BarSpecification) -> anyhow::Result<AxCandleWidth> {
    match spec.step.get() {
        1 => match spec.aggregation {
            BarAggregation::Second => Ok(AxCandleWidth::Seconds1),
            BarAggregation::Minute => Ok(AxCandleWidth::Minutes1),
            BarAggregation::Hour => Ok(AxCandleWidth::Hours1),
            BarAggregation::Day => Ok(AxCandleWidth::Days1),
            _ => anyhow::bail!("Unsupported bar aggregation: {:?}", spec.aggregation),
        },
        5 => match spec.aggregation {
            BarAggregation::Second => Ok(AxCandleWidth::Seconds5),
            BarAggregation::Minute => Ok(AxCandleWidth::Minutes5),
            _ => anyhow::bail!(
                "Unsupported bar step 5 with aggregation {:?}",
                spec.aggregation
            ),
        },
        15 if spec.aggregation == BarAggregation::Minute => Ok(AxCandleWidth::Minutes15),
        step => anyhow::bail!(
            "Unsupported bar step: {step} with aggregation {:?}",
            spec.aggregation
        ),
    }
}

/// Converts a [`Quantity`] to an i64 contract count for AX orders.
///
/// AX uses integer contracts only. Uses integer arithmetic to avoid
/// floating-point precision issues.
///
/// # Errors
///
/// Returns an error if:
/// - The quantity represents a fractional number of contracts.
/// - The quantity is zero.
pub fn quantity_to_contracts(quantity: Quantity) -> anyhow::Result<u64> {
    let raw = quantity.raw;
    let scale = 10_u64.pow(FIXED_PRECISION as u32) as QuantityRaw;

    // AX requires whole contract quantities
    if !raw.is_multiple_of(scale) {
        anyhow::bail!(
            "AX requires whole contract quantities, was {}",
            quantity.as_f64()
        );
    }

    let contracts = (raw / scale) as u64;
    if contracts == 0 {
        anyhow::bail!("Order quantity must be at least 1 contract");
    }
    Ok(contracts)
}

#[cfg(test)]
mod tests {
    use nautilus_model::{enums::PriceType, types::Quantity};
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_quantity_to_contracts_valid_precision_zero() {
        let qty = Quantity::new(10.0, 0);
        let result = quantity_to_contracts(qty);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 10);
    }

    #[rstest]
    fn test_quantity_to_contracts_valid_with_precision() {
        // Whole number with non-zero precision should work
        let qty = Quantity::new(10.0, 2);
        let result = quantity_to_contracts(qty);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 10);
    }

    #[rstest]
    fn test_quantity_to_contracts_fractional_rejects() {
        let qty = Quantity::new(10.5, 1);
        let result = quantity_to_contracts(qty);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_quantity_to_contracts_zero_rejects() {
        let qty = Quantity::new(0.0, 0);
        let result = quantity_to_contracts(qty);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_map_bar_spec_1_second() {
        let spec = BarSpecification::new(1, BarAggregation::Second, PriceType::Last);
        let result = map_bar_spec_to_candle_width(&spec);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), AxCandleWidth::Seconds1));
    }

    #[rstest]
    fn test_map_bar_spec_5_second() {
        let spec = BarSpecification::new(5, BarAggregation::Second, PriceType::Last);
        let result = map_bar_spec_to_candle_width(&spec);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), AxCandleWidth::Seconds5));
    }

    #[rstest]
    fn test_map_bar_spec_1_minute() {
        let spec = BarSpecification::new(1, BarAggregation::Minute, PriceType::Last);
        let result = map_bar_spec_to_candle_width(&spec);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), AxCandleWidth::Minutes1));
    }

    #[rstest]
    fn test_map_bar_spec_5_minute() {
        let spec = BarSpecification::new(5, BarAggregation::Minute, PriceType::Last);
        let result = map_bar_spec_to_candle_width(&spec);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), AxCandleWidth::Minutes5));
    }

    #[rstest]
    fn test_map_bar_spec_15_minute() {
        let spec = BarSpecification::new(15, BarAggregation::Minute, PriceType::Last);
        let result = map_bar_spec_to_candle_width(&spec);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), AxCandleWidth::Minutes15));
    }

    #[rstest]
    fn test_map_bar_spec_1_hour() {
        let spec = BarSpecification::new(1, BarAggregation::Hour, PriceType::Last);
        let result = map_bar_spec_to_candle_width(&spec);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), AxCandleWidth::Hours1));
    }

    #[rstest]
    fn test_map_bar_spec_1_day() {
        let spec = BarSpecification::new(1, BarAggregation::Day, PriceType::Last);
        let result = map_bar_spec_to_candle_width(&spec);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), AxCandleWidth::Days1));
    }

    #[rstest]
    fn test_map_bar_spec_unsupported_step() {
        let spec = BarSpecification::new(3, BarAggregation::Minute, PriceType::Last);
        let result = map_bar_spec_to_candle_width(&spec);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_map_bar_spec_unsupported_aggregation() {
        let spec = BarSpecification::new(1, BarAggregation::Tick, PriceType::Last);
        let result = map_bar_spec_to_candle_width(&spec);
        assert!(result.is_err());
    }
}
