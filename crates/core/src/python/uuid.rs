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

//! UUID helpers for PyO3.

use std::{
    collections::hash_map::DefaultHasher,
    ffi::CStr,
    hash::{Hash, Hasher},
    str::FromStr,
};

use pyo3::{
    IntoPyObjectExt, Py,
    prelude::*,
    pyclass::CompareOp,
    types::{PyBytes, PyTuple},
};

use super::{IntoPyObjectNautilusExt, to_pyvalue_err};
use crate::uuid::{UUID4, UUID4_LEN};

#[pymethods]
impl UUID4 {
    /// Creates a new [`UUID4`] instance.
    ///
    /// If a string value is provided, it attempts to parse it into a UUID.
    /// If no value is provided, a new random UUID is generated.
    #[new]
    fn py_new() -> Self {
        Self::new()
    }

    /// Sets the state of the `UUID4` instance during unpickling.
    #[allow(clippy::needless_pass_by_value)]
    fn __setstate__(&mut self, py: Python<'_>, state: Py<PyAny>) -> PyResult<()> {
        let bytes: &Bound<'_, PyBytes> = state.downcast_bound::<PyBytes>(py)?;
        let slice = bytes.as_bytes();

        if slice.len() != UUID4_LEN {
            return Err(to_pyvalue_err(
                "Invalid state for deserializing, incorrect bytes length",
            ));
        }

        if slice[UUID4_LEN - 1] != 0 {
            return Err(to_pyvalue_err(
                "Invalid state for deserializing, missing null terminator",
            ));
        }

        let cstr = CStr::from_bytes_with_nul(slice).map_err(|_| {
            to_pyvalue_err("Invalid state for deserializing, bytes must be null-terminated UTF-8")
        })?;

        let value = cstr.to_str().map_err(|_| {
            to_pyvalue_err("Invalid state for deserializing, bytes must be valid UTF-8")
        })?;

        let parsed = Self::from_str(value).map_err(|e| {
            to_pyvalue_err(format!(
                "Invalid state for deserializing, unable to parse UUID: {e}"
            ))
        })?;

        self.value.copy_from_slice(&parsed.value);
        Ok(())
    }

    /// Gets the state of the `UUID4` instance for pickling.
    fn __getstate__(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        PyBytes::new(py, &self.value).into_py_any(py)
    }

    /// Reduces the `UUID4` instance for pickling.
    fn __reduce__(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let safe_constructor = py.get_type::<Self>().getattr("_safe_constructor")?;
        let state = self.__getstate__(py)?;
        (safe_constructor, PyTuple::empty(py), state).into_py_any(py)
    }

    /// A safe constructor used during unpickling to ensure the correct initialization of `UUID4`.
    #[staticmethod]
    #[allow(clippy::unnecessary_wraps)]
    fn _safe_constructor() -> PyResult<Self> {
        Ok(Self::new()) // Safe default
    }

    /// Compares two `UUID4` instances for equality
    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py_any_unwrap(py),
            CompareOp::Ne => self.ne(other).into_py_any_unwrap(py),
            _ => py.NotImplemented(),
        }
    }

    /// Returns a hash value for the `UUID4` instance.
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    fn __hash__(&self) -> isize {
        let mut h = DefaultHasher::new();
        self.hash(&mut h);
        h.finish() as isize
    }

    /// Returns a detailed string representation of the `UUID4` instance.
    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    /// Returns the `UUID4` as a string.
    fn __str__(&self) -> String {
        self.to_string()
    }

    /// Gets the `UUID4` value as a string.
    #[getter]
    #[pyo3(name = "value")]
    fn py_value(&self) -> String {
        self.to_string()
    }

    /// Creates a new [`UUID4`] from a string representation.
    #[staticmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(value: &str) -> PyResult<Self> {
        Self::from_str(value).map_err(to_pyvalue_err)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Once;

    use pyo3::Python;

    use super::*;

    fn ensure_python_initialized() {
        static INIT: Once = Once::new();
        INIT.call_once(|| {
            pyo3::prepare_freethreaded_python();
        });
    }

    #[test]
    fn test_setstate_rejects_invalid_uuid_bytes() {
        ensure_python_initialized();
        Python::with_gil(|py| {
            let mut uuid = UUID4::new();
            let mut invalid = [b'a'; UUID4_LEN];
            invalid[UUID4_LEN - 1] = 0;
            let py_bytes = PyBytes::new(py, &invalid);
            let err = uuid
                .__setstate__(py, py_bytes.into_py_any_unwrap(py))
                .expect_err("expected invalid state to error");
            assert!(err.to_string().contains("Invalid state for deserializing"));
        });
    }

    #[test]
    fn test_setstate_rejects_missing_null_terminator() {
        ensure_python_initialized();
        Python::with_gil(|py| {
            let mut uuid = UUID4::new();
            let mut bytes = uuid.value;
            bytes[UUID4_LEN - 1] = b'0';
            let py_bytes = PyBytes::new(py, &bytes);
            let err = uuid
                .__setstate__(py, py_bytes.into_py_any_unwrap(py))
                .expect_err("expected missing NUL terminator to error");
            assert!(
                err.to_string()
                    .contains("Invalid state for deserializing, missing null terminator")
            );
        });
    }

    #[test]
    fn test_setstate_accepts_valid_state() {
        ensure_python_initialized();
        Python::with_gil(|py| {
            let source = UUID4::new();
            let mut target = UUID4::new();
            let py_bytes = PyBytes::new(py, &source.value);
            target
                .__setstate__(py, py_bytes.into_py_any_unwrap(py))
                .expect("valid state should succeed");
            assert_eq!(target.to_string(), source.to_string());
        });
    }
}
