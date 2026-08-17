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

//! Registry of the strong references which keep registered components' Python wrappers alive.
//!
//! Component inners hold only a weak reference to their Python wrapper, so something must own the
//! wrapper for as long as the component stays registered. This registry is that owner, and it is
//! thread-local for the same reason the component and actor registries are: those hold
//! `Rc<UnsafeCell<..>>` and cannot leave their thread, so a wrapper registry with wider visibility
//! would desync from the registrations it shadows.

use std::cell::RefCell;

use ahash::AHashMap;
use nautilus_model::identifiers::ComponentId;
use pyo3::prelude::*;

thread_local! {
    static PYTHON_WRAPPERS: RefCell<AHashMap<ComponentId, Py<PyAny>>> =
        RefCell::new(AHashMap::new());
}

/// Retains the strong reference which keeps the Python wrapper for `component_id` alive.
pub fn retain_python_wrapper(component_id: ComponentId, wrapper: Py<PyAny>) {
    let displaced =
        PYTHON_WRAPPERS.with_borrow_mut(|wrappers| wrappers.insert(component_id, wrapper));

    // Dropping a wrapper can run Python finalization which re-enters Rust, so the value leaves the
    // registry before the borrow ends
    drop(displaced);
}

/// Releases the strong reference retained for `component_id`.
pub fn release_python_wrapper(component_id: ComponentId) {
    let released = PYTHON_WRAPPERS.with_borrow_mut(|wrappers| wrappers.remove(&component_id));

    drop(released);
}

/// Returns the Python wrapper retained for `component_id`, or `None` when nothing is retained.
#[must_use]
pub fn get_python_wrapper(component_id: ComponentId) -> Option<Py<PyAny>> {
    PYTHON_WRAPPERS.with_borrow(|wrappers| {
        wrappers
            .get(&component_id)
            .map(|wrapper| Python::attach(|py| wrapper.clone_ref(py)))
    })
}

#[cfg(test)]
mod tests {
    use pyo3::{ffi::c_str, types::PyModule, wrap_pyfunction};
    use rstest::rstest;

    use super::*;

    /// Lets a finalizing Python wrapper re-enter the registry.
    #[pyfunction]
    fn wrapper_is_retained(component_id: &str) -> bool {
        get_python_wrapper(ComponentId::from(component_id)).is_some()
    }

    #[rstest]
    fn test_wrapper_finalization_re_enters_an_unborrowed_registry() {
        Python::initialize();

        Python::attach(|py| {
            let module = PyModule::new(py, "test_wrapper_finalization").unwrap();
            module
                .add_function(wrap_pyfunction!(wrapper_is_retained, &module).unwrap())
                .unwrap();

            let code = c_str!(
                r#"
OBSERVED = []


class Finalizing:
    def __del__(self):
        OBSERVED.append(wrapper_is_retained("Finalizing-Component"))
"#
            );
            py.run(code, Some(&module.dict()), None).unwrap();

            let finalizing = module.getattr("Finalizing").unwrap();
            let component_id = ComponentId::from("Finalizing-Component");

            retain_python_wrapper(component_id, finalizing.call0().unwrap().unbind());

            // The registry holds the only reference to each wrapper, so both the displaced wrapper
            // and the released one run `__del__` while being dropped
            retain_python_wrapper(component_id, finalizing.call0().unwrap().unbind());
            release_python_wrapper(component_id);

            let observed = module
                .getattr("OBSERVED")
                .unwrap()
                .extract::<Vec<bool>>()
                .unwrap();

            assert_eq!(observed, vec![true, false]);
        });
    }
}
