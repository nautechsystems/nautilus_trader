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
    deserialize_decimal_or_zero, deserialize_optional_decimal,
    deserialize_optional_decimal_from_str, deserialize_optional_decimal_or_zero, parse_decimal,
    parse_optional_decimal,
};
use nautilus_model::{data::BarSpecification, enums::BarAggregation};

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

#[cfg(test)]
mod tests {
    use nautilus_model::enums::PriceType;
    use rstest::rstest;

    use super::*;

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
