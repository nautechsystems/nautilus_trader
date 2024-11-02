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

use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    hash::{Hash, Hasher},
    str::FromStr,
};

use nautilus_core::{
    python::{serialization::from_dict_pyo3, to_pyvalue_err},
    serialization::Serializable,
};
use pyo3::{prelude::*, pyclass::CompareOp, types::PyDict};
use ustr::Ustr;

use crate::{
    data::status::InstrumentStatus,
    enums::{FromU16, MarketStatusAction},
    identifiers::InstrumentId,
    python::common::PY_MODULE_MODEL,
};

impl InstrumentStatus {
    /// Create a new [`InstrumentStatus`] extracted from the given [`PyAny`].
    pub fn from_pyobject(obj: &Bound<'_, PyAny>) -> PyResult<Self> {
        let instrument_id_obj: Bound<'_, PyAny> = obj.getattr("instrument_id")?.extract()?;
        let instrument_id_str: String = instrument_id_obj.getattr("value")?.extract()?;
        let instrument_id =
            InstrumentId::from_str(instrument_id_str.as_str()).map_err(to_pyvalue_err)?;

        let action_obj: Bound<'_, PyAny> = obj.getattr("action")?.extract()?;
        let action_u16: u16 = action_obj.getattr("value")?.extract()?;
        let action = MarketStatusAction::from_u16(action_u16).unwrap();

        let ts_event: u64 = obj.getattr("ts_event")?.extract()?;
        let ts_init: u64 = obj.getattr("ts_init")?.extract()?;

        let reason_str: Option<String> = obj.getattr("reason")?.extract()?;
        let reason = reason_str.map(|reason_str| Ustr::from(&reason_str));

        let trading_event_str: Option<String> = obj.getattr("trading_event")?.extract()?;
        let trading_event =
            trading_event_str.map(|trading_event_str| Ustr::from(&trading_event_str));

        let is_trading: Option<bool> = obj.getattr("is_trading")?.extract()?;
        let is_quoting: Option<bool> = obj.getattr("is_quoting")?.extract()?;
        let is_short_sell_restricted: Option<bool> =
            obj.getattr("is_short_sell_restricted")?.extract()?;

        Ok(Self::new(
            instrument_id,
            action,
            ts_event.into(),
            ts_init.into(),
            reason,
            trading_event,
            is_trading,
            is_quoting,
            is_short_sell_restricted,
        ))
    }
}

