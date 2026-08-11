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

use nautilus_core::{UUID4, UnixNanos, python::IntoPyObjectNautilusExt};
use nautilus_model::identifiers::{ClientId, TraderId, Venue};
use pyo3::{basic::CompareOp, prelude::*};
use ustr::Ustr;

use crate::{
    messages::system::{
        QueueCondition, QueueState, QueueStateChanged, SocketState, SocketStateChanged,
    },
    runner::SystemChannel,
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl SystemChannel {
    const fn __hash__(&self) -> isize {
        *self as isize
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl QueueCondition {
    const fn __hash__(&self) -> isize {
        *self as isize
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl QueueState {
    const fn __hash__(&self) -> isize {
        *self as isize
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl SocketState {
    const fn __hash__(&self) -> isize {
        *self as isize
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl QueueStateChanged {
    /// Represents an event where a runner queue pressure condition has changed.
    #[new]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        trader_id: TraderId,
        channel: SystemChannel,
        condition: QueueCondition,
        state: QueueState,
        queue_depth: usize,
        mean_dispatch_ns: u64,
        event_id: UUID4,
        ts_event: u64,
        ts_init: u64,
    ) -> Self {
        Self::new(
            trader_id,
            channel,
            condition,
            state,
            queue_depth,
            mean_dispatch_ns,
            event_id,
            UnixNanos::from(ts_event),
            UnixNanos::from(ts_init),
        )
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py_any_unwrap(py),
            CompareOp::Ne => self.ne(other).into_py_any_unwrap(py),
            _ => py.NotImplemented(),
        }
    }

    fn __repr__(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[pyo3(name = "trader_id")]
    const fn py_trader_id(&self) -> TraderId {
        self.trader_id
    }

    #[getter]
    #[pyo3(name = "channel")]
    const fn py_channel(&self) -> SystemChannel {
        self.channel
    }

    #[getter]
    #[pyo3(name = "condition")]
    const fn py_condition(&self) -> QueueCondition {
        self.condition
    }

    #[getter]
    #[pyo3(name = "state")]
    const fn py_state(&self) -> QueueState {
        self.state
    }

    #[getter]
    #[pyo3(name = "queue_depth")]
    const fn py_queue_depth(&self) -> usize {
        self.queue_depth
    }

    #[getter]
    #[pyo3(name = "mean_dispatch_ns")]
    const fn py_mean_dispatch_ns(&self) -> u64 {
        self.mean_dispatch_ns
    }

    #[getter]
    #[pyo3(name = "event_id")]
    const fn py_event_id(&self) -> UUID4 {
        self.event_id
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

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl SocketStateChanged {
    /// Represents an event where a socket transport state has changed.
    #[new]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        trader_id: TraderId,
        client_id: ClientId,
        venue: Option<Venue>,
        endpoint: &str,
        state: SocketState,
        event_id: UUID4,
        ts_event: u64,
        ts_init: u64,
    ) -> Self {
        Self::new(
            trader_id,
            client_id,
            venue,
            Ustr::from(endpoint),
            state,
            event_id,
            UnixNanos::from(ts_event),
            UnixNanos::from(ts_init),
        )
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py_any_unwrap(py),
            CompareOp::Ne => self.ne(other).into_py_any_unwrap(py),
            _ => py.NotImplemented(),
        }
    }

    fn __repr__(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[pyo3(name = "trader_id")]
    const fn py_trader_id(&self) -> TraderId {
        self.trader_id
    }

    #[getter]
    #[pyo3(name = "client_id")]
    const fn py_client_id(&self) -> ClientId {
        self.client_id
    }

    #[getter]
    #[pyo3(name = "venue")]
    const fn py_venue(&self) -> Option<Venue> {
        self.venue
    }

    #[getter]
    #[pyo3(name = "endpoint")]
    fn py_endpoint(&self) -> &str {
        self.endpoint.as_str()
    }

    #[getter]
    #[pyo3(name = "state")]
    const fn py_state(&self) -> SocketState {
        self.state
    }

    #[getter]
    #[pyo3(name = "event_id")]
    const fn py_event_id(&self) -> UUID4 {
        self.event_id
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
