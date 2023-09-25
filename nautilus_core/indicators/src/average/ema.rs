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

use anyhow::Result;
use nautilus_core::python::to_pyvalue_err;
use nautilus_model::{
    data::{bar::Bar, quote::QuoteTick, trade::TradeTick},
    enums::PriceType,
};
use pyo3::prelude::*;

use crate::indicator::Indicator;

#[repr(C)]
#[derive(Debug)]
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators")]
pub struct ExponentialMovingAverage {
    pub period: usize,
    pub price_type: PriceType,
    pub alpha: f64,
    pub value: f64,
    pub count: usize,
    has_inputs: bool,
    is_initialized: bool,
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
        self.count = 0;
        self.has_inputs = false;
        self.is_initialized = false;
    }
}

impl ExponentialMovingAverage {
    pub fn new(period: usize, price_type: Option<PriceType>) -> Result<Self> {
        // Inputs don't require validation, however we return a `Result`
        // to standardize with other indicators which do need validation.
        Ok(Self {
            period,
            price_type: price_type.unwrap_or(PriceType::Last),
            alpha: 2.0 / (period as f64 + 1.0),
            value: 0.0,
            count: 0,
            has_inputs: false,
            is_initialized: false,
        })
    }

    pub fn update_raw(&mut self, value: f64) {
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
    fn py_new(period: usize, price_type: Option<PriceType>) -> PyResult<Self> {
        Self::new(period, price_type).map_err(to_pyvalue_err)
    }

    #[getter]
    #[pyo3(name = "name")]
    fn py_name(&self) -> String {
        self.name()
    }

    #[getter]
    #[pyo3(name = "period")]
    fn py_period(&self) -> usize {
        self.period
    }

    #[getter]
    #[pyo3(name = "alpha")]
    fn py_alpha(&self) -> f64 {
        self.alpha
    }

    #[getter]
    #[pyo3(name = "count")]
    fn py_count(&self) -> usize {
        self.count
    }

    #[getter]
    #[pyo3(name = "value")]
    fn py_value(&self) -> f64 {
        self.value
    }

    #[getter]
    #[pyo3(name = "has_inputs")]
    fn py_has_inputs(&self) -> bool {
        self.has_inputs()
    }

    #[getter]
    #[pyo3(name = "initialized")]
    fn py_initialized(&self) -> bool {
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

    fn __repr__(&self) -> String {
        format!("ExponentialMovingAverage({})", self.period)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_model::{
        data::{bar::Bar, quote::QuoteTick, trade::TradeTick},
        enums::PriceType,
    };
    use rstest::rstest;

    use crate::{average::ema::ExponentialMovingAverage, indicator::Indicator, stubs::*};

    #[rstest]
    fn test_ema_initialized(indicator_ema_10: ExponentialMovingAverage) {
        let ema = indicator_ema_10;
        let display_str = format!("{ema}");
        assert_eq!(display_str, "ExponentialMovingAverage(10)");
        assert_eq!(ema.period, 10);
        assert_eq!(ema.price_type, PriceType::Mid);
        assert_eq!(ema.alpha, 0.18181818181818182);
        assert_eq!(ema.is_initialized, false);
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
        assert!(ema.is_initialized());
        assert_eq!(ema.count, 10);
        assert_eq!(ema.value, 6.2393684801212155);
    }

    #[rstest]
    fn test_reset(indicator_ema_10: ExponentialMovingAverage) {
        let mut ema = indicator_ema_10;
        ema.update_raw(1.0);
        assert_eq!(ema.count, 1);
        ema.reset();
        assert_eq!(ema.count, 0);
        assert_eq!(ema.value, 0.0);
        assert_eq!(ema.is_initialized, false)
    }

    #[rstest]
    fn test_handle_quote_tick_single(
        indicator_ema_10: ExponentialMovingAverage,
        quote_tick: QuoteTick,
    ) {
        let mut ema = indicator_ema_10;
        ema.handle_quote_tick(&quote_tick);
        assert_eq!(ema.has_inputs(), true);
        assert_eq!(ema.value, 1501.0);
    }

    #[rstest]
    fn test_handle_quote_tick_multi(mut indicator_ema_10: ExponentialMovingAverage) {
        let tick1 = quote_tick("1500.0", "1502.0");
        let tick2 = quote_tick("1502.0", "1504.0");

        indicator_ema_10.handle_quote_tick(&tick1);
        indicator_ema_10.handle_quote_tick(&tick2);
        assert_eq!(indicator_ema_10.count, 2);
        assert_eq!(indicator_ema_10.value, 1501.3636363636363);
    }

    #[rstest]
    fn test_handle_trade_tick(indicator_ema_10: ExponentialMovingAverage, trade_tick: TradeTick) {
        let mut ema = indicator_ema_10;
        ema.handle_trade_tick(&trade_tick);
        assert_eq!(ema.has_inputs(), true);
        assert_eq!(ema.value, 1500.0);
    }

    #[rstest]
    fn handle_handle_bar(
        mut indicator_ema_10: ExponentialMovingAverage,
        bar_ethusdt_binance_minute_bid: Bar,
    ) {
        indicator_ema_10.handle_bar(&bar_ethusdt_binance_minute_bid);
        assert_eq!(indicator_ema_10.has_inputs, true);
        assert_eq!(indicator_ema_10.is_initialized, false);
        assert_eq!(indicator_ema_10.value, 1522.0);
    }
}
