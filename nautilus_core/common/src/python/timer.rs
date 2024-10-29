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

use std::{str::FromStr, sync::Arc};

use nautilus_core::{nanos::UnixNanos, python::to_pyvalue_err, uuid::UUID4};
use pyo3::{
    basic::CompareOp,
    prelude::*,
    types::{PyLong, PyString, PyTuple},
};
use ustr::Ustr;

use crate::timer::{TimeEvent, TimeEventCallback, TimeEventHandlerV2};

#[pyo3::pyclass(
    module = "nautilus_trader.core.nautilus_pyo3.common",
    name = "TimeEventHandler"
)]
#[derive(Clone)]
/// Temporary time event handler for Python inter-operatbility
///
/// TODO: Remove once control flow moves into Rust
///
/// `TimeEventHandler` associates a `TimeEvent` with a callback function that is triggered
/// when the event's timestamp is reached.
#[allow(non_camel_case_types)]
pub struct TimeEventHandler_Py {
    /// The time event.
    pub event: TimeEvent,
    /// The callable python object.
    pub callback: Arc<PyObject>,
}

impl From<TimeEventHandlerV2> for TimeEventHandler_Py {
    fn from(value: TimeEventHandlerV2) -> Self {
        Self {
            event: value.event,
            callback: match value.callback {
                TimeEventCallback::Python(callback) => callback,
                TimeEventCallback::Rust(_) => {
                    panic!("Python time event handler is not supported for Rust callback")
                }
            },
        }
    }
}

#[pymethods]
impl TimeEvent {
    #[new]
    fn py_new(name: &str, event_id: UUID4, ts_event: u64, ts_init: u64) -> Self {
        Self::new(Ustr::from(name), event_id, ts_event.into(), ts_init.into())
    }

    fn __setstate__(&mut self, state: &Bound<'_, PyAny>) -> PyResult<()> {
        let py_tuple: &Bound<'_, PyTuple> = state.downcast::<PyTuple>()?;

        let ts_event = py_tuple
            .get_item(2)?
            .downcast::<PyLong>()?
            .extract::<u64>()?;
        let ts_init: u64 = py_tuple
            .get_item(3)?
            .downcast::<PyLong>()?
            .extract::<u64>()?;

        self.name = Ustr::from(
            py_tuple
                .get_item(0)?
                .downcast::<PyString>()?
                .extract::<&str>()?,
        );
        self.event_id = UUID4::from_str(
            py_tuple
                .get_item(1)?
                .downcast::<PyString>()?
                .extract::<&str>()?,
        )
        .map_err(to_pyvalue_err)?;
        self.ts_event = ts_event.into();
        self.ts_init = ts_init.into();

        Ok(())
    }

    fn __getstate__(&self, py: Python) -> PyResult<PyObject> {
        Ok((
            self.name.to_string(),
            self.event_id.to_string(),
            self.ts_event.as_u64(),
            self.ts_init.as_u64(),
        )
            .to_object(py))
    }

    fn __reduce__(&self, py: Python) -> PyResult<PyObject> {
        let safe_constructor = py.get_type_bound::<Self>().getattr("_safe_constructor")?;
        let state = self.__getstate__(py)?;
        Ok((safe_constructor, PyTuple::empty_bound(py), state).to_object(py))
    }

    #[staticmethod]
    fn _safe_constructor() -> Self {
        Self::new(
            Ustr::from("NULL"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        )
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py(py),
            CompareOp::Ne => self.ne(other).into_py(py),
            _ => py.NotImplemented(),
        }
    }

    fn __repr__(&self) -> String {
        format!("{}('{}')", stringify!(TimeEvent), self)
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[pyo3(name = "name")]
    fn py_name(&self) -> String {
        self.name.to_string()
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

#[cfg(test)]
mod tests {
    use nautilus_core::{
        datetime::NANOSECONDS_IN_MILLISECOND, nanos::UnixNanos, time::get_atomic_clock_realtime,
    };
    use pyo3::prelude::*;
    use tokio::time::Duration;

    use crate::{
        testing::wait_until,
        timer::{LiveTimer, TimeEvent, TimeEventCallback},
    };

    #[pyfunction]
    const fn receive_event(_py: Python, _event: TimeEvent) -> PyResult<()> {
        // TODO: Assert the length of a handler vec
        Ok(())
    }

    #[tokio::test]
    async fn test_live_timer_starts_and_stops() {
        pyo3::prepare_freethreaded_python();

        let callback = Python::with_gil(|py| {
            let callable = wrap_pyfunction_bound!(receive_event, py).unwrap();
            TimeEventCallback::from(callable.into_py(py))
        });

        // Create a new LiveTimer with no stop time
        let clock = get_atomic_clock_realtime();
        let start_time = clock.get_time_ns();
        let interval_ns = 100 * NANOSECONDS_IN_MILLISECOND;
        let mut timer = LiveTimer::new("TEST_TIMER", interval_ns, start_time, None, callback);
        let next_time_ns = timer.next_time_ns();
        timer.start();

        // Wait for timer to run
        tokio::time::sleep(Duration::from_millis(300)).await;

        timer.cancel().unwrap();
        wait_until(|| timer.is_expired(), Duration::from_secs(2));
        assert!(timer.next_time_ns() > next_time_ns);
    }

    #[tokio::test]
    async fn test_live_timer_with_stop_time() {
        pyo3::prepare_freethreaded_python();

        let callback = Python::with_gil(|py| {
            let callable = wrap_pyfunction_bound!(receive_event, py).unwrap();
            TimeEventCallback::from(callable.into_py(py))
        });

        // Create a new LiveTimer with a stop time
        let clock = get_atomic_clock_realtime();
        let start_time = clock.get_time_ns();
        let interval_ns = 100 * NANOSECONDS_IN_MILLISECOND;
        let stop_time = start_time + 500 * NANOSECONDS_IN_MILLISECOND;
        let mut timer = LiveTimer::new(
            "TEST_TIMER",
            interval_ns,
            start_time,
            Some(stop_time),
            callback,
        );
        let next_time_ns = timer.next_time_ns();
        timer.start();

        // Wait for a longer time than the stop time
        tokio::time::sleep(Duration::from_secs(1)).await;

        wait_until(|| timer.is_expired(), Duration::from_secs(2));
        assert!(timer.next_time_ns() > next_time_ns);
    }

    #[tokio::test]
    async fn test_live_timer_with_zero_interval_and_immediate_stop_time() {
        pyo3::prepare_freethreaded_python();

        let callback = Python::with_gil(|py| {
            let callable = wrap_pyfunction_bound!(receive_event, py).unwrap();
            TimeEventCallback::from(callable.into_py(py))
        });

        // Create a new LiveTimer with a stop time
        let clock = get_atomic_clock_realtime();
        let start_time = UnixNanos::default();
        let interval_ns = 0;
        let stop_time = clock.get_time_ns();
        let mut timer = LiveTimer::new(
            "TEST_TIMER",
            interval_ns,
            start_time,
            Some(stop_time),
            callback,
        );
        timer.start();

        wait_until(|| timer.is_expired(), Duration::from_secs(2));
    }
}
