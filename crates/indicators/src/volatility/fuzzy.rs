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

use arraydeque::{ArrayDeque, Wrapping};
use nautilus_model::data::Bar;
use strum::Display;

use crate::indicator::Indicator;

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

const MAX_CAPACITY: usize = 1024;

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
    lengths: ArrayDeque<f64, MAX_CAPACITY, Wrapping>,
    body_percents: ArrayDeque<f64, MAX_CAPACITY, Wrapping>,
    upper_wick_percents: ArrayDeque<f64, MAX_CAPACITY, Wrapping>,
    lower_wick_percents: ArrayDeque<f64, MAX_CAPACITY, Wrapping>,
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
    ///
    /// # Panics
    ///
    /// This function panics if:
    /// - `period` is greater than `MAX_CAPACITY`.
    /// - Period: usize : The rolling window period for the indicator (> 0).
    /// - Threshold1: f64 : The membership function x threshold1 (> 0).
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
        assert!(period <= MAX_CAPACITY);
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
            lengths: ArrayDeque::new(),
            body_percents: ArrayDeque::new(),
            upper_wick_percents: ArrayDeque::new(),
            lower_wick_percents: ArrayDeque::new(),
            last_open: 0.0,
            last_high: 0.0,
            last_low: 0.0,
            last_close: 0.0,
        }
    }

    pub fn update_raw(&mut self, open: f64, high: f64, low: f64, close: f64) {
        if !self.has_inputs {
            self.last_close = close;
            self.last_open = open;
            self.last_high = high;
            self.last_low = low;
            self.has_inputs = true;
        }

        self.last_close = close;
        self.last_open = open;
        self.last_high = high;
        self.last_low = low;

        let total = (high - low).abs();
        let _ = self.lengths.push_back(total);

        if total == 0.0 {
            let _ = self.body_percents.push_back(0.0);
            let _ = self.upper_wick_percents.push_back(0.0);
            let _ = self.lower_wick_percents.push_back(0.0);
        } else {
            let body = (close - open).abs();
            let upper_wick = high - f64::max(open, close);
            let lower_wick = f64::min(open, close) - low;

            let _ = self.body_percents.push_back(body / total);
            let _ = self.upper_wick_percents.push_back(upper_wick / total);
            let _ = self.lower_wick_percents.push_back(lower_wick / total);
        }

        if self.lengths.len() >= self.period {
            self.initialized = true;
        }

        // not enough data to compute stddev, will div self.period later
        if !self.initialized {
            return;
        }

        let mean_length = self.lengths.iter().sum::<f64>() / (self.period as f64);
        let mean_body_percent = self.body_percents.iter().sum::<f64>() / (self.period as f64);
        let mean_upper_percent =
            self.upper_wick_percents.iter().sum::<f64>() / (self.period as f64);
        let mean_lower_percent =
            self.lower_wick_percents.iter().sum::<f64>() / (self.period as f64);

        let sd_length = Self::std_dev(&self.lengths, mean_length);
        let sd_body = Self::std_dev(&self.body_percents, mean_body_percent);
        let sd_upper = Self::std_dev(&self.upper_wick_percents, mean_upper_percent);
        let sd_lower = Self::std_dev(&self.lower_wick_percents, mean_lower_percent);
        let latest_body = *self.body_percents.back().unwrap_or(&0.0);
        let latest_upper = *self.upper_wick_percents.back().unwrap_or(&0.0);
        let latest_lower = *self.lower_wick_percents.back().unwrap_or(&0.0);

        self.value = FuzzyCandle::new(
            self.fuzzify_direction(open, close),
            self.fuzzify_size(total, mean_length, sd_length),
            self.fuzzify_body_size(latest_body, mean_body_percent, sd_body),
            self.fuzzify_wick_size(latest_upper, mean_upper_percent, sd_upper),
            self.fuzzify_wick_size(latest_lower, mean_lower_percent, sd_lower),
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
        if !length.is_finite() || length == 0.0 {
            return CandleSize::None;
        }

        let thresholds = [
            mean_length - self.threshold2 * sd_lengths, // VerySmall
            mean_length - self.threshold1 * sd_lengths, // Small
            mean_length + self.threshold1 * sd_lengths, // Medium
            mean_length + self.threshold2 * sd_lengths, // Large
            mean_length + self.threshold3 * sd_lengths, // VeryLarge
        ];
        if length <= thresholds[0] {
            CandleSize::VerySmall
        } else if length <= thresholds[1] {
            CandleSize::Small
        } else if length <= thresholds[2] {
            CandleSize::Medium
        } else if length <= thresholds[3] {
            CandleSize::Large
        } else if length <= thresholds[4] {
            CandleSize::VeryLarge
        } else {
            CandleSize::ExtremelyLarge
        }
    }

    fn fuzzify_body_size(
        &self,
        body_percent: f64,
        mean_body_percent: f64,
        sd_body_percent: f64,
    ) -> CandleBodySize {
        if body_percent == 0.0 {
            return CandleBodySize::None;
        }

        let mut x;

        x = sd_body_percent.mul_add(-self.threshold1, mean_body_percent);
        if body_percent <= x {
            return CandleBodySize::Small;
        }

        x = sd_body_percent.mul_add(self.threshold1, mean_body_percent);
        if body_percent <= x {
            return CandleBodySize::Medium;
        }

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
        if wick_percent == 0.0 {
            return CandleWickSize::None;
        }

        let mut x;
        x = sd_wick_percents.mul_add(-self.threshold1, mean_wick_percent);
        if wick_percent <= x {
            return CandleWickSize::Small;
        }

        x = sd_wick_percents.mul_add(self.threshold2, mean_wick_percent);
        if wick_percent <= x {
            return CandleWickSize::Medium;
        }

        CandleWickSize::Large
    }

    fn std_dev<const CAP: usize>(buffer: &ArrayDeque<f64, CAP, Wrapping>, mean: f64) -> f64 {
        if buffer.is_empty() {
            return 0.0;
        }
        let variance = buffer
            .iter()
            .map(|v| {
                let d = v - mean;
                d * d
            })
            .sum::<f64>()
            / (buffer.len() as f64);
        variance.sqrt()
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::{
        stubs::{fuzzy_candlesticks_1, fuzzy_candlesticks_3, fuzzy_candlesticks_10},
        volatility::fuzzy::FuzzyCandlesticks,
    };

    #[rstest]
    fn test_psl_initialized(fuzzy_candlesticks_10: FuzzyCandlesticks) {
        let display_str = format!("{fuzzy_candlesticks_10}");
        assert_eq!(display_str, "FuzzyCandlesticks(10,0.1,0.15,0.2,0.3)");
        assert_eq!(fuzzy_candlesticks_10.period, 10);
        assert!(!fuzzy_candlesticks_10.initialized);
        assert!(!fuzzy_candlesticks_10.has_inputs);
    }

    #[rstest]
    fn test_value_with_one_input(mut fuzzy_candlesticks_1: FuzzyCandlesticks) {
        //fix: When period = 1, the standard deviation is 0, and all fuzzy divisions based on mean ± threshold * sd become invalid.
        fuzzy_candlesticks_1.update_raw(123.90, 135.79, 117.09, 125.09);
        assert_eq!(fuzzy_candlesticks_1.value.direction, CandleDirection::Bull);
        assert_eq!(fuzzy_candlesticks_1.value.size, CandleSize::VerySmall);
        assert_eq!(fuzzy_candlesticks_1.value.body_size, CandleBodySize::Small);
        assert_eq!(
            fuzzy_candlesticks_1.value.upper_wick_size,
            CandleWickSize::Small
        );
        assert_eq!(
            fuzzy_candlesticks_1.value.lower_wick_size,
            CandleWickSize::Small
        );

        let expected_vec = vec![1, 1, 1, 1, 1];
        assert_eq!(fuzzy_candlesticks_1.vector, expected_vec);
    }

    #[rstest]
    fn test_value_with_three_inputs(mut fuzzy_candlesticks_3: FuzzyCandlesticks) {
        // fix: self.lengths[0] : ArrayDeque is oldest value, old test is not right
        fuzzy_candlesticks_3.update_raw(142.35, 145.82, 141.20, 144.75);
        fuzzy_candlesticks_3.update_raw(144.75, 144.93, 103.55, 108.22);
        fuzzy_candlesticks_3.update_raw(108.22, 120.15, 105.01, 119.89);
        assert_eq!(fuzzy_candlesticks_3.value.direction, CandleDirection::Bull);
        assert_eq!(fuzzy_candlesticks_3.value.size, CandleSize::VerySmall);
        assert_eq!(fuzzy_candlesticks_3.value.body_size, CandleBodySize::Trend);
        assert_eq!(
            fuzzy_candlesticks_3.value.upper_wick_size,
            CandleWickSize::Small
        );
        assert_eq!(
            fuzzy_candlesticks_3.value.lower_wick_size,
            CandleWickSize::Large
        );

        let expected_vec = vec![1, 1, 4, 1, 3];
        assert_eq!(fuzzy_candlesticks_3.vector, expected_vec);
    }

    #[rstest]
    fn test_value_not_updated_before_initialization(mut fuzzy_candlesticks_10: FuzzyCandlesticks) {
        //fix: period not reached, should not update value
        fuzzy_candlesticks_10.update_raw(100.0, 105.0, 95.0, 102.0);
        fuzzy_candlesticks_10.update_raw(102.0, 108.0, 100.0, 98.0);
        fuzzy_candlesticks_10.update_raw(98.0, 101.0, 96.0, 100.0);

        assert_eq!(fuzzy_candlesticks_10.vector.len(), 0);
        assert!(
            !fuzzy_candlesticks_10.initialized,
            "Should not be initialized before period"
        );
        assert!(fuzzy_candlesticks_10.has_inputs, "Should  has inputs");
        assert_eq!(fuzzy_candlesticks_10.lengths.len(), 3);
        assert_eq!(fuzzy_candlesticks_10.body_percents.len(), 3);
    }

    #[rstest]
    fn test_value_with_ten_inputs(mut fuzzy_candlesticks_10: FuzzyCandlesticks) {
        fuzzy_candlesticks_10.update_raw(150.25, 153.4, 148.1, 152.75);
        fuzzy_candlesticks_10.update_raw(152.8, 155.2, 151.3, 151.95);
        fuzzy_candlesticks_10.update_raw(151.9, 152.85, 147.6, 148.2);
        fuzzy_candlesticks_10.update_raw(148.3, 150.75, 146.9, 150.4);
        fuzzy_candlesticks_10.update_raw(150.5, 154.3, 149.8, 153.9);
        fuzzy_candlesticks_10.update_raw(153.95, 155.8, 152.2, 152.6);
        fuzzy_candlesticks_10.update_raw(152.7, 153.4, 148.5, 149.1);
        fuzzy_candlesticks_10.update_raw(149.2, 151.9, 147.3, 151.5);
        fuzzy_candlesticks_10.update_raw(151.6, 156.4, 151.0, 155.8);
        fuzzy_candlesticks_10.update_raw(155.9, 157.2, 153.7, 154.3);

        assert_eq!(fuzzy_candlesticks_10.value.direction, CandleDirection::Bear);
        assert_eq!(fuzzy_candlesticks_10.value.size, CandleSize::VerySmall);
        assert_eq!(fuzzy_candlesticks_10.value.body_size, CandleBodySize::Small);
        assert_eq!(
            fuzzy_candlesticks_10.value.upper_wick_size,
            CandleWickSize::Large
        );
        assert_eq!(
            fuzzy_candlesticks_10.value.lower_wick_size,
            CandleWickSize::Small
        );

        let expected_vec = vec![-1, 1, 1, 3, 1];
        assert_eq!(fuzzy_candlesticks_10.vector, expected_vec);
    }

    #[rstest]
    fn test_reset(mut fuzzy_candlesticks_10: FuzzyCandlesticks) {
        fuzzy_candlesticks_10.update_raw(151.6, 156.4, 151.0, 155.8);
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
    #[rstest]
    fn test_zero_length_candle(mut fuzzy_candlesticks_1: FuzzyCandlesticks) {
        fuzzy_candlesticks_1.update_raw(100.0, 100.0, 100.0, 100.0); // high == low
        assert_eq!(fuzzy_candlesticks_1.value.size, CandleSize::None);
        assert_eq!(fuzzy_candlesticks_1.value.body_size, CandleBodySize::None);
        assert_eq!(
            fuzzy_candlesticks_1.value.upper_wick_size,
            CandleWickSize::None
        );
        assert_eq!(
            fuzzy_candlesticks_1.value.lower_wick_size,
            CandleWickSize::None
        );
        assert_eq!(fuzzy_candlesticks_1.value.direction, CandleDirection::None);
    }

    #[rstest]
    fn test_constant_input_stddev_zero(mut fuzzy_candlesticks_1: FuzzyCandlesticks) {
        for _ in 0..10 {
            fuzzy_candlesticks_1.update_raw(100.0, 110.0, 90.0, 105.0);
        }
        assert!(fuzzy_candlesticks_1.lengths.iter().all(|&v| v == 20.0));
        assert!(matches!(
            fuzzy_candlesticks_1.value.size,
            CandleSize::VerySmall | CandleSize::Small | CandleSize::Medium
        ));
    }

    #[rstest]
    fn test_nan_inf_safety(mut fuzzy_candlesticks_1: FuzzyCandlesticks) {
        fuzzy_candlesticks_1.update_raw(f64::INFINITY, f64::INFINITY, f64::INFINITY, f64::INFINITY);
        fuzzy_candlesticks_1.update_raw(f64::NAN, f64::NAN, f64::NAN, f64::NAN);
        assert_eq!(fuzzy_candlesticks_1.value.direction, CandleDirection::None);
    }

    #[rstest]
    fn test_direction_cases(mut fuzzy_candlesticks_1: FuzzyCandlesticks) {
        fuzzy_candlesticks_1.update_raw(100.0, 105.0, 95.0, 110.0); // Bull
        assert_eq!(fuzzy_candlesticks_1.value.direction, CandleDirection::Bull);

        fuzzy_candlesticks_1.update_raw(110.0, 115.0, 105.0, 100.0); // Bear
        assert_eq!(fuzzy_candlesticks_1.value.direction, CandleDirection::Bear);

        fuzzy_candlesticks_1.update_raw(100.0, 110.0, 90.0, 100.0); // None
        assert_eq!(fuzzy_candlesticks_1.value.direction, CandleDirection::None);
    }

    #[rstest]
    fn test_body_and_wick_percentages(mut fuzzy_candlesticks_1: FuzzyCandlesticks) {
        let open: f64 = 100.0;
        let close: f64 = 110.0;
        let high: f64 = 120.0;
        let low: f64 = 90.0;

        let total = high - low; // 30
        let expected_body = (close - open).abs() / total; // 10 / 30 = 0.3333
        let expected_upper_wick = (high - close.max(open)) / total; // (120 - 110) / 30 = 0.3333
        let expected_lower_wick = (open.min(close) - low) / total; // (100 - 90) / 30 = 0.3333

        fuzzy_candlesticks_1.update_raw(open, high, low, close);

        let actual_body = fuzzy_candlesticks_1.body_percents[0];
        let actual_upper = fuzzy_candlesticks_1.upper_wick_percents[0];
        let actual_lower = fuzzy_candlesticks_1.lower_wick_percents[0];

        assert!(
            (actual_body - expected_body).abs() < 1e-6,
            "Body percent mismatch"
        );
        assert!(
            (actual_upper - expected_upper_wick).abs() < 1e-6,
            "Upper wick percent mismatch"
        );
        assert!(
            (actual_lower - expected_lower_wick).abs() < 1e-6,
            "Lower wick percent mismatch"
        );
    }

    #[rstest]
    fn test_body_size_large(mut fuzzy_candlesticks_3: FuzzyCandlesticks) {
        // K1: Almost no body (open == close)
        fuzzy_candlesticks_3.update_raw(100.0, 101.0, 99.0, 100.0);
        // body = 0.0 → body% = 0.0 / 2.0 = 0.0%

        // K2: Small body
        fuzzy_candlesticks_3.update_raw(100.0, 102.0, 98.0, 100.5);
        // body = 0.5 → body% = 0.5 / 4.0 = 12.5%

        // K3: Large body, nearly fills the range
        fuzzy_candlesticks_3.update_raw(101.0, 105.0, 100.0, 104.8);
        // body = |104.8 - 101.0| = 3.8
        // length = 5.0
        // body_percent = 3.8 / 5.0 = 76.0%

        // Due to high deviation from mean, should be classified as Large
        assert_eq!(fuzzy_candlesticks_3.value.body_size, CandleBodySize::Trend);
    }

    #[rstest]
    fn test_lower_wick_size_large(mut fuzzy_candlesticks_3: FuzzyCandlesticks) {
        // K1: No lower wick (low == close)
        fuzzy_candlesticks_3.update_raw(100.0, 101.0, 100.0, 101.0);
        // lower_wick = min(open, close) - low = 100 - 100 = 0 → 0%

        // K2: Short lower wick
        fuzzy_candlesticks_3.update_raw(102.0, 103.0, 101.5, 102.5);
        // min(open, close) = 102.0
        // lower_wick = 102.0 - 101.5 = 0.5
        // length = 1.5
        // lower_wick_percent = 0.5 / 1.5 ≈ 33.3%

        // K3: Long lower wick, strong rebound from low
        fuzzy_candlesticks_3.update_raw(110.0, 115.0, 100.0, 114.0);
        // min(open, close) = 110.0
        // lower_wick = 110.0 - 100.0 = 10.0
        // length = 15.0
        // lower_wick_percent = 10.0 / 15.0 ≈ 66.7%

        // Value is significantly above mean + 0.15*sd → should be Large
        assert_eq!(
            fuzzy_candlesticks_3.value.lower_wick_size,
            CandleWickSize::Large
        );
    }

    #[rstest]
    fn test_upper_wick_size_large(mut fuzzy_candlesticks_3: FuzzyCandlesticks) {
        // K1: No upper wick (high == open/close)
        fuzzy_candlesticks_3.update_raw(100.0, 100.0, 99.0, 100.0);
        // upper_wick = 0

        // K2: Short upper wick
        fuzzy_candlesticks_3.update_raw(101.0, 102.0, 100.0, 101.5);
        // max(open, close) = 102.0? No: max is 102.0 (high), close=101.5
        // upper_wick = 102.0 - 101.5 = 0.5
        // length = 2.0 → percent = 25.0%

        // K3: Long upper wick, price rejected from high
        fuzzy_candlesticks_3.update_raw(105.0, 115.0, 104.0, 106.0);
        // max(open, close) = max(105.0, 106.0) = 106.0
        // upper_wick = 115.0 - 106.0 = 9.0
        // length = 11.0
        // upper_wick_percent = 9.0 / 11.0 ≈ 81.8%

        // Should be classified as Large due to high relative size
        assert_eq!(
            fuzzy_candlesticks_3.value.upper_wick_size,
            CandleWickSize::Large
        );
    }
}