#[pymethods]
impl InstrumentStatus {
    #[new]
    #[pyo3(signature = (instrument_id, action, ts_event, ts_init, reason=None, trading_event=None, is_trading=None, is_quoting=None, is_short_sell_restricted=None))]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        instrument_id: InstrumentId,
        action: MarketStatusAction,
        ts_event: u64,
        ts_init: u64,
        reason: Option<String>,
        trading_event: Option<String>,
        is_trading: Option<bool>,
        is_quoting: Option<bool>,
        is_short_sell_restricted: Option<bool>,
    ) -> Self {
        Self::new(
            instrument_id,
            action,
            ts_event.into(),
            ts_init.into(),
            reason.map(|s| Ustr::from(&s)),
            trading_event.map(|s| Ustr::from(&s)),
            is_trading,
            is_quoting,
            is_short_sell_restricted,
        )
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py(py),
            CompareOp::Ne => self.ne(other).into_py(py),
            _ => py.NotImplemented(),
        }
    }

    fn __hash__(&self) -> isize {
        let mut h = DefaultHasher::new();
        self.hash(&mut h);
        h.finish() as isize
    }

    fn __repr__(&self) -> String {
        format!("{}({})", stringify!(InstrumentStatus), self)
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
    fn py_action(&self) -> MarketStatusAction {
        self.action
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

    #[getter]
    #[pyo3(name = "reason")]
    fn py_reason(&self) -> Option<String> {
        self.reason.map(|x| x.to_string())
    }

    #[getter]
    #[pyo3(name = "trading_event")]
    fn py_trading_event(&self) -> Option<String> {
        self.trading_event.map(|x| x.to_string())
    }

    #[getter]
    #[pyo3(name = "is_trading")]
    fn py_is_trading(&self) -> Option<bool> {
        self.is_trading
    }

    #[getter]
    #[pyo3(name = "is_quoting")]
    fn py_is_quoting(&self) -> Option<bool> {
        self.is_quoting
    }

    #[getter]
    #[pyo3(name = "is_short_sell_restricted")]
    fn py_is_short_sell_restricted(&self) -> Option<bool> {
        self.is_short_sell_restricted
    }

    #[staticmethod]
    #[pyo3(name = "fully_qualified_name")]
    fn py_fully_qualified_name() -> String {
        format!("{}:{}", PY_MODULE_MODEL, stringify!(InstrumentStatus))
    }

    // TODO: Implement for `Data`
    /// Creates a `PyCapsule` containing a raw pointer to a `Data::Quote` object.
    ///
    /// This function takes the current object (assumed to be of a type that can be represented as
    /// `Data::Quote`), and encapsulates a raw pointer to it within a `PyCapsule`.
    ///
    /// # Safety
    ///
    /// This function is safe as long as the following conditions are met:
    /// - The `Data::Quote` object pointed to by the capsule must remain valid for the lifetime of the capsule.
    /// - The consumer of the capsule must ensure proper handling to avoid dereferencing a dangling pointer.
    ///
    /// # Panics
    ///
    /// The function will panic if the `PyCapsule` creation fails, which can occur if the
    /// `Data::Quote` object cannot be converted into a raw pointer.
    // #[pyo3(name = "as_pycapsule")]
    // fn py_as_pycapsule(&self, py: Python<'_>) -> PyObject {
    //     data_to_pycapsule(py, Data::Quote(*self))
    // }

    /// Return a dictionary representation of the object.
    #[pyo3(name = "as_dict")]
    fn py_as_dict(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        // Serialize object to JSON bytes
        let json_str = serde_json::to_string(self).map_err(to_pyvalue_err)?;
        // Parse JSON into a Python dictionary
        let py_dict: Py<PyDict> = PyModule::import_bound(py, "json")?
            .call_method("loads", (json_str,), None)?
            .extract()?;
        Ok(py_dict)
    }

    /// Returns a new object from the given dictionary representation.
    #[staticmethod]
    #[pyo3(name = "from_dict")]
    fn py_from_dict(py: Python<'_>, values: Py<PyDict>) -> PyResult<Self> {
        from_dict_pyo3(py, values)
    }

    #[staticmethod]
    #[pyo3(name = "get_metadata")]
    fn py_get_metadata(instrument_id: &InstrumentId) -> PyResult<HashMap<String, String>> {
        Ok(Self::get_metadata(instrument_id))
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

    /// Return JSON encoded bytes representation of the object.
    #[pyo3(name = "as_json")]
    fn py_as_json(&self, py: Python<'_>) -> Py<PyAny> {
        // Unwrapping is safe when serializing a valid object
        self.as_json_bytes().unwrap().into_py(py)
    }

    /// Return MsgPack encoded bytes representation of the object.
    #[pyo3(name = "as_msgpack")]
    fn py_as_msgpack(&self, py: Python<'_>) -> Py<PyAny> {
        // Unwrapping is safe when serializing a valid object
        self.as_msgpack_bytes().unwrap().into_py(py)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use pyo3::{IntoPy, Python};
    use rstest::rstest;

    use crate::data::{status::InstrumentStatus, stubs::stub_instrument_status};

    #[rstest]
    fn test_as_dict(stub_instrument_status: InstrumentStatus) {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let dict_string = stub_instrument_status.py_as_dict(py).unwrap().to_string();
            let expected_string = r"{'type': 'InstrumentStatus', 'instrument_id': 'MSFT.XNAS', 'action': 'TRADING', 'ts_event': 1, 'ts_init': 2, 'reason': None, 'trading_event': None, 'is_trading': None, 'is_quoting': None, 'is_short_sell_restricted': None}";
            assert_eq!(dict_string, expected_string);
        });
    }

    #[rstest]
    fn test_from_dict(stub_instrument_status: InstrumentStatus) {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let dict = stub_instrument_status.py_as_dict(py).unwrap();
            let parsed = InstrumentStatus::py_from_dict(py, dict).unwrap();
            assert_eq!(parsed, stub_instrument_status);
        });
    }

    #[rstest]
    fn test_from_pyobject(stub_instrument_status: InstrumentStatus) {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let status_pyobject = stub_instrument_status.into_py(py);
            let parsed_status = InstrumentStatus::from_pyobject(status_pyobject.bind(py)).unwrap();
            assert_eq!(parsed_status, stub_instrument_status);
        });
    }
}
