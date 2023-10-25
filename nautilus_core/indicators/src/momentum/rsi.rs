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

use std::fmt::{Debug, Display};

use anyhow::Result;
use nautilus_model::{
    data::{bar::Bar, quote::QuoteTick, trade::TradeTick},
    enums::PriceType,
};
use pyo3::prelude::*;

use crate::{
    average::{MovingAverageFactory, MovingAverageType},
    indicator::{Indicator, MovingAverage},
};

/// An indicator which calculates a relative strength index (RSI) across a rolling window.
#[repr(C)]
#[derive(Debug)]
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators")]
pub struct RelativeStrengthIndex {
    pub period: usize,
    pub ma_type: MovingAverageType,
    pub value: f64,
    pub count: usize,
    pub is_initialized: bool,
    _has_inputs: bool,
    _last_value: f64,
    _average_gain: Box<dyn MovingAverage + Send + 'static>,
    _average_loss: Box<dyn MovingAverage + Send + 'static>,
    _rsi_max: f64,
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
        self._has_inputs
    }

    fn is_initialized(&self) -> bool {
        self.is_initialized
    }

    fn handle_quote_tick(&mut self, tick: &QuoteTick) {
        self.update_raw(tick.extract_price(PriceType::Mid).into());
    }

    fn handle_trade_tick(&mut self, tick: &TradeTick) {
        self.update_raw((tick.price).into());
    }

    fn handle_bar(&mut self, bar: &Bar) {
        self.update_raw((&bar.close).into());
    }

    fn reset(&mut self) {
        self.value = 0.0;
        self._last_value = 0.0;
        self.count = 0;
        self._has_inputs = false;
        self.is_initialized = false;
    }
}

impl RelativeStrengthIndex {
    pub fn new(period: usize, ma_type: Option<MovingAverageType>) -> Result<Self> {
        Ok(Self {
            period,
            ma_type: ma_type.unwrap_or(MovingAverageType::Exponential),
            value: 0.0,
            _last_value: 0.0,
            count: 0,
            // inputs: Vec::new(),
            _has_inputs: false,
            _average_gain: MovingAverageFactory::create(MovingAverageType::Exponential, period),
            _average_loss: MovingAverageFactory::create(MovingAverageType::Exponential, period),
            _rsi_max: 1.0,
            is_initialized: false,
        })
    }

    pub fn update_raw(&mut self, value: f64) {
        if !self._has_inputs {
            self._last_value = value;
            self._has_inputs = true
        }
        let gain = value - self._last_value;
        if gain > 0.0 {
            self._average_gain.update_raw(gain);
            self._average_loss.update_raw(0.0);
        } else if gain < 0.0 {
            self._average_loss.update_raw(-gain);
            self._average_gain.update_raw(0.0);
        } else {
            self._average_loss.update_raw(0.0);
            self._average_gain.update_raw(0.0);
        }
        // init count from average gain MA
        self.count = self._average_gain.count();
        if !self.is_initialized
            && self._average_loss.is_initialized()
            && self._average_gain.is_initialized()
        {
            self.is_initialized = true;
        }

        if self._average_loss.value() == 0.0 {
            self.value = self._rsi_max;
            return;
        }

        let rs = self._average_gain.value() / self._average_loss.value();
        self.value = self._rsi_max - (self._rsi_max / (1.0 + rs));
        self._last_value = value;

        if !self.is_initialized && self.count >= self.period {
            self.is_initialized = true;
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_model::data::{bar::Bar, quote::QuoteTick, trade::TradeTick};
    use rstest::rstest;

    use crate::{indicator::Indicator, momentum::rsi::RelativeStrengthIndex, stubs::*};

    #[rstest]
    fn test_rsi_initialized(rsi_10: RelativeStrengthIndex) {
        let display_str = format!("{}", rsi_10);
        assert_eq!(display_str, "RelativeStrengthIndex(10,EXPONENTIAL)");
        assert_eq!(rsi_10.period, 10);
        assert_eq!(rsi_10.is_initialized, false)
    }

    #[rstest]
    fn test_initialized_with_required_inputs_returns_true(mut rsi_10: RelativeStrengthIndex) {
        for i in 0..12 {
            rsi_10.update_raw(i as f64);
        }
        assert_eq!(rsi_10.is_initialized, true)
    }

    #[rstest]
    fn test_value_with_one_input_returns_expected_value(mut rsi_10: RelativeStrengthIndex) {
        rsi_10.update_raw(1.0);
        assert_eq!(rsi_10.value, 1.0)
    }

    #[rstest]
    fn test_value_all_higher_inputs_returns_expected_value(mut rsi_10: RelativeStrengthIndex) {
        for i in 1..4 {
            rsi_10.update_raw(i as f64);
        }
        assert_eq!(rsi_10.value, 1.0)
    }

    #[rstest]
    fn test_value_with_all_lower_inputs_returns_expected_value(mut rsi_10: RelativeStrengthIndex) {
        for i in (1..4).rev() {
            rsi_10.update_raw(i as f64);
        }
        assert_eq!(rsi_10.value, 0.0)
    }

    #[rstest]
    fn test_value_with_various_input_returns_expected_value(mut rsi_10: RelativeStrengthIndex) {
        rsi_10.update_raw(3.0);
        rsi_10.update_raw(2.0);
        rsi_10.update_raw(5.0);
        rsi_10.update_raw(6.0);
        rsi_10.update_raw(7.0);
        rsi_10.update_raw(6.0);

        assert_eq!(rsi_10.value, 0.6837363325825265)
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

        assert_eq!(rsi_10.value, 0.7615344667662725);
    }

    #[rstest]
    fn test_reset(mut rsi_10: RelativeStrengthIndex) {
        rsi_10.update_raw(1.0);
        rsi_10.update_raw(2.0);
        rsi_10.reset();
        assert_eq!(rsi_10.is_initialized(), false);
        assert_eq!(rsi_10.count, 0)
    }

    #[rstest]
    fn test_handle_quote_tick(mut rsi_10: RelativeStrengthIndex, quote_tick: QuoteTick) {
        rsi_10.handle_quote_tick(&quote_tick);
        assert_eq!(rsi_10.count, 1);
        assert_eq!(rsi_10.value, 1.0)
    }

    #[rstest]
    fn test_handle_trade_tick(mut rsi_10: RelativeStrengthIndex, trade_tick: TradeTick) {
        rsi_10.handle_trade_tick(&trade_tick);
        assert_eq!(rsi_10.count, 1);
        assert_eq!(rsi_10.value, 1.0)
    }

    #[rstest]
    fn test_handle_bar(mut rsi_10: RelativeStrengthIndex, bar_ethusdt_binance_minute_bid: Bar) {
        rsi_10.handle_bar(&bar_ethusdt_binance_minute_bid);
        assert_eq!(rsi_10.count, 1);
        assert_eq!(rsi_10.value, 1.0)
    }
}
