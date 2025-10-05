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

use std::{cell::RefCell, rc::Rc};

use chrono::{DateTime, Duration, Utc};
use nautilus_core::{UnixNanos, python::to_pyvalue_err};
use pyo3::prelude::*;

use crate::{
    clock::{Clock, LiveClock, TestClock},
    timer::TimeEventCallback,
};

/// Unified PyO3 interface over both [`TestClock`] and [`LiveClock`].
///
/// A `PyClock` instance owns a boxed trait object implementing [`Clock`].  It
/// delegates method calls to this inner clock, allowing a single Python class
/// to transparently wrap either implementation and eliminating the large
/// amount of duplicated glue code previously required.
///
/// It intentionally does **not** expose a `__new__` constructor to Python –
/// clocks should be created from Rust and handed over to Python as needed.
#[allow(non_camel_case_types)]
#[pyo3::pyclass(
    module = "nautilus_trader.core.nautilus_pyo3.common",
    name = "Clock",
    unsendable
)]
#[derive(Debug, Clone)]
pub struct PyClock(Rc<RefCell<dyn Clock>>);

#[pymethods]
impl PyClock {
    #[pyo3(name = "register_default_handler")]
    fn py_register_default_handler(&mut self, callback: Py<PyAny>) {
        self.0
            .borrow_mut()
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
        callback: Option<Py<PyAny>>,
        allow_past: Option<bool>,
    ) -> PyResult<()> {
        self.0
            .borrow_mut()
            .set_time_alert(
                name,
                alert_time,
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
        callback: Option<Py<PyAny>>,
        allow_past: Option<bool>,
    ) -> PyResult<()> {
        self.0
            .borrow_mut()
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
        callback: Option<Py<PyAny>>,
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
            .borrow_mut()
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
        signature = (name, interval_ns, start_time_ns=None, stop_time_ns=None, callback=None, allow_past=None, fire_immediately=None)
    )]
    fn py_set_timer_ns(
        &mut self,
        name: &str,
        interval_ns: u64,
        start_time_ns: Option<u64>,
        stop_time_ns: Option<u64>,
        callback: Option<Py<PyAny>>,
        allow_past: Option<bool>,
        fire_immediately: Option<bool>,
    ) -> PyResult<()> {
        self.0
            .borrow_mut()
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
        self.0.borrow().next_time_ns(name).map(|t| t.as_u64())
    }

    #[pyo3(name = "cancel_timer")]
    fn py_cancel_timer(&mut self, name: &str) {
        self.0.borrow_mut().cancel_timer(name);
    }

    #[pyo3(name = "cancel_timers")]
    fn py_cancel_timers(&mut self) {
        self.0.borrow_mut().cancel_timers();
    }
}

impl PyClock {
    /// Creates a `PyClock` directly from an `Rc<RefCell<dyn Clock>>`.
    #[must_use]
    pub fn from_rc(rc: Rc<RefCell<dyn Clock>>) -> Self {
        Self(rc)
    }

    /// Creates a clock backed by [`TestClock`].
    #[must_use]
    pub fn new_test() -> Self {
        Self(Rc::new(RefCell::new(TestClock::default())))
    }

    /// Creates a clock backed by [`LiveClock`].
    #[must_use]
    pub fn new_live() -> Self {
        Self(Rc::new(RefCell::new(LiveClock::default())))
    }

