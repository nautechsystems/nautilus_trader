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

use std::fmt::{Display, Formatter};

use nautilus_model::{
    data::{Bar, QuoteTick, TradeTick},
    enums::PriceType,
};

use crate::{
    average::ema::ExponentialMovingAverage,
    indicator::{Indicator, MovingAverage},
};

/// The Double Exponential Moving Average attempts to a smoother average with less
/// lag than the normal Exponential Moving Average (EMA)
#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators")
)]
pub struct DoubleExponentialMovingAverage {
    /// The rolling window period for the indicator (> 0).
    pub period: usize,
    /// The price type used for calculations.
    pub price_type: PriceType,
    /// The last indicator value.
    pub value: f64,
    /// The input count for the indicator.
    pub count: usize,
    pub initialized: bool,
    has_inputs: bool,
    ema1: ExponentialMovingAverage,
    ema2: ExponentialMovingAverage,
}

impl Display for DoubleExponentialMovingAverage {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "DoubleExponentialMovingAverage(period={})", self.period)
    }
}

impl Indicator for DoubleExponentialMovingAverage {
    fn name(&self) -> String {
        stringify!(DoubleExponentialMovingAverage).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.has_inputs
    }
    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_quote(&mut self, quote: &QuoteTick) {
        self.update_raw(quote.extract_price(self.price_type).into());
    }

    fn handle_trade(&mut self, trade: &TradeTick) {
        self.update_raw((&trade.price).into());
    }

    fn handle_bar(&mut self, bar: &Bar) {
        self.update_raw((&bar.close).into());
    }

    fn reset(&mut self) {
        self.value = 0.0;
        self.count = 0;
        self.has_inputs = false;
        self.initialized = false;
    }
}

impl DoubleExponentialMovingAverage {
    /// Creates a new [`DoubleExponentialMovingAverage`] instance.
    #[must_use]
    pub fn new(period: usize, price_type: Option<PriceType>) -> Self {
        Self {
            period,
            price_type: price_type.unwrap_or(PriceType::Last),
            value: 0.0,
            count: 0,
            has_inputs: false,
            initialized: false,
            ema1: ExponentialMovingAverage::new(period, price_type),
            ema2: ExponentialMovingAverage::new(period, price_type),
        }
    }
}

impl MovingAverage for DoubleExponentialMovingAverage {
    fn value(&self) -> f64 {
        self.value
    }

    fn count(&self) -> usize {
        self.count
    }
    fn update_raw(&mut self, value: f64) {
        if !self.has_inputs {
            self.has_inputs = true;
            self.value = value;
        }
        self.ema1.update_raw(value);
        self.ema2.update_raw(self.ema1.value);

        self.value = 2.0f64.mul_add(self.ema1.value, -self.ema2.value);
        self.count += 1;

        if !self.initialized && self.count >= self.period {
            self.initialized = true;
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_model::data::{Bar, QuoteTick, TradeTick};
    use rstest::rstest;

    use crate::{
        average::dema::DoubleExponentialMovingAverage,
        indicator::{Indicator, MovingAverage},
        stubs::*,
    };

    #[rstest]
    fn test_dema_initialized(indicator_dema_10: DoubleExponentialMovingAverage) {
        let display_str = format!("{indicator_dema_10}");
        assert_eq!(display_str, "DoubleExponentialMovingAverage(period=10)");
        assert_eq!(indicator_dema_10.period, 10);
        assert!(!indicator_dema_10.initialized);
        assert!(!indicator_dema_10.has_inputs);
    }

    #[rstest]
    fn test_value_with_one_input(mut indicator_dema_10: DoubleExponentialMovingAverage) {
        indicator_dema_10.update_raw(1.0);
        assert_eq!(indicator_dema_10.value, 1.0);
    }

    #[rstest]
    fn test_value_with_three_inputs(mut indicator_dema_10: DoubleExponentialMovingAverage) {
        indicator_dema_10.update_raw(1.0);
        indicator_dema_10.update_raw(2.0);
        indicator_dema_10.update_raw(3.0);
        assert_eq!(indicator_dema_10.value, 1.904_583_020_285_499_4);
    }

    #[rstest]
    fn test_initialized_with_required_input(mut indicator_dema_10: DoubleExponentialMovingAverage) {
        for i in 1..10 {
            indicator_dema_10.update_raw(f64::from(i));
        }
        assert!(!indicator_dema_10.initialized);
        indicator_dema_10.update_raw(10.0);
        assert!(indicator_dema_10.initialized);
    }

    #[rstest]
    fn test_handle_quote(
        mut indicator_dema_10: DoubleExponentialMovingAverage,
        stub_quote: QuoteTick,
    ) {
        indicator_dema_10.handle_quote(&stub_quote);
        assert_eq!(indicator_dema_10.value, 1501.0);
    }

    #[rstest]
    fn test_handle_trade(
        mut indicator_dema_10: DoubleExponentialMovingAverage,
        stub_trade: TradeTick,
    ) {
        indicator_dema_10.handle_trade(&stub_trade);
        assert_eq!(indicator_dema_10.value, 1500.0);
    }

    #[rstest]
    fn test_handle_bar(
        mut indicator_dema_10: DoubleExponentialMovingAverage,
        bar_ethusdt_binance_minute_bid: Bar,
    ) {
        indicator_dema_10.handle_bar(&bar_ethusdt_binance_minute_bid);
        assert_eq!(indicator_dema_10.value, 1522.0);
        assert!(indicator_dema_10.has_inputs);
        assert!(!indicator_dema_10.initialized);
    }

    #[rstest]
    fn test_reset(mut indicator_dema_10: DoubleExponentialMovingAverage) {
        indicator_dema_10.update_raw(1.0);
        assert_eq!(indicator_dema_10.count, 1);
        indicator_dema_10.reset();
        assert_eq!(indicator_dema_10.value, 0.0);
        assert_eq!(indicator_dema_10.count, 0);
        assert!(!indicator_dema_10.has_inputs);
        assert!(!indicator_dema_10.initialized);
    }
}
