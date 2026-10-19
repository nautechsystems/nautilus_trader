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

//! Registries which resolve Python objects into owned Rust factories.
//!
//! A Python caller cannot hand ownership of a `#[pyclass]` instance to Rust, so subsystems which
//! let Python install a backing accept a factory object instead. Each subsystem keeps a
//! [`FactoryRegistry`] of extractors keyed by Python class name, then resolves an owned
//! `Box<dyn Factory>` from an arbitrary Python object at configuration time.

use ahash::AHashMap;
use nautilus_core::python::to_pynotimplemented_err;
use parking_lot::Mutex;
use pyo3::{Py, PyAny, PyResult, Python};

/// Function type for extracting a Python object into a boxed factory.
pub type FactoryExtractor<T> = fn(Python<'_>, Py<PyAny>) -> PyResult<Box<T>>;

/// Registry of Python factory extractors keyed by Python class name.
///
/// The `label` names the factory kind in error messages, for example `"message bus factory"`.
#[derive(Debug)]
pub struct FactoryRegistry<T: ?Sized> {
    label: &'static str,
    extractors_by_type: Mutex<AHashMap<String, FactoryExtractor<T>>>,
}

impl<T: ?Sized> FactoryRegistry<T> {
    /// Creates an empty registry which describes itself with `label` in error messages.
    #[must_use]
    pub fn new(label: &'static str) -> Self {
        Self {
            label,
            extractors_by_type: Mutex::new(AHashMap::new()),
        }
    }

    /// Registers an extractor for a Python factory type name.
    ///
    /// Registering the same extractor again succeeds without change, so a Python module
    /// initializer can run more than once per process.
    ///
    /// # Errors
    ///
    /// Returns an error if a different extractor is already registered for the type name.
    pub fn register(
        &self,
        type_name: String,
        extractor: FactoryExtractor<T>,
    ) -> anyhow::Result<()> {
        let mut extractors = self.extractors_by_type.lock();

        if let Some(registered) = extractors.get(&type_name) {
            if std::ptr::fn_addr_eq(*registered, extractor) {
                return Ok(());
            }

            anyhow::bail!(
                "A different {label} extractor is already registered for '{type_name}'",
                label = self.label
            );
        }

        extractors.insert(type_name, extractor);
        Ok(())
    }

    /// Extracts a Python object into a boxed factory.
    ///
    /// # Errors
    ///
    /// Returns an error if no extractor is registered for the Python type or extraction fails.
    pub fn extract(&self, py: Python<'_>, factory: Py<PyAny>) -> PyResult<Box<T>> {
        let type_name = factory
            .getattr(py, "__class__")?
            .getattr(py, "__name__")?
            .extract::<String>(py)?;
        let extractors = self.extractors_by_type.lock();

        match extractors.get(&type_name) {
            Some(extractor) => extractor(py, factory),
            None => Err(to_pynotimplemented_err(format!(
                "No {label} extractor registered for '{type_name}'",
                label = self.label
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Debug;

    use pyo3::{exceptions::PyNotImplementedError, types::PyDict};
    use rstest::rstest;

    use super::*;

    trait StubFactory: Debug + Send + Sync {
        fn name(&self) -> &'static str;
    }

    #[derive(Debug)]
    #[pyo3::pyclass(name = "StubFactoryOne")]
    struct StubFactoryOne;

    impl StubFactory for StubFactoryOne {
        fn name(&self) -> &'static str {
            "one"
        }
    }

    #[expect(
        clippy::unnecessary_wraps,
        reason = "signature must match the FactoryExtractor fn pointer"
    )]
    fn extract_one(_py: Python<'_>, _factory: Py<PyAny>) -> PyResult<Box<dyn StubFactory>> {
        Ok(Box::new(StubFactoryOne))
    }

    #[expect(
        clippy::unnecessary_wraps,
        reason = "signature must match the FactoryExtractor fn pointer"
    )]
    fn extract_conflicting(_py: Python<'_>, _factory: Py<PyAny>) -> PyResult<Box<dyn StubFactory>> {
        Ok(Box::new(StubFactoryOne))
    }

    #[rstest]
    fn test_extract_resolves_registered_python_class() {
        Python::initialize();
        let registry = FactoryRegistry::<dyn StubFactory>::new("stub factory");
        registry
            .register("StubFactoryOne".to_string(), extract_one)
            .unwrap();

        Python::attach(|py| {
            let factory = Py::new(py, StubFactoryOne).unwrap().into_any();

            let extracted = registry.extract(py, factory).unwrap();

            assert_eq!(extracted.name(), "one");
        });
    }

    #[rstest]
    fn test_extract_rejects_unregistered_python_class() {
        Python::initialize();
        let registry = FactoryRegistry::<dyn StubFactory>::new("stub factory");

        Python::attach(|py| {
            let factory = PyDict::new(py).unbind().into_any();

            let error = registry.extract(py, factory).unwrap_err();

            assert!(error.is_instance_of::<PyNotImplementedError>(py));
            assert_eq!(
                error.to_string(),
                "NotImplementedError: No stub factory extractor registered for 'dict'"
            );
        });
    }

    #[rstest]
    fn test_register_is_idempotent_for_the_same_extractor() {
        let registry = FactoryRegistry::<dyn StubFactory>::new("stub factory");

        registry
            .register("StubFactoryOne".to_string(), extract_one)
            .unwrap();

        assert!(
            registry
                .register("StubFactoryOne".to_string(), extract_one)
                .is_ok()
        );
    }

    #[rstest]
    fn test_register_rejects_a_conflicting_extractor() {
        let registry = FactoryRegistry::<dyn StubFactory>::new("stub factory");
        registry
            .register("StubFactoryOne".to_string(), extract_one)
            .unwrap();

        let error = registry
            .register("StubFactoryOne".to_string(), extract_conflicting)
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "A different stub factory extractor is already registered for 'StubFactoryOne'"
        );
    }
}
