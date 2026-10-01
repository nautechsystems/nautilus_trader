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

use std::{
    collections::VecDeque,
    fmt::{Debug, Display},
};

use nautilus_core::correctness::FAILED;
use nautilus_model::{
    data::{Bar, QuoteTick, TradeTick},
    enums::PriceType,
};

use crate::{
    average::{MovingAverageFactory, MovingAverageType},
    indicator::{Indicator, MovingAverage},
    support::{MAX_PERIOD, ShiftedMoments},
};

/// An indicator which calculates a Relative Volatility Index (RVI) across a rolling window.
#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.indicators", unsendable)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.indicators")
)]
pub struct RelativeVolatilityIndex {
    pub period: usize,
    pub scalar: f64,
    pub ma_type: MovingAverageType,
    pub value: f64,
    pub initialized: bool,
    prices: VecDeque<f64>,
    moments: ShiftedMoments,
    pos_ma: Box<dyn MovingAverage + Send + 'static>,
    neg_ma: Box<dyn MovingAverage + Send + 'static>,
    previous_close: f64,
    has_inputs: bool,
}

impl Display for RelativeVolatilityIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}({},{},{})",
            self.name(),
            self.period,
            self.scalar,
            self.ma_type,
        )
    }
}

impl Indicator for RelativeVolatilityIndex {
    fn name(&self) -> String {
        stringify!(RelativeVolatilityIndex).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.has_inputs
    }

    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        self.update_raw(quote.extract_price(PriceType::Mid)?.into());
        Ok(())
    }

    fn handle_trade(&mut self, trade: &TradeTick) {
        self.update_raw((&trade.price).into());
    }

    fn handle_bar(&mut self, bar: &Bar) {
        self.update_raw((&bar.close).into());
    }

    fn reset(&mut self) {
        self.previous_close = 0.0;
        self.value = 0.0;
        self.has_inputs = false;
        self.initialized = false;
        self.prices.clear();
        self.moments.reset();
        self.pos_ma.reset();
        self.neg_ma.reset();
    }
}

impl RelativeVolatilityIndex {
    /// Creates a new [`RelativeVolatilityIndex`] instance.
    ///
    /// # Panics
    ///
    /// This function panics if:
    /// - `period` is not in the range of 2 to `MAX_PERIOD` (inclusive).
    /// - `scalar` is not in the range of 0.0 to 100.0 (inclusive).
    /// - `ma_type` is not a valid [`MovingAverageType`].
    #[must_use]
    pub fn new(period: usize, scalar: Option<f64>, ma_type: Option<MovingAverageType>) -> Self {
        Self::new_checked(period, scalar, ma_type).expect(FAILED)
    }

