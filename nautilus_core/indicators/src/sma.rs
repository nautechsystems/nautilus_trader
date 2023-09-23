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
    is_initialized: bool,
}

impl Display for SimpleMovingAverage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}({})",
            self.name(),
            self.period,
        )
    }
}

impl Indicator for SimpleMovingAverage {
    fn name(&self) -> String {
        stringify!(SimpleMovingAverage).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.inputs.len() > 0
    }

    fn is_initialized(&self) -> bool {
        self.is_initialized
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
        self.inputs.clear();
        self.is_initialized = false;
    }
}


impl SimpleMovingAverage{
    pub fn update_raw(&mut self,value: f64){
        if self.inputs.len() == self.period{
            self.inputs.remove(0);
            self.count -=1;
        }
        self.inputs.push(value);
        self.count += 1;
        let sum = self.inputs.iter().sum::<f64>();
        self.value = sum / self.count as f64;

        if !self.is_initialized && self.count >= self.period{
            self.is_initialized = true;
        }
    }
}

#[cfg(feature = "python")]
#[pymethods]
impl SimpleMovingAverage {

    #[new]
    pub fn new(period: usize, price_type: Option<PriceType>) -> Self {
        Self {
            period,
            price_type: price_type.unwrap_or(PriceType::Last),
            value: 0.0,
            count: 0,
            inputs: Vec::new(),
            is_initialized: false,
        }
    }

    #[getter]
    #[pyo3(name = "name")]
    pub fn py_name(&self) -> String{
        self.name()
    }

    #[getter]
    #[pyo3(name = "period")]
    pub fn py_period(&self) -> usize{
        self.period
    }

    #[getter]
    #[pyo3(name = "count")]
    pub fn py_count(&self)->usize{
        self.count
    }

    #[getter]
    #[pyo3(name = "value")]
    pub fn py_value(&self)->f64{
        self.value
    }

    #[getter]
    #[pyo3(name = "initialized")]
    pub fn py_initialized(&self) -> bool{
        self.is_initialized
    }

    #[pyo3(name= "has_inputs")]
    fn has_inputs_py(&self) -> bool{
        self.has_inputs()
    }

    #[pyo3(name = "update_raw")]
    fn py_update_raw(&mut self, value: f64) {
        self.update_raw(value);
    }

    fn __repr__(&self) -> String {
        format!("SimpleMovingAverage({})", self.period)
    }
}



////////////////////////////////////////////////////////////////////////////////
// Stubs
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
pub mod stubs{
    use rstest::fixture;
    use nautilus_model::enums::PriceType;
    use crate::sma::SimpleMovingAverage;

    #[fixture]
    pub fn indicator_sma_10() -> SimpleMovingAverage{
        SimpleMovingAverage::new(10, Some(PriceType::Mid))
    }
}

////////////////////////////////////////////////////////////////////////////////
// Test
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests{
    use rstest::rstest;
    use nautilus_model::data::quote::QuoteTick;
    use nautilus_model::data::trade::TradeTick;
    use nautilus_model::enums::{AggressorSide, PriceType};
    use nautilus_model::identifiers::instrument_id::InstrumentId;
    use nautilus_model::identifiers::trade_id::TradeId;
    use nautilus_model::types::price::Price;
    use nautilus_model::types::quantity::Quantity;
    use crate::Indicator;
    use crate::sma::SimpleMovingAverage;
    use super::stubs::*;

    #[rstest]
    fn test_sma_initialized(indicator_sma_10: SimpleMovingAverage){
        let display_str = format!("{indicator_sma_10}");
        assert_eq!(display_str, "SimpleMovingAverage(10)");
        assert_eq!(indicator_sma_10.period, 10);
        assert_eq!(indicator_sma_10.price_type, PriceType::Mid);
        assert_eq!(indicator_sma_10.value, 0.0);
        assert_eq!(indicator_sma_10.count, 0);
    }

    #[rstest]
    fn test_sma_update_raw_exact_period(indicator_sma_10: SimpleMovingAverage){
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
        assert!(sma.is_initialized());
        assert_eq!(sma.count, 10);
        assert_eq!(sma.value, 5.5);
    }

   #[rstest]
    fn test_reset(
        indicator_sma_10: SimpleMovingAverage,
    ){
        let mut sma = indicator_sma_10;
        sma.update_raw(1.0);
        assert_eq!(sma.count, 1);
        sma.reset();
        assert_eq!(sma.count, 0);
        assert_eq!(sma.value, 0.0);
        assert_eq!(sma.is_initialized,false)
    }


    #[rstest]
    fn test_handle_quote_tick(
        indicator_sma_10: SimpleMovingAverage,
    ){
        let mut sma = indicator_sma_10;
        let tick = QuoteTick {
            instrument_id: InstrumentId::from("ETHUSDT-PERP.BINANCE"),
            bid_price: Price::from("1500.0000"),
            ask_price: Price::from("1502.0000"),
            bid_size: Quantity::from("1.00000000"),
            ask_size: Quantity::from("1.00000000"),
            ts_event: 1,
            ts_init: 0,
        };
        sma.handle_quote_tick(&tick);
        assert_eq!(sma.count, 1);
        assert_eq!(sma.value, 1501.0);
    }

    #[rstest]
    fn test_handle_trade_tick(
        indicator_sma_10: SimpleMovingAverage,
    ){
        let mut sma = indicator_sma_10;
        let tick = TradeTick {
            instrument_id: InstrumentId::from("ETHUSDT-PERP.BINANCE"),
            price: Price::from("1500.0000"),
            size: Quantity::from("1.00000000"),
            aggressor_side: AggressorSide::Buyer,
            trade_id: TradeId::new("123456789").unwrap(),
            ts_event: 1,
            ts_init: 0,
        };
        sma.handle_trade_tick(&tick);
        assert_eq!(sma.count, 1);
        assert_eq!(sma.value, 1500.0);
    }


}