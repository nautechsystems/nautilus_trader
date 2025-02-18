// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::collections::HashMap;

use nautilus_core::python::to_pyvalue_err;
use nautilus_model::enums::PriceType;
use pyo3::prelude::*;
use ustr::Ustr;

use crate::xrate::get_exchange_rate;

#[pyfunction]
#[pyo3(name = "get_exchange_rate")]
#[pyo3(signature = (from_currency, to_currency, price_type, quotes_bid, quotes_ask))]
pub fn py_get_exchange_rate(
    from_currency: &str,
    to_currency: &str,
    price_type: PriceType,
    quotes_bid: HashMap<String, f64>,
    quotes_ask: HashMap<String, f64>,
) -> PyResult<Option<f64>> {
    get_exchange_rate(
        Ustr::from(from_currency),
        Ustr::from(to_currency),
        price_type,
        quotes_bid,
        quotes_ask,
    )
    .map_err(to_pyvalue_err)
}
