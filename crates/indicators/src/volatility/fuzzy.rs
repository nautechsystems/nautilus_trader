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
use strum::Display;

use crate::{indicator::Indicator, momentum::bb::fast_std_with_mean};

#[repr(C)]
#[derive(Debug, Display, Clone, PartialEq, Eq, Copy)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.indicators")
)]
pub enum CandleBodySize {
    None = 0,
    Small = 1,
    Medium = 2,
    Large = 3,
    Trend = 4,
}

#[repr(C)]
#[derive(Debug, Display, Clone, PartialEq, Eq, Copy)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.indicators")
)]
pub enum CandleDirection {
    Bull = 1,
    None = 0,
    Bear = -1,
}

#[repr(C)]
#[derive(Debug, Display, Clone, PartialEq, Eq, Copy)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.indicators")
)]
pub enum CandleSize {
    None = 0,
    VerySmall = 1,
    Small = 2,
    Medium = 3,
    Large = 4,
    VeryLarge = 5,
    ExtremelyLarge = 6,
}

#[repr(C)]
#[derive(Debug, Display, Clone, PartialEq, Eq, Copy)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.indicators")
)]
pub enum CandleWickSize {
    None = 0,
    Small = 1,
    Medium = 2,
    Large = 3,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators")
)]
pub struct FuzzyCandle {
    pub direction: CandleDirection,
    pub size: CandleSize,
    pub body_size: CandleBodySize,
    pub upper_wick_size: CandleWickSize,
    pub lower_wick_size: CandleWickSize,
}

impl Display for FuzzyCandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}({},{},{},{})",
            self.direction, self.size, self.body_size, self.lower_wick_size, self.upper_wick_size
        )
    }
}

impl FuzzyCandle {
    /// Creates a new [`FuzzyCandle`] instance.
    #[must_use]
    pub const fn new(
        direction: CandleDirection,
        size: CandleSize,
        body_size: CandleBodySize,
        upper_wick_size: CandleWickSize,
        lower_wick_size: CandleWickSize,
    ) -> Self {
        Self {
            direction,
            size,
            body_size,
            upper_wick_size,
            lower_wick_size,
        }
    }
}

#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators")
)]
pub struct FuzzyCandlesticks {
    pub period: usize,
    pub threshold1: f64,
    pub threshold2: f64,
    pub threshold3: f64,
    pub threshold4: f64,
    pub vector: Vec<i32>,
    pub value: FuzzyCandle,
    pub initialized: bool,
    has_inputs: bool,
    lengths: VecDeque<f64>,
    body_percents: VecDeque<f64>,
    upper_wick_percents: VecDeque<f64>,
    lower_wick_percents: VecDeque<f64>,
    last_open: f64,
    last_high: f64,
    last_low: f64,
    last_close: f64,
}

impl Display for FuzzyCandlesticks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}({},{},{},{},{})",
            self.name(),
            self.period,
            self.threshold1,
            self.threshold2,
            self.threshold3,
            self.threshold4
        )
    }
}

impl Indicator for FuzzyCandlesticks {
    fn name(&self) -> String {
        stringify!(FuzzyCandlesticks).to_string()
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
            (&bar.high).into(),
            (&bar.low).into(),
            (&bar.close).into(),
        );
    }

    fn reset(&mut self) {
        self.lengths.clear();
        self.body_percents.clear();
        self.upper_wick_percents.clear();
        self.lower_wick_percents.clear();
        self.last_open = 0.0;
        self.last_high = 0.0;
        self.last_close = 0.0;
        self.last_low = 0.0;
        self.has_inputs = false;
        self.initialized = false;
    }
}

impl FuzzyCandlesticks {
    /// Creates a new [`FuzzyCandle`] instance.
    /// - Period: usize : The rolling window period for the indicator (> 0).
    /// - Threshold1: f64 : The membership function x threshold1 (>= 0).
    /// - Threshold2: f64 : The membership function x threshold2 (> threshold1).
    /// - Threshold3: f64 : The membership function x threshold3 (> threshold2).
    /// - Threshold4: f64 : The membership function x threshold4 (> threshold3).
    #[must_use]
    pub fn new(
        period: usize,
        threshold1: f64,
        threshold2: f64,
        threshold3: f64,
        threshold4: f64,
    ) -> Self {
        Self {
            period,
            threshold1,
            threshold2,
            threshold3,
            threshold4,
            vector: Vec::new(),
            value: FuzzyCandle::new(
                CandleDirection::None,
                CandleSize::None,
                CandleBodySize::None,
                CandleWickSize::None,
                CandleWickSize::None,
            ),
            has_inputs: false,
            initialized: false,
            lengths: VecDeque::with_capacity(period),
            body_percents: VecDeque::with_capacity(period),
            upper_wick_percents: VecDeque::with_capacity(period),
            lower_wick_percents: VecDeque::with_capacity(period),
            last_open: 0.0,
            last_high: 0.0,
            last_low: 0.0,
            last_close: 0.0,
        }
    }

