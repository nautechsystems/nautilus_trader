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

use crate::{
    average::{MovingAverageFactory, MovingAverageType},
    indicator::{Indicator, MovingAverage},
    support::MAX_PERIOD,
};

/// An indicator which calculates an Average True Range (ATR) across a rolling window.
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
pub struct AverageTrueRange {
    pub period: usize,
    pub ma_type: MovingAverageType,
    pub use_previous: bool,
    pub value_floor: f64,
    pub value: f64,
    pub count: usize,
    pub initialized: bool,
    ma: Box<dyn MovingAverage + Send + 'static>,
    has_inputs: bool,
    previous_close: f64,
}

impl Display for AverageTrueRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}({},{},{},{})",
            self.name(),
            self.period,
            self.ma_type,
            self.use_previous,
            self.value_floor,
        )
    }
}

impl Indicator for AverageTrueRange {
    fn name(&self) -> String {
        stringify!(AverageTrueRange).to_string()
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
        self.ma.reset();
        self.previous_close = 0.0;
        self.value = 0.0;
        self.count = 0;
        self.has_inputs = false;
        self.initialized = false;
    }
}

impl AverageTrueRange {
    /// Creates a new [`AverageTrueRange`] instance.
    ///
    /// # Panics
    ///
    /// Panics if `period` is outside `1..=MAX_PERIOD`, or `value_floor` is negative or non-finite.
    #[must_use]
    pub fn new(
        period: usize,
        ma_type: Option<MovingAverageType>,
        use_previous: Option<bool>,
        value_floor: Option<f64>,
    ) -> Self {
        Self::new_checked(period, ma_type, use_previous, value_floor).expect(FAILED)
    }

