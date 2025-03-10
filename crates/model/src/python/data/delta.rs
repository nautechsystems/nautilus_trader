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
    collections::{HashMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    str::FromStr,
};

use nautilus_core::{
    python::{
        IntoPyObjectNautilusExt,
        serialization::{from_dict_pyo3, to_dict_pyo3},
        to_pyvalue_err,
    },
    serialization::Serializable,
};
use pyo3::{basic::CompareOp, prelude::*, types::PyDict};

use super::data_to_pycapsule;
use crate::{
    data::{BookOrder, Data, NULL_ORDER, OrderBookDelta, order::OrderId},
    enums::{BookAction, FromU8, OrderSide},
    identifiers::InstrumentId,
    python::common::PY_MODULE_MODEL,
    types::{
        price::{Price, PriceRaw},
        quantity::{Quantity, QuantityRaw},
    },
};

impl OrderBookDelta {
    /// Create a new [`OrderBookDelta`] extracted from the given [`PyAny`].
    pub fn from_pyobject(obj: &Bound<'_, PyAny>) -> PyResult<Self> {
        let instrument_id_obj: Bound<'_, PyAny> = obj.getattr("instrument_id")?.extract()?;
        let instrument_id_str: String = instrument_id_obj.getattr("value")?.extract()?;
        let instrument_id = InstrumentId::from_str(instrument_id_str.as_str())
            .map_err(to_pyvalue_err)
            .unwrap();

        let action_obj: Bound<'_, PyAny> = obj.getattr("action")?.extract()?;
        let action_u8 = action_obj.getattr("value")?.extract()?;
        let action = BookAction::from_u8(action_u8).unwrap();

        let flags: u8 = obj.getattr("flags")?.extract()?;
        let sequence: u64 = obj.getattr("sequence")?.extract()?;
        let ts_event: u64 = obj.getattr("ts_event")?.extract()?;
        let ts_init: u64 = obj.getattr("ts_init")?.extract()?;

        let order_pyobject = obj.getattr("order")?;
        let order: BookOrder = if order_pyobject.is_none() {
            NULL_ORDER
        } else {
            let side_obj: Bound<'_, PyAny> = order_pyobject.getattr("side")?.extract()?;
            let side_u8 = side_obj.getattr("value")?.extract()?;
            let side = OrderSide::from_u8(side_u8).unwrap();

            let price_py: Bound<'_, PyAny> = order_pyobject.getattr("price")?;
            let price_raw: PriceRaw = price_py.getattr("raw")?.extract()?;
            let price_prec: u8 = price_py.getattr("precision")?.extract()?;
            let price = Price::from_raw(price_raw, price_prec);

            let size_py: Bound<'_, PyAny> = order_pyobject.getattr("size")?;
            let size_raw: QuantityRaw = size_py.getattr("raw")?.extract()?;
            let size_prec: u8 = size_py.getattr("precision")?.extract()?;
            let size = Quantity::from_raw(size_raw, size_prec);

            let order_id: OrderId = order_pyobject.getattr("order_id")?.extract()?;
            BookOrder {
                side,
                price,
                size,
                order_id,
            }
        };

        Ok(Self::new(
            instrument_id,
            action,
            order,
            flags,
            sequence,
            ts_event.into(),
            ts_init.into(),
        ))
    }
}