    pub fn update_raw(&mut self, open: f64, high: f64, low: f64, close: f64) {
        //check if this is the first input
        if !self.has_inputs {
            self.last_close = close;
            self.last_open = open;
            self.last_high = high;
            self.last_low = low;
        }

        // Update last prices
        self.last_close = close;
        self.last_open = open;
        self.last_high = high;
        self.last_low = low;

        // Update measurements
        self.lengths.push_back((high - low).abs());

        if self.lengths[0] == 0.0 {
            self.body_percents.push_back(0.0);
            self.upper_wick_percents.push_back(0.0);
            self.lower_wick_percents.push_back(0.0);
        } else {
            self.body_percents
                .push_back((open - low / self.lengths[0]).abs());
            self.upper_wick_percents
                .push_back(high - f64::max(open, close) / self.lengths[0]);
            self.lower_wick_percents
                .push_back(f64::max(open, close) - low / self.lengths[0]);
        }

        // Calculate statistics for bars
        let mean_length = self.lengths.iter().sum::<f64>() / self.period as f64;
        let mean_body_percent = self.body_percents.iter().sum::<f64>() / self.period as f64;
        let mean_upper_wick_percent =
            self.upper_wick_percents.iter().sum::<f64>() / self.period as f64;
        let mean_lower_wick_percent =
            self.lower_wick_percents.iter().sum::<f64>() / self.period as f64;

        let sd_lengths = fast_std_with_mean(self.lengths.clone(), mean_length);
        let sd_body_percent = fast_std_with_mean(self.body_percents.clone(), mean_body_percent);
        let sd_upper_wick_percent =
            fast_std_with_mean(self.upper_wick_percents.clone(), mean_upper_wick_percent);
        let sd_lower_wick_percent =
            fast_std_with_mean(self.lower_wick_percents.clone(), mean_lower_wick_percent);

        // Create fuzzy candle
        self.value = FuzzyCandle::new(
            self.fuzzify_direction(open, close),
            self.fuzzify_size(self.lengths[0], mean_length, sd_lengths),
            self.fuzzify_body_size(self.body_percents[0], mean_body_percent, sd_body_percent),
            self.fuzzify_wick_size(
                self.upper_wick_percents[0],
                mean_upper_wick_percent,
                sd_upper_wick_percent,
            ),
            self.fuzzify_wick_size(
                self.lower_wick_percents[0],
                mean_lower_wick_percent,
                sd_lower_wick_percent,
            ),
        );

        self.vector = vec![
            self.value.direction as i32,
            self.value.size as i32,
            self.value.body_size as i32,
            self.value.upper_wick_size as i32,
            self.value.lower_wick_size as i32,
        ];
    }

    pub fn reset(&mut self) {
        self.lengths.clear();
        self.body_percents.clear();
        self.upper_wick_percents.clear();
        self.lower_wick_percents.clear();
        self.value = FuzzyCandle::new(
            CandleDirection::None,
            CandleSize::None,
            CandleBodySize::None,
            CandleWickSize::None,
            CandleWickSize::None,
        );
        self.vector = Vec::new();
        self.last_open = 0.0;
        self.last_high = 0.0;
        self.last_close = 0.0;
        self.last_low = 0.0;
        self.has_inputs = false;
        self.initialized = false;
    }

    fn fuzzify_direction(&self, open: f64, close: f64) -> CandleDirection {
        if close > open {
            CandleDirection::Bull
        } else if close < open {
            CandleDirection::Bear
        } else {
            CandleDirection::None
        }
    }

