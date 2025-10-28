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

use std::fmt::Display;

use arraydeque::{ArrayDeque, Wrapping};
use nautilus_model::{
    data::{Bar, QuoteTick, TradeTick},
    enums::PriceType,
};

use crate::indicator::{Indicator, MovingAverage};

const MAX_PERIOD: usize = 1_024;

#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators")
)]
pub struct SimpleMovingAverage {
    pub period: usize,
    pub price_type: PriceType,
    pub value: f64,
    sum: f64,
    pub count: usize,
    buf: ArrayDeque<f64, MAX_PERIOD, Wrapping>,
    pub initialized: bool,
}

impl Display for SimpleMovingAverage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.name(), self.period)
    }
}

impl Indicator for SimpleMovingAverage {
    fn name(&self) -> String {
        stringify!(SimpleMovingAverage).into()
    }

    fn has_inputs(&self) -> bool {
        self.count > 0
    }

    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_quote(&mut self, quote: &QuoteTick) {
        self.process_raw(quote.extract_price(self.price_type).into());
    }

    fn handle_trade(&mut self, trade: &TradeTick) {
        self.process_raw(trade.price.into());
    }

    fn handle_bar(&mut self, bar: &Bar) {
        self.process_raw(bar.close.into());
    }

    fn reset(&mut self) {
        self.value = 0.0;
        self.sum = 0.0;
        self.count = 0;
        self.buf.clear();
        self.initialized = false;
    }
}

impl MovingAverage for SimpleMovingAverage {
    fn value(&self) -> f64 {
        self.value
    }

    fn count(&self) -> usize {
        self.count
    }

    fn update_raw(&mut self, value: f64) {
        self.process_raw(value);
    }
}

impl SimpleMovingAverage {
    /// Creates a new [`SimpleMovingAverage`] instance.
    ///
    /// # Panics
    ///
    /// Panics if `period` is not positive (> 0).
    #[must_use]
    pub fn new(period: usize, price_type: Option<PriceType>) -> Self {
        assert!(period > 0, "SimpleMovingAverage: period must be > 0");
        assert!(
            period <= MAX_PERIOD,
            "SimpleMovingAverage: period {period} exceeds MAX_PERIOD ({MAX_PERIOD})"
        );

        Self {
            period,
            price_type: price_type.unwrap_or(PriceType::Last),
            value: 0.0,
            sum: 0.0,
            count: 0,
            buf: ArrayDeque::new(),
            initialized: false,
        }
    }

