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

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use nautilus_core::{
    python::{
        IntoPyObjectNautilusExt,
        serialization::{from_dict_pyo3, to_dict_pyo3},
        to_pyvalue_err,
    },
    serialization::Serializable,
};
use pyo3::{prelude::*, pyclass::CompareOp, types::PyDict};

use crate::{
    data::order::{BookOrder, OrderId},
    enums::OrderSide,
    python::common::PY_MODULE_MODEL,
    types::{Price, Quantity},
};

#[pymethods]
impl BookOrder {
    #[new]
    fn py_new(side: OrderSide, price: Price, size: Quantity, order_id: OrderId) -> Self {
        Self::new(side, price, size, order_id)
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py_any_unwrap(py),
            CompareOp::Ne => self.ne(other).into_py_any_unwrap(py),
            _ => py.NotImplemented(),
        }
    }

    fn __hash__(&self) -> isize {
        let mut h = DefaultHasher::new();
        self.hash(&mut h);
        h.finish() as isize
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[pyo3(name = "side")]
    fn py_side(&self) -> OrderSide {
        self.side
    }

    #[getter]
    #[pyo3(name = "price")]
    fn py_price(&self) -> Price {
        self.price
    }

    #[getter]
    #[pyo3(name = "size")]
    fn py_size(&self) -> Quantity {
        self.size
    }

    #[getter]
    #[pyo3(name = "order_id")]
    fn py_order_id(&self) -> u64 {
        self.order_id
    }

    #[staticmethod]
    #[pyo3(name = "fully_qualified_name")]
    fn py_fully_qualified_name() -> String {
        format!("{}:{}", PY_MODULE_MODEL, stringify!(BookOrder))
    }

    #[pyo3(name = "exposure")]
    fn py_exposure(&self) -> f64 {
        self.exposure()
    }

    #[pyo3(name = "signed_size")]
    fn py_signed_size(&self) -> f64 {
        self.signed_size()
    }

    /// Returns a new object from the given dictionary representation.
    #[staticmethod]
    #[pyo3(name = "from_dict")]
    pub fn py_from_dict(py: Python<'_>, values: Py<PyDict>) -> PyResult<Self> {
        from_dict_pyo3(py, values)
    }

    #[staticmethod]
    #[pyo3(name = "from_json")]
    fn py_from_json(data: Vec<u8>) -> PyResult<Self> {
        Self::from_json_bytes(&data).map_err(to_pyvalue_err)
    }

    #[staticmethod]
    #[pyo3(name = "from_msgpack")]
    fn py_from_msgpack(data: Vec<u8>) -> PyResult<Self> {
        Self::from_msgpack_bytes(&data).map_err(to_pyvalue_err)
    }

    /// Return a dictionary representation of the object.
    #[pyo3(name = "as_dict")]
    pub fn py_as_dict(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        to_dict_pyo3(py, self)
    }

    /// Return JSON encoded bytes representation of the object.
    #[pyo3(name = "as_json")]
    fn py_as_json(&self, py: Python<'_>) -> Py<PyAny> {
        // Unwrapping is safe when serializing a valid object
        self.as_json_bytes().unwrap().into_py_any_unwrap(py)
    }

    /// Return MsgPack encoded bytes representation of the object.
    #[pyo3(name = "as_msgpack")]
    fn py_as_msgpack(&self, py: Python<'_>) -> Py<PyAny> {
        // Unwrapping is safe when serializing a valid object
        self.as_msgpack_bytes().unwrap().into_py_any_unwrap(py)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::data::stubs::stub_book_order;

    #[rstest]
    fn test_as_dict(stub_book_order: BookOrder) {
        pyo3::prepare_freethreaded_python();
        let book_order = stub_book_order;

        Python::with_gil(|py| {
            let dict_string = book_order.py_as_dict(py).unwrap().to_string();
            let expected_string =
                r"{'side': 'BUY', 'price': '100.00', 'size': '10', 'order_id': 123456}";
            assert_eq!(dict_string, expected_string);
        });
    }

    #[rstest]
    fn test_from_dict(stub_book_order: BookOrder) {
        pyo3::prepare_freethreaded_python();
        let book_order = stub_book_order;

        Python::with_gil(|py| {
            let dict = book_order.py_as_dict(py).unwrap();
            let parsed = BookOrder::py_from_dict(py, dict).unwrap();
            assert_eq!(parsed, book_order);
        });
    }
}
