// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_model::{data::bar::BarSpecification, identifiers::InstrumentId};
use pyo3::prelude::*;

use crate::tardis::{machine::InstrumentMiniInfo, parse::bar_spec_to_tardis_trade_bar_string};

#[pymethods]
impl InstrumentMiniInfo {
    #[new]
    fn py_new(
        instrument_id: InstrumentId,
        price_precision: u8,
        size_precision: u8,
    ) -> PyResult<Self> {
        Ok(Self::new(instrument_id, price_precision, size_precision))
    }
}

#[must_use]
#[pyfunction(name = "bar_spec_to_tardis_trade_bar_string")]
pub fn py_bar_spec_to_tardis_trade_bar_string(bar_spec: &BarSpecification) -> String {
    bar_spec_to_tardis_trade_bar_string(bar_spec)
}
