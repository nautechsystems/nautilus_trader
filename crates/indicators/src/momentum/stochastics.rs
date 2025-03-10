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
pub struct Stochastics {
    pub period_k: usize,
    pub period_d: usize,
    pub value_k: f64,
    pub value_d: f64,
    pub initialized: bool,
    has_inputs: bool,
    highs: VecDeque<f64>,
    lows: VecDeque<f64>,
    c_sub_1: VecDeque<f64>,
    h_sub_l: VecDeque<f64>,
}

impl Display for Stochastics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({},{})", self.name(), self.period_k, self.period_d,)
    }
}

impl Indicator for Stochastics {
    fn name(&self) -> String {
        stringify!(Stochastics).to_string()
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
        self.highs.clear();
        self.lows.clear();
        self.c_sub_1.clear();
        self.h_sub_l.clear();
        self.value_k = 0.0;
        self.value_d = 0.0;
        self.has_inputs = false;
        self.initialized = false;
    }
}

impl Stochastics {
    /// Creates a new [`Stochastics`] instance.
    #[must_use]
    pub fn new(period_k: usize, period_d: usize) -> Self {
        Self {
            period_k,
            period_d,
            has_inputs: false,
            initialized: false,
            value_k: 0.0,
            value_d: 0.0,
            highs: VecDeque::with_capacity(period_k),
            lows: VecDeque::with_capacity(period_k),
            h_sub_l: VecDeque::with_capacity(period_d),
            c_sub_1: VecDeque::with_capacity(period_d),
        }
    }

    pub fn update_raw(&mut self, high: f64, low: f64, close: f64) {
        if !self.has_inputs {
            self.has_inputs = true;
        }

        self.highs.push_back(high);
        self.lows.push_back(low);

        // Initialization logic
        if !self.initialized
            && self.highs.len() == self.period_k
            && self.lows.len() == self.period_k
        {
            self.initialized = true;
        }

        let k_max_high = self.highs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let k_min_low = self.lows.iter().copied().fold(f64::INFINITY, f64::min);

        self.c_sub_1.push_back(close - k_min_low);
        self.h_sub_l.push_back(k_max_high - k_min_low);

        if k_max_high == k_min_low {
            return;
        }

        self.value_k = 100.0 * ((close - k_min_low) / (k_max_high - k_min_low));
        self.value_d =
            100.0 * (self.c_sub_1.iter().sum::<f64>() / self.h_sub_l.iter().sum::<f64>());
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
        momentum::stochastics::Stochastics,
        stubs::{bar_ethusdt_binance_minute_bid, stochastics_10},
    };

    #[rstest]
    fn test_stochastics_initialized(stochastics_10: Stochastics) {
        let display_str = format!("{stochastics_10}");
        assert_eq!(display_str, "Stochastics(10,10)");
        assert_eq!(stochastics_10.period_d, 10);
        assert_eq!(stochastics_10.period_k, 10);
        assert!(!stochastics_10.initialized);
        assert!(!stochastics_10.has_inputs);
    }

    #[rstest]
    fn test_value_with_one_input(mut stochastics_10: Stochastics) {
        stochastics_10.update_raw(1.0, 1.0, 1.0);
        assert_eq!(stochastics_10.value_d, 0.0);
        assert_eq!(stochastics_10.value_k, 0.0);
    }

    #[rstest]
    fn test_value_with_three_inputs(mut stochastics_10: Stochastics) {
        stochastics_10.update_raw(1.0, 1.0, 1.0);
        stochastics_10.update_raw(2.0, 2.0, 2.0);
        stochastics_10.update_raw(3.0, 3.0, 3.0);
        assert_eq!(stochastics_10.value_d, 100.0);
        assert_eq!(stochastics_10.value_k, 100.0);
    }

    #[rstest]
    fn test_value_with_ten_inputs(mut stochastics_10: Stochastics) {
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
            stochastics_10.update_raw(high_values[i], low_values[i], close_values[i]);
        }

        assert!(stochastics_10.initialized());
        assert_eq!(stochastics_10.value_d, 100.0);
        assert_eq!(stochastics_10.value_k, 100.0);
    }

    #[rstest]
    fn test_initialized_with_required_input(mut stochastics_10: Stochastics) {
        for i in 1..10 {
            stochastics_10.update_raw(f64::from(i), f64::from(i), f64::from(i));
        }
        assert!(!stochastics_10.initialized);
        stochastics_10.update_raw(10.0, 12.0, 14.0);
        assert!(stochastics_10.initialized);
    }

    #[rstest]
    fn test_handle_bar(mut stochastics_10: Stochastics, bar_ethusdt_binance_minute_bid: Bar) {
        stochastics_10.handle_bar(&bar_ethusdt_binance_minute_bid);
        assert_eq!(stochastics_10.value_d, 49.090_909_090_909_09);
        assert_eq!(stochastics_10.value_k, 49.090_909_090_909_09);
        assert!(stochastics_10.has_inputs);
        assert!(!stochastics_10.initialized);
    }

    #[rstest]
    fn test_reset(mut stochastics_10: Stochastics) {
        stochastics_10.update_raw(1.0, 1.0, 1.0);
        assert_eq!(stochastics_10.c_sub_1.len(), 1);
        assert_eq!(stochastics_10.h_sub_l.len(), 1);

        stochastics_10.reset();
        assert_eq!(stochastics_10.value_d, 0.0);
        assert_eq!(stochastics_10.value_k, 0.0);
        assert_eq!(stochastics_10.h_sub_l.len(), 0);
        assert_eq!(stochastics_10.c_sub_1.len(), 0);
        assert!(!stochastics_10.has_inputs);
        assert!(!stochastics_10.initialized);
    }
}
