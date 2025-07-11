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

const MAX_PERIOD: usize = 1024;

#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators", unsendable)
)]
pub struct Bias {
    pub period: usize,
    pub ma_type: MovingAverageType,
    pub value: f64,
    pub count: usize,
    pub initialized: bool,
    ma: Box<dyn MovingAverage + Send + 'static>,
    has_inputs: bool,
}

impl Display for Bias {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({},{})", self.name(), self.period, self.ma_type,)
    }
}

impl Indicator for Bias {
    fn name(&self) -> String {
        stringify!(Bias).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.has_inputs
    }

    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_bar(&mut self, bar: &Bar) {
        self.update_raw((&bar.close).into());
    }

    fn reset(&mut self) {
        self.ma.reset();
        self.value = 0.0;
        self.count = 0;
        self.has_inputs = false;
        self.initialized = false;
    }
}

impl Bias {
    /// Creates a new [`Bias`] instance.
    ///
    /// # Panics
    ///
    /// - If `period` is less than or equal to 0.
    /// - If `period` exceeds `MAX_PERIOD`.
    #[must_use]
    pub fn new(period: usize, ma_type: Option<MovingAverageType>) -> Self {
        assert!(
            period > 0,
            "BollingerBands: period must be > 0 (received {period})"
        );
        assert!(
            period <= MAX_PERIOD,
            "Bias: period {period} exceeds MAX_PERIOD {MAX_PERIOD}"
        );
        Self {
            period,
            ma_type: ma_type.unwrap_or(MovingAverageType::Simple),
            value: 0.0,
            count: 0,
            ma: MovingAverageFactory::create(ma_type.unwrap_or(MovingAverageType::Simple), period),
            has_inputs: false,
            initialized: false,
        }
    }

    pub fn update_raw(&mut self, close: f64) {
        self.count += 1;
        self.ma.update_raw(close);
        self.value = (close / self.ma.value()) - 1.0;
        self._check_initialized();
    }

    pub fn _check_initialized(&mut self) {
        if !self.initialized {
            self.has_inputs = true;
            if self.ma.initialized() {
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
    use rstest::{fixture, rstest};

    use super::*;

    #[fixture]
    fn bias() -> Bias {
        Bias::new(10, None)
    }

    #[rstest]
    fn test_name_returns_expected_string(bias: Bias) {
        assert_eq!(bias.name(), "Bias");
    }

    #[rstest]
    fn test_str_repr_returns_expected_string(bias: Bias) {
        assert_eq!(format!("{bias}"), "Bias(10,SIMPLE)");
    }

    #[rstest]
    fn test_period_returns_expected_value(bias: Bias) {
        assert_eq!(bias.period, 10);
    }

    #[rstest]
    fn test_initialized_without_inputs_returns_false(bias: Bias) {
        assert!(!bias.initialized());
    }

    #[rstest]
    fn test_initialized_with_required_inputs_returns_true(mut bias: Bias) {
        for i in 1..=10 {
            bias.update_raw(f64::from(i));
        }
        assert!(bias.initialized());
    }

    #[rstest]
    fn test_value_with_one_input_returns_expected_value(mut bias: Bias) {
        bias.update_raw(1.0);
        assert_eq!(bias.value, 0.0);
    }

    #[rstest]
    fn test_value_with_all_higher_inputs_returns_expected_value(mut bias: Bias) {
        let inputs = [
            109.93, 110.0, 109.77, 109.96, 110.29, 110.53, 110.27, 110.21, 110.06, 110.19, 109.83,
            109.9, 110.0, 110.03, 110.13, 109.95, 109.75, 110.15, 109.9, 110.04,
        ];
        const EPS: f64 = 1e-12;
        const EXPECTED: f64 = 0.000_654_735_923_177_662_8;
        fn abs_diff_lt(lhs: f64, rhs: f64) -> bool {
            (lhs - rhs).abs() < EPS
        }

        for &price in &inputs {
            bias.update_raw(price);
        }

        assert!(
            abs_diff_lt(bias.value, EXPECTED),
            "bias.value = {:.16} did not match expected value",
            bias.value
        );
    }

    #[rstest]
    fn test_reset_successfully_returns_indicator_to_fresh_state(mut bias: Bias) {
        bias.update_raw(1.00020);
        bias.update_raw(1.00030);
        bias.update_raw(1.00050);

        bias.reset();

        assert!(!bias.initialized());
        assert_eq!(bias.value, 0.0);
    }

    #[rstest]
    fn test_reset_resets_moving_average_state() {
        let mut bias = Bias::new(3, None);
        bias.update_raw(1.0);
        bias.update_raw(2.0);
        bias.update_raw(3.0);
        assert!(bias.ma.initialized());
        bias.reset();
        assert!(!bias.ma.initialized());
        assert_eq!(bias.value, 0.0);
    }

    #[rstest]
    fn test_count_increments_and_resets(mut bias: Bias) {
        assert_eq!(bias.count, 0);
        bias.update_raw(1.0);
        assert_eq!(bias.count, 1);
        bias.update_raw(1.1);
        assert_eq!(bias.count, 2);
        bias.reset();
        assert_eq!(bias.count, 0);
    }
}
