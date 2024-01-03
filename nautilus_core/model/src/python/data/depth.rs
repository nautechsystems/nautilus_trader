// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_core::time::UnixNanos;
use pyo3::{prelude::*, pyclass::CompareOp};

use crate::{
    data::{
        depth::{OrderBookDepth10, DEPTH10_LEN},
        order::BookOrder,
    },
    identifiers::instrument_id::InstrumentId,
    python::PY_MODULE_MODEL,
};

#[pymethods]
impl OrderBookDepth10 {
    #[allow(clippy::too_many_arguments)]
    #[new]
    fn py_new(
        instrument_id: InstrumentId,
        bids: [BookOrder; DEPTH10_LEN],
        asks: [BookOrder; DEPTH10_LEN],
        bid_counts: [u32; DEPTH10_LEN],
        ask_counts: [u32; DEPTH10_LEN],
        flags: u8,
        sequence: u64,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self::new(
            instrument_id,
            bids,
            asks,
            bid_counts,
            ask_counts,
            flags,
            sequence,
            ts_event,
            ts_init,
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

    fn __str__(&self) -> String {
        self.to_string()
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    #[getter]
    fn instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    fn bids(&self) -> [BookOrder; DEPTH10_LEN] {
        self.bids
    }

    #[getter]
    fn asks(&self) -> [BookOrder; DEPTH10_LEN] {
        self.asks
    }

    #[getter]
    fn bid_counts(&self) -> [u32; DEPTH10_LEN] {
        self.bid_counts
    }

    #[getter]
    fn ask_counts(&self) -> [u32; DEPTH10_LEN] {
        self.ask_counts
    }

    #[getter]
    fn flags(&self) -> u8 {
        self.flags
    }

    #[getter]
    fn sequence(&self) -> u64 {
        self.sequence
    }

    #[getter]
    fn ts_event(&self) -> UnixNanos {
        self.ts_event
    }

    #[getter]
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }

    #[staticmethod]
    #[pyo3(name = "fully_qualified_name")]
    fn py_fully_qualified_name() -> String {
        format!("{}:{}", PY_MODULE_MODEL, stringify!(OrderBookDepth10))
    }
}
