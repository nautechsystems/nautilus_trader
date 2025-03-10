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
pub struct SimpleMovingAverage {
    pub period: usize,
    pub price_type: PriceType,
    pub value: f64,
    pub count: usize,
    pub inputs: Vec<f64>,
    pub initialized: bool,
}

impl Display for SimpleMovingAverage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.name(), self.period,)
    }
}

impl Indicator for SimpleMovingAverage {
    fn name(&self) -> String {
        stringify!(SimpleMovingAverage).to_string()
    }

    fn has_inputs(&self) -> bool {
        !self.inputs.is_empty()
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
        self.inputs.clear();
        self.initialized = false;
    }
}

impl SimpleMovingAverage {
    /// Creates a new [`SimpleMovingAverage`] instance.
    #[must_use]
    pub fn new(period: usize, price_type: Option<PriceType>) -> Self {
        Self {
            period,
            price_type: price_type.unwrap_or(PriceType::Last),
            value: 0.0,
            count: 0,
            inputs: Vec::with_capacity(period),
            initialized: false,
        }
    }
}

impl MovingAverage for SimpleMovingAverage {
    fn value(&self) -> f64 {
        self.value
    }

    fn count(&self) -> usize {
        self.count
    }
    fn update_raw(&mut self, value: f64) {
        if self.inputs.len() == self.period {
            self.inputs.remove(0);
            self.count -= 1;
        }
        self.inputs.push(value);
        self.count += 1;
        let sum = self.inputs.iter().sum::<f64>();
        self.value = sum / self.count as f64;

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
        data::{QuoteTick, TradeTick},
        enums::PriceType,
    };
    use rstest::rstest;

    use crate::{
        average::sma::SimpleMovingAverage,
        indicator::{Indicator, MovingAverage},
        stubs::*,
    };

    #[rstest]
    fn test_sma_initialized(indicator_sma_10: SimpleMovingAverage) {
        let display_str = format!("{indicator_sma_10}");
        assert_eq!(display_str, "SimpleMovingAverage(10)");
        assert_eq!(indicator_sma_10.period, 10);
        assert_eq!(indicator_sma_10.price_type, PriceType::Mid);
        assert_eq!(indicator_sma_10.value, 0.0);
        assert_eq!(indicator_sma_10.count, 0);
    }

    #[rstest]
    fn test_sma_update_raw_exact_period(indicator_sma_10: SimpleMovingAverage) {
        let mut sma = indicator_sma_10;
        sma.update_raw(1.0);
        sma.update_raw(2.0);
        sma.update_raw(3.0);
        sma.update_raw(4.0);
        sma.update_raw(5.0);
        sma.update_raw(6.0);
        sma.update_raw(7.0);
        sma.update_raw(8.0);
        sma.update_raw(9.0);
        sma.update_raw(10.0);

        assert!(sma.has_inputs());
        assert!(sma.initialized());
        assert_eq!(sma.count, 10);
        assert_eq!(sma.value, 5.5);
    }

    #[rstest]
    fn test_reset(indicator_sma_10: SimpleMovingAverage) {
        let mut sma = indicator_sma_10;
        sma.update_raw(1.0);
        assert_eq!(sma.count, 1);
        sma.reset();
        assert_eq!(sma.count, 0);
        assert_eq!(sma.value, 0.0);
        assert!(!sma.initialized);
    }

    #[rstest]
    fn test_handle_quote_tick_single(indicator_sma_10: SimpleMovingAverage, stub_quote: QuoteTick) {
        let mut sma = indicator_sma_10;
        sma.handle_quote(&stub_quote);
        assert_eq!(sma.count, 1);
        assert_eq!(sma.value, 1501.0);
    }

    #[rstest]
    fn test_handle_quote_tick_multi(indicator_sma_10: SimpleMovingAverage) {
        let mut sma = indicator_sma_10;
        let tick1 = stub_quote("1500.0", "1502.0");
        let tick2 = stub_quote("1502.0", "1504.0");

        sma.handle_quote(&tick1);
        sma.handle_quote(&tick2);
        assert_eq!(sma.count, 2);
        assert_eq!(sma.value, 1502.0);
    }

    #[rstest]
    fn test_handle_trade_tick(indicator_sma_10: SimpleMovingAverage, stub_trade: TradeTick) {
        let mut sma = indicator_sma_10;
        sma.handle_trade(&stub_trade);
        assert_eq!(sma.count, 1);
        assert_eq!(sma.value, 1500.0);
    }
}
