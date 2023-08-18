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

use std::io::Read;
use nautilus_model::{
    enums::{PriceType}
};

use pyo3::prelude::*;
use nautilus_model::data::bar::Bar;
use nautilus_model::data::quote::QuoteTick;
use nautilus_model::data::trade::TradeTick;
use crate::Indicator;

#[repr(C)]
#[derive(Debug)]
#[pyclass]
pub struct SimpleMovingAverage {
    pub period: usize,
    pub price_type: PriceType,
    pub value: f64,
    pub count: usize,
    pub inputs: Vec<f64>,
    _has_inputs: bool,
    _is_initialized: bool,
}

impl Indicator for SimpleMovingAverage {
    fn name(&self) -> String {
        stringify!(SimpleMovingAverage).to_string()
    }

    fn has_inputs(&self) -> bool {
        self._has_inputs
    }

    fn is_initialized(&self) -> bool {
        self._is_initialized
    }

    fn handle_quote_tick(&mut self, tick: &QuoteTick) {
        todo!()
    }

    fn handle_trade_tick(&mut self, tick: &TradeTick) {
        todo!()
    }

    fn handle_bar(&mut self, bar: &Bar) {
        todo!()
    }

    fn reset(&mut self) {
        todo!()
    }
}

#[pymethods]
impl SimpleMovingAverage {

    #[must_use]
    #[new]
    pub fn new(period: usize, price_type: Option<PriceType>) -> Self {
        Self {
            period,
            price_type: price_type.unwrap_or(PriceType::Last),
            value: 0.0,
            count: 0,
            inputs: Vec::new(),
            _has_inputs: false,
            _is_initialized: false,
        }
    }

    #[getter]
    #[pyo3(name = "name")]
    #[must_use]
    pub fn name_py(&self) -> String{
        self.name()
    }

    #[pyo3(name= "has_inputs")]
    fn has_inputs_py(&self) -> bool{
        self.has_inputs()
    }

    #[pyo3(name = "is_initialized")]
    fn is_initialized(&self) -> bool {
        self._is_initialized
    }

    pub fn update_raw(&mut self,value: f64){
        if self.inputs.len() == self.period{
            self.inputs.remove(0);
            self.count -=1;
        }
        self.inputs.push(value);
        self.count += 1;
        let sum = self.inputs.iter().sum::<f64>();
        self.value = sum / self.count as f64;
    }
}



#[cfg(test)]
mod tests{
    use super::*;

    fn get_sma(period: usize)-> SimpleMovingAverage{
        SimpleMovingAverage::new(period, Some(PriceType::Mid))
    }

    #[test]
    fn test_sma_init(){
        let sma = get_sma(10);
        let display_str = format!("{:?}", sma);
        assert_eq!(display_str, "SimpleMovingAverage { period: 10, price_type: Mid, value: 0.0, count: 0, inputs: [], _has_inputs: false, _is_initialized: false }")
    }

    #[test]
    fn test_name_returns_expected_string(){
        let sma = get_sma(10);
        assert_eq!(sma.name(), "SimpleMovingAverage")
    }

    #[test]
    fn test_sma_update_raw_exact_period(){
        let mut sma = get_sma(3);
        sma.update_raw(1.0);
        sma.update_raw(2.0);
        sma.update_raw(3.0);

        assert!(sma.has_inputs());
        assert!(sma.is_initialized());
        assert_eq!(sma.count, 3);
        assert_eq!(sma.value, 2.0);
    }

    #[test]
    fn test_sma_update_raw_more_values_than_period(){
        let mut sma = get_sma(3);
        sma.update_raw(1.0);
        sma.update_raw(2.0);
        sma.update_raw(3.0);
        sma.update_raw(4.0);

        assert!(sma.has_inputs());
        assert!(sma.is_initialized());
        assert_eq!(sma.count, 3);
        assert_eq!(sma.value, 3.0);
    }
}