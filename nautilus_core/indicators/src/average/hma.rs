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

use std::fmt::{Display, Formatter};

use anyhow::Result;
use nautilus_model::{
    data::{bar::Bar, quote::QuoteTick, trade::TradeTick},
    enums::PriceType,
};
use pyo3::prelude::*;

use crate::{
    average::wma::WeightedMovingAverage,
    indicator::{Indicator, MovingAverage},
};

/// An indicator which calculates a Hull Moving Average (HMA) across a rolling
/// window. The HMA, developed by Alan Hull, is an extremely fast and smooth
/// moving average.
#[repr(C)]
#[derive(Debug)]
#[pyclass(module = "nautilus_trader.core.nautilus.pyo3.indicators")]
pub struct HullMovingAverage {
    pub period: usize,
    pub price_type: PriceType,
    pub value: f64,
    pub count: usize,
    pub is_initialized: bool,
    has_inputs: bool,
    _ma1: WeightedMovingAverage,
    _ma2: WeightedMovingAverage,
    _ma3: WeightedMovingAverage,
}

impl Display for HullMovingAverage {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.name(), self.period)
    }
}

impl Indicator for HullMovingAverage {
    fn name(&self) -> String {
        stringify!(HullMovingAverage).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.has_inputs
    }

    fn is_initialized(&self) -> bool {
        self.is_initialized
    }

    fn handle_quote_tick(&mut self, tick: &QuoteTick) {
        self.update_raw(tick.extract_price(self.price_type).into());
    }

    fn handle_trade_tick(&mut self, tick: &TradeTick) {
        self.update_raw((&tick.price).into());
    }

    fn handle_bar(&mut self, bar: &Bar) {
        self.update_raw((&bar.close).into());
    }

    fn reset(&mut self) {
        self.value = 0.0;
        self._ma1.reset();
        self._ma2.reset();
        self._ma3.reset();
        self.count = 0;
        self.has_inputs = false;
        self.is_initialized = false;
    }
}

fn _get_weights(size: usize) -> Vec<f64> {
    let mut weights: Vec<f64> = (1..size + 1).map(|x| x as f64).collect();
    let divisor: f64 = weights.iter().sum();
    weights = weights.iter().map(|x| x / divisor).collect();
    weights
}

impl HullMovingAverage {
    pub fn new(period: usize, price_type: Option<PriceType>) -> Result<Self> {
        let period_halved = period / 2;
        let period_sqrt = (period as f64).sqrt() as usize;

        let w1 = _get_weights(period_halved);
        let w2 = _get_weights(period);
        let w3 = _get_weights(period_sqrt);

        let _ma1 = WeightedMovingAverage::new(period_halved, w1, price_type)?;
        let _ma2 = WeightedMovingAverage::new(period, w2, price_type)?;
        let _ma3 = WeightedMovingAverage::new(period_sqrt, w3, price_type)?;

        Ok(Self {
            period,
            price_type: price_type.unwrap_or(PriceType::Last),
            value: 0.0,
            count: 0,
            has_inputs: false,
            is_initialized: false,
            _ma1,
            _ma2,
            _ma3,
        })
    }
}

impl MovingAverage for HullMovingAverage {
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

        self._ma1.update_raw(value);
        self._ma2.update_raw(value);
        self._ma3
            .update_raw(2.0f64.mul_add(self._ma1.value, -self._ma2.value));

        self.value = self._ma3.value;
        self.count += 1;

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

    use crate::{
        average::hma::HullMovingAverage,
        indicator::{Indicator, MovingAverage},
        stubs::*,
    };

    #[rstest]
    fn test_hma_initialized(indicator_hma_10: HullMovingAverage) {
        let display_str = format!("{indicator_hma_10}");
        assert_eq!(display_str, "HullMovingAverage(10)");
        assert_eq!(indicator_hma_10.period, 10);
        assert!(!indicator_hma_10.is_initialized);
        assert!(!indicator_hma_10.has_inputs);
    }

    #[rstest]
    fn test_initialized_with_required_input(mut indicator_hma_10: HullMovingAverage) {
        for i in 1..10 {
            indicator_hma_10.update_raw(f64::from(i));
        }
        assert!(!indicator_hma_10.is_initialized);
        indicator_hma_10.update_raw(10.0);
        assert!(indicator_hma_10.is_initialized);
    }

    #[rstest]
    fn test_value_with_one_input(mut indicator_hma_10: HullMovingAverage) {
        indicator_hma_10.update_raw(1.0);
        assert_eq!(indicator_hma_10.value, 1.0);
    }

    #[rstest]
    fn test_value_with_three_inputs(mut indicator_hma_10: HullMovingAverage) {
        indicator_hma_10.update_raw(1.0);
        indicator_hma_10.update_raw(2.0);
        indicator_hma_10.update_raw(3.0);
        assert_eq!(indicator_hma_10.value, 1.824_561_403_508_772);
    }

    #[rstest]
    fn test_value_with_ten_inputs(mut indicator_hma_10: HullMovingAverage) {
        indicator_hma_10.update_raw(1.00000);
        indicator_hma_10.update_raw(1.00010);
        indicator_hma_10.update_raw(1.00020);
        indicator_hma_10.update_raw(1.00030);
        indicator_hma_10.update_raw(1.00040);
        indicator_hma_10.update_raw(1.00050);
        indicator_hma_10.update_raw(1.00040);
        indicator_hma_10.update_raw(1.00030);
        indicator_hma_10.update_raw(1.00020);
        indicator_hma_10.update_raw(1.00010);
        indicator_hma_10.update_raw(1.00000);
        assert_eq!(indicator_hma_10.value, 1.000_140_392_817_059_8);
    }

    #[rstest]
    fn test_handle_quote_tick(mut indicator_hma_10: HullMovingAverage, quote_tick: QuoteTick) {
        indicator_hma_10.handle_quote_tick(&quote_tick);
        assert_eq!(indicator_hma_10.value, 1501.0);
    }

    #[rstest]
    fn test_handle_trade_tick(mut indicator_hma_10: HullMovingAverage, trade_tick: TradeTick) {
        indicator_hma_10.handle_trade_tick(&trade_tick);
        assert_eq!(indicator_hma_10.value, 1500.0);
    }

    #[rstest]
    fn test_handle_bar(
        mut indicator_hma_10: HullMovingAverage,
        bar_ethusdt_binance_minute_bid: Bar,
    ) {
        indicator_hma_10.handle_bar(&bar_ethusdt_binance_minute_bid);
        assert_eq!(indicator_hma_10.value, 1522.0);
        assert!(indicator_hma_10.has_inputs);
        assert!(!indicator_hma_10.is_initialized);
    }

    #[rstest]
    fn test_reset(mut indicator_hma_10: HullMovingAverage) {
        indicator_hma_10.update_raw(1.0);
        assert_eq!(indicator_hma_10.count, 1);
        assert_eq!(indicator_hma_10.value, 1.0);
        assert_eq!(indicator_hma_10._ma1.value, 1.0);
        assert_eq!(indicator_hma_10._ma2.value, 1.0);
        assert_eq!(indicator_hma_10._ma3.value, 1.0);
        indicator_hma_10.reset();
        assert_eq!(indicator_hma_10.value, 0.0);
        assert_eq!(indicator_hma_10.count, 0);
        assert_eq!(indicator_hma_10._ma1.value, 0.0);
        assert_eq!(indicator_hma_10._ma2.value, 0.0);
        assert_eq!(indicator_hma_10._ma3.value, 0.0);
        assert!(!indicator_hma_10.has_inputs);
        assert!(!indicator_hma_10.is_initialized);
    }
}
