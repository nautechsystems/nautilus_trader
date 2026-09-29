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

use std::fmt::Display;

use nautilus_core::correctness::FAILED;
use nautilus_model::data::{Bar, QuoteTick, TradeTick};

use crate::{
    average::{MovingAverageFactory, MovingAverageType},
    indicator::{Indicator, MovingAverage},
    support::MAX_PERIOD,
};

/// Chande momentum oscillator.
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
pub struct ChandeMomentumOscillator {
    pub period: usize,
    pub ma_type: MovingAverageType,
    pub value: f64,
    pub count: usize,
    pub initialized: bool,
    previous_close: f64,
    average_gain: Box<dyn MovingAverage + Send + 'static>,
    average_loss: Box<dyn MovingAverage + Send + 'static>,
    has_inputs: bool,
}

impl Display for ChandeMomentumOscillator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.name(), self.period)
    }
}

impl Indicator for ChandeMomentumOscillator {
    fn name(&self) -> String {
        stringify!(ChandeMomentumOscillator).to_string()
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
        self.update_raw((&bar.close).into());
    }

    fn reset(&mut self) {
        self.value = 0.0;
        self.count = 0;
        self.has_inputs = false;
        self.initialized = false;
        self.previous_close = 0.0;
        self.average_gain.reset();
        self.average_loss.reset();
    }
}

