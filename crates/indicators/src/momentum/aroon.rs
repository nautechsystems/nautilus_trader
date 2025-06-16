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
use nautilus_model::{
    data::{Bar, QuoteTick, TradeTick},
    enums::PriceType,
};

use crate::indicator::Indicator;

pub const MAX_PERIOD: usize = 1_024;

const ROUND_DP: f64 = 1_000_000_000_000.0;

/// The Aroon Oscillator calculates the Aroon Up and Aroon Down indicators to
/// determine if an instrument is trending, and the strength of the trend.
#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators")
)]
pub struct AroonOscillator {
    pub period: usize,
    pub aroon_up: f64,
    pub aroon_down: f64,
    pub value: f64,
    pub count: usize,
    pub initialized: bool,
    has_inputs: bool,
    total_count: usize,
    high_inputs: ArrayDeque<f64, MAX_PERIOD, Wrapping>,
    low_inputs: ArrayDeque<f64, MAX_PERIOD, Wrapping>,
}

impl Display for AroonOscillator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.name(), self.period)
    }
}

impl Indicator for AroonOscillator {
    fn name(&self) -> String {
        stringify!(AroonOscillator).into()
    }

    fn has_inputs(&self) -> bool {
        self.has_inputs
    }

    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_quote(&mut self, quote: &QuoteTick) {
        let price = quote.extract_price(PriceType::Mid).into();
        self.update_raw(price, price);
    }

    fn handle_trade(&mut self, trade: &TradeTick) {
        let price: f64 = trade.price.into();
        self.update_raw(price, price);
    }

    fn handle_bar(&mut self, bar: &Bar) {
        let high: f64 = (&bar.high).into();
        let low: f64 = (&bar.low).into();
        self.update_raw(high, low);
    }

    fn reset(&mut self) {
        self.high_inputs.clear();
        self.low_inputs.clear();
        self.aroon_up = 0.0;
        self.aroon_down = 0.0;
        self.value = 0.0;
        self.count = 0;
        self.total_count = 0;
        self.has_inputs = false;
        self.initialized = false;
    }
}

impl AroonOscillator {
    /// Creates a new [`AroonOscillator`] instance.
    ///
    /// # Panics
    ///
    /// Panics if `period` is not positive (> 0).
    #[must_use]
    pub fn new(period: usize) -> Self {
        assert!(
            period > 0,
            "AroonOscillator: period must be > 0 (received {period})"
        );
        assert!(
            period <= MAX_PERIOD,
            "AroonOscillator: period must be ≤ {MAX_PERIOD} (received {period})"
        );

        Self {
            period,
            aroon_up: 0.0,
            aroon_down: 0.0,
            value: 0.0,
            count: 0,
            total_count: 0,
            has_inputs: false,
            initialized: false,
            high_inputs: ArrayDeque::new(),
            low_inputs: ArrayDeque::new(),
        }
    }

    pub fn update_raw(&mut self, high: f64, low: f64) {
        debug_assert!(
            high >= low,
            "AroonOscillator::update_raw - high must be ≥ low"
        );

        self.total_count = self.total_count.saturating_add(1);

        if self.count == self.period + 1 {
            let _ = self.high_inputs.pop_front();
            let _ = self.low_inputs.pop_front();
        } else {
            self.count += 1;
        }

        let _ = self.high_inputs.push_back(high);
        let _ = self.low_inputs.push_back(low);

        let required = self.period + 1;
        if !self.initialized && self.total_count >= required {
            self.initialized = true;
        }
        self.has_inputs = true;

        if self.initialized {
            self.calculate_aroon();
        }
    }

    fn calculate_aroon(&mut self) {
        let len = self.high_inputs.len();
        debug_assert!(len == self.period + 1);

        let mut max_idx = 0_usize;
        let mut max_val = f64::MIN;
        for (idx, &hi) in self.high_inputs.iter().enumerate() {
            if hi > max_val {
                max_val = hi;
                max_idx = idx;
            }
        }

        let mut min_idx_rel = 0_usize;
        let mut min_val = f64::MAX;
        for (idx, &lo) in self.low_inputs.iter().skip(1).enumerate() {
            if lo < min_val {
                min_val = lo;
                min_idx_rel = idx;
            }
        }

        let periods_since_high = len - 1 - max_idx;
        let periods_since_low = self.period - 1 - min_idx_rel;

        self.aroon_up =
            Self::round(100.0 * (self.period - periods_since_high) as f64 / self.period as f64);
        self.aroon_down =
            Self::round(100.0 * (self.period - periods_since_low) as f64 / self.period as f64);
        self.value = Self::round(self.aroon_up - self.aroon_down);
    }

