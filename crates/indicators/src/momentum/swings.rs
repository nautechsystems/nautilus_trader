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
pub struct Swings {
    pub period: usize,
    pub direction: i64,
    pub changed: bool,
    pub high_datetime: f64,
    pub low_datetime: f64,
    pub high_price: f64,
    pub low_price: f64,
    pub length: usize,
    pub duration: usize,
    pub since_high: usize,
    pub since_low: usize,
    high_inputs: VecDeque<f64>,
    low_inputs: VecDeque<f64>,
    has_inputs: bool,
    initialized: bool,
}

impl Display for Swings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.name(), self.period,)
    }
}

impl Indicator for Swings {
    fn name(&self) -> String {
        stringify!(Swings).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.has_inputs
    }

    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_bar(&mut self, bar: &Bar) {
        self.update_raw((&bar.high).into(), (&bar.low).into(), bar.ts_init.as_f64());
    }

    fn reset(&mut self) {
        self.high_inputs.clear();
        self.low_inputs.clear();
        self.has_inputs = false;
        self.initialized = false;
        self.direction = 0;
        self.changed = false;
        self.high_datetime = 0.0;
        self.low_datetime = 0.0;
        self.high_price = 0.0;
        self.low_price = 0.0;
        self.length = 0;
        self.duration = 0;
        self.since_high = 0;
        self.since_low = 0;
    }
}

impl Swings {
    /// Creates a new [`Swings`] instance.
    #[must_use]
    pub fn new(period: usize) -> Self {
        Self {
            period,
            high_inputs: VecDeque::with_capacity(period + 1),
            low_inputs: VecDeque::with_capacity(period + 1),
            has_inputs: false,
            initialized: false,
            direction: 0,
            changed: false,
            high_datetime: 0.0,
            low_datetime: 0.0,
            high_price: 0.0,
            low_price: 0.0,
            length: 0,
            duration: 0,
            since_high: 0,
            since_low: 0,
        }
    }

    pub fn update_raw(&mut self, high: f64, low: f64, timestamp: f64) {
        // Update inputs
        self.high_inputs.push_back(high);
        self.low_inputs.push_back(low);

        // Update max high and min low
        let max_high = self.high_inputs.iter().fold(f64::MIN, |a, &b| a.max(b));
        let min_low = self.low_inputs.iter().fold(f64::MAX, |a, &b| a.min(b));

        // Calculate if swings
        let is_swing_high = high >= max_high && low >= min_low;
        let is_swing_low = high <= max_high && low <= min_low;

        // Swing logic
        self.changed = true;

        if is_swing_high && !is_swing_low {
            if self.direction == -1 {
                self.changed = true;
            }
            self.high_price = high;
            self.high_datetime = timestamp;
            self.direction = 1;
            self.since_high = 0;
            self.since_low += 1;
        } else if is_swing_low && !is_swing_high {
            if self.direction == 1 {
                self.changed = true;
            }
            self.low_price = low;
            self.low_datetime = timestamp;
            self.direction = -1;
            self.since_high += 1;
            self.since_low = 0;
        } else {
            self.since_high += 1;
            self.since_low += 1;
        }

        // Initialization logic
        if self.initialized {
            self.length = (self.high_price - self.low_price) as usize;
            if self.direction == 1 {
                self.duration = self.since_low;
            } else {
                self.duration = self.since_high;
            }
        } else {
            self.has_inputs = true;
            if self.high_price != 0.0 && self.low_price != 0.0 {
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
    use crate::stubs::swings_10;

    #[rstest]
    fn test_name_returns_expected_string(swings_10: Swings) {
        assert_eq!(swings_10.name(), "Swings");
    }

    #[rstest]
    fn test_str_repr_returns_expected_string(swings_10: Swings) {
        assert_eq!(format!("{swings_10}"), "Swings(10)");
    }

    #[rstest]
    fn test_period_returns_expected_value(swings_10: Swings) {
        assert_eq!(swings_10.period, 10);
    }

    #[rstest]
    fn test_initialized_without_inputs_returns_false(swings_10: Swings) {
        assert!(!swings_10.initialized());
    }

    #[rstest]
    fn test_value_with_all_higher_inputs_returns_expected_value(mut swings_10: Swings) {
        let high = [
            0.9, 1.9, 2.9, 3.9, 4.9, 3.2, 6.9, 7.9, 8.9, 9.9, 1.1, 3.2, 10.3, 11.1, 11.4,
        ];
        let low = [
            0.8, 1.8, 2.8, 3.8, 4.8, 3.1, 6.8, 7.8, 0.8, 9.8, 1.0, 3.1, 10.2, 11.0, 11.3,
        ];
        let time = [
            1_643_723_400.0,
            1_643_723_410.0,
            1_643_723_420.0,
            1_643_723_430.0,
            1_643_723_440.0,
            1_643_723_450.0,
            1_643_723_460.0,
            1_643_723_470.0,
            1_643_723_480.0,
            1_643_723_490.0,
            1_643_723_500.0,
            1_643_723_510.0,
            1_643_723_520.0,
            1_643_723_530.0,
            1_643_723_540.0,
        ];

        for i in 0..15 {
            swings_10.update_raw(high[i], low[i], time[i]);
        }

        assert_eq!(swings_10.direction, 1);
        assert_eq!(swings_10.high_price, 11.4);
        assert_eq!(swings_10.low_price, 0.0);
        assert_eq!(swings_10.high_datetime, time[14]);
        assert_eq!(swings_10.low_datetime, 0.0);
        assert_eq!(swings_10.length, 0);
        assert_eq!(swings_10.duration, 0);
        assert_eq!(swings_10.since_high, 0);
        assert_eq!(swings_10.since_low, 15);
    }

    #[rstest]
    fn test_reset_successfully_returns_indicator_to_fresh_state(mut swings_10: Swings) {
        // Update the indicator with some values
        let high = [1.0, 2.0, 3.0, 4.0, 5.0];
        let low = [0.9, 1.9, 2.9, 3.9, 4.9];
        let time = [
            1_643_723_400.0,
            1_643_723_410.0,
            1_643_723_420.0,
            1_643_723_430.0,
            1_643_723_440.0,
        ];

        for i in 0..5 {
            swings_10.update_raw(high[i], low[i], time[i]);
        }

        swings_10.reset();

        assert!(!swings_10.initialized());
        assert_eq!(swings_10.direction, 0);
        assert_eq!(swings_10.high_price, 0.0);
        assert_eq!(swings_10.low_price, 0.0);
        assert_eq!(swings_10.high_datetime, 0.0);
        assert_eq!(swings_10.low_datetime, 0.0);
        assert_eq!(swings_10.length, 0);
        assert_eq!(swings_10.duration, 0);
        assert_eq!(swings_10.since_high, 0);
        assert_eq!(swings_10.since_low, 0);
        assert!(swings_10.high_inputs.is_empty());
        assert!(swings_10.low_inputs.is_empty());
    }
}
