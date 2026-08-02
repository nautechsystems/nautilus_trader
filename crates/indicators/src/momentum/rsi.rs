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

use nautilus_model::{
    data::{Bar, QuoteTick, TradeTick},
    enums::PriceType,
};

use crate::{
    average::{MovingAverageFactory, MovingAverageType},
    indicator::{Indicator, MovingAverage},
};

/// An indicator which calculates a relative strength index (RSI) across a rolling window.
#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators", unsendable)
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
    #[must_use]
    pub fn new(period: usize, ma_type: Option<MovingAverageType>) -> Self {
        let ma_type = ma_type.unwrap_or(MovingAverageType::Exponential);
        Self {
            period,
            ma_type,
            value: 0.0,
            last_value: 0.0,
            count: 0,
            has_inputs: false,
            average_gain: MovingAverageFactory::create(ma_type, period),
            average_loss: MovingAverageFactory::create(ma_type, period),
            rsi_max: 1.0,
            initialized: false,
        }
    }

    pub fn update_raw(&mut self, value: f64) {
        if !self.has_inputs {
            self.last_value = value;
            self.has_inputs = true;
        }
        let gain = value - self.last_value;
        if gain > 0.0 {
            self.average_gain.update_raw(gain);
            self.average_loss.update_raw(0.0);
        } else if gain < 0.0 {
            self.average_loss.update_raw(-gain);
            self.average_gain.update_raw(0.0);
        } else {
            self.average_loss.update_raw(0.0);
            self.average_gain.update_raw(0.0);
        }
        self.count = self.average_gain.count();
        if !self.initialized && self.average_loss.initialized() && self.average_gain.initialized() {
            self.initialized = true;
        }

        if self.average_loss.value() == 0.0 {
            self.value = self.rsi_max;
            self.last_value = value;
            return;
        }

        let rs = self.average_gain.value() / self.average_loss.value();
        self.value = self.rsi_max - (self.rsi_max / (1.0 + rs));
        self.last_value = value;

        if !self.initialized && self.count >= self.period {
            self.initialized = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::data::{Bar, QuoteTick, TradeTick};
    use rstest::rstest;

    use crate::{
        average::MovingAverageType, indicator::Indicator, momentum::rsi::RelativeStrengthIndex,
        stubs::*,
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
        assert_eq!(rsi_10.value, 1.0);
    }

    #[rstest]
    fn test_value_all_higher_inputs_returns_expected_value(mut rsi_10: RelativeStrengthIndex) {
        for i in 1..4 {
            rsi_10.update_raw(f64::from(i));
        }
        assert_eq!(rsi_10.value, 1.0);
    }

    #[rstest]
    fn test_value_with_all_lower_inputs_returns_expected_value(mut rsi_10: RelativeStrengthIndex) {
        for i in (1..4).rev() {
            rsi_10.update_raw(f64::from(i));
        }
        assert_eq!(rsi_10.value, 0.0);
    }

    #[rstest]
    fn test_value_with_various_input_returns_expected_value(mut rsi_10: RelativeStrengthIndex) {
        rsi_10.update_raw(3.0);
        rsi_10.update_raw(2.0);
        rsi_10.update_raw(5.0);
        rsi_10.update_raw(6.0);
        rsi_10.update_raw(7.0);
        rsi_10.update_raw(6.0);

        assert_eq!(rsi_10.value, 0.683_736_332_582_526_5);
    }

    #[rstest]
    fn test_value_at_returns_expected_value(mut rsi_10: RelativeStrengthIndex) {
        rsi_10.update_raw(3.0);
        rsi_10.update_raw(2.0);
        rsi_10.update_raw(5.0);
        rsi_10.update_raw(6.0);
        rsi_10.update_raw(7.0);
        rsi_10.update_raw(6.0);
        rsi_10.update_raw(6.0);
        rsi_10.update_raw(7.0);

        assert_eq!(rsi_10.value, 0.761_534_466_766_272_5);
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
        assert_eq!(rsi_10.count, 1);
        assert_eq!(rsi_10.value, 1.0);
    }

    #[rstest]
    fn test_handle_trade_tick(mut rsi_10: RelativeStrengthIndex, stub_trade: TradeTick) {
        rsi_10.handle_trade(&stub_trade);
        assert_eq!(rsi_10.count, 1);
        assert_eq!(rsi_10.value, 1.0);
    }

    #[rstest]
    fn test_handle_bar(mut rsi_10: RelativeStrengthIndex, bar_ethusdt_binance_minute_bid: Bar) {
        rsi_10.handle_bar(&bar_ethusdt_binance_minute_bid);
        assert_eq!(rsi_10.count, 1);
        assert_eq!(rsi_10.value, 1.0);
    }

    #[rstest]
    fn test_constant_inputs_initializes_and_value_max(mut rsi_10: RelativeStrengthIndex) {
        for _ in 0..12 {
            rsi_10.update_raw(5.0);
        }
        assert!(rsi_10.initialized);
        assert_eq!(rsi_10.value, 1.0);
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
        // The `ma_type` argument must reach the inner gain/loss averages, so distinct
        // moving-average types must produce distinct output on the same series.
        // Previously all types collapsed onto Exponential (see issue: v2 RSI ignores ma_type).
        let series = [
            44.34, 44.09, 44.15, 43.61, 44.33, 44.83, 45.10, 45.42, 45.84, 46.08, 45.89, 46.03,
            45.61, 46.28, 46.28,
        ];

        let wilder = run_rsi(&series, 14, MovingAverageType::Wilder);
        let simple = run_rsi(&series, 14, MovingAverageType::Simple);
        let exponential = run_rsi(&series, 14, MovingAverageType::Exponential);

        assert_ne!(wilder, simple);
        assert_ne!(wilder, exponential);
        assert_ne!(simple, exponential);
    }

    #[rstest]
    fn test_recovers_below_max_after_losses() {
        // Regression for the flat-1.0 defect (mirrors Cython fix #2703): once real down-moves
        // arrive, RSI must fall below `rsi_max` rather than staying pinned at 1.0 because
        // `last_value` was never advanced on zero-loss bars.
        let mut values: Vec<f64> = (1..=15).map(f64::from).collect();
        values.extend([14.0, 12.0, 9.0, 5.0, 2.0]);

        let value = run_rsi(&values, 14, MovingAverageType::Wilder);
        assert!(
            value < 1.0,
            "RSI should drop below rsi_max after losses, was {value}"
        );
    }

    #[rstest]
    fn test_wilder_golden_series() {
        // Golden reference: up 1..15 then down 14, 12, 9, 5, 2 with period 14, Wilder MA.
        // Expected Wilder RSI values (×100) after each down-move, per the published reference.
        let base: Vec<f64> = (1..=15).map(f64::from).collect();
        let downs = [14.0, 12.0, 9.0, 5.0, 2.0];
        let expected = [0.8935, 0.7269, 0.5586, 0.4192, 0.3489];

        let mut rsi = RelativeStrengthIndex::new(14, Some(MovingAverageType::Wilder));
        for &v in &base {
            rsi.update_raw(v);
        }

        for (i, &v) in downs.iter().enumerate() {
            rsi.update_raw(v);
            assert!(
                (rsi.value - expected[i]).abs() < 1e-4,
                "step {i}: expected {}, was {}",
                expected[i],
                rsi.value
            );
        }
    }
}
