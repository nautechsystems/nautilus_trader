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

use std::{collections::VecDeque, fmt::Display};

use nautilus_core::correctness::FAILED;
use nautilus_model::{
    data::{Bar, QuoteTick, TradeTick},
    enums::PriceType,
};

use crate::{
    indicator::Indicator,
    support::{MAX_PERIOD, ShiftedMoments},
};

/// Z-Score: how many standard deviations the latest price sits from its rolling
/// mean.
///
/// ```text
/// ZScore = (price - SMA(price, n)) / population_stddev(price, n)
/// ```
///
/// A reading of `+2` means price is two standard deviations above its recent
/// average, statistically stretched to the upside; `-2` is the mirror. It is the
/// standard normalization behind mean-reversion strategies: a large magnitude
/// flags an extension, a return toward `0` flags reversion. A window with zero
/// dispersion yields `0` rather than dividing by zero.
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
pub struct ZScore {
    pub period: usize,
    pub price_type: PriceType,
    pub value: f64,
    pub mean: f64,
    pub std: f64,
    pub count: usize,
    pub initialized: bool,
    has_inputs: bool,
    window: VecDeque<f64>,
    moments: ShiftedMoments,
}

impl Display for ZScore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.name(), self.period)
    }
}

impl Indicator for ZScore {
    fn name(&self) -> String {
        stringify!(ZScore).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.has_inputs
    }

    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        self.update_raw(quote.extract_price(self.price_type)?.into());
        Ok(())
    }

    fn handle_trade(&mut self, trade: &TradeTick) {
        self.update_raw((&trade.price).into());
    }

    fn handle_bar(&mut self, bar: &Bar) {
        self.update_raw((&bar.close).into());
    }

    fn reset(&mut self) {
        self.window.clear();
        self.moments.reset();
        self.value = 0.0;
        self.mean = 0.0;
        self.std = 0.0;
        self.count = 0;
        self.has_inputs = false;
        self.initialized = false;
    }
}

impl ZScore {
    /// Creates a new [`ZScore`] instance.
    ///
    /// # Panics
    ///
    /// Panics if a requested allocation exceeds `MAX_PERIOD` elements.
    /// Panics if `period` is zero.
    #[must_use]
    pub fn new(period: usize, price_type: Option<PriceType>) -> Self {
        Self::new_checked(period, price_type).expect(FAILED)
    }

    /// Creates a new [`ZScore`] instance with a validated period.
    ///
    /// # Errors
    ///
    /// Returns an error if `period` is zero or exceeds `MAX_PERIOD`.
    pub fn new_checked(period: usize, price_type: Option<PriceType>) -> anyhow::Result<Self> {
        anyhow::ensure!(period <= MAX_PERIOD, "period cannot exceed {MAX_PERIOD}");
        anyhow::ensure!(period > 0, "ZScore: period must be > 0 (received {period})");
        Ok(Self {
            period,
            price_type: price_type.unwrap_or(PriceType::Last),
            value: 0.0,
            mean: 0.0,
            std: 0.0,
            count: 0,
            has_inputs: false,
            initialized: false,
            window: VecDeque::with_capacity(period),
            moments: ShiftedMoments::new(),
        })
    }

    /// Updates the indicator with the given raw price value.
    pub fn update_raw(&mut self, value: f64) {
        if !value.is_finite() {
            return;
        }
        self.count += 1;
        self.has_inputs = true;

        if self.window.len() == self.period
            && let Some(old) = self.window.pop_front()
        {
            self.moments.evict(old);
        }
        self.window.push_back(value);
        self.moments.push(value);
        if self.moments.needs_reseed(self.period) {
            self.moments.reseed(&self.window);
        }

        if self.window.len() < self.period {
            return;
        }
        self.mean = self.moments.mean(self.period);
        self.std = self.moments.std_dev(self.period);
        // A window with no dispersion: the price is exactly its own mean.
        self.value = if self.std == 0.0 {
            0.0
        } else {
            (value - self.mean) / self.std
        };
        self.initialized = true;
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::indicator::Indicator;

    #[rstest]
    fn test_name_and_display() {
        let indicator = ZScore::new(20, None);
        assert_eq!(indicator.name(), "ZScore");
        assert_eq!(format!("{indicator}"), "ZScore(20)");
        assert_eq!(indicator.period, 20);
        assert!(!indicator.initialized());
        assert!(!indicator.has_inputs());
    }

    #[rstest]
    #[should_panic(expected = "period must be > 0")]
    fn test_zero_period_panics() {
        let _ = ZScore::new(0, None);
    }

    #[rstest]
    fn test_first_value_on_period_th_input() {
        let mut indicator = ZScore::new(5, None);
        for i in 1..5 {
            indicator.update_raw(f64::from(i));
            assert!(!indicator.initialized(), "initialized early at input {i}");
        }
        indicator.update_raw(5.0);
        assert!(indicator.initialized());
    }

    #[rstest]
    fn test_reference_value() {
        // Window [1, 3]: mean 2, population variance (1 + 9)/2 - 4 = 1, stddev 1;
        // the latest price 3 is (3 - 2) / 1 = 1 stddev above.
        let mut indicator = ZScore::new(2, None);
        indicator.update_raw(1.0);
        assert!(!indicator.initialized());
        indicator.update_raw(3.0);
        assert!(indicator.initialized());
        assert_eq!(indicator.value, 1.0);
    }

    #[rstest]
    fn test_constant_series_yields_zero() {
        let mut indicator = ZScore::new(10, None);
        for _ in 0..30 {
            indicator.update_raw(42.0);
        }
        assert_eq!(indicator.value, 0.0);
    }

    #[rstest]
    fn test_matches_naive_definition() {
        // Independent two-pass reference computed straight from the definition.
        fn naive(window: &[f64]) -> f64 {
            let n = window.len() as f64;
            let mean = window.iter().sum::<f64>() / n;
            let var = window.iter().map(|y| (y - mean) * (y - mean)).sum::<f64>() / n;
            (window[window.len() - 1] - mean) / var.sqrt()
        }

        let prices: Vec<f64> = (0..60)
            .map(|i| 50.0 + (f64::from(i) * 0.3).sin() * 10.0)
            .collect();
        let period = 20;
        let mut indicator = ZScore::new(period, None);
        let mut compared = 0_usize;

        for (i, &p) in prices.iter().enumerate() {
            indicator.update_raw(p);

            if i + 1 < period {
                continue;
            }
            let want = naive(&prices[i + 1 - period..=i]);
            assert!(
                (indicator.value - want).abs() <= 1e-12 * want.abs().max(1.0),
                "bar {i}: got {} want {want}",
                indicator.value
            );
            compared += 1;
        }
        assert_eq!(compared, prices.len() - period + 1);
    }

    #[rstest]
    fn test_reset() {
        let mut indicator = ZScore::new(5, None);
        for i in 0..20 {
            indicator.update_raw(f64::from(i));
        }
        indicator.reset();
        assert!(!indicator.initialized());
        assert!(!indicator.has_inputs());
        assert_eq!(indicator.value, 0.0);
        assert_eq!(indicator.count, 0);
    }
}
