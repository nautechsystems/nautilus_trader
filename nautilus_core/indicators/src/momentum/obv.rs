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
pub struct OnBalanceVolume {
    pub period: usize,
    pub value: f64,
    pub initialized: bool,
    has_inputs: bool,
    obv: VecDeque<f64>,
}

impl Display for OnBalanceVolume {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.name(), self.period)
    }
}

impl Indicator for OnBalanceVolume {
    fn name(&self) -> String {
        stringify!(OnBalanceVolume).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.has_inputs
    }

    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_bar(&mut self, bar: &Bar) {
        self.update_raw(
            (&bar.open).into(),
            (&bar.close).into(),
            (&bar.volume).into(),
        );
    }

    fn reset(&mut self) {
        self.obv.clear();
        self.value = 0.0;
        self.has_inputs = false;
        self.initialized = false;
    }
}

impl OnBalanceVolume {
    /// Creates a new [`OnBalanceVolume`] instance.
    #[must_use]
    pub fn new(period: usize) -> Self {
        Self {
            period,
            value: 0.0,
            obv: VecDeque::with_capacity(period),
            has_inputs: false,
            initialized: false,
        }
    }

    pub fn update_raw(&mut self, open: f64, close: f64, volume: f64) {
        if close > open {
            self.obv.push_back(volume);
        } else if close < open {
            self.obv.push_back(-volume);
        } else {
            self.obv.push_back(0.0);
        }

        self.value = self.obv.iter().sum();

        // Initialization logic
        if !self.initialized {
            self.has_inputs = true;
            if self.period == 0 && !self.obv.is_empty() || self.obv.len() >= self.period {
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
    use crate::stubs::obv_10;

    #[rstest]
    fn test_name_returns_expected_string(obv_10: OnBalanceVolume) {
        assert_eq!(obv_10.name(), "OnBalanceVolume");
    }

    #[rstest]
    fn test_str_repr_returns_expected_string(obv_10: OnBalanceVolume) {
        assert_eq!(format!("{obv_10}"), "OnBalanceVolume(10)");
    }

    #[rstest]
    fn test_period_returns_expected_value(obv_10: OnBalanceVolume) {
        assert_eq!(obv_10.period, 10);
    }

    #[rstest]
    fn test_initialized_without_inputs_returns_false(obv_10: OnBalanceVolume) {
        assert!(!obv_10.initialized());
    }

    #[rstest]
    fn test_value_with_all_higher_inputs_returns_expected_value(mut obv_10: OnBalanceVolume) {
        let open_values = [
            104.25, 105.50, 106.75, 108.00, 109.25, 110.50, 111.75, 113.00, 114.25, 115.50, 116.75,
            118.00, 119.25, 120.50, 121.75,
        ];

        let close_values = [
            105.50, 106.75, 108.00, 109.25, 110.50, 111.75, 113.00, 114.25, 115.50, 116.75, 118.00,
            119.25, 120.50, 121.75, 123.00,
        ];

        let volume_values = [
            1000.0, 1200.0, 1500.0, 1800.0, 2000.0, 2200.0, 2500.0, 2800.0, 3000.0, 3200.0, 3500.0,
            3800.0, 4000.0, 4200.0, 4500.0,
        ];
        for i in 0..15 {
            obv_10.update_raw(open_values[i], close_values[i], volume_values[i]);
        }

        assert!(obv_10.initialized());
        assert_eq!(obv_10.value, 41200.0);
    }

    #[rstest]
    fn test_reset_successfully_returns_indicator_to_fresh_state(mut obv_10: OnBalanceVolume) {
        obv_10.update_raw(1.00020, 1.00050, 1000.0);
        obv_10.update_raw(1.00030, 1.00060, 1200.0);
        obv_10.update_raw(1.00070, 1.00080, 1500.0);

        obv_10.reset();

        assert!(!obv_10.initialized());
        assert_eq!(obv_10.value, 0.0);
        assert_eq!(obv_10.obv.len(), 0);
        assert!(!obv_10.has_inputs);
        assert!(!obv_10.initialized);
    }
}
