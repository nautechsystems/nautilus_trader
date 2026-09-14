// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use nautilus_core::python::IntoPyObjectNautilusExt;
use pyo3::{prelude::*, pyclass::CompareOp};

use crate::{
    data::order::BookOrder,
    enums::OrderSide,
    orderbook::BookLevel,
    types::{price::Price, quantity::QuantityRaw},
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BookLevel {
    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        // TODO: Return debug string for now
        format!("{self:?}")
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py_any_unwrap(py),
            CompareOp::Ne => self.ne(other).into_py_any_unwrap(py),
            CompareOp::Ge if self.side() == other.side() => self.ge(other).into_py_any_unwrap(py),
            CompareOp::Gt if self.side() == other.side() => self.gt(other).into_py_any_unwrap(py),
            CompareOp::Le if self.side() == other.side() => self.le(other).into_py_any_unwrap(py),
            CompareOp::Lt if self.side() == other.side() => self.lt(other).into_py_any_unwrap(py),
            CompareOp::Ge | CompareOp::Gt | CompareOp::Le | CompareOp::Lt => py.NotImplemented(),
        }
    }

    fn __hash__(&self) -> isize {
        let mut hasher = DefaultHasher::new();
        self.side().hash(&mut hasher);
        self.price.value.hash(&mut hasher);
        hasher.finish() as isize
    }

    #[getter]
    #[pyo3(name = "price")]
    fn py_price(&self) -> Price {
        self.price.value
    }

    #[getter]
    #[pyo3(name = "side")]
    fn py_side(&self) -> OrderSide {
        self.side()
    }

    /// Returns the number of orders at this price level.
    #[pyo3(name = "len")]
    fn py_len(&self) -> usize {
        self.len()
    }

    /// Returns true if this price level has no orders.
    #[pyo3(name = "is_empty")]
    fn py_is_empty(&self) -> bool {
        self.is_empty()
    }

    /// Returns the total size of all orders at this price level as a float.
    #[pyo3(name = "size")]
    fn py_size(&self) -> f64 {
        self.size()
    }

    /// Returns the total size of all orders at this price level as raw integer units.
    #[pyo3(name = "size_raw")]
    fn py_size_raw(&self) -> QuantityRaw {
        self.size_raw()
    }

    /// Returns the total exposure (price * size) of all orders at this price level as a float.
    #[pyo3(name = "exposure")]
    fn py_exposure(&self) -> f64 {
        self.exposure()
    }

    /// Returns the total exposure (price * size) of all orders at this price level as raw integer units.
    ///
    /// Fixed-scale orders contribute `price.raw * size.raw / FIXED_SCALAR`.
    /// Native DeFi scales are normalized to the same fixed-scale result.
    /// Division truncates toward zero.
    /// Non-positive prices contribute zero.
    /// Saturates at `QuantityRaw::MAX` if the total exposure would overflow.
    #[pyo3(name = "exposure_raw")]
    fn py_exposure_raw(&self) -> QuantityRaw {
        self.exposure_raw()
    }

    #[pyo3(name = "first")]
    fn py_fist(&self) -> Option<BookOrder> {
        self.first().copied()
    }

    /// Returns all orders at this price level in FIFO insertion order.
    #[pyo3(name = "get_orders")]
    fn py_get_orders(&self) -> Vec<BookOrder> {
        self.get_orders()
    }
}