    /// Provides access to the inner [`Clock`] trait object.
    #[must_use]
    pub fn inner(&self) -> std::cell::Ref<'_, dyn Clock> {
        self.0.borrow()
    }

    /// Mutably accesses the underlying [`Clock`].
    #[must_use]
    pub fn inner_mut(&mut self) -> std::cell::RefMut<'_, dyn Clock> {
        self.0.borrow_mut()
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
        python::clock::PyClock,
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
        Python::initialize();
        Python::attach(|py| {
            let py_list = PyList::empty(py);
            let py_append = Py::from(py_list.getattr("append").unwrap());
            let py_append = py_append.into_py_any_unwrap(py);
            TimeEventCallback::from(py_append)
        })
    }

    pub fn test_py_callback() -> Py<PyAny> {
        Python::initialize();
        Python::attach(|py| {
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
        Python::initialize();
        Python::attach(|_py| {
            let mut py_clock = PyClock::new_test();
            let callback = test_py_callback();
            py_clock.py_register_default_handler(callback);
            let dt = Utc::now() + Duration::seconds(1);
            py_clock
                .py_set_time_alert("ALERT1", dt, None, None)
                .expect("set_time_alert failed");
        });
    }

    #[rstest]
    fn test_test_clock_py_set_timer() {
        Python::initialize();
        Python::attach(|_py| {
            let mut py_clock = PyClock::new_test();
            let callback = test_py_callback();
            py_clock.py_register_default_handler(callback);
            let interval = Duration::seconds(2);
            py_clock
                .py_set_timer("TIMER1", interval, None, None, None, None, None)
                .expect("set_timer failed");
        });
    }

    #[rstest]
    fn test_test_clock_py_set_time_alert_ns() {
        Python::initialize();
        Python::attach(|_py| {
            let mut py_clock = PyClock::new_test();
            let callback = test_py_callback();
            py_clock.py_register_default_handler(callback);
            let ts_ns = (Utc::now() + Duration::seconds(1))
                .timestamp_nanos_opt()
                .unwrap() as u64;
            py_clock
                .py_set_time_alert_ns("ALERT_NS", ts_ns, None, None)
                .expect("set_time_alert_ns failed");
        });
    }

    #[rstest]
    fn test_test_clock_py_set_timer_ns() {
        Python::initialize();
        Python::attach(|_py| {
            let mut py_clock = PyClock::new_test();
            let callback = test_py_callback();
            py_clock.py_register_default_handler(callback);
            py_clock
                .py_set_timer_ns("TIMER_NS", 1_000_000, None, None, None, None, None)
                .expect("set_timer_ns failed");
        });
    }

    #[rstest]
    fn test_test_clock_raw_set_timer_ns(mut test_clock: TestClock) {
        Python::initialize();
        Python::attach(|_py| {
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
        Python::initialize();
        Python::attach(|_py| {
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
        Python::initialize();
        Python::attach(|_py| {
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
        Python::initialize();
        Python::attach(|_py| {
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
        Python::initialize();
        Python::attach(|_py| {
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
        Python::initialize();
        Python::attach(|_py| {
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
        ensure_sender();

        Python::initialize();
        Python::attach(|_py| {
            let mut py_clock = PyClock::new_live();
            let callback = test_py_callback();
            py_clock.py_register_default_handler(callback);
            let dt = Utc::now() + Duration::seconds(1);

            py_clock
                .py_set_time_alert("ALERT1", dt, None, None)
                .expect("live set_time_alert failed");
        });
    }

    #[rstest]
    fn test_live_clock_py_set_timer() {
        ensure_sender();

        Python::initialize();
        Python::attach(|_py| {
            let mut py_clock = PyClock::new_live();
            let callback = test_py_callback();
            py_clock.py_register_default_handler(callback);
            let interval = Duration::seconds(3);

            py_clock
                .py_set_timer("TIMER1", interval, None, None, None, None, None)
                .expect("live set_timer failed");
        });
    }

    #[rstest]
    fn test_live_clock_py_set_time_alert_ns() {
        ensure_sender();

        Python::initialize();
        Python::attach(|_py| {
            let mut py_clock = PyClock::new_live();
            let callback = test_py_callback();
            py_clock.py_register_default_handler(callback);
            let dt_ns = (Utc::now() + Duration::seconds(1))
                .timestamp_nanos_opt()
                .unwrap() as u64;

            py_clock
                .py_set_time_alert_ns("ALERT_NS", dt_ns, None, None)
                .expect("live set_time_alert_ns failed");
        });
    }

    #[rstest]
    fn test_live_clock_py_set_timer_ns() {
        ensure_sender();

        Python::initialize();
        Python::attach(|_py| {
            let mut py_clock = PyClock::new_live();
            let callback = test_py_callback();
            py_clock.py_register_default_handler(callback);
            let interval_ns = 1_000_000_000_u64; // 1 second

            py_clock
                .py_set_timer_ns("TIMER_NS", interval_ns, None, None, None, None, None)
                .expect("live set_timer_ns failed");
        });
    }
}
