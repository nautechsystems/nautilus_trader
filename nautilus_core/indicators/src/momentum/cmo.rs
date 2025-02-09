// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_model::data::{Bar, QuoteTick, TradeTick};

use crate::{
    average::{MovingAverageFactory, MovingAverageType},
    indicator::{Indicator, MovingAverage},
};

#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators", unsendable)
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

    fn handle_quote(&mut self, _quote: &QuoteTick) {
        // Function body intentionally left blank.
    }

    fn handle_trade(&mut self, _trade: &TradeTick) {
        // Function body intentionally left blank.
    }

    fn handle_bar(&mut self, bar: &Bar) {
        self.update_raw((&bar.close).into());
    }

    fn reset(&mut self) {
        self.value = 0.0;
        self.count = 0;
        self.has_inputs = false;
        self.initialized = false;
        self.previous_close = 0.0;
    }
}

impl ChandeMomentumOscillator {
    /// Creates a new [`ChandeMomentumOscillator`] instance.
    #[must_use]
    pub fn new(period: usize, ma_type: Option<MovingAverageType>) -> Self {
        Self {
            period,
            ma_type: ma_type.unwrap_or(MovingAverageType::Wilder),
            average_gain: MovingAverageFactory::create(MovingAverageType::Wilder, period),
            average_loss: MovingAverageFactory::create(MovingAverageType::Wilder, period),
            previous_close: 0.0,
            value: 0.0,
            count: 0,
            initialized: false,
            has_inputs: false,
        }
    }

    pub fn update_raw(&mut self, close: f64) {
        if !self.has_inputs {
            self.previous_close = close;
            self.has_inputs = true;
        }

        let gain: f64 = close - self.previous_close;
        if gain > 0.0 {
            self.average_gain.update_raw(gain);
            self.average_loss.update_raw(0.0);
        } else if gain < 0.0 {
            self.average_gain.update_raw(0.0);
            self.average_loss.update_raw(-gain);
        } else {
            self.average_gain.update_raw(0.0);
            self.average_loss.update_raw(0.0);
        }

        if !self.initialized && self.average_gain.initialized() && self.average_loss.initialized() {
            self.initialized = true;
        }
        if self.initialized {
            let divisor = self.average_gain.value() + self.average_loss.value();
            if divisor == 0.0 {
                self.value = 0.0;
            } else {
                self.value =
                    100.0 * (self.average_gain.value() - self.average_loss.value()) / divisor;
            }
        }
        self.previous_close = close;
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_model::data::{Bar, QuoteTick};
    use rstest::rstest;

    use crate::{indicator::Indicator, momentum::cmo::ChandeMomentumOscillator, stubs::*};

    #[rstest]
    fn test_cmo_initialized(cmo_10: ChandeMomentumOscillator) {
        let display_str = format!("{cmo_10}");
        assert_eq!(display_str, "ChandeMomentumOscillator(10)");
        assert_eq!(cmo_10.period, 10);
        assert!(!cmo_10.initialized);
    }

    #[rstest]
    fn test_initialized_with_required_inputs_returns_true(mut cmo_10: ChandeMomentumOscillator) {
        for i in 0..12 {
            cmo_10.update_raw(f64::from(i));
        }
        assert!(cmo_10.initialized);
    }

    #[rstest]
    fn test_value_all_higher_inputs_returns_expected_value(mut cmo_10: ChandeMomentumOscillator) {
        cmo_10.update_raw(109.93);
        cmo_10.update_raw(110.0);
        cmo_10.update_raw(109.77);
        cmo_10.update_raw(109.96);
        cmo_10.update_raw(110.29);
        cmo_10.update_raw(110.53);
        cmo_10.update_raw(110.27);
        cmo_10.update_raw(110.21);
        cmo_10.update_raw(110.06);
        cmo_10.update_raw(110.19);
        cmo_10.update_raw(109.83);
        cmo_10.update_raw(109.9);
        cmo_10.update_raw(110.0);
        cmo_10.update_raw(110.03);
        cmo_10.update_raw(110.13);
        cmo_10.update_raw(109.95);
        cmo_10.update_raw(109.75);
        cmo_10.update_raw(110.15);
        cmo_10.update_raw(109.9);
        cmo_10.update_raw(110.04);
        assert_eq!(cmo_10.value, 2.089_629_456_238_705_4);
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
        cmo_10.handle_quote(&stub_quote);
        assert_eq!(cmo_10.count, 0);
        assert_eq!(cmo_10.value, 0.0);
    }

    #[rstest]
    fn test_handle_bar(mut cmo_10: ChandeMomentumOscillator, bar_ethusdt_binance_minute_bid: Bar) {
        cmo_10.handle_bar(&bar_ethusdt_binance_minute_bid);
        assert_eq!(cmo_10.count, 0);
        assert_eq!(cmo_10.value, 0.0);
    }
}
