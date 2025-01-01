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

use std::fmt::Display;

use nautilus_model::{
    data::{Bar, QuoteTick, TradeTick},
    enums::PriceType,
};

use crate::indicator::{Indicator, MovingAverage};

#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators")
)]
pub struct ExponentialMovingAverage {
    pub period: usize,
    pub price_type: PriceType,
    pub alpha: f64,
    pub value: f64,
    pub count: usize,
    pub initialized: bool,
    has_inputs: bool,
}

impl Display for ExponentialMovingAverage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.name(), self.period,)
    }
}

impl Indicator for ExponentialMovingAverage {
    fn name(&self) -> String {
        stringify!(ExponentialMovingAverage).to_string()
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

impl ExponentialMovingAverage {
    /// Creates a new [`ExponentialMovingAverage`] instance.
    #[must_use]
    pub fn new(period: usize, price_type: Option<PriceType>) -> Self {
        Self {
            period,
            price_type: price_type.unwrap_or(PriceType::Last),
            alpha: 2.0 / (period as f64 + 1.0),
            value: 0.0,
            count: 0,
            has_inputs: false,
            initialized: false,
        }
    }
}

impl MovingAverage for ExponentialMovingAverage {
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

        self.value = self.alpha.mul_add(value, (1.0 - self.alpha) * self.value);
        self.count += 1;

        // Initialization logic
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
    use nautilus_model::{
        data::{Bar, QuoteTick, TradeTick},
        enums::PriceType,
    };
    use rstest::rstest;

    use crate::{
        average::ema::ExponentialMovingAverage,
        indicator::{Indicator, MovingAverage},
        stubs::*,
    };

    #[rstest]
    fn test_ema_initialized(indicator_ema_10: ExponentialMovingAverage) {
        let ema = indicator_ema_10;
        let display_str = format!("{ema}");
        assert_eq!(display_str, "ExponentialMovingAverage(10)");
        assert_eq!(ema.period, 10);
        assert_eq!(ema.price_type, PriceType::Mid);
        assert_eq!(ema.alpha, 0.181_818_181_818_181_82);
        assert!(!ema.initialized);
    }

    #[rstest]
    fn test_one_value_input(indicator_ema_10: ExponentialMovingAverage) {
        let mut ema = indicator_ema_10;
        ema.update_raw(1.0);
        assert_eq!(ema.count, 1);
        assert_eq!(ema.value, 1.0);
    }

    #[rstest]
    fn test_ema_update_raw(indicator_ema_10: ExponentialMovingAverage) {
        let mut ema = indicator_ema_10;
        ema.update_raw(1.0);
        ema.update_raw(2.0);
        ema.update_raw(3.0);
        ema.update_raw(4.0);
        ema.update_raw(5.0);
        ema.update_raw(6.0);
        ema.update_raw(7.0);
        ema.update_raw(8.0);
        ema.update_raw(9.0);
        ema.update_raw(10.0);

        assert!(ema.has_inputs());
        assert!(ema.initialized());
        assert_eq!(ema.count, 10);
        assert_eq!(ema.value, 6.239_368_480_121_215_5);
    }

    #[rstest]
    fn test_reset(indicator_ema_10: ExponentialMovingAverage) {
        let mut ema = indicator_ema_10;
        ema.update_raw(1.0);
        assert_eq!(ema.count, 1);
        ema.reset();
        assert_eq!(ema.count, 0);
        assert_eq!(ema.value, 0.0);
        assert!(!ema.initialized);
    }

    #[rstest]
    fn test_handle_quote_tick_single(
        indicator_ema_10: ExponentialMovingAverage,
        stub_quote: QuoteTick,
    ) {
        let mut ema = indicator_ema_10;
        ema.handle_quote(&stub_quote);
        assert!(ema.has_inputs());
        assert_eq!(ema.value, 1501.0);
    }

    #[rstest]
    fn test_handle_quote_tick_multi(mut indicator_ema_10: ExponentialMovingAverage) {
        let tick1 = stub_quote("1500.0", "1502.0");
        let tick2 = stub_quote("1502.0", "1504.0");

        indicator_ema_10.handle_quote(&tick1);
        indicator_ema_10.handle_quote(&tick2);
        assert_eq!(indicator_ema_10.count, 2);
        assert_eq!(indicator_ema_10.value, 1_501.363_636_363_636_3);
    }

    #[rstest]
    fn test_handle_trade_tick(indicator_ema_10: ExponentialMovingAverage, stub_trade: TradeTick) {
        let mut ema = indicator_ema_10;
        ema.handle_trade(&stub_trade);
        assert!(ema.has_inputs());
        assert_eq!(ema.value, 1500.0);
    }

    #[rstest]
    fn handle_handle_bar(
        mut indicator_ema_10: ExponentialMovingAverage,
        bar_ethusdt_binance_minute_bid: Bar,
    ) {
        indicator_ema_10.handle_bar(&bar_ethusdt_binance_minute_bid);
        assert!(indicator_ema_10.has_inputs);
        assert!(!indicator_ema_10.initialized);
        assert_eq!(indicator_ema_10.value, 1522.0);
    }
}