    fn process_raw(&mut self, price: f64) {
        if self.count == self.period {
            if let Some(oldest) = self.buf.pop_front() {
                self.sum -= oldest;
            }
        } else {
            self.count += 1;
        }

        let _ = self.buf.push_back(price);
        self.sum += price;

        self.value = self.sum / self.count as f64;
        self.initialized = self.count >= self.period;
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use arraydeque::{ArrayDeque, Wrapping};
    use nautilus_model::{
        data::{QuoteTick, TradeTick},
        enums::PriceType,
    };
    use rstest::rstest;

    use super::MAX_PERIOD;
    use crate::{
        average::sma::SimpleMovingAverage,
        indicator::{Indicator, MovingAverage},
        stubs::*,
    };

    #[rstest]
    fn sma_initialized_state(indicator_sma_10: SimpleMovingAverage) {
        let display_str = format!("{indicator_sma_10}");
        assert_eq!(display_str, "SimpleMovingAverage(10)");
        assert_eq!(indicator_sma_10.period, 10);
        assert_eq!(indicator_sma_10.price_type, PriceType::Mid);
        assert_eq!(indicator_sma_10.value, 0.0);
        assert_eq!(indicator_sma_10.count, 0);
        assert!(!indicator_sma_10.initialized());
        assert!(!indicator_sma_10.has_inputs());
    }

    #[rstest]
    fn sma_update_raw_exact_period(indicator_sma_10: SimpleMovingAverage) {
        let mut sma = indicator_sma_10;
        for i in 1..=10 {
            sma.update_raw(f64::from(i));
        }
        assert!(sma.has_inputs());
        assert!(sma.initialized());
        assert_eq!(sma.count, 10);
        assert_eq!(sma.value, 5.5);
    }

    #[rstest]
    fn sma_reset_smoke(indicator_sma_10: SimpleMovingAverage) {
        let mut sma = indicator_sma_10;
        sma.update_raw(1.0);
        assert_eq!(sma.count, 1);
        sma.reset();
        assert_eq!(sma.count, 0);
        assert_eq!(sma.value, 0.0);
        assert!(!sma.initialized());
    }

    #[rstest]
    fn sma_handle_single_quote(indicator_sma_10: SimpleMovingAverage, stub_quote: QuoteTick) {
        let mut sma = indicator_sma_10;
        sma.handle_quote(&stub_quote);
        assert_eq!(sma.count, 1);
        assert_eq!(sma.value, 1501.0);
    }

    #[rstest]
    fn sma_handle_multiple_quotes(indicator_sma_10: SimpleMovingAverage) {
        let mut sma = indicator_sma_10;
        let q1 = stub_quote("1500.0", "1502.0");
        let q2 = stub_quote("1502.0", "1504.0");

        sma.handle_quote(&q1);
        sma.handle_quote(&q2);
        assert_eq!(sma.count, 2);
        assert_eq!(sma.value, 1502.0);
    }

    #[rstest]
    fn sma_handle_trade(indicator_sma_10: SimpleMovingAverage, stub_trade: TradeTick) {
        let mut sma = indicator_sma_10;
        sma.handle_trade(&stub_trade);
        assert_eq!(sma.count, 1);
        assert_eq!(sma.value, 1500.0);
    }

    #[rstest]
    #[case(1)]
    #[case(3)]
    #[case(5)]
    #[case(16)]
    fn count_progression_respects_period(#[case] period: usize) {
        let mut sma = SimpleMovingAverage::new(period, None);

        for i in 0..(period * 3) {
            sma.update_raw(i as f64);

            assert!(
                sma.count() <= period,
                "period={period}, step={i}, count={}",
                sma.count()
            );

            let expected = usize::min(i + 1, period);
            assert_eq!(
                sma.count(),
                expected,
                "period={period}, step={i}, expected={expected}, was={}",
                sma.count()
            );
        }
    }

    #[rstest]
    #[case(1)]
    #[case(4)]
    #[case(10)]
    fn count_after_reset_is_zero(#[case] period: usize) {
        let mut sma = SimpleMovingAverage::new(period, None);

        for i in 0..(period + 2) {
            sma.update_raw(i as f64);
        }
        assert_eq!(sma.count(), period, "pre-reset saturation failed");

        sma.reset();
        assert_eq!(sma.count(), 0, "count not reset to zero");
        assert_eq!(sma.value(), 0.0, "value not reset to zero");
        assert!(!sma.initialized(), "initialized flag not cleared");
    }

    #[rstest]
    fn count_edge_case_period_one() {
        let mut sma = SimpleMovingAverage::new(1, None);

        sma.update_raw(10.0);
        assert_eq!(sma.count(), 1);
        assert_eq!(sma.value(), 10.0);

        sma.update_raw(20.0);
        assert_eq!(sma.count(), 1, "count exceeded 1 with period==1");
        assert_eq!(sma.value(), 20.0, "value not equal to latest price");
    }

    #[rstest]
    fn sliding_window_correctness() {
        let mut sma = SimpleMovingAverage::new(3, None);

        let prices = [1.0, 2.0, 3.0, 4.0, 5.0];
        let expect_avg = [1.0, 1.5, 2.0, 3.0, 4.0];

        for (i, &p) in prices.iter().enumerate() {
            sma.update_raw(p);
            assert!(
                (sma.value() - expect_avg[i]).abs() < 1e-9,
                "step {i}: expected {}, was {}",
                expect_avg[i],
                sma.value()
            );
        }
    }

    #[rstest]
    #[case(2)]
    #[case(6)]
    fn initialized_transitions_with_count(#[case] period: usize) {
        let mut sma = SimpleMovingAverage::new(period, None);

        for i in 0..(period - 1) {
            sma.update_raw(i as f64);
            assert!(
                !sma.initialized(),
                "initialized early at i={i} (period={period})"
            );
        }

        sma.update_raw(42.0);
        assert_eq!(sma.count(), period);
        assert!(sma.initialized(), "initialized flag not set at period");
    }

    #[rstest]
    #[should_panic(expected = "period must be > 0")]
    fn sma_new_with_zero_period_panics() {
        let _ = SimpleMovingAverage::new(0, None);
    }

    #[rstest]
    fn sma_rolling_mean_exact_values() {
        let mut sma = SimpleMovingAverage::new(3, None);
        let inputs = [1.0, 2.0, 3.0, 4.0, 5.0];
        let expected = [1.0, 1.5, 2.0, 3.0, 4.0];

        for (&price, &exp_mean) in inputs.iter().zip(expected.iter()) {
            sma.update_raw(price);
            assert!(
                (sma.value() - exp_mean).abs() < 1e-12,
                "input={price}, expected={exp_mean}, got={}",
                sma.value()
            );
        }
    }

    #[rstest]
    fn sma_matches_reference_implementation() {
        const PERIOD: usize = 5;
        let mut sma = SimpleMovingAverage::new(PERIOD, None);
        let mut window: ArrayDeque<f64, PERIOD, Wrapping> = ArrayDeque::new();

        for step in 0..20 {
            let price = f64::from(step) * 10.0;
            sma.update_raw(price);

            if window.len() == PERIOD {
                window.pop_front();
            }
            let _ = window.push_back(price);

            let ref_mean: f64 = window.iter().sum::<f64>() / window.len() as f64;
            assert!(
                (sma.value() - ref_mean).abs() < 1e-12,
                "step={step}, expected={ref_mean}, was={}",
                sma.value()
            );
        }
    }

    #[rstest]
    #[case(f64::NAN)]
    #[case(f64::INFINITY)]
    #[case(f64::NEG_INFINITY)]
    fn sma_handles_bad_floats(#[case] bad: f64) {
        let mut sma = SimpleMovingAverage::new(3, None);
        sma.update_raw(1.0);
        sma.update_raw(bad);
        sma.update_raw(3.0);
        assert!(
            sma.value().is_nan() || !sma.value().is_finite(),
            "bad float not propagated"
        );
    }

    #[rstest]
    fn deque_and_count_always_match() {
        const PERIOD: usize = 8;
        let mut sma = SimpleMovingAverage::new(PERIOD, None);
        for i in 0..50 {
            sma.update_raw(f64::from(i));
            assert!(
                sma.buf.len() == sma.count,
                "buf.len() != count at step {i}: {} != {}",
                sma.buf.len(),
                sma.count
            );
        }
    }

    #[rstest]
    fn sma_multiple_resets() {
        let mut sma = SimpleMovingAverage::new(4, None);
        for cycle in 0..5 {
            for x in 0..4 {
                sma.update_raw(f64::from(x));
            }
            assert!(sma.initialized(), "cycle {cycle}: not initialized");
            sma.reset();
            assert_eq!(sma.count(), 0);
            assert_eq!(sma.value(), 0.0);
            assert!(!sma.initialized());
        }
    }

    #[rstest]
    fn sma_buffer_never_exceeds_capacity() {
        const PERIOD: usize = MAX_PERIOD;
        let mut sma = super::SimpleMovingAverage::new(PERIOD, None);

        for i in 0..(PERIOD * 2) {
            sma.update_raw(i as f64);

            assert!(
                sma.buf.len() <= PERIOD,
                "step {i}: buf.len()={}, exceeds PERIOD={PERIOD}",
                sma.buf.len(),
            );
        }
        assert!(
            sma.buf.is_full(),
            "buffer not reported as full after saturation"
        );
        assert_eq!(
            sma.count(),
            PERIOD,
            "count diverged from logical window length"
        );
    }

    #[rstest]
    fn sma_deque_eviction_order() {
        let mut sma = super::SimpleMovingAverage::new(3, None);

        sma.update_raw(1.0);
        sma.update_raw(2.0);
        sma.update_raw(3.0);
        sma.update_raw(4.0);

        assert_eq!(sma.buf.front().copied(), Some(2.0), "oldest element wrong");
        assert_eq!(sma.buf.back().copied(), Some(4.0), "newest element wrong");

        assert!(
            (sma.value() - 3.0).abs() < 1e-12,
            "unexpected mean after eviction: {}",
            sma.value()
        );
    }

    #[rstest]
    fn sma_sum_consistent_with_buffer() {
        const PERIOD: usize = 7;
        let mut sma = super::SimpleMovingAverage::new(PERIOD, None);

        for i in 0..40 {
            sma.update_raw(f64::from(i));

            let deque_sum: f64 = sma.buf.iter().copied().sum();
            assert!(
                (sma.sum - deque_sum).abs() < 1e-12,
                "step {i}: internal sum={} differs from buf sum={}",
                sma.sum,
                deque_sum
            );
        }
    }
}
