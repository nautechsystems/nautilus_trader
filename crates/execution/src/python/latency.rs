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

//! Python bindings for latency model types.

use nautilus_core::UnixNanos;
use pyo3::prelude::*;

use crate::models::latency::StaticLatencyModel;

#[pymethods]
impl StaticLatencyModel {
    #[new]
    #[pyo3(signature = (
        base_latency_nanos = 0,
        insert_latency_nanos = 0,
        update_latency_nanos = 0,
        cancel_latency_nanos = 0,
    ))]
    fn py_new(
        base_latency_nanos: u64,
        insert_latency_nanos: u64,
        update_latency_nanos: u64,
        cancel_latency_nanos: u64,
    ) -> Self {
        Self::new(
            UnixNanos::from(base_latency_nanos),
            UnixNanos::from(insert_latency_nanos),
            UnixNanos::from(update_latency_nanos),
            UnixNanos::from(cancel_latency_nanos),
        )
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}
