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

use chrono::{DateTime, Duration, Utc};
use nautilus_core::{UnixNanos, python::to_pyvalue_err};
use pyo3::prelude::*;

use super::timer::TimeEventHandler_Py;
use crate::{
    clock::{Clock, LiveClock, TestClock},
    timer::{TimeEvent, TimeEventCallback},
};

/// PyO3 compatible interface for an underlying [`TestClock`].
///
/// This struct wraps `TestClock` in a way that makes it possible to create
/// Python bindings for it.
///
/// It implements the `Deref` trait, allowing instances of `TestClock_API` to be
/// dereferenced to `TestClock`, providing access to `TestClock`'s methods without
/// having to manually access the underlying `TestClock` instance.
#[allow(non_camel_case_types)]
#[pyo3::pyclass(
    module = "nautilus_trader.core.nautilus_pyo3.common",
    name = "TestClock",
    unsendable
)]
#[derive(Debug)]
pub struct TestClock_Py(Box<TestClock>);

#[pymethods]
impl TestClock_Py {
    #[new]
    fn py_new() -> Self {
        Self(Box::default())
    }

    #[pyo3(name = "advance_time")]
    fn py_advance_time(&mut self, to_time_ns: u64, set_time: bool) -> Vec<TimeEvent> {
        self.0.advance_time(to_time_ns.into(), set_time)
    }

    #[pyo3(name = "match_handlers")]
    fn py_match_handlers(&self, events: Vec<TimeEvent>) -> Vec<TimeEventHandler_Py> {
        self.0
            .match_handlers(events)
            .into_iter()
            .map(Into::into)
            .collect()
    }

    #[pyo3(name = "register_default_handler")]
    fn py_register_default_handler(&mut self, callback: PyObject) {
        self.0
            .register_default_handler(TimeEventCallback::from(callback));
    }

    #[pyo3(
        name = "set_time_alert",
        signature = (name, alert_time, callback=None, allow_past=None)
    )]
    fn py_set_time_alert(
        &mut self,
        name: &str,
        alert_time: DateTime<Utc>,
        callback: Option<PyObject>,
        allow_past: Option<bool>,
    ) -> PyResult<()> {
        let alert_time_ns = UnixNanos::from(alert_time);
        self.0
            .set_time_alert_ns(
                name,
                alert_time_ns,
                callback.map(TimeEventCallback::from),
                allow_past,
            )
            .map_err(to_pyvalue_err)
    }

    #[pyo3(
        name = "set_time_alert_ns",
        signature = (name, alert_time_ns, callback=None, allow_past=None)
    )]
    fn py_set_time_alert_ns(
        &mut self,
        name: &str,
        alert_time_ns: u64,
        callback: Option<PyObject>,
        allow_past: Option<bool>,
    ) -> PyResult<()> {
        self.0
            .set_time_alert_ns(
                name,
                alert_time_ns.into(),
                callback.map(TimeEventCallback::from),
                allow_past,
            )
            .map_err(to_pyvalue_err)
    }

    #[allow(clippy::too_many_arguments)]
    #[pyo3(
        name = "set_timer",
        signature = (name, interval, start_time=None, stop_time=None, callback=None, allow_past=None, fire_immediately=None)
    )]
    fn py_set_timer(
        &mut self,
        name: &str,
        interval: Duration,
        start_time: Option<DateTime<Utc>>,
        stop_time: Option<DateTime<Utc>>,
        callback: Option<PyObject>,
        allow_past: Option<bool>,
        fire_immediately: Option<bool>,
    ) -> PyResult<()> {
        let interval_ns_i64 = interval
            .num_nanoseconds()
            .ok_or_else(|| pyo3::exceptions::PyValueError::new_err("Interval too large"))?;
        if interval_ns_i64 <= 0 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "Interval must be positive",
            ));
        }
        let interval_ns = interval_ns_i64 as u64;
        self.0
            .set_timer_ns(
                name,
                interval_ns,
                start_time.map(UnixNanos::from),
                stop_time.map(UnixNanos::from),
                callback.map(TimeEventCallback::from),
                allow_past,
                fire_immediately,
            )
            .map_err(to_pyvalue_err)
    }

    #[allow(clippy::too_many_arguments)]
    #[pyo3(
        name = "set_timer_ns",
        signature = (name, interval_ns, start_time_ns, stop_time_ns=None, callback=None, allow_past=None, fire_immediately=None)
    )]
    fn py_set_timer_ns(
        &mut self,
        name: &str,
        interval_ns: u64,
        start_time_ns: Option<u64>,
        stop_time_ns: Option<u64>,
        callback: Option<PyObject>,
        allow_past: Option<bool>,
        fire_immediately: Option<bool>,
    ) -> PyResult<()> {
        self.0
            .set_timer_ns(
                name,
                interval_ns,
                start_time_ns.map(UnixNanos::from),
                stop_time_ns.map(UnixNanos::from),
                callback.map(TimeEventCallback::from),
                allow_past,
                fire_immediately,
            )
            .map_err(to_pyvalue_err)
    }

    #[pyo3(name = "next_time_ns")]
    fn py_next_time_ns(&self, name: &str) -> Option<u64> {
        self.0.next_time_ns(name).map(|n| n.as_u64())
    }

    #[pyo3(name = "cancel_timer")]
    fn py_cancel_timer(&mut self, name: &str) {
        self.0.cancel_timer(name);
    }

    #[pyo3(name = "cancel_timers")]
    fn py_cancel_timers(&mut self) {
        self.0.cancel_timers();
    }
}

