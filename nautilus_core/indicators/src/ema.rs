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

use nautilus_model::{
    data::{bar::Bar, quote::QuoteTick, trade::TradeTick},
    enums::PriceType,
};
use pyo3::prelude::*;

use crate::Indicator;

#[repr(C)]
#[derive(Debug)]
#[pyclass]
pub struct ExponentialMovingAverage {
    pub period: usize,
    pub price_type: PriceType,
    pub alpha: f64,
    pub value: f64,
    pub count: usize,
    has_inputs: bool,
    is_initialized: bool,
}

impl Indicator for ExponentialMovingAverage {
    fn name(&self) -> String {
        stringify!(ExponentialMovingAverage).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.has_inputs
    }

    fn is_initialized(&self) -> bool {
        self.is_initialized
    }

    fn handle_quote_tick(&mut self, tick: &QuoteTick) {
        self.py_update_raw(tick.extract_price(self.price_type).into());
    }

    fn handle_trade_tick(&mut self, tick: &TradeTick) {
        self.py_update_raw((&tick.price).into());
    }

    fn handle_bar(&mut self, bar: &Bar) {
        self.py_update_raw((&bar.close).into());
    }

    fn reset(&mut self) {
        self.value = 0.0;
        self.count = 0;
        self.has_inputs = false;
        self.is_initialized = false;
    }
}

impl ExponentialMovingAverage {
    fn update_raw(&mut self, value: f64) {
        if !self.has_inputs {
            self.has_inputs = true;
            self.value = value;
        }

        self.value = self.alpha.mul_add(value, (1.0 - self.alpha) * self.value);
        self.count += 1;

        // Initialization logic
        if !self.is_initialized && self.count >= self.period {
            self.is_initialized = true;
        }
    }
}

#[cfg(feature = "python")]
#[pymethods]
impl ExponentialMovingAverage {
    #[new]
    fn new(period: usize, price_type: Option<PriceType>) -> Self {
        Self {
            period,
            price_type: price_type.unwrap_or(PriceType::Last),
            alpha: 2.0 / (period as f64 + 1.0),
            value: 0.0,
            count: 0,
            has_inputs: false,
            is_initialized: false,
        }
    }

    #[getter]
    #[pyo3(name = "name")]
    fn py_name(&self) -> String {
        self.name()
    }

    #[pyo3(name = "has_inputs")]
    fn py_has_inputs(&self) -> bool {
        self.has_inputs()
    }

    #[pyo3(name = "is_initialized")]
    fn py_is_initialized(&self) -> bool {
        self.is_initialized
    }

    #[pyo3(name = "handle_quote_tick")]
    fn py_handle_quote_tick(&mut self, tick: &QuoteTick) {
        self.py_update_raw(tick.extract_price(self.price_type).into());
    }

    #[pyo3(name = "handle_trade_tick")]
    fn py_handle_trade_tick(&mut self, tick: &TradeTick) {
        self.update_raw((&tick.price).into());
    }

    #[pyo3(name = "handle_bar")]
    fn py_handle_bar(&mut self, bar: &Bar) {
        self.update_raw((&bar.close).into());
    }

    #[pyo3(name = "reset")]
    fn py_reset(&mut self) {
        self.reset();
    }

    #[pyo3(name = "update_raw")]
    fn py_update_raw(&mut self, value: f64) {
        self.update_raw(value);
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_ema_initialized() {
        let ema = ExponentialMovingAverage::new(20, Some(PriceType::Mid));
        let display_str = format!("{ema:?}");
        assert_eq!(display_str, "ExponentialMovingAverage { period: 20, price_type: Mid, alpha: 0.09523809523809523, value: 0.0, count: 0, has_inputs: false, is_initialized: false }");
    }

    #[rstest]
    fn test_ema_update_raw() {
        let mut ema = ExponentialMovingAverage::new(3, Some(PriceType::Mid));
        ema.py_update_raw(1.0);
        ema.py_update_raw(2.0);
        ema.py_update_raw(3.0);

        assert!(ema.has_inputs());
        assert!(ema.is_initialized());
        assert_eq!(ema.count, 3);
        assert_eq!(ema.value, 2.25);
    }

    #[rstest]
    fn test_ema_reset() {
        let mut ema = ExponentialMovingAverage::new(3, Some(PriceType::Mid));
        ema.py_update_raw(1.0);
        ema.py_update_raw(2.0);
        ema.py_update_raw(3.0);

        ema.reset();

        assert_eq!(ema.count, 0);
        assert_eq!(ema.value, 0.0);
        assert!(!ema.has_inputs());
        assert!(!ema.is_initialized());
    }
}