    fn fuzzify_size(&self, length: f64, mean_length: f64, sd_lengths: f64) -> CandleSize {
        // Fuzzify the candle size from the given inputs
        if length == 0.0 {
            return CandleSize::None;
        }

        let mut x;

        // Determine CandleSize fuzzy membership
        // -------------------------------------
        // CandleSize::VerySmall
        x = sd_lengths.mul_add(-self.threshold2, mean_length);
        if length <= x {
            return CandleSize::VerySmall;
        }

        // CandleSize::Small
        x = sd_lengths.mul_add(self.threshold1, mean_length);
        if length <= x {
            return CandleSize::Small;
        }

        // CandleSize::Medium
        x = sd_lengths * self.threshold2;
        if length <= x {
            return CandleSize::Medium;
        }

        // CandleSize.Large
        x = sd_lengths.mul_add(self.threshold3, mean_length);
        if length <= x {
            return CandleSize::Large;
        }

        // CandleSize::VeryLarge
        x = sd_lengths.mul_add(self.threshold4, mean_length);
        if length <= x {
            return CandleSize::VeryLarge;
        }

        CandleSize::ExtremelyLarge
    }

    fn fuzzify_body_size(
        &self,
        body_percent: f64,
        mean_body_percent: f64,
        sd_body_percent: f64,
    ) -> CandleBodySize {
        // Fuzzify the candle body size from the given inputs
        if body_percent == 0.0 {
            return CandleBodySize::None;
        }

        let mut x;

        // Determine CandleBodySize fuzzy membership
        // -------------------------------------
        // CandleBodySize::Small
        x = sd_body_percent.mul_add(-self.threshold1, mean_body_percent);
        if body_percent <= x {
            return CandleBodySize::Small;
        }

        // CandleBodySize::Medium
        x = sd_body_percent.mul_add(self.threshold1, mean_body_percent);
        if body_percent <= x {
            return CandleBodySize::Medium;
        }

        // CandleBodySize::Large
        x = sd_body_percent.mul_add(self.threshold2, mean_body_percent);
        if body_percent <= x {
            return CandleBodySize::Large;
        }

        CandleBodySize::Trend
    }