/// PyO3 compatible interface for an underlying [`LiveClock`].
///
/// This struct wraps `LiveClock` in a way that makes it possible to create
/// Python bindings for it.
///
/// It implements the `Deref` trait, allowing instances of `LiveClock_Py` to be
/// dereferenced to `LiveClock`, providing access to `LiveClock`'s methods without
/// having to manually access the underlying `LiveClock` instance.
#[allow(non_camel_case_types)]
#[pyo3::pyclass(
    module = "nautilus_trader.core.nautilus_pyo3.common",
    name = "LiveClock",
    unsendable
)]
#[derive(Debug)]
pub struct LiveClock_Py(Box<LiveClock>);

#[pymethods]
impl LiveClock_Py {
    #[new]
    fn py_new() -> Self {
        Self(Box::default())
    }

    #[pyo3(name = "register_default_handler")]
    fn py_register_default_handler(&mut self, callback: PyObject) {
        self.0
            .register_default_handler(TimeEventCallback::from(callback));
    }

    #[pyo3(
        name = "set_time_alert",
        signature = (name, alert_time, callback=None, allow_past=None)
    )]
    fn py_set_time_alert(
        &mut self,
        name: &str,
        alert_time: DateTime<Utc>,
        callback: Option<PyObject>,
        allow_past: Option<bool>,
    ) -> PyResult<()> {
        let alert_time_ns = UnixNanos::from(alert_time);
        self.0
            .set_time_alert_ns(
                name,
                alert_time_ns,
                callback.map(TimeEventCallback::from),
                allow_past,
            )
            .map_err(to_pyvalue_err)
    }

    #[pyo3(
        name = "set_time_alert_ns",
        signature = (name, alert_time_ns, callback=None, allow_past=None)
    )]
    fn py_set_time_alert_ns(
        &mut self,
        name: &str,
        alert_time_ns: u64,
        callback: Option<PyObject>,
        allow_past: Option<bool>,
    ) -> PyResult<()> {
        self.0
            .set_time_alert_ns(
                name,
                alert_time_ns.into(),
                callback.map(TimeEventCallback::from),
                allow_past,
            )
            .map_err(to_pyvalue_err)
    }

    #[allow(clippy::too_many_arguments)]
    #[pyo3(
        name = "set_timer",
        signature = (name, interval, start_time=None, stop_time=None, callback=None, allow_past=None, fire_immediately=None)
    )]
    fn py_set_timer(
        &mut self,
        name: &str,
        interval: Duration,
        start_time: Option<DateTime<Utc>>,
        stop_time: Option<DateTime<Utc>>,
        callback: Option<PyObject>,
        allow_past: Option<bool>,
        fire_immediately: Option<bool>,
    ) -> PyResult<()> {
        let interval_ns_i64 = interval
            .num_nanoseconds()
            .ok_or_else(|| pyo3::exceptions::PyValueError::new_err("Interval too large"))?;
        if interval_ns_i64 <= 0 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "Interval must be positive",
            ));
        }
        let interval_ns = interval_ns_i64 as u64;
        self.0
            .set_timer_ns(
                name,
                interval_ns,
                start_time.map(UnixNanos::from),
                stop_time.map(UnixNanos::from),
                callback.map(TimeEventCallback::from),
                allow_past,
                fire_immediately,
            )
            .map_err(to_pyvalue_err)
    }

    #[allow(clippy::too_many_arguments)]
    #[pyo3(
        name = "set_timer_ns",
        signature = (name, interval_ns, start_time_ns, stop_time_ns=None, callback=None, allow_past=None, fire_immediately=None)
    )]
    fn py_set_timer_ns(
        &mut self,
        name: &str,
        interval_ns: u64,
        start_time_ns: Option<u64>,
        stop_time_ns: Option<u64>,
        callback: Option<PyObject>,
        allow_past: Option<bool>,
        fire_immediately: Option<bool>,
    ) -> PyResult<()> {
        self.0
            .set_timer_ns(
                name,
                interval_ns,
                start_time_ns.map(UnixNanos::from),
                stop_time_ns.map(UnixNanos::from),
                callback.map(TimeEventCallback::from),
                allow_past,
                fire_immediately,
            )
            .map_err(to_pyvalue_err)
    }

    #[pyo3(name = "next_time_ns")]
    fn py_next_time_ns(&self, name: &str) -> Option<u64> {
        self.0.next_time_ns(name).map(|t| t.as_u64())
    }

    #[pyo3(name = "cancel_timer")]
    fn py_cancel_timer(&mut self, name: &str) {
        self.0.cancel_timer(name);
    }

    #[pyo3(name = "cancel_timers")]
    fn py_cancel_timers(&mut self) {
        self.0.cancel_timers();
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::{Duration, Utc};
    use nautilus_core::{UnixNanos, python::IntoPyObjectNautilusExt};
    use pyo3::{prelude::*, types::PyList};
    use rstest::*;

    use crate::{
        clock::{Clock, TestClock},
        python::clock::{LiveClock_Py, TestClock_Py},
        runner::{TimeEventSender, set_time_event_sender},
        timer::{TimeEventCallback, TimeEventHandlerV2},
    };

    fn ensure_sender() {
        if crate::runner::try_get_time_event_sender().is_none() {
            set_time_event_sender(Arc::new(DummySender));
        }
    }

    // Dummy TimeEventSender for LiveClock tests
    #[derive(Debug)]
    struct DummySender;

    impl TimeEventSender for DummySender {
        fn send(&self, _handler: TimeEventHandlerV2) {}
    }

    #[fixture]
    pub fn test_clock() -> TestClock {
        TestClock::new()
    }

    pub fn test_callback() -> TimeEventCallback {
        Python::with_gil(|py| {
            let py_list = PyList::empty(py);
            let py_append = Py::from(py_list.getattr("append").unwrap());
            let py_append = py_append.into_py_any_unwrap(py);
            TimeEventCallback::from(py_append)
        })
    }

    pub fn test_py_callback() -> PyObject {
        Python::with_gil(|py| {
            let py_list = PyList::empty(py);
            let py_append = Py::from(py_list.getattr("append").unwrap());
            py_append.into_py_any_unwrap(py)
        })
    }

    ////////////////////////////////////////////////////////////////////////////////
    // TestClock_Py
    ////////////////////////////////////////////////////////////////////////////////

    #[rstest]
    fn test_test_clock_py_set_time_alert() {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|_py| {
            let mut clock_py = TestClock_Py::py_new();
            let callback = test_py_callback();
            clock_py.py_register_default_handler(callback);
            let dt = Utc::now() + Duration::seconds(1);
            clock_py
                .py_set_time_alert("ALERT1", dt, None, None)
                .expect("set_time_alert failed");
        });
    }

    #[rstest]
    fn test_test_clock_py_set_timer() {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|_py| {
            let mut clock_py = TestClock_Py::py_new();
            let callback = test_py_callback();
            clock_py.py_register_default_handler(callback);
            let interval = Duration::seconds(2);
            clock_py
                .py_set_timer("TIMER1", interval, None, None, None, None, None)
                .expect("set_timer failed");
        });
    }

    #[rstest]
    fn test_test_clock_py_set_time_alert_ns() {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|_py| {
            let mut clock_py = TestClock_Py::py_new();
            let callback = test_py_callback();
            clock_py.py_register_default_handler(callback);
            let ts_ns = (Utc::now() + Duration::seconds(1))
                .timestamp_nanos_opt()
                .unwrap() as u64;
            clock_py
                .py_set_time_alert_ns("ALERT_NS", ts_ns, None, None)
                .expect("set_time_alert_ns failed");
        });
    }

    #[rstest]
    fn test_test_clock_py_set_timer_ns() {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|_py| {
            let mut clock_py = TestClock_Py::py_new();
            let callback = test_py_callback();
            clock_py.py_register_default_handler(callback);
            clock_py
                .py_set_timer_ns("TIMER_NS", 1_000_000, None, None, None, None, None)
                .expect("set_timer_ns failed");
        });
    }

    #[rstest]
    fn test_test_clock_raw_set_timer_ns(mut test_clock: TestClock) {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|_py| {
            let callback = test_callback();
            test_clock.register_default_handler(callback);

            let timer_name = "TEST_TIME1";
            test_clock
                .set_timer_ns(timer_name, 10, None, None, None, None, None)
                .unwrap();

            assert_eq!(test_clock.timer_names(), [timer_name]);
            assert_eq!(test_clock.timer_count(), 1);
        });
    }

    #[rstest]
    fn test_test_clock_cancel_timer(mut test_clock: TestClock) {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|_py| {
            let callback = test_callback();
            test_clock.register_default_handler(callback);

            let timer_name = "TEST_TIME1";
            test_clock
                .set_timer_ns(timer_name, 10, None, None, None, None, None)
                .unwrap();
            test_clock.cancel_timer(timer_name);

            assert!(test_clock.timer_names().is_empty());
            assert_eq!(test_clock.timer_count(), 0);
        });
    }

    #[rstest]
    fn test_test_clock_cancel_timers(mut test_clock: TestClock) {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|_py| {
            let callback = test_callback();
            test_clock.register_default_handler(callback);

            let timer_name = "TEST_TIME1";
            test_clock
                .set_timer_ns(timer_name, 10, None, None, None, None, None)
                .unwrap();
            test_clock.cancel_timers();

            assert!(test_clock.timer_names().is_empty());
            assert_eq!(test_clock.timer_count(), 0);
        });
    }

    #[rstest]
    fn test_test_clock_advance_within_stop_time_py(mut test_clock: TestClock) {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|_py| {
            let callback = test_callback();
            test_clock.register_default_handler(callback);

            let timer_name = "TEST_TIME1";
            test_clock
                .set_timer_ns(
                    timer_name,
                    1,
                    Some(UnixNanos::from(1)),
                    Some(UnixNanos::from(3)),
                    None,
                    None,
                    None,
                )
                .unwrap();
            test_clock.advance_time(2.into(), true);

            assert_eq!(test_clock.timer_names(), [timer_name]);
            assert_eq!(test_clock.timer_count(), 1);
        });
    }

    #[rstest]
    fn test_test_clock_advance_time_to_stop_time_with_set_time_true(mut test_clock: TestClock) {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|_py| {
            let callback = test_callback();
            test_clock.register_default_handler(callback);

            test_clock
                .set_timer_ns(
                    "TEST_TIME1",
                    2,
                    None,
                    Some(UnixNanos::from(3)),
                    None,
                    None,
                    None,
                )
                .unwrap();
            test_clock.advance_time(3.into(), true);

            assert_eq!(test_clock.timer_names().len(), 1);
            assert_eq!(test_clock.timer_count(), 1);
            assert_eq!(test_clock.get_time_ns(), 3);
        });
    }

    #[rstest]
    fn test_test_clock_advance_time_to_stop_time_with_set_time_false(mut test_clock: TestClock) {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|_py| {
            let callback = test_callback();
            test_clock.register_default_handler(callback);

            test_clock
                .set_timer_ns(
                    "TEST_TIME1",
                    2,
                    None,
                    Some(UnixNanos::from(3)),
                    None,
                    None,
                    None,
                )
                .unwrap();
            test_clock.advance_time(3.into(), false);

            assert_eq!(test_clock.timer_names().len(), 1);
            assert_eq!(test_clock.timer_count(), 1);
            assert_eq!(test_clock.get_time_ns(), 0);
        });
    }

    ////////////////////////////////////////////////////////////////////////////////
    // LiveClock_Py
    ////////////////////////////////////////////////////////////////////////////////

    #[rstest]
    fn test_live_clock_py_set_time_alert() {
        pyo3::prepare_freethreaded_python();
        ensure_sender();

        Python::with_gil(|_py| {
            let mut live_clock_py = LiveClock_Py::py_new();
            let callback = test_py_callback();
            live_clock_py.py_register_default_handler(callback);
            let dt = Utc::now() + Duration::seconds(1);

            live_clock_py
                .py_set_time_alert("ALERT1", dt, None, None)
                .expect("live set_time_alert failed");
        });
    }

    #[rstest]
    fn test_live_clock_py_set_timer() {
        pyo3::prepare_freethreaded_python();
        ensure_sender();

        Python::with_gil(|_py| {
            let mut live_clock_py = LiveClock_Py::py_new();
            let callback = test_py_callback();
            live_clock_py.py_register_default_handler(callback);
            let interval = Duration::seconds(3);

            live_clock_py
                .py_set_timer("TIMER1", interval, None, None, None, None, None)
                .expect("live set_timer failed");
        });
    }

    #[rstest]
    fn test_live_clock_py_set_time_alert_ns() {
        pyo3::prepare_freethreaded_python();
        ensure_sender();

        Python::with_gil(|_py| {
            let mut live_clock_py = LiveClock_Py::py_new();
            let callback = test_py_callback();
            live_clock_py.py_register_default_handler(callback);
            let dt_ns = (Utc::now() + Duration::seconds(1))
                .timestamp_nanos_opt()
                .unwrap() as u64;

            live_clock_py
                .py_set_time_alert_ns("ALERT_NS", dt_ns, None, None)
                .expect("live set_time_alert_ns failed");
        });
    }

    #[rstest]
    fn test_live_clock_py_set_timer_ns() {
        pyo3::prepare_freethreaded_python();
        ensure_sender();

        Python::with_gil(|_py| {
            let mut live_clock_py = LiveClock_Py::py_new();
            let callback = test_py_callback();
            live_clock_py.py_register_default_handler(callback);
            let interval_ns = 1_000_000_000_u64; // 1 second

            live_clock_py
                .py_set_timer_ns("TIMER_NS", interval_ns, None, None, None, None, None)
                .expect("live set_timer_ns failed");
        });
    }
}