    #[inline]
    fn round(v: f64) -> f64 {
        (v * ROUND_DP).round() / ROUND_DP
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::indicator::Indicator;

    #[rstest]
    fn test_name() {
        let aroon = AroonOscillator::new(10);
        assert_eq!(aroon.name(), "AroonOscillator");
    }

    #[rstest]
    fn test_period() {
        let aroon = AroonOscillator::new(10);
        assert_eq!(aroon.period, 10);
    }

    #[rstest]
    fn test_initialized_false() {
        let aroon = AroonOscillator::new(10);
        assert!(!aroon.initialized());
    }

    #[rstest]
    fn test_initialized_true() {
        let mut aroon = AroonOscillator::new(10);
        for _ in 0..=10 {
            aroon.update_raw(110.08, 109.61);
        }
        assert!(aroon.initialized());
    }

    #[rstest]
    fn test_value_one_input() {
        let mut aroon = AroonOscillator::new(1);
        aroon.update_raw(110.08, 109.61);
        assert_eq!(aroon.aroon_up, 0.0);
        assert_eq!(aroon.aroon_down, 0.0);
        assert_eq!(aroon.value, 0.0);
        assert!(!aroon.initialized());
        aroon.update_raw(110.10, 109.70);
        assert!(aroon.initialized());
        assert_eq!(aroon.aroon_up, 100.0);
        assert_eq!(aroon.aroon_down, 100.0);
        assert_eq!(aroon.value, 0.0);
    }

    #[rstest]
    fn test_value_twenty_inputs() {
        let mut aroon = AroonOscillator::new(20);
        let inputs = [
            (110.08, 109.61),
            (110.15, 109.91),
            (110.10, 109.73),
            (110.06, 109.77),
            (110.29, 109.88),
            (110.53, 110.29),
            (110.61, 110.26),
            (110.28, 110.17),
            (110.30, 110.00),
            (110.25, 110.01),
            (110.25, 109.81),
            (109.92, 109.71),
            (110.21, 109.84),
            (110.08, 109.95),
            (110.20, 109.96),
            (110.16, 109.95),
            (109.99, 109.75),
            (110.20, 109.73),
            (110.10, 109.81),
            (110.04, 109.96),
            (110.02, 109.90),
        ];
        for &(h, l) in &inputs {
            aroon.update_raw(h, l);
        }
        assert!(aroon.initialized());
        assert_eq!(aroon.aroon_up, 30.0);
        assert_eq!(aroon.value, -25.0);
    }

    #[rstest]
    fn test_reset() {
        let mut aroon = AroonOscillator::new(10);
        for _ in 0..12 {
            aroon.update_raw(110.08, 109.61);
        }
        aroon.reset();
        assert!(!aroon.initialized());
        assert_eq!(aroon.aroon_up, 0.0);
        assert_eq!(aroon.aroon_down, 0.0);
        assert_eq!(aroon.value, 0.0);
    }

    #[rstest]
    fn test_initialized_boundary() {
        let mut aroon = AroonOscillator::new(5);
        for _ in 0..5 {
            aroon.update_raw(1.0, 0.0);
            assert!(!aroon.initialized());
        }
        aroon.update_raw(1.0, 0.0);
        assert!(aroon.initialized());
    }

    #[rstest]
    #[case(1, 0)]
    #[case(5, 0)]
    #[case(5, 2)]
    #[case(10, 0)]
    #[case(10, 9)]
    fn test_formula_equivalence(#[case] period: usize, #[case] high_idx: usize) {
        let mut aroon = AroonOscillator::new(period);
        for idx in 0..=period {
            let h = if idx == high_idx { 1_000.0 } else { idx as f64 };
            aroon.update_raw(h, h);
        }
        assert!(aroon.initialized());
        let expected = 100.0 * (high_idx as f64) / period as f64;
        let diff = aroon.aroon_up - expected;
        assert!(diff.abs() < 1e-6);
    }

    #[rstest]
    fn test_window_size_period_plus_one() {
        let period = 7;
        let mut aroon = AroonOscillator::new(period);
        for _ in 0..=period {
            aroon.update_raw(1.0, 0.0);
        }
        assert_eq!(aroon.high_inputs.len(), period + 1);
        assert_eq!(aroon.low_inputs.len(), period + 1);
    }

    #[rstest]
    fn test_ignore_oldest_low() {
        let mut aroon = AroonOscillator::new(5);
        aroon.update_raw(10.0, 0.0);
        let inputs = [
            (11.0, 9.0),
            (12.0, 9.5),
            (13.0, 9.2),
            (14.0, 9.3),
            (15.0, 9.4),
        ];
        for &(h, l) in &inputs {
            aroon.update_raw(h, l);
        }
        assert!(aroon.initialized());
        assert_eq!(aroon.aroon_up, 100.0);
        assert_eq!(aroon.aroon_down, 20.0);
        assert_eq!(aroon.value, 80.0);
    }
}
