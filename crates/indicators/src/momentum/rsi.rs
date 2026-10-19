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
use nautilus_model::{
    data::{Bar, QuoteTick, TradeTick},
    enums::PriceType,
};

use crate::{
    average::{MovingAverageFactory, MovingAverageType},
    indicator::{Indicator, MovingAverage},
    support::MAX_PERIOD,
};

/// An indicator which calculates a relative strength index (RSI) across a rolling window.
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
pub struct RelativeStrengthIndex {
    pub period: usize,
    pub ma_type: MovingAverageType,
    pub value: f64,
    pub count: usize,
    pub initialized: bool,
    has_inputs: bool,
    last_value: f64,
    average_gain: Box<dyn MovingAverage + Send + 'static>,
    average_loss: Box<dyn MovingAverage + Send + 'static>,
    rsi_max: f64,
}

impl Display for RelativeStrengthIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({},{})", self.name(), self.period, self.ma_type)
    }
}

impl Indicator for RelativeStrengthIndex {
    fn name(&self) -> String {
        stringify!(RelativeStrengthIndex).to_string()
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
        self.update_raw((trade.price).into());
    }

    fn handle_bar(&mut self, bar: &Bar) {
        self.update_raw((&bar.close).into());
    }

    fn reset(&mut self) {
        self.value = 0.0;
        self.last_value = 0.0;
        self.count = 0;
        self.has_inputs = false;
        self.initialized = false;
        self.average_gain.reset();
        self.average_loss.reset();
    }
}

impl RelativeStrengthIndex {
    /// Creates a new [`RelativeStrengthIndex`] instance.
    ///
    /// # Panics
    ///
    /// Panics if `period` is zero or exceeds 16,777,216.
    #[must_use]
    pub fn new(period: usize, ma_type: Option<MovingAverageType>) -> Self {
        Self::new_checked(period, ma_type).expect(FAILED)
    }

