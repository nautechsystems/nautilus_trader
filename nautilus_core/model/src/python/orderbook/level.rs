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

use pyo3::prelude::*;

use crate::{
    data::order::BookOrder,
    orderbook::BookLevel,
    types::{price::Price, quantity::QuantityRaw},
};

#[pymethods]
impl BookLevel {
    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        // TODO: Return debug string for now
        format!("{self:?}")
    }

    #[getter]
    #[pyo3(name = "price")]
    fn py_price(&self) -> Price {
        self.price.value
    }

    #[pyo3(name = "len")]
    fn py_len(&self) -> usize {
        self.len()
    }

    #[pyo3(name = "is_empty")]
    fn py_is_empty(&self) -> bool {
        self.is_empty()
    }

    #[pyo3(name = "size")]
    fn py_size(&self) -> f64 {
        self.size()
    }

    #[pyo3(name = "size_raw")]
    fn py_size_raw(&self) -> QuantityRaw {
        self.size_raw()
    }

    #[pyo3(name = "exposure")]
    fn py_exposure(&self) -> f64 {
        self.exposure()
    }

    #[pyo3(name = "exposure_raw")]
    fn py_exposure_raw(&self) -> QuantityRaw {
        self.exposure_raw()
    }

    #[pyo3(name = "first")]
    fn py_fist(&self) -> Option<BookOrder> {
        self.first().copied()
    }

    #[pyo3(name = "get_orders")]
    fn py_get_orders(&self) -> Vec<BookOrder> {
        self.get_orders()
    }
}