#[pymethods]
impl OrderBookDelta {
    #[new]
    fn py_new(
        instrument_id: InstrumentId,
        action: BookAction,
        order: BookOrder,
        flags: u8,
        sequence: u64,
        ts_event: u64,
        ts_init: u64,
    ) -> PyResult<Self> {
        Self::new_checked(
            instrument_id,
            action,
            order,
            flags,
            sequence,
            ts_event.into(),
            ts_init.into(),
        )
        .map_err(to_pyvalue_err)
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
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    #[pyo3(name = "action")]
    fn py_action(&self) -> BookAction {
        self.action
    }

    #[getter]
    #[pyo3(name = "order")]
    fn py_order(&self) -> BookOrder {
        self.order
    }

    #[getter]
    #[pyo3(name = "flags")]
    fn py_flags(&self) -> u8 {
        self.flags
    }

    #[getter]
    #[pyo3(name = "sequence")]
    fn py_sequence(&self) -> u64 {
        self.sequence
    }

    #[getter]
    #[pyo3(name = "ts_event")]
    fn py_ts_event(&self) -> u64 {
        self.ts_event.as_u64()
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[staticmethod]
    #[pyo3(name = "fully_qualified_name")]
    fn py_fully_qualified_name() -> String {
        format!("{}:{}", PY_MODULE_MODEL, stringify!(OrderBookDelta))
    }

    #[staticmethod]
    #[pyo3(name = "get_metadata")]
    fn py_get_metadata(
        instrument_id: &InstrumentId,
        price_precision: u8,
        size_precision: u8,
    ) -> PyResult<HashMap<String, String>> {
        Ok(Self::get_metadata(
            instrument_id,
            price_precision,
            size_precision,
        ))
    }

    #[staticmethod]
    #[pyo3(name = "get_fields")]
    fn py_get_fields(py: Python<'_>) -> PyResult<Bound<'_, PyDict>> {
        let py_dict = PyDict::new(py);
        for (k, v) in Self::get_fields() {
            py_dict.set_item(k, v)?;
        }

        Ok(py_dict)
    }

    /// Returns a new object from the given dictionary representation.
    #[staticmethod]
    #[pyo3(name = "from_dict")]
    fn py_from_dict(py: Python<'_>, values: Py<PyDict>) -> PyResult<Self> {
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

    /// Creates a `PyCapsule` containing a raw pointer to a `Data::Delta` object.
    ///
    /// This function takes the current object (assumed to be of a type that can be represented as
    /// `Data::Delta`), and encapsulates a raw pointer to it within a `PyCapsule`.
    ///
    /// # Safety
    ///
    /// This function is safe as long as the following conditions are met:
    /// - The `Data::Delta` object pointed to by the capsule must remain valid for the lifetime of the capsule.
    /// - The consumer of the capsule must ensure proper handling to avoid dereferencing a dangling pointer.
    ///
    /// # Panics
    ///
    /// The function will panic if the `PyCapsule` creation fails, which can occur if the
    /// `Data::Delta` object cannot be converted into a raw pointer.
    #[pyo3(name = "as_pycapsule")]
    fn py_as_pycapsule(&self, py: Python<'_>) -> PyObject {
        data_to_pycapsule(py, Data::Delta(*self))
    }

    /// Return a dictionary representation of the object.
    #[pyo3(name = "as_dict")]
    fn py_as_dict(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
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
    use crate::data::stubs::*;

    #[rstest]
    fn test_order_book_delta_py_new_with_zero_size_returns_error() {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|_py| {
            let instrument_id = InstrumentId::from("AAPL.XNAS");
            let action = BookAction::Add;
            let zero_size = Quantity::from(0);
            let price = Price::from("100.00");
            let side = OrderSide::Buy;
            let order_id = 123_456;
            let flags = 0;
            let sequence = 1;
            let ts_event = 1;
            let ts_init = 2;

            let order = BookOrder::new(side, price, zero_size, order_id);

            let result = OrderBookDelta::py_new(
                instrument_id,
                action,
                order,
                flags,
                sequence,
                ts_event,
                ts_init,
            );
            assert!(result.is_err());
        });
    }

    #[rstest]
    fn test_as_dict(stub_delta: OrderBookDelta) {
        pyo3::prepare_freethreaded_python();
        let delta = stub_delta;

        Python::with_gil(|py| {
            let dict_string = delta.py_as_dict(py).unwrap().to_string();
            let expected_string = r"{'type': 'OrderBookDelta', 'instrument_id': 'AAPL.XNAS', 'action': 'ADD', 'order': {'side': 'BUY', 'price': '100.00', 'size': '10', 'order_id': 123456}, 'flags': 0, 'sequence': 1, 'ts_event': 1, 'ts_init': 2}";
            assert_eq!(dict_string, expected_string);
        });
    }

    #[rstest]
    fn test_from_dict(stub_delta: OrderBookDelta) {
        pyo3::prepare_freethreaded_python();
        let delta = stub_delta;

        Python::with_gil(|py| {
            let dict = delta.py_as_dict(py).unwrap();
            let parsed = OrderBookDelta::py_from_dict(py, dict).unwrap();
            assert_eq!(parsed, delta);
        });
    }

    #[rstest]
    fn test_from_pyobject(stub_delta: OrderBookDelta) {
        pyo3::prepare_freethreaded_python();
        let delta = stub_delta;

        Python::with_gil(|py| {
            let delta_pyobject = delta.into_py_any_unwrap(py);
            let parsed_delta = OrderBookDelta::from_pyobject(delta_pyobject.bind(py)).unwrap();
            assert_eq!(parsed_delta, delta);
        });
    }
}