    pub(crate) fn new_checked(
        period: usize,
        ma_type: Option<MovingAverageType>,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            (1..=MAX_PERIOD).contains(&period),
            "period must be in 1..={MAX_PERIOD}"
        );
        let ma_type = ma_type.unwrap_or(MovingAverageType::Wilder);
        Ok(Self {
            period,
            ma_type,
            value: 0.0,
            last_value: 0.0,
            count: 0,
            has_inputs: false,
            average_gain: MovingAverageFactory::create(ma_type, period),
            average_loss: MovingAverageFactory::create(ma_type, period),
            rsi_max: 100.0,
            initialized: false,
        })
    }

    pub fn update_raw(&mut self, value: f64) {
        if !value.is_finite() {
            return;
        }

        self.count += 1;
        self.has_inputs = true;

        if self.count == 1 {
            self.last_value = value;
            return;
        }

        let change = value - self.last_value;
        self.last_value = value;
        self.average_gain.update_raw(change.max(0.0));
        self.average_loss.update_raw((-change).max(0.0));
        if !self.average_gain.initialized() || !self.average_loss.initialized() {
            return;
        }

        let average_gain = self.average_gain.value();
        let average_loss = self.average_loss.value();
        let total = average_gain + average_loss;
        self.value = if total == 0.0 {
            50.0
        } else {
            self.rsi_max * (average_gain / total)
        };
        self.initialized = true;
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::data::{Bar, QuoteTick, TradeTick};
    use rstest::rstest;

    use crate::{
        average::MovingAverageType, indicator::Indicator, momentum::rsi::RelativeStrengthIndex,
        stubs::*, testing::assert_approx_equal,
    };

    #[rstest]
    fn test_rsi_initialized(rsi_10: RelativeStrengthIndex) {
        let display_str = format!("{rsi_10}");
        assert_eq!(display_str, "RelativeStrengthIndex(10,EXPONENTIAL)");
        assert_eq!(rsi_10.period, 10);
        assert!(!rsi_10.initialized);
    }

    #[rstest]
    fn test_initialized_with_required_inputs_returns_true(mut rsi_10: RelativeStrengthIndex) {
        for i in 0..12 {
            rsi_10.update_raw(f64::from(i));
        }
        assert!(rsi_10.initialized);
    }

    #[rstest]
    fn test_value_with_one_input_returns_expected_value(mut rsi_10: RelativeStrengthIndex) {
        rsi_10.update_raw(1.0);
        assert_eq!(rsi_10.value, 0.0);
        assert!(!rsi_10.initialized());
    }

    #[rstest]
    fn test_value_all_higher_inputs_returns_expected_value() {
        let mut rsi_10 = RelativeStrengthIndex::new(3, None);
        for value in [1.0, 2.0, 3.0, 4.0] {
            rsi_10.update_raw(value);
        }

        assert_eq!(rsi_10.value, 100.0);
        assert!(rsi_10.initialized());
    }

    #[rstest]
    fn test_value_with_all_lower_inputs_returns_expected_value() {
        let mut rsi_10 = RelativeStrengthIndex::new(3, None);
        for value in [4.0, 3.0, 2.0, 1.0] {
            rsi_10.update_raw(value);
        }

        assert_eq!(rsi_10.value, 0.0);
        assert!(rsi_10.initialized());
    }

    #[rstest]
    fn test_value_with_various_input_returns_expected_value() {
        let mut rsi_10 = RelativeStrengthIndex::new(3, None);
        for value in [1.0, 2.0, 1.0, 3.0] {
            rsi_10.update_raw(value);
        }

        assert_eq!(rsi_10.value, 75.0);
    }

    #[rstest]
    fn test_value_at_returns_expected_value() {
        let mut rsi_10 = RelativeStrengthIndex::new(3, None);
        for value in [1.0, 2.0, 1.0, 3.0, 2.0] {
            rsi_10.update_raw(value);
        }

        assert_approx_equal(rsi_10.value, 54.545_454_545_454_55);
    }

    #[rstest]
    fn test_reset(mut rsi_10: RelativeStrengthIndex) {
        rsi_10.update_raw(1.0);
        rsi_10.update_raw(2.0);
        rsi_10.reset();
        assert!(!rsi_10.initialized());
        assert_eq!(rsi_10.count, 0);
    }

    #[rstest]
    fn test_reset_resets_inner_mas(mut rsi_10: RelativeStrengthIndex) {
        rsi_10.update_raw(1.0);
        rsi_10.update_raw(2.0);
        rsi_10.reset();
        assert_eq!(rsi_10.average_gain.count(), 0);
        assert_eq!(rsi_10.average_loss.count(), 0);
    }

    #[rstest]
    fn test_handle_quote_tick(mut rsi_10: RelativeStrengthIndex, stub_quote: QuoteTick) {
        rsi_10.handle_quote(&stub_quote).unwrap();
        assert!(rsi_10.has_inputs());
        assert_eq!(rsi_10.count, 1);
        assert_eq!(rsi_10.value, 0.0);
        assert!(!rsi_10.initialized());
    }

    #[rstest]
    fn test_handle_trade_tick(mut rsi_10: RelativeStrengthIndex, stub_trade: TradeTick) {
        rsi_10.handle_trade(&stub_trade);
        assert!(rsi_10.has_inputs());
        assert_eq!(rsi_10.count, 1);
        assert_eq!(rsi_10.value, 0.0);
        assert!(!rsi_10.initialized());
    }

    #[rstest]
    fn test_handle_bar(mut rsi_10: RelativeStrengthIndex, bar_ethusdt_binance_minute_bid: Bar) {
        rsi_10.handle_bar(&bar_ethusdt_binance_minute_bid);
        assert!(rsi_10.has_inputs());
        assert_eq!(rsi_10.count, 1);
        assert_eq!(rsi_10.value, 0.0);
        assert!(!rsi_10.initialized());
    }

    #[rstest]
    fn test_constant_inputs_initializes_and_value_max() {
        let mut rsi = RelativeStrengthIndex::new(3, None);
        for _ in 0..4 {
            rsi.update_raw(42.0);
        }

        assert!(rsi.initialized());
        assert_eq!(rsi.value, 50.0);
    }

    #[rstest]
    fn test_reset_resets_has_inputs_and_value(mut rsi_10: RelativeStrengthIndex) {
        rsi_10.update_raw(1.0);
        rsi_10.reset();
        assert!(!rsi_10.has_inputs());
        assert_eq!(rsi_10.value, 0.0);
    }

    // Feeds `values` through a fresh RSI of the given `ma_type` and returns the final value.
    fn run_rsi(values: &[f64], period: usize, ma_type: MovingAverageType) -> f64 {
        let mut rsi = RelativeStrengthIndex::new(period, Some(ma_type));
        for &v in values {
            rsi.update_raw(v);
        }
        rsi.value
    }

    #[rstest]
    fn test_ma_type_is_plumbed_into_inner_averages() {
        let prices = [1.0, 2.0, 1.0, 3.0, 2.0, 4.0];
        let simple = run_rsi(&prices, 3, MovingAverageType::Simple);
        let wilder = run_rsi(&prices, 3, MovingAverageType::Wilder);

        assert_ne!(simple, wilder);
    }

    #[rstest]
    fn test_recovers_below_max_after_losses() {
        let mut rsi = RelativeStrengthIndex::new(3, None);
        for value in [1.0, 2.0, 3.0, 4.0, 3.0] {
            rsi.update_raw(value);
        }

        assert!(rsi.value < 100.0);
        assert!(rsi.value > 0.0);
    }

    #[rstest]
    fn test_wilder_smoothed_series() {
        let mut rsi = RelativeStrengthIndex::new(3, None);
        let inputs = [1.0, 2.0, 1.0, 3.0, 2.0];
        let expected = [0.0, 0.0, 0.0, 75.0, 54.545_454_545_454_55];
        for (input, expected) in inputs.into_iter().zip(expected) {
            rsi.update_raw(input);
            assert_approx_equal(rsi.value, expected);
        }
    }
}
