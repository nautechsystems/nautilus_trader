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

/// An indicator which calculates the efficiency ratio across a rolling window.
/// The Kaufman Efficiency measures the ratio of the relative market speed in
/// relation to the volatility, this could be thought of as a proxy for noise.
#[repr(C)]
#[derive(Debug)]
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators")]
pub struct EfficiencyRatio {
    /// The rolling window period for the indicator (>= 2).
    pub period: usize,
    pub price_type: PriceType,
    pub value: f64,
    pub inputs: Vec<f64>,
    _deltas: Vec<f64>,
    is_initialized: bool,
}

impl Display for EfficiencyRatio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.name(), self.period,)
    }
}

impl Indicator for EfficiencyRatio {
    fn name(&self) -> String {
        stringify!(EfficiencyRatio).to_string()
    }

    fn has_inputs(&self) -> bool {
        !self.inputs.is_empty()
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
        self.inputs.clear();
        self.is_initialized = false;
    }
}

impl EfficiencyRatio {
    pub fn new(period: usize, price_type: Option<PriceType>) -> Result<Self> {
        Ok(Self {
            period,
            price_type: price_type.unwrap_or(PriceType::Last),
            value: 0.0,
            inputs: Vec::with_capacity(period),
            _deltas: Vec::with_capacity(period),
            is_initialized: false,
        })
    }

    pub fn update_raw(&mut self, value: f64) {
        self.inputs.push(value);
        if self.inputs.len() < 2 {
            self.value = 0.0;
            return;
        } else if !self.is_initialized && self.inputs.len() >= self.period {
            self.is_initialized = true;
        }
        let last_diff =
            (self.inputs[self.inputs.len() - 1] - self.inputs[self.inputs.len() - 2]).abs();
        self._deltas.push(last_diff);
        let sum_deltas = self._deltas.iter().sum::<f64>().abs();
        let net_diff = (self.inputs[self.inputs.len() - 1] - self.inputs[0]).abs();
        self.value = if sum_deltas == 0.0 {
            0.0
        } else {
            net_diff / sum_deltas
        };
    }
}

#[cfg(feature = "python")]
#[pymethods]
impl EfficiencyRatio {
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
    #[pyo3(name = "value")]
    fn py_value(&self) -> f64 {
        self.value
    }

    #[getter]
    #[pyo3(name = "initialized")]
    fn py_initialized(&self) -> bool {
        self.is_initialized
    }

    #[pyo3(name = "has_inputs")]
    fn py_has_inputs(&self) -> bool {
        self.has_inputs()
    }

    #[pyo3(name = "update_raw")]
    fn py_update_raw(&mut self, value: f64) {
        self.update_raw(value);
    }

