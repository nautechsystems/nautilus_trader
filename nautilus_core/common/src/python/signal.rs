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

use nautilus_core::UnixNanos;
use pyo3::prelude::*;
use ustr::Ustr;

use crate::signal::Signal;

#[pymethods]
impl Signal {
    #[new]
    fn py_new(name: &str, value: String, ts_event: u64, ts_init: u64) -> Self {
        Self::new(
            Ustr::from(name),
            value,
            UnixNanos::from(ts_event),
            UnixNanos::from(ts_init),
        )
    }

    #[getter]
    #[pyo3(name = "name")]
    fn py_name(&self) -> &str {
        self.name.as_str()
    }

    #[getter]
    #[pyo3(name = "value")]
    fn py_value(&self) -> &str {
        self.value.as_str()
    }

    #[getter]
    #[pyo3(name = "ts_event")]
    const fn py_ts_event(&self) -> u64 {
        self.ts_event.as_u64()
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    const fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }
}
