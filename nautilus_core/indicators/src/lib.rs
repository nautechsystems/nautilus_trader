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

pub mod ema;
pub mod sma;

use nautilus_model::data::{bar::Bar, quote::QuoteTick, trade::TradeTick};
use pyo3::{prelude::*, types::PyModule, Python};

/// Loaded as nautilus_pyo3.indicators
#[pymodule]
pub fn indicators(_: Python<'_>, m: &PyModule) -> PyResult<()> {
    m.add_class::<ema::ExponentialMovingAverage>()?;
    Ok(())
}

pub trait Indicator {
    fn name(&self) -> String;
    fn has_inputs(&self) -> bool;
    fn is_initialized(&self) -> bool;
    fn handle_quote_tick(&mut self, tick: &QuoteTick);
    fn handle_trade_tick(&mut self, tick: &TradeTick);
    fn handle_bar(&mut self, bar: &Bar);
    fn reset(&mut self);
}