    fn __repr__(&self) -> String {
        format!("EfficiencyRatio({})", self.period)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {

    use rstest::rstest;

    use crate::{indicator::Indicator, ratio::efficiency_ratio::EfficiencyRatio, stubs::*};

    #[rstest]
    fn test_efficiency_ratio_initialized(efficiency_ratio_10: EfficiencyRatio) {
        let display_str = format!("{efficiency_ratio_10}");
        assert_eq!(display_str, "EfficiencyRatio(10)");
        assert_eq!(efficiency_ratio_10.period, 10);
        assert!(!efficiency_ratio_10.is_initialized);
    }

    #[rstest]
    fn test_with_correct_number_of_required_inputs(mut efficiency_ratio_10: EfficiencyRatio) {
        for i in 1..10 {
            efficiency_ratio_10.update_raw(f64::from(i));
        }
        assert_eq!(efficiency_ratio_10.inputs.len(), 9);
        assert!(!efficiency_ratio_10.is_initialized);
        efficiency_ratio_10.update_raw(1.0);
        assert_eq!(efficiency_ratio_10.inputs.len(), 10);
        assert!(efficiency_ratio_10.is_initialized);
    }

    #[rstest]
    fn test_value_with_one_input(mut efficiency_ratio_10: EfficiencyRatio) {
        efficiency_ratio_10.update_raw(1.0);
        assert_eq!(efficiency_ratio_10.value, 0.0);
    }

    #[rstest]
    fn test_value_with_efficient_higher_inputs(mut efficiency_ratio_10: EfficiencyRatio) {
        let mut initial_price = 1.0;
        for _ in 1..=10 {
            initial_price += 0.0001;
            efficiency_ratio_10.update_raw(initial_price);
        }
        assert_eq!(efficiency_ratio_10.value, 1.0);
    }

    #[rstest]
    fn test_value_with_efficient_lower_inputs(mut efficiency_ratio_10: EfficiencyRatio) {
        let mut initial_price = 1.0;
        for _ in 1..=10 {
            initial_price -= 0.0001;
            efficiency_ratio_10.update_raw(initial_price);
        }
        assert_eq!(efficiency_ratio_10.value, 1.0);
    }

    #[rstest]
    fn test_value_with_oscillating_inputs_returns_zero(mut efficiency_ratio_10: EfficiencyRatio) {
        efficiency_ratio_10.update_raw(1.00000);
        efficiency_ratio_10.update_raw(1.00010);
        efficiency_ratio_10.update_raw(1.00000);
        efficiency_ratio_10.update_raw(0.99990);
        efficiency_ratio_10.update_raw(1.00000);
        assert_eq!(efficiency_ratio_10.value, 0.0);
    }

    #[rstest]
    fn test_value_with_half_oscillating(mut efficiency_ratio_10: EfficiencyRatio) {
        efficiency_ratio_10.update_raw(1.00000);
        efficiency_ratio_10.update_raw(1.00020);
        efficiency_ratio_10.update_raw(1.00010);
        efficiency_ratio_10.update_raw(1.00030);
        efficiency_ratio_10.update_raw(1.00020);
        assert_eq!(efficiency_ratio_10.value, 0.333_333_333_333_333_3);
    }

    #[rstest]
    fn test_value_with_noisy_inputs(mut efficiency_ratio_10: EfficiencyRatio) {
        efficiency_ratio_10.update_raw(1.00000);
        efficiency_ratio_10.update_raw(1.00010);
        efficiency_ratio_10.update_raw(1.00008);
        efficiency_ratio_10.update_raw(1.00007);
        efficiency_ratio_10.update_raw(1.00012);
        efficiency_ratio_10.update_raw(1.00005);
        efficiency_ratio_10.update_raw(1.00015);
        assert_eq!(efficiency_ratio_10.value, 0.428_571_428_572_153_63);
    }

    #[rstest]
    fn test_reset(mut efficiency_ratio_10: EfficiencyRatio) {
        for i in 1..=10 {
            efficiency_ratio_10.update_raw(f64::from(i));
        }
        assert!(efficiency_ratio_10.is_initialized);
        efficiency_ratio_10.reset();
        assert!(!efficiency_ratio_10.is_initialized);
        assert_eq!(efficiency_ratio_10.value, 0.0);
    }

    #[rstest]
    fn test_handle_quote_tick(mut efficiency_ratio_10: EfficiencyRatio) {
        let quote_tick1 = quote_tick("1500.0", "1502.0");
        let quote_tick2 = quote_tick("1502.0", "1504.0");

        efficiency_ratio_10.handle_quote_tick(&quote_tick1);
        efficiency_ratio_10.handle_quote_tick(&quote_tick2);
        assert_eq!(efficiency_ratio_10.value, 1.0);
    }

    #[rstest]
    fn test_handle_bar(mut efficiency_ratio_10: EfficiencyRatio) {
        let bar1 = bar_ethusdt_binance_minute_bid("1500.0");
        let bar2 = bar_ethusdt_binance_minute_bid("1510.0");

        efficiency_ratio_10.handle_bar(&bar1);
        efficiency_ratio_10.handle_bar(&bar2);
        assert_eq!(efficiency_ratio_10.value, 1.0);
    }
}
