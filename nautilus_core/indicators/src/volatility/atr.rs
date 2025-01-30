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

use crate::{
    average::{MovingAverageFactory, MovingAverageType},
    indicator::{Indicator, MovingAverage},
};

/// An indicator which calculates a Average True Range (ATR) across a rolling window.
#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators", unsendable)
)]
pub struct AverageTrueRange {
    pub period: usize,
    pub ma_type: MovingAverageType,
    pub use_previous: bool,
    pub value_floor: f64,
    pub value: f64,
    pub count: usize,
    pub initialized: bool,
    ma: Box<dyn MovingAverage + Send + 'static>,
    has_inputs: bool,
    previous_close: f64,
}

impl Display for AverageTrueRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}({},{},{},{})",
            self.name(),
            self.period,
            self.ma_type,
            self.use_previous,
            self.value_floor,
        )
    }
}

impl Indicator for AverageTrueRange {
    fn name(&self) -> String {
        stringify!(AverageTrueRange).to_string()
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
        self.previous_close = 0.0;
        self.value = 0.0;
        self.count = 0;
        self.has_inputs = false;
        self.initialized = false;
    }
}

impl AverageTrueRange {
    /// Creates a new [`AverageTrueRange`] instance.
    #[must_use]
    pub fn new(
        period: usize,
        ma_type: Option<MovingAverageType>,
        use_previous: Option<bool>,
        value_floor: Option<f64>,
    ) -> Self {
        Self {
            period,
            ma_type: ma_type.unwrap_or(MovingAverageType::Simple),
            use_previous: use_previous.unwrap_or(true),
            value_floor: value_floor.unwrap_or(0.0),
            value: 0.0,
            count: 0,
            previous_close: 0.0,
            ma: MovingAverageFactory::create(MovingAverageType::Simple, period),
            has_inputs: false,
            initialized: false,
        }
    }

    pub fn update_raw(&mut self, high: f64, low: f64, close: f64) {
        if self.use_previous {
            if !self.has_inputs {
                self.previous_close = close;
            }
            self.ma.update_raw(
                f64::max(self.previous_close, high) - f64::min(low, self.previous_close),
            );
            self.previous_close = close;
        } else {
            self.ma.update_raw(high - low);
        }

        self._floor_value();
        self.increment_count();
    }

    fn _floor_value(&mut self) {
        if self.value_floor == 0.0 || self.value_floor < self.ma.value() {
            self.value = self.ma.value();
        } else {
            // Floor the value
            self.value = self.value_floor;
        }
    }