    pub(crate) fn new_checked(
        period: usize,
        ma_type: Option<MovingAverageType>,
        use_previous: Option<bool>,
        value_floor: Option<f64>,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            (1..=MAX_PERIOD).contains(&period),
            "period must be in 1..={MAX_PERIOD}"
        );
        let value_floor = value_floor.unwrap_or(0.0);
        anyhow::ensure!(
            value_floor.is_finite() && value_floor >= 0.0,
            "value_floor must be finite and non-negative"
        );
        Ok(Self {
            period,
            ma_type: ma_type.unwrap_or(MovingAverageType::Wilder),
            use_previous: use_previous.unwrap_or(true),
            value_floor,
            value: 0.0,
            count: 0,
            previous_close: 0.0,
            ma: MovingAverageFactory::create(ma_type.unwrap_or(MovingAverageType::Wilder), period),
            has_inputs: false,
            initialized: false,
        })
    }

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
        let range = if self.use_previous && self.has_inputs {
            f64::max(self.previous_close, high) - f64::min(low, self.previous_close)
        } else {
            high - low
        };

        if !range.is_finite() {
            return;
        }
        self.ma.update_raw(range);

        if self.use_previous {
            self.previous_close = close;
        }

        self.increment_count();

        if self.initialized {
            self.apply_floor();
        }
    }

    fn apply_floor(&mut self) {
        if self.value_floor == 0.0 || self.value_floor < self.ma.value() {
            self.value = self.ma.value();
        } else {
            // Floor the value
            self.value = self.value_floor;
        }
    }

    fn increment_count(&mut self) {
        self.count += 1;

        if !self.initialized {
            self.has_inputs = true;

            if self.ma.initialized() {
                self.initialized = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::{
        stubs::{stub_quote, stub_trade},
        testing::assert_approx_equal,
    };

    #[rstest]
    fn test_name_returns_expected_string() {
        let atr = AverageTrueRange::new(10, Some(MovingAverageType::Simple), None, None);
        assert_eq!(atr.name(), "AverageTrueRange");
    }

    #[rstest]
    fn test_str_repr_returns_expected_string() {
        let atr = AverageTrueRange::new(10, Some(MovingAverageType::Simple), Some(true), Some(0.0));
        assert_eq!(format!("{atr}"), "AverageTrueRange(10,SIMPLE,true,0)");
    }

    #[rstest]
    fn test_period() {
        let atr = AverageTrueRange::new(10, Some(MovingAverageType::Simple), None, None);
        assert_eq!(atr.period, 10);
    }

    #[rstest]
    #[case(None, "WilderMovingAverage")]
    #[case(Some(MovingAverageType::Simple), "SimpleMovingAverage")]
    #[case(Some(MovingAverageType::Exponential), "ExponentialMovingAverage")]
    #[case(
        Some(MovingAverageType::DoubleExponential),
        "DoubleExponentialMovingAverage"
    )]
    #[case(Some(MovingAverageType::Wilder), "WilderMovingAverage")]
    #[case(Some(MovingAverageType::Hull), "HullMovingAverage")]
    fn test_ma_type_creates_expected_inner_ma(
        #[case] ma_type: Option<MovingAverageType>,
        #[case] expected: &str,
    ) {
        let atr = AverageTrueRange::new(10, ma_type, None, None);
        assert_eq!(atr.ma.name(), expected);
    }

    #[rstest]
    fn test_initialized_without_inputs_returns_false() {
        let atr = AverageTrueRange::new(10, Some(MovingAverageType::Simple), None, None);
        assert!(!atr.initialized());
    }

    #[rstest]
    fn test_initialized_with_required_inputs_returns_true() {
        let mut atr = AverageTrueRange::new(10, Some(MovingAverageType::Simple), None, None);
        for _ in 0..10 {
            atr.update_raw(1.0, 1.0, 1.0);
        }
        assert!(atr.initialized());
    }

    #[rstest]
    fn test_value_with_no_inputs_returns_zero() {
        let atr = AverageTrueRange::new(10, Some(MovingAverageType::Simple), None, None);
        assert_eq!(atr.value, 0.0);
    }

    #[rstest]
    fn test_value_with_epsilon_input() {
        let mut atr = AverageTrueRange::new(10, Some(MovingAverageType::Simple), None, None);
        let epsilon = f64::EPSILON;
        atr.update_raw(epsilon, epsilon, epsilon);
        assert_eq!(atr.value, 0.0);
    }

    #[rstest]
    fn test_value_with_one_ones_input() {
        let mut atr = AverageTrueRange::new(10, Some(MovingAverageType::Simple), None, None);
        atr.update_raw(1.0, 1.0, 1.0);
        assert_eq!(atr.value, 0.0);
    }

    #[rstest]
    fn test_value_with_one_input() {
        let mut atr = AverageTrueRange::new(10, Some(MovingAverageType::Simple), None, None);
        atr.update_raw(1.00020, 1.0, 1.00010);
        assert_eq!(atr.value, 0.0);
        assert!(!atr.initialized);
    }

    #[rstest]
    fn test_value_with_three_inputs() {
        let mut atr = AverageTrueRange::new(10, Some(MovingAverageType::Simple), None, None);
        atr.update_raw(1.00020, 1.0, 1.00010);
        atr.update_raw(1.00020, 1.0, 1.00010);
        atr.update_raw(1.00020, 1.0, 1.00010);
        assert_eq!(atr.value, 0.0);
        assert!(!atr.initialized);
    }

    #[rstest]
    fn test_value_with_close_on_high() {
        let mut atr = AverageTrueRange::new(10, Some(MovingAverageType::Simple), None, None);
        let mut high = 1.00010;
        let mut low = 1.0;

        for _ in 0..1000 {
            high += 0.00010;
            low += 0.00010;
            let close = high;
            atr.update_raw(high, low, close);
        }
        assert_approx_equal(atr.value, 0.0001);
    }

    #[rstest]
    fn test_value_with_close_on_low() {
        let mut atr = AverageTrueRange::new(10, Some(MovingAverageType::Simple), None, None);
        let mut high = 1.00010;
        let mut low = 1.0;

        for _ in 0..1000 {
            high -= 0.00010;
            low -= 0.00010;
            let close = low;
            atr.update_raw(high, low, close);
        }
        assert_approx_equal(atr.value, 0.0001);
    }

    #[rstest]
    fn test_floor_with_ten_ones_inputs() {
        let floor = 0.00005;
        let mut floored_atr =
            AverageTrueRange::new(10, Some(MovingAverageType::Simple), None, Some(floor));

        for _ in 0..20 {
            floored_atr.update_raw(1.0, 1.0, 1.0);
        }
        assert_eq!(floored_atr.value, 5e-05);
    }

    #[rstest]
    #[case(MovingAverageType::Simple, 3)]
    #[case(MovingAverageType::Exponential, 3)]
    #[case(MovingAverageType::DoubleExponential, 5)]
    #[case(MovingAverageType::Wilder, 3)]
    #[case(MovingAverageType::Hull, 3)]
    fn test_selected_smoother_controls_readiness_and_floor(
        #[case] ma_type: MovingAverageType,
        #[case] first_ready: usize,
    ) {
        let mut indicator = AverageTrueRange::new(3, Some(ma_type), None, Some(3.0));
        for count in 1..=10 {
            indicator.update_raw(12.0, 10.0, 11.0);
            assert_eq!(indicator.count, count);
            assert_eq!(indicator.initialized, count >= first_ready);
            assert_eq!(
                indicator.value,
                if count >= first_ready { 3.0 } else { 0.0 }
            );
        }
    }

    #[rstest]
    fn test_rejected_candle_does_not_change_the_next_true_range() {
        let mut indicator = AverageTrueRange::new(2, None, None, None);
        indicator.update_raw(12.0, 10.0, 11.0);

        for (high, low, close) in [
            (10.0, 12.0, 11.0),
            (12.0, 10.0, 13.0),
            (12.0, 10.0, f64::NAN),
            (1e308, -1e308, 0.0),
        ] {
            indicator.update_raw(high, low, close);
            assert_eq!(indicator.count, 1);
            assert_eq!(indicator.value, 0.0);
            assert!(!indicator.initialized);
        }
        indicator.update_raw(22.0, 20.0, 21.0);
        assert_eq!(indicator.count, 2);
        assert_eq!(indicator.value, 6.5);
        assert!(indicator.initialized);
    }

    #[rstest]
    fn test_floor_with_exponentially_decreasing_high_inputs() {
        let floor = 0.00005;
        let mut floored_atr =
            AverageTrueRange::new(10, Some(MovingAverageType::Simple), None, Some(floor));
        let mut high = 1.00020;
        let low = 1.0;
        let close = 1.0;

        for _ in 0..20 {
            high -= (high - low) / 2.0;
            floored_atr.update_raw(high, low, close);
        }
        assert_eq!(floored_atr.value, floor);
    }

    #[rstest]
    fn test_reset_successfully_returns_indicator_to_fresh_state() {
        let mut atr = AverageTrueRange::new(10, Some(MovingAverageType::Simple), None, None);
        for _ in 0..1000 {
            atr.update_raw(1.00010, 1.0, 1.00005);
        }
        atr.reset();
        assert!(!atr.initialized);
        assert_eq!(atr.value, 0.0);
    }

    #[rstest]
    fn test_reset_resets_inner_ma() {
        let mut atr = AverageTrueRange::new(10, Some(MovingAverageType::Simple), None, None);
        atr.update_raw(1.00010, 1.0, 1.00005);
        atr.reset();
        assert_eq!(atr.ma.count(), 0);
    }

    #[rstest]
    fn test_quote_and_trade_are_ignored(stub_quote: QuoteTick, stub_trade: TradeTick) {
        let mut atr = AverageTrueRange::new(10, None, None, None);

        let result = atr.handle_quote(&stub_quote);
        atr.handle_trade(&stub_trade);

        assert!(result.is_ok());
        assert!(!atr.has_inputs());
        assert_eq!(atr.count, 0);
        assert_eq!(atr.value, 0.0);
    }
}
