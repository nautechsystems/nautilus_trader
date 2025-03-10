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
pub struct DonchianChannel {
    pub period: usize,
    pub upper: f64,
    pub middle: f64,
    pub lower: f64,
    pub initialized: bool,
    has_inputs: bool,
    upper_prices: VecDeque<f64>,
    lower_prices: VecDeque<f64>,
}

impl Display for DonchianChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.name(), self.period)
    }
}

impl Indicator for DonchianChannel {
    fn name(&self) -> String {
        stringify!(DonchianChannel).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.has_inputs
    }

    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_bar(&mut self, bar: &Bar) {
        self.update_raw((&bar.high).into(), (&bar.low).into());
    }

    fn reset(&mut self) {
        self.upper_prices.clear();
        self.lower_prices.clear();
        self.upper = 0.0;
        self.middle = 0.0;
        self.lower = 0.0;
        self.has_inputs = false;
        self.initialized = false;
    }
}

impl DonchianChannel {
    /// Creates a new [`DonchianChannel`] instance.
    #[must_use]
    pub fn new(period: usize) -> Self {
        Self {
            period,
            upper: 0.0,
            middle: 0.0,
            lower: 0.0,
            upper_prices: VecDeque::with_capacity(period),
            lower_prices: VecDeque::with_capacity(period),
            has_inputs: false,
            initialized: false,
        }
    }

    pub fn update_raw(&mut self, high: f64, low: f64) {
        self.upper_prices.push_back(high);
        self.lower_prices.push_back(low);

        // Initialization logic
        if !self.initialized {
            self.has_inputs = true;
            if self.upper_prices.len() >= self.period && self.lower_prices.len() >= self.period {
                self.initialized = true;
            }
        }

        // Set values
        self.upper = self
            .upper_prices
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        self.lower = self
            .lower_prices
            .iter()
            .copied()
            .fold(f64::INFINITY, f64::min);
        self.middle = f64::midpoint(self.upper, self.lower);
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_model::data::Bar;
    use rstest::rstest;

    use crate::{
        indicator::Indicator,
        stubs::{bar_ethusdt_binance_minute_bid, dc_10},
        volatility::dc::DonchianChannel,
    };

    #[rstest]
    fn test_psl_initialized(dc_10: DonchianChannel) {
        let display_str = format!("{dc_10}");
        assert_eq!(display_str, "DonchianChannel(10)");
        assert_eq!(dc_10.period, 10);
        assert!(!dc_10.initialized);
        assert!(!dc_10.has_inputs);
    }

    #[rstest]
    fn test_value_with_one_input(mut dc_10: DonchianChannel) {
        dc_10.update_raw(1.0, 0.9);
        assert_eq!(dc_10.upper, 1.0);
        assert_eq!(dc_10.middle, 0.95);
        assert_eq!(dc_10.lower, 0.9);
    }

    #[rstest]
    fn test_value_with_three_inputs(mut dc_10: DonchianChannel) {
        dc_10.update_raw(1.0, 0.9);
        dc_10.update_raw(2.0, 1.8);
        dc_10.update_raw(3.0, 2.7);
        assert_eq!(dc_10.upper, 3.0);
        assert_eq!(dc_10.middle, 1.95);
        assert_eq!(dc_10.lower, 0.9);
    }

    #[rstest]
    fn test_value_with_ten_inputs(mut dc_10: DonchianChannel) {
        let high_values = [
            1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0,
        ];
        let low_values = [
            0.9, 1.9, 2.9, 3.9, 4.9, 5.9, 6.9, 7.9, 8.9, 9.9, 10.1, 10.2, 10.3, 11.1, 11.4,
        ];

        for i in 0..15 {
            dc_10.update_raw(high_values[i], low_values[i]);
        }

        assert_eq!(dc_10.upper, 15.0);
        assert_eq!(dc_10.middle, 7.95);
        assert_eq!(dc_10.lower, 0.9);
    }

    #[rstest]
    fn test_handle_bar(mut dc_10: DonchianChannel, bar_ethusdt_binance_minute_bid: Bar) {
        dc_10.handle_bar(&bar_ethusdt_binance_minute_bid);
        assert_eq!(dc_10.upper, 1550.0);
        assert_eq!(dc_10.middle, 1522.5);
        assert_eq!(dc_10.lower, 1495.0);
        assert!(dc_10.has_inputs);
        assert!(!dc_10.initialized);
    }

    #[rstest]
    fn test_reset(mut dc_10: DonchianChannel) {
        dc_10.update_raw(1.0, 0.9);
        dc_10.reset();
        assert_eq!(dc_10.upper_prices.len(), 0);
        assert_eq!(dc_10.lower_prices.len(), 0);
        assert_eq!(dc_10.upper, 0.0);
        assert_eq!(dc_10.middle, 0.0);
        assert_eq!(dc_10.lower, 0.0);
        assert!(!dc_10.has_inputs);
        assert!(!dc_10.initialized);
    }
}
