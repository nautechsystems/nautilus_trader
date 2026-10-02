// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_core::correctness::FAILED;
use nautilus_model::data::{Bar, QuoteTick, TradeTick};

use crate::{indicator::Indicator, support::MAX_PERIOD};

/// Schwager volatility ratio.
#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.indicators")
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.indicators")
)]
pub struct VolatilityRatio {
    pub period: usize,
    pub value: f64,
    pub count: usize,
    pub initialized: bool,
    has_inputs: bool,
    alpha: f64,
    previous_close: Option<f64>,
    seed_sum: f64,
    seed_count: usize,
    average: Option<f64>,
}

impl Display for VolatilityRatio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.name(), self.period)
    }
}

impl Indicator for VolatilityRatio {
    fn name(&self) -> String {
        stringify!(VolatilityRatio).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.has_inputs
    }

    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_quote(&mut self, _quote: &QuoteTick) -> anyhow::Result<()> {
        Ok(())
    }

    fn handle_trade(&mut self, _trade: &TradeTick) {}

    fn handle_bar(&mut self, bar: &Bar) {
        self.update_raw((&bar.high).into(), (&bar.low).into(), (&bar.close).into());
    }

    fn reset(&mut self) {
        self.value = 0.0;
        self.count = 0;
        self.initialized = false;
        self.has_inputs = false;
        self.previous_close = None;
        self.seed_sum = 0.0;
        self.seed_count = 0;
        self.average = None;
    }
}

impl VolatilityRatio {
    /// Creates Schwager's volatility ratio.
    ///
    /// # Panics
    ///
    /// Panics if `period` is zero.
    #[must_use]
    pub fn new(period: usize) -> Self {
        Self::new_checked(period).expect(FAILED)
    }

    pub(crate) fn new_checked(period: usize) -> anyhow::Result<Self> {
        anyhow::ensure!(period <= MAX_PERIOD, "period cannot exceed {MAX_PERIOD}");
        anyhow::ensure!(
            period > 0,
            "VolatilityRatio: period must be > 0 (received {period})"
        );
        Ok(Self {
            period,
            value: 0.0,
            count: 0,
            initialized: false,
            has_inputs: false,
            alpha: 2.0 / (period as f64 + 1.0),
            previous_close: None,
            seed_sum: 0.0,
            seed_count: 0,
            average: None,
        })
    }

    /// Updates the indicator from high, low, and close prices.
    pub fn update_raw(&mut self, high: f64, low: f64, close: f64) {
        if !high.is_finite()
            || !low.is_finite()
            || !close.is_finite()
            || high < low
            || close < low
            || close > high
        {
            return;
        }
        let true_range = self.previous_close.map(|previous_close| {
            (high - low)
                .max((high - previous_close).abs())
                .max((low - previous_close).abs())
        });

        self.previous_close = Some(close);
        self.count += 1;
        self.has_inputs = true;
        let Some(true_range) = true_range else {
            return;
        };
        let Some(previous_average) = self.average else {
            self.seed_sum += true_range;
            self.seed_count += 1;
            if self.seed_count == self.period {
                self.average = Some(self.seed_sum / self.period as f64);
            }
            return;
        };

        self.value = if previous_average > 0.0 {
            true_range / previous_average
        } else {
            0.0
        };
        self.average = Some(
            self.alpha
                .mul_add(true_range, (1.0 - self.alpha) * previous_average),
        );
        self.initialized = true;
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::stubs::{stub_quote, stub_trade, vr_10};

    const BARS: [(f64, f64, f64); 5] = [
        (10.0, 8.0, 9.0),
        (11.0, 9.0, 10.0),
        (12.0, 10.0, 11.0),
        (14.0, 11.0, 13.0),
        (13.0, 10.0, 12.0),
    ];

    fn feed(indicator: &mut VolatilityRatio, count: usize) {
        for &(high, low, close) in &BARS[..count] {
            indicator.update_raw(high, low, close);
        }
    }

    #[rstest]
    fn test_name_and_display(vr_10: VolatilityRatio) {
        assert_eq!(vr_10.name(), "VolatilityRatio");
        assert_eq!(format!("{vr_10}"), "VolatilityRatio(10)");
        assert_eq!(vr_10.period, 10);
        assert!(!vr_10.initialized());
        assert!(!vr_10.has_inputs());
    }

    #[rstest]
    #[should_panic(expected = "period must be > 0")]
    fn test_zero_period_panics() {
        let _ = VolatilityRatio::new(0);
    }

    #[rstest]
    fn test_period_above_maximum_is_rejected() {
        assert!(VolatilityRatio::new_checked(MAX_PERIOD + 1).is_err());
    }

    #[rstest]
    #[case(1, 0.0, false)]
    #[case(2, 0.0, false)]
    #[case(3, 0.0, false)]
    #[case(4, 1.5, true)]
    fn test_readiness_follows_seeded_true_range_average(
        #[case] inputs: usize,
        #[case] expected: f64,
        #[case] initialized: bool,
    ) {
        let mut indicator = VolatilityRatio::new(2);
        feed(&mut indicator, inputs);

        assert_eq!(indicator.count, inputs);
        assert_eq!(indicator.value, expected);
        assert_eq!(indicator.initialized(), initialized);
        assert!(indicator.has_inputs());
    }

    #[rstest]
    fn test_value_divides_true_range_by_prior_average() {
        let mut indicator = VolatilityRatio::new(2);
        feed(&mut indicator, 5);

        assert_eq!(indicator.value, 3.0 / (8.0 / 3.0));
    }

    #[rstest]
    fn test_flat_market_yields_zero() {
        let mut indicator = VolatilityRatio::new(2);
        for _ in 0..6 {
            indicator.update_raw(5.0, 5.0, 5.0);
        }

        assert!(indicator.initialized());
        assert_eq!(indicator.value, 0.0);
    }

    #[rstest]
    #[case(f64::NAN, 1.0, 1.0)]
    #[case(2.0, f64::INFINITY, 1.0)]
    #[case(1.0, 2.0, 1.5)]
    #[case(2.0, 1.0, 3.0)]
    #[case(2.0, 1.0, 0.5)]
    fn test_rejected_candle_leaves_state_unchanged(
        #[case] high: f64,
        #[case] low: f64,
        #[case] close: f64,
    ) {
        let mut indicator = VolatilityRatio::new(2);
        feed(&mut indicator, 3);
        indicator.update_raw(high, low, close);
        indicator.update_raw(BARS[3].0, BARS[3].1, BARS[3].2);

        assert_eq!(indicator.count, 4);
        assert_eq!(indicator.value, 1.5);
    }

    #[rstest]
    fn test_reset_restores_fresh_state() {
        let mut indicator = VolatilityRatio::new(2);
        feed(&mut indicator, 5);
        indicator.reset();

        assert_eq!(indicator.count, 0);
        assert_eq!(indicator.value, 0.0);
        assert!(!indicator.initialized());
        assert!(!indicator.has_inputs());

        feed(&mut indicator, 4);
        assert_eq!(indicator.value, 1.5);
    }

    #[rstest]
    fn test_quote_and_trade_are_ignored(stub_quote: QuoteTick, stub_trade: TradeTick) {
        let mut indicator = VolatilityRatio::new(2);

        let result = indicator.handle_quote(&stub_quote);
        indicator.handle_trade(&stub_trade);

        assert!(result.is_ok());
        assert!(!indicator.has_inputs());
        assert_eq!(indicator.count, 0);
        assert_eq!(indicator.value, 0.0);
    }
}
