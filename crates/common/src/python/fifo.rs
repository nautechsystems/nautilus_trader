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

//! Python bindings for FIFO cache.

use pyo3::prelude::*;

use crate::cache::fifo::FifoCache;

#[pyo3::pyclass(
    name = "FifoCache",
    module = "nautilus_trader.core.nautilus_pyo3.common"
)]
#[derive(Debug)]
pub struct PyFifoCache {
    inner: FifoCache<String, 10_000>,
}

#[pymethods]
impl PyFifoCache {
    #[new]
    fn py_new() -> Self {
        Self {
            inner: FifoCache::new(),
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "FifoCache(capacity={}, len={})",
            self.inner.capacity(),
            self.inner.len()
        )
    }

    #[getter]
    fn capacity(&self) -> usize {
        self.inner.capacity()
    }

    fn __len__(&self) -> usize {
        self.inner.len()
    }

    fn __contains__(&self, key: String) -> bool {
        self.inner.contains(&key)
    }

    fn add(&mut self, key: String) {
        self.inner.add(key);
    }

    fn remove(&mut self, key: String) {
        self.inner.remove(&key);
    }

    fn clear(&mut self) {
        self.inner.clear();
    }
}