impl ChandeMomentumOscillator {
    /// Creates a new [`ChandeMomentumOscillator`] instance.
    ///
    /// # Panics
    ///
    /// Panics if `period` is not positive (> 0).
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
        let ma_type = ma_type.unwrap_or(MovingAverageType::Simple);
        Ok(Self {
            period,
            ma_type,
            average_gain: MovingAverageFactory::create(ma_type, period),
            average_loss: MovingAverageFactory::create(ma_type, period),
            previous_close: 0.0,
            value: 0.0,
            count: 0,
            initialized: false,
            has_inputs: false,
        })
    }

    pub fn update_raw(&mut self, close: f64) {
        if !close.is_finite() {
            return;
        }

        self.count += 1;

        if !self.has_inputs {
            self.previous_close = close;
            self.has_inputs = true;
            return;
        }

        let change = close - self.previous_close;
        self.previous_close = close;
        self.average_gain.update_raw(change.max(0.0));
        self.average_loss.update_raw((-change).max(0.0));
        if !self.average_gain.initialized() || !self.average_loss.initialized() {
            return;
        }

        let gain = self.average_gain.value();
        let loss = self.average_loss.value();
        let total = gain + loss;
        self.value = if total == 0.0 {
            0.0
        } else {
            let value = 100.0 * (gain - loss) / total;
            if total.is_finite() && value.is_finite() {
                value
            } else if total.is_finite() {
                100.0 * (gain / total - loss / total)
            } else {
                let scale = gain.abs().max(loss.abs());
                let gain = gain / scale;
                let loss = loss / scale;
                100.0 * ((gain - loss) / (gain + loss))
            }
        };
        self.initialized = true;
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::data::{Bar, QuoteTick};
    use rstest::rstest;

    use crate::{
        average::MovingAverageType, indicator::Indicator, momentum::cmo::ChandeMomentumOscillator,
        stubs::*,
    };

    #[rstest]
    fn test_cmo_initialized(cmo_10: ChandeMomentumOscillator) {
        let display_str = format!("{cmo_10}");
        assert_eq!(display_str, "ChandeMomentumOscillator(10)");
        assert_eq!(cmo_10.period, 10);
        assert!(!cmo_10.initialized);
    }

    #[rstest]
    fn test_value_stays_within_bounds_when_gain_average_is_zero() {
        let mut cmo = ChandeMomentumOscillator::new(3, None);
        for value in [4.0, 3.0, 2.0, 1.0] {
            cmo.update_raw(value);
        }

        assert!(cmo.initialized());
        assert_eq!(cmo.value, -100.0);
    }

    #[rstest]
    fn test_initialized_with_required_inputs_returns_true(mut cmo_10: ChandeMomentumOscillator) {
        for i in 0..12 {
            cmo_10.update_raw(f64::from(i));
        }
        assert!(cmo_10.initialized);
    }

    #[rstest]
    fn test_value_all_higher_inputs_returns_expected_value() {
        let mut cmo_10 = ChandeMomentumOscillator::new(10, None);
        for value in 1..=11 {
            cmo_10.update_raw(f64::from(value));
        }

        assert_eq!(cmo_10.value, 100.0);
        assert!(cmo_10.initialized());
    }

    #[rstest]
    fn test_value_with_one_input_returns_expected_value(mut cmo_10: ChandeMomentumOscillator) {
        cmo_10.update_raw(1.00000);
        assert_eq!(cmo_10.value, 0.0);
    }

    #[rstest]
    fn test_reset(mut cmo_10: ChandeMomentumOscillator) {
        cmo_10.update_raw(1.00020);
        cmo_10.update_raw(1.00030);
        cmo_10.update_raw(1.00050);
        cmo_10.reset();
        assert!(!cmo_10.initialized());
        assert_eq!(cmo_10.count, 0);
        assert_eq!(cmo_10.value, 0.0);
        assert_eq!(cmo_10.previous_close, 0.0);
    }

    #[rstest]
    fn test_handle_quote_tick(mut cmo_10: ChandeMomentumOscillator, stub_quote: QuoteTick) {
        cmo_10.handle_quote(&stub_quote).unwrap();
        assert_eq!(cmo_10.count, 0);
        assert_eq!(cmo_10.value, 0.0);
    }

    #[rstest]
    fn test_handle_bar(mut cmo_10: ChandeMomentumOscillator, bar_ethusdt_binance_minute_bid: Bar) {
        cmo_10.handle_bar(&bar_ethusdt_binance_minute_bid);
        assert_eq!(cmo_10.count, 1);
        assert_eq!(cmo_10.value, 0.0);
    }

    #[rstest]
    fn test_ma_type_affects_value() {
        let mut cmo_sma = ChandeMomentumOscillator::new(3, Some(MovingAverageType::Simple));
        let mut cmo_wilder = ChandeMomentumOscillator::new(3, Some(MovingAverageType::Wilder));
        let prices = [1.0, 2.0, 3.0, 2.5, 3.5];
        for price in prices {
            cmo_sma.update_raw(price);
            cmo_wilder.update_raw(price);
        }
        assert_ne!(cmo_sma.value, cmo_wilder.value);
    }

    #[rstest]
    fn test_count_increments(mut cmo_10: ChandeMomentumOscillator) {
        for i in 0..5 {
            cmo_10.update_raw(f64::from(i));
        }
        assert_eq!(cmo_10.count, 5);
    }

    #[rstest]
    fn test_reset_resets_inner_mas() {
        let mut cmo = ChandeMomentumOscillator::new(3, None);
        for value in [1.0, 2.0, 3.0, 4.0] {
            cmo.update_raw(value);
        }
        assert!(cmo.average_gain.initialized());
        assert!(cmo.average_loss.initialized());

        cmo.reset();

        assert!(!cmo.average_gain.initialized());
        assert!(!cmo.average_loss.initialized());
        assert_eq!(cmo.average_gain.count(), 0);
        assert_eq!(cmo.average_loss.count(), 0);
    }

    #[rstest]
    #[should_panic]
    fn test_invalid_period_panics() {
        let _ = ChandeMomentumOscillator::new(0, None);
    }

    #[rstest]
    fn test_ma_type_propagation() {
        let cmo = ChandeMomentumOscillator::new(5, Some(MovingAverageType::Simple));
        assert_eq!(cmo.ma_type, MovingAverageType::Simple);
    }

    #[rstest]
    fn test_zero_divisor_returns_zero() {
        let mut cmo = ChandeMomentumOscillator::new(3, None);
        for _ in 0..5 {
            cmo.update_raw(100.0);
        }
        assert!(cmo.initialized);
        assert_eq!(cmo.value, 0.0);
    }

    #[rstest]
    fn test_random_walk_values_within_bounds() {
        let prices = [
            100.0, 100.5, 99.8, 100.3, 101.0, 100.7, 101.5, 101.2, 100.6, 101.1, 100.9, 101.4,
            100.8, 101.2, 100.6, 100.9, 101.3, 101.0, 100.5, 101.1, 100.7, 101.4, 100.9, 100.8,
            101.2, 100.6, 100.9, 101.3, 101.0, 100.5,
        ];
        let mut cmo = ChandeMomentumOscillator::new(10, None);
        for price in prices {
            cmo.update_raw(price);
        }
        assert!(cmo.initialized);
        assert!(cmo.value <= 100.0 && cmo.value >= -100.0);
    }
}