    fn fuzzify_wick_size(
        &self,
        wick_percent: f64,
        mean_wick_percent: f64,
        sd_wick_percents: f64,
    ) -> CandleWickSize {
        // Fuzzify the candle wick size from the given inputs
        if wick_percent == 0.0 {
            return CandleWickSize::None;
        }

        let mut x;

        // Determine CandleWickSize fuzzy membership
        // -------------------------------------
        // CandleWickSize::Small
        x = sd_wick_percents.mul_add(-self.threshold1, mean_wick_percent);
        if wick_percent <= x {
            return CandleWickSize::Small;
        }

        // CandleWickSize::Medium
        x = sd_wick_percents.mul_add(self.threshold2, mean_wick_percent);
        if wick_percent <= x {
            return CandleWickSize::Medium;
        }

        CandleWickSize::Large
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::{stubs::fuzzy_candlesticks_10, volatility::fuzzy::FuzzyCandlesticks};

    #[rstest]
    fn test_psl_initialized(fuzzy_candlesticks_10: FuzzyCandlesticks) {
        let display_str = format!("{fuzzy_candlesticks_10}");
        assert_eq!(display_str, "FuzzyCandlesticks(10,0.1,0.15,0.2,0.3)");
        assert_eq!(fuzzy_candlesticks_10.period, 10);
        assert!(!fuzzy_candlesticks_10.initialized);
        assert!(!fuzzy_candlesticks_10.has_inputs);
    }

    #[rstest]
    fn test_value_with_one_input(mut fuzzy_candlesticks_10: FuzzyCandlesticks) {
        fuzzy_candlesticks_10.update_raw(123.90, 135.79, 117.09, 125.09);
        assert_eq!(fuzzy_candlesticks_10.value.direction, CandleDirection::Bull);
        assert_eq!(fuzzy_candlesticks_10.value.size, CandleSize::ExtremelyLarge);
        assert_eq!(fuzzy_candlesticks_10.value.body_size, CandleBodySize::Trend);
        assert_eq!(
            fuzzy_candlesticks_10.value.upper_wick_size,
            CandleWickSize::Large
        );
        assert_eq!(
            fuzzy_candlesticks_10.value.lower_wick_size,
            CandleWickSize::Large
        );

        let expected_vec = vec![1, 6, 4, 3, 3];
        assert_eq!(fuzzy_candlesticks_10.vector, expected_vec);
    }

    #[rstest]
    fn test_value_with_three_inputs(mut fuzzy_candlesticks_10: FuzzyCandlesticks) {
        fuzzy_candlesticks_10.update_raw(142.35, 145.82, 141.20, 144.75);
        fuzzy_candlesticks_10.update_raw(144.75, 144.93, 103.55, 108.22);
        fuzzy_candlesticks_10.update_raw(108.22, 120.15, 105.01, 119.89);
        assert_eq!(fuzzy_candlesticks_10.value.direction, CandleDirection::Bull);
        assert_eq!(fuzzy_candlesticks_10.value.size, CandleSize::Small);
        assert_eq!(fuzzy_candlesticks_10.value.body_size, CandleBodySize::Trend);
        assert_eq!(
            fuzzy_candlesticks_10.value.upper_wick_size,
            CandleWickSize::Large
        );
        assert_eq!(
            fuzzy_candlesticks_10.value.lower_wick_size,
            CandleWickSize::Large
        );

        let expected_vec = vec![1, 2, 4, 3, 3];
        assert_eq!(fuzzy_candlesticks_10.vector, expected_vec);
    }

    #[rstest]
    fn test_value_with_ten_inputs(mut fuzzy_candlesticks_10: FuzzyCandlesticks) {
        fuzzy_candlesticks_10.update_raw(150.25, 153.40, 148.10, 152.75);
        fuzzy_candlesticks_10.update_raw(152.80, 155.20, 151.30, 151.95);
        fuzzy_candlesticks_10.update_raw(151.90, 152.85, 147.60, 148.20);
        fuzzy_candlesticks_10.update_raw(148.30, 150.75, 146.90, 150.40);
        fuzzy_candlesticks_10.update_raw(150.50, 154.30, 149.80, 153.90);
        fuzzy_candlesticks_10.update_raw(153.95, 155.80, 152.20, 152.60);
        fuzzy_candlesticks_10.update_raw(152.70, 153.40, 148.50, 149.10);
        fuzzy_candlesticks_10.update_raw(149.20, 151.90, 147.30, 151.50);
        fuzzy_candlesticks_10.update_raw(151.60, 156.40, 151.00, 155.80);
        fuzzy_candlesticks_10.update_raw(155.90, 157.20, 153.70, 154.30);

        assert_eq!(fuzzy_candlesticks_10.value.direction, CandleDirection::Bear);
        assert_eq!(fuzzy_candlesticks_10.value.size, CandleSize::ExtremelyLarge);
        assert_eq!(fuzzy_candlesticks_10.value.body_size, CandleBodySize::Small);
        assert_eq!(
            fuzzy_candlesticks_10.value.upper_wick_size,
            CandleWickSize::Small
        );
        assert_eq!(
            fuzzy_candlesticks_10.value.lower_wick_size,
            CandleWickSize::Medium
        );

        let expected_vec = vec![-1, 6, 1, 1, 2];
        assert_eq!(fuzzy_candlesticks_10.vector, expected_vec);
    }

    #[rstest]
    fn test_reset(mut fuzzy_candlesticks_10: FuzzyCandlesticks) {
        fuzzy_candlesticks_10.update_raw(151.60, 156.40, 151.00, 155.80);
        fuzzy_candlesticks_10.reset();
        assert_eq!(fuzzy_candlesticks_10.lengths.len(), 0);
        assert_eq!(fuzzy_candlesticks_10.body_percents.len(), 0);
        assert_eq!(fuzzy_candlesticks_10.upper_wick_percents.len(), 0);
        assert_eq!(fuzzy_candlesticks_10.lower_wick_percents.len(), 0);
        assert_eq!(fuzzy_candlesticks_10.value.direction, CandleDirection::None);
        assert_eq!(fuzzy_candlesticks_10.value.size, CandleSize::None);
        assert_eq!(fuzzy_candlesticks_10.value.body_size, CandleBodySize::None);
        assert_eq!(
            fuzzy_candlesticks_10.value.upper_wick_size,
            CandleWickSize::None
        );
        assert_eq!(
            fuzzy_candlesticks_10.value.lower_wick_size,
            CandleWickSize::None
        );
        assert_eq!(fuzzy_candlesticks_10.vector.len(), 0);
        assert_eq!(fuzzy_candlesticks_10.last_open, 0.0);
        assert_eq!(fuzzy_candlesticks_10.last_low, 0.0);
        assert_eq!(fuzzy_candlesticks_10.last_high, 0.0);
        assert_eq!(fuzzy_candlesticks_10.last_close, 0.0);
        assert!(!fuzzy_candlesticks_10.has_inputs);
        assert!(!fuzzy_candlesticks_10.initialized);
    }
}