    pub(crate) fn new_checked(
        period: usize,
        scalar: Option<f64>,
        ma_type: Option<MovingAverageType>,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            (2..=MAX_PERIOD).contains(&period),
            "period must be in 2..={MAX_PERIOD}"
        );
        let scalar = scalar.unwrap_or(100.0);
        anyhow::ensure!(
            scalar.is_finite() && (0.0..=100.0).contains(&scalar),
            "scalar must be finite and in 0..=100"
        );
        Ok(Self {
            period,
            scalar,
            ma_type: ma_type.unwrap_or(MovingAverageType::Wilder),
            value: 0.0,
            initialized: false,
            prices: VecDeque::with_capacity(period),
            moments: ShiftedMoments::new(),
            pos_ma: MovingAverageFactory::create(
                ma_type.unwrap_or(MovingAverageType::Wilder),
                period,
            ),
            neg_ma: MovingAverageFactory::create(
                ma_type.unwrap_or(MovingAverageType::Wilder),
                period,
            ),
            previous_close: 0.0,
            has_inputs: false,
        })
    }

    pub fn update_raw(&mut self, close: f64) {
        if !close.is_finite() {
            return;
        }

        if self.prices.len() == self.period
            && let Some(old) = self.prices.pop_front()
        {
            self.moments.evict(old);
        }
        self.prices.push_back(close);
        self.moments.push(close);
        if self.moments.needs_reseed(self.period) {
            self.moments.reseed(self.prices.iter());
        }
        self.has_inputs = true;

        if self.prices.len() < self.period {
            self.previous_close = close;
            return;
        }

        let std_dev = self.moments.std_dev(self.period);
        self.pos_ma.update_raw(if close > self.previous_close {
            std_dev
        } else {
            0.0
        });
        self.neg_ma.update_raw(if close < self.previous_close {
            std_dev
        } else {
            0.0
        });
        self.previous_close = close;

        if !self.pos_ma.initialized() || !self.neg_ma.initialized() {
            return;
        }
        let denominator = self.pos_ma.value() + self.neg_ma.value();
        self.value = if denominator == 0.0 {
            self.scalar * 0.5
        } else {
            self.scalar * self.pos_ma.value() / denominator
        };
        self.initialized = true;
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::stubs::rvi_10;

    #[rstest]
    fn test_name_returns_expected_string(rvi_10: RelativeVolatilityIndex) {
        assert_eq!(rvi_10.name(), "RelativeVolatilityIndex");
    }

    #[rstest]
    fn test_str_repr_returns_expected_string(rvi_10: RelativeVolatilityIndex) {
        assert_eq!(format!("{rvi_10}"), "RelativeVolatilityIndex(10,10,SIMPLE)");
    }

    #[rstest]
    fn test_period_returns_expected_value(rvi_10: RelativeVolatilityIndex) {
        assert_eq!(rvi_10.period, 10);
        assert_eq!(rvi_10.scalar, 10.0);
        assert_eq!(rvi_10.ma_type, MovingAverageType::Simple);
    }

    #[rstest]
    fn test_initialized_without_inputs_returns_false(rvi_10: RelativeVolatilityIndex) {
        assert!(!rvi_10.initialized());
    }

    #[rstest]
    fn test_value_with_all_higher_inputs_returns_expected_value(
        mut rvi_10: RelativeVolatilityIndex,
    ) {
        let close_values = [
            105.25, 107.50, 109.75, 112.00, 114.25, 116.50, 118.75, 121.00, 123.25, 125.50, 127.75,
            130.00, 132.25, 134.50, 136.75, 139.00, 141.25, 143.50, 145.75, 148.00, 150.25, 152.50,
            154.75, 157.00, 159.25, 161.50, 163.75, 166.00, 168.25, 170.50,
        ];

        for close in close_values {
            rvi_10.update_raw(close);
        }

        assert!(rvi_10.initialized());
        assert_eq!(rvi_10.value, 10.0);
    }

    #[rstest]
    fn test_prices_window_bounded_to_period(mut rvi_10: RelativeVolatilityIndex) {
        // Regression: the price window must stay bounded to `period`. Previously the
        // fixed-capacity deque grew to its 1024 capacity, so the standard deviation was
        // computed over far more than `period` prices while using a `period`-window mean.
        for i in 0..50 {
            rvi_10.update_raw(100.0 + f64::from(i));
        }

        assert!(rvi_10.initialized());
        assert_eq!(rvi_10.prices.len(), 10);
        assert_eq!(rvi_10.value, 10.0);
    }

    #[rstest]
    fn test_reset_successfully_returns_indicator_to_fresh_state(
        mut rvi_10: RelativeVolatilityIndex,
    ) {
        rvi_10.update_raw(1.00020);
        rvi_10.update_raw(1.00030);
        rvi_10.update_raw(1.00070);

        rvi_10.reset();

        assert!(!rvi_10.initialized());
        assert_eq!(rvi_10.value, 0.0);
        assert!(!rvi_10.initialized);
        assert!(!rvi_10.has_inputs);
        assert_eq!(rvi_10.prices.len(), 0);
        assert_eq!(rvi_10.moments.mean(rvi_10.period), 0.0);
        assert_eq!(rvi_10.pos_ma.value(), 0.0);
        assert_eq!(rvi_10.neg_ma.value(), 0.0);
    }
}
