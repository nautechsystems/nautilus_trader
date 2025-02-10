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

use std::{
    collections::VecDeque,
    fmt::{Debug, Display},
};

use nautilus_model::data::Bar;

use crate::indicator::Indicator;

#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators")
)]
pub struct RateOfChange {
    pub period: usize,
    pub use_log: bool,
    pub value: f64,
    pub initialized: bool,
    has_inputs: bool,
    prices: VecDeque<f64>,
}

impl Display for RateOfChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.name(), self.period)
    }
}

impl Indicator for RateOfChange {
    fn name(&self) -> String {
        stringify!(RateOfChange).to_string()
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
        self.prices.clear();
        self.value = 0.0;
        self.has_inputs = false;
        self.initialized = false;
    }
}

impl RateOfChange {
    /// Creates a new [`RateOfChange`] instance.
    #[must_use]
    pub fn new(period: usize, use_log: Option<bool>) -> Self {
        Self {
            period,
            use_log: use_log.unwrap_or(false),
            value: 0.0,
            prices: VecDeque::with_capacity(period),
            has_inputs: false,
            initialized: false,
        }
    }

    pub fn update_raw(&mut self, price: f64) {
        self.prices.push_back(price);

        if !self.initialized {
            self.has_inputs = true;
            if self.prices.len() >= self.period {
                self.initialized = true;
            }
        }

        if self.use_log {
            // add maths log here
            self.value = (price / self.prices[0]).log10();
        } else {
            self.value = (price - self.prices[0]) / self.prices[0];
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
    use crate::stubs::roc_10;

    #[rstest]
    fn test_name_returns_expected_string(roc_10: RateOfChange) {
        assert_eq!(roc_10.name(), "RateOfChange");
    }

    #[rstest]
    fn test_str_repr_returns_expected_string(roc_10: RateOfChange) {
        assert_eq!(format!("{roc_10}"), "RateOfChange(10)");
    }

    #[rstest]
    fn test_period_returns_expected_value(roc_10: RateOfChange) {
        assert_eq!(roc_10.period, 10);
        assert!(roc_10.use_log);
    }

    #[rstest]
    fn test_initialized_without_inputs_returns_false(roc_10: RateOfChange) {
        assert!(!roc_10.initialized());
    }

    #[rstest]
    fn test_value_with_all_higher_inputs_returns_expected_value(mut roc_10: RateOfChange) {
        let close_values = [
            0.95, 1.95, 2.95, 3.95, 4.95, 5.95, 6.95, 7.95, 8.95, 9.95, 10.05, 10.15, 10.25, 11.05,
            11.45,
        ];

        for close in &close_values {
            roc_10.update_raw(*close);
        }

        assert!(roc_10.initialized());
        assert_eq!(roc_10.value, 1.081_081_881_387_059);
    }

    #[rstest]
    fn test_reset_successfully_returns_indicator_to_fresh_state(mut roc_10: RateOfChange) {
        roc_10.update_raw(1.00020);
        roc_10.update_raw(1.00030);
        roc_10.update_raw(1.00070);

        roc_10.reset();

        assert!(!roc_10.initialized());
        assert!(!roc_10.has_inputs);
        assert_eq!(roc_10.value, 0.0);
    }
}