    const fn increment_count(&mut self) {
        self.count += 1;

        if !self.initialized {
            self.has_inputs = true;
            if self.count >= self.period {
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
    use crate::testing::approx_equal;

    #[rstest]
    fn test_name_returns_expected_string() {
        let atr = AverageTrueRange::new(10, Some(MovingAverageType::Simple), None, None);
        assert_eq!(atr.name(), "AverageTrueRange");
    }

    #[rstest]
    fn test_str_repr_returns_expected_string() {
        let atr = AverageTrueRange::new(10, Some(MovingAverageType::Simple), Some(true), Some(0.0));
        assert_eq!(format!("{atr}"), "AverageTrueRange(10,SIMPLE,true,0)");
    }

    #[rstest]
    fn test_period() {
        let atr = AverageTrueRange::new(10, Some(MovingAverageType::Simple), None, None);
        assert_eq!(atr.period, 10);
    }

    #[rstest]
    fn test_initialized_without_inputs_returns_false() {
        let atr = AverageTrueRange::new(10, Some(MovingAverageType::Simple), None, None);
        assert!(!atr.initialized());
    }

    #[rstest]
    fn test_initialized_with_required_inputs_returns_true() {
        let mut atr = AverageTrueRange::new(10, Some(MovingAverageType::Simple), None, None);
        for _ in 0..10 {
            atr.update_raw(1.0, 1.0, 1.0);
        }
        assert!(atr.initialized());
    }

    #[rstest]
    fn test_value_with_no_inputs_returns_zero() {
        let atr = AverageTrueRange::new(10, Some(MovingAverageType::Simple), None, None);
        assert_eq!(atr.value, 0.0);
    }

    #[rstest]
    fn test_value_with_epsilon_input() {
        let mut atr = AverageTrueRange::new(10, Some(MovingAverageType::Simple), None, None);
        let epsilon = std::f64::EPSILON;
        atr.update_raw(epsilon, epsilon, epsilon);
        assert_eq!(atr.value, 0.0);
    }

    #[rstest]
    fn test_value_with_one_ones_input() {
        let mut atr = AverageTrueRange::new(10, Some(MovingAverageType::Simple), None, None);
        atr.update_raw(1.0, 1.0, 1.0);
        assert_eq!(atr.value, 0.0);
    }

    #[rstest]
    fn test_value_with_one_input() {
        let mut atr = AverageTrueRange::new(10, Some(MovingAverageType::Simple), None, None);
        atr.update_raw(1.00020, 1.0, 1.00010);
        assert!(approx_equal(atr.value, 0.0002));
    }

    #[rstest]
    fn test_value_with_three_inputs() {
        let mut atr = AverageTrueRange::new(10, Some(MovingAverageType::Simple), None, None);
        atr.update_raw(1.00020, 1.0, 1.00010);
        atr.update_raw(1.00020, 1.0, 1.00010);
        atr.update_raw(1.00020, 1.0, 1.00010);
        assert!(approx_equal(atr.value, 0.0002));
    }

    #[rstest]
    fn test_value_with_close_on_high() {
        let mut atr = AverageTrueRange::new(10, Some(MovingAverageType::Simple), None, None);
        let mut high = 1.00010;
        let mut low = 1.0;
        for _ in 0..1000 {
            high += 0.00010;
            low += 0.00010;
            let close = high;
            atr.update_raw(high, low, close);
        }
        assert!(approx_equal(atr.value, 0.000_099_999_999_999_988_99));
    }

    #[rstest]
    fn test_value_with_close_on_low() {
        let mut atr = AverageTrueRange::new(10, Some(MovingAverageType::Simple), None, None);
        let mut high = 1.00010;
        let mut low = 1.0;
        for _ in 0..1000 {
            high -= 0.00010;
            low -= 0.00010;
            let close = low;
            atr.update_raw(high, low, close);
        }
        assert!(approx_equal(atr.value, 0.000_099_999_999_999_988_99));
    }

    #[rstest]
    fn test_floor_with_ten_ones_inputs() {
        let floor = 0.00005;
        let mut floored_atr =
            AverageTrueRange::new(10, Some(MovingAverageType::Simple), None, Some(floor));
        for _ in 0..20 {
            floored_atr.update_raw(1.0, 1.0, 1.0);
        }
        assert_eq!(floored_atr.value, 5e-05);
    }

    #[rstest]
    fn test_floor_with_exponentially_decreasing_high_inputs() {
        let floor = 0.00005;
        let mut floored_atr =
            AverageTrueRange::new(10, Some(MovingAverageType::Simple), None, Some(floor));
        let mut high = 1.00020;
        let low = 1.0;
        let close = 1.0;
        for _ in 0..20 {
            high -= (high - low) / 2.0;
            floored_atr.update_raw(high, low, close);
        }
        assert_eq!(floored_atr.value, floor);
    }

    #[rstest]
    fn test_reset_successfully_returns_indicator_to_fresh_state() {
        let mut atr = AverageTrueRange::new(10, Some(MovingAverageType::Simple), None, None);
        for _ in 0..1000 {
            atr.update_raw(1.00010, 1.0, 1.00005);
        }
        atr.reset();
        assert!(!atr.initialized);
        assert_eq!(atr.value, 0.0);
    }
}
