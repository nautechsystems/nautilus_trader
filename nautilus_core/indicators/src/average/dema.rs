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
use nautilus_core::python::to_pyvalue_err;
use nautilus_model::{
    data::{bar::Bar, quote::QuoteTick, trade::TradeTick},
    enums::PriceType,
};
use pyo3::prelude::*;

use crate::{average::ema::ExponentialMovingAverage, indicator::Indicator};

/// The Double Exponential Moving Average attempts to a smoother average with less
/// lag than the normal Exponential Moving Average (EMA)
#[repr(C)]
#[derive(Debug)]
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators")]
pub struct DoubleExponentialMovingAverage {
    /// The rolling window period for the indicator (> 0).
    pub period: usize,
    /// The price type used for calculations.
    pub price_type: PriceType,
    /// The last indicator value.
    pub value: f64,
    /// The input count for the indicator.
    pub count: usize,
    has_inputs: bool,
    is_initialized: bool,
    _ema1: ExponentialMovingAverage,
    _ema2: ExponentialMovingAverage,
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

impl DoubleExponentialMovingAverage {
    pub fn new(period: usize, price_type: Option<PriceType>) -> Result<Self> {
        Ok(Self {
            period,
            price_type: price_type.unwrap_or(PriceType::Last),
            value: 0.0,
            count: 0,
            has_inputs: false,
            is_initialized: false,
            _ema1: ExponentialMovingAverage::new(period, price_type)?,
            _ema2: ExponentialMovingAverage::new(period, price_type)?,
        })
    }

    pub fn update_raw(&mut self, value: f64) {
        if !self.has_inputs {
            self.has_inputs = true;
            self.value = value;
        }
        self._ema1.update_raw(value);
        self._ema2.update_raw(self._ema1.value);

        self.value = 2.0f64.mul_add(self._ema1.value, -self._ema2.value);
        self.count += 1;

        if !self.is_initialized && self.count >= self.period {
            self.is_initialized = true;
        }
    }
}

#[cfg(feature = "python")]
#[pymethods]
impl DoubleExponentialMovingAverage {
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
        format!("DoubleExponentialMovingAverage({})", self.period)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_model::data::{bar::Bar, quote::QuoteTick, trade::TradeTick};
    use rstest::rstest;

    use crate::{average::dema::DoubleExponentialMovingAverage, indicator::Indicator, stubs::*};

    #[rstest]
    fn test_dema_initialized(indicator_dema_10: DoubleExponentialMovingAverage) {
        let display_str = format!("{indicator_dema_10}");
        assert_eq!(display_str, "DoubleExponentialMovingAverage(period=10)");
        assert_eq!(indicator_dema_10.period, 10);
        assert!(!indicator_dema_10.is_initialized);
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
        assert!(!indicator_dema_10.is_initialized);
        indicator_dema_10.update_raw(10.0);
        assert!(indicator_dema_10.is_initialized);
    }

    #[rstest]
    fn test_handle_quote_tick(
        mut indicator_dema_10: DoubleExponentialMovingAverage,
        quote_tick: QuoteTick,
    ) {
        indicator_dema_10.handle_quote_tick(&quote_tick);
        assert_eq!(indicator_dema_10.value, 1501.0);
    }

    #[rstest]
    fn test_handle_trade_tick(
        mut indicator_dema_10: DoubleExponentialMovingAverage,
        trade_tick: TradeTick,
    ) {
        indicator_dema_10.handle_trade_tick(&trade_tick);
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
        assert!(!indicator_dema_10.is_initialized);
    }

    #[rstest]
    fn test_reset(mut indicator_dema_10: DoubleExponentialMovingAverage) {
        indicator_dema_10.update_raw(1.0);
        assert_eq!(indicator_dema_10.count, 1);
        indicator_dema_10.reset();
        assert_eq!(indicator_dema_10.value, 0.0);
        assert_eq!(indicator_dema_10.count, 0);
        assert!(!indicator_dema_10.has_inputs);
        assert!(!indicator_dema_10.is_initialized);
    }
}
