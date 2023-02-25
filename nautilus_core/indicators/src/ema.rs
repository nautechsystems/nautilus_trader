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

use pyo3::prelude::*;

use nautilus_model::{
    data::{
        bar::Bar,
        tick::{QuoteTick, TradeTick},
    },
    enums::PriceType,
};

use crate::Indicator;

#[pyclass]
#[derive(Debug)]
pub struct ExponentialMovingAverage {
    pub period: usize,
    pub price_type: PriceType,
    pub alpha: f64,
    pub value: f64,
    pub count: usize,
    _has_inputs: bool,
    _is_initialized: bool,
}

#[pymethods]
impl ExponentialMovingAverage {
    #[new]
    pub fn new(period: usize, price_type: Option<PriceType>) -> Self {
        ExponentialMovingAverage {
            period,
            price_type: price_type.unwrap_or(PriceType::Last),
            alpha: 2.0 / (period as f64 + 1.0),
            value: 0.0,
            count: 0,
            _has_inputs: false,
            _is_initialized: false,
        }
    }

    pub fn update_raw(&mut self, value: f64) {
        if !self._has_inputs {
            self._has_inputs = true;
            self.value = value;
        }

        self.value = self.alpha * value + ((1.0 - self.alpha) * self.value);
        self.count += 1;

        // Initialization logic
        if !self._is_initialized && self.count >= self.period {
            self._is_initialized = true;
        }
    }
}

impl Indicator for ExponentialMovingAverage {
    fn has_inputs(&self) -> bool {
        self._has_inputs
    }

    fn is_initialized(&self) -> bool {
        self._is_initialized
    }

    fn handle_quote_tick(&mut self, tick: &QuoteTick) {
        self.update_raw(tick.extract_price(self.price_type).into())
    }

    fn handle_trade_tick(&mut self, tick: &TradeTick) {
        self.update_raw((&tick.price).into())
    }

    fn handle_bar(&mut self, bar: &Bar) {
        self.update_raw((&bar.close).into())
    }

    fn reset(&mut self) {
        self.value = 0.0;
        self.count = 0;
        self._has_inputs = false;
        self._is_initialized = false;
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ema_initialized() {
        let ema = ExponentialMovingAverage::new(20, Some(PriceType::Mid));
        let display_str = format!("{:?}", ema);
        assert_eq!(display_str, "ExponentialMovingAverage { period: 20, price_type: Mid, alpha: 0.09523809523809523, value: 0.0, count: 0, _has_inputs: false, _is_initialized: false }");
    }

    #[test]
    fn test_ema_update_raw() {
        let mut ema = ExponentialMovingAverage::new(3, Some(PriceType::Mid));
        ema.update_raw(1.0);
        ema.update_raw(2.0);
        ema.update_raw(3.0);

        assert!(ema.has_inputs());
        assert!(ema.is_initialized());
        assert_eq!(ema.count, 3);
        assert_eq!(ema.value, 2.25);
    }

    #[test]
    fn test_ema_reset() {
        let mut ema = ExponentialMovingAverage::new(3, Some(PriceType::Mid));
        ema.update_raw(1.0);
        ema.update_raw(2.0);
        ema.update_raw(3.0);

        ema.reset();

        assert_eq!(ema.count, 0);
        assert_eq!(ema.value, 0.0);
        assert!(!ema.has_inputs());
        assert!(!ema.is_initialized());
    }
}
