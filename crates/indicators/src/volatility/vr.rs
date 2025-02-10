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

use std::fmt::{Debug, Display};

use nautilus_model::data::Bar;

use crate::{average::MovingAverageType, indicator::Indicator, volatility::atr::AverageTrueRange};

#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators", unsendable)
)]
pub struct VolatilityRatio {
    pub fast_period: usize,
    pub slow_period: usize,
    pub ma_type: MovingAverageType,
    pub use_previous: bool,
    pub value_floor: f64,
    pub value: f64,
    pub initialized: bool,
    has_inputs: bool,
    atr_fast: AverageTrueRange,
    atr_slow: AverageTrueRange,
}

impl Display for VolatilityRatio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}({},{},{})",
            self.name(),
            self.fast_period,
            self.slow_period,
            self.ma_type,
        )
    }
}

impl Indicator for VolatilityRatio {
    fn name(&self) -> String {
        stringify!(VolatilityRatio).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.has_inputs
    }

    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_bar(&mut self, bar: &Bar) {
        self.update_raw((&bar.high).into(), (&bar.low).into(), (&bar.close).into());
    }

    fn reset(&mut self) {
        self.atr_fast.reset();
        self.atr_slow.reset();
        self.value = 0.0;
        self.initialized = false;
        self.has_inputs = false;
    }
}

impl VolatilityRatio {
    /// Creates a new [`VolatilityRatio`] instance.
    #[must_use]
    pub fn new(
        fast_period: usize,
        slow_period: usize,
        ma_type: Option<MovingAverageType>,
        use_previous: Option<bool>,
        value_floor: Option<f64>,
    ) -> Self {
        Self {
            fast_period,
            slow_period,
            ma_type: ma_type.unwrap_or(MovingAverageType::Simple),
            use_previous: use_previous.unwrap_or(false),
            value_floor: value_floor.unwrap_or(0.0),
            value: 0.0,
            has_inputs: false,
            initialized: false,
            atr_fast: AverageTrueRange::new(fast_period, ma_type, use_previous, value_floor),
            atr_slow: AverageTrueRange::new(slow_period, ma_type, use_previous, value_floor),
        }
    }

    pub fn update_raw(&mut self, high: f64, low: f64, close: f64) {
        self.atr_fast.update_raw(high, low, close);
        self.atr_slow.update_raw(high, low, close);

        if self.atr_fast.value > 0.0 {
            self.value = self.atr_slow.value / self.atr_fast.value;
        }

        if !self.initialized {
            self.has_inputs = true;

            if self.atr_fast.initialized && self.atr_slow.initialized {
                self.initialized = true;
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
    use crate::stubs::vr_10;

    #[rstest]
    fn test_name_returns_expected_string(vr_10: VolatilityRatio) {
        assert_eq!(vr_10.name(), "VolatilityRatio");
    }

    #[rstest]
    fn test_str_repr_returns_expected_string(vr_10: VolatilityRatio) {
        assert_eq!(format!("{vr_10}"), "VolatilityRatio(10,10,SIMPLE)");
    }

    #[rstest]
    fn test_period_returns_expected_value(vr_10: VolatilityRatio) {
        assert_eq!(vr_10.fast_period, 10);
        assert_eq!(vr_10.slow_period, 10);
        assert!(!vr_10.use_previous);
        assert_eq!(vr_10.value_floor, 10.0);
    }

    #[rstest]
    fn test_initialized_without_inputs_returns_false(vr_10: VolatilityRatio) {
        assert!(!vr_10.initialized());
    }

    #[rstest]
    fn test_value_with_all_higher_inputs_returns_expected_value(mut vr_10: VolatilityRatio) {
        let high_values = [
            1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0,
        ];
        let low_values = [
            0.9, 1.9, 2.9, 3.9, 4.9, 5.9, 6.9, 7.9, 8.9, 9.9, 10.1, 10.2, 10.3, 11.1, 11.4,
        ];
        let close_values = [
            1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0,
        ];

        for i in 0..15 {
            vr_10.update_raw(high_values[i], low_values[i], close_values[i]);
        }

        assert!(vr_10.initialized());
        assert_eq!(vr_10.value, 1.0);
    }

    #[rstest]
    fn test_reset_successfully_returns_indicator_to_fresh_state(mut vr_10: VolatilityRatio) {
        vr_10.update_raw(1.00020, 1.00050, 1.00030);
        vr_10.update_raw(1.00030, 1.00060, 1.00030);
        vr_10.update_raw(1.00070, 1.00080, 1.00030);

        vr_10.reset();

        assert!(!vr_10.initialized());
        assert_eq!(vr_10.value, 0.0);
        assert!(!vr_10.initialized);
        assert!(!vr_10.has_inputs);
        assert_eq!(vr_10.atr_fast.value, 0.0);
        assert_eq!(vr_10.atr_slow.value, 0.0);
    }
}
