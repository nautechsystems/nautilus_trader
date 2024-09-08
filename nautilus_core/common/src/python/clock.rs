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

// use crate::{
//     clock::{Clock, LiveClock, TestClock},
//     handlers::EventHandler,
//     timer::{LiveTimer, TestTimer, TimeEvent, TimeEventHandler},
// };
// use nautilus_core::{nanos::UnixNanos, time::AtomicTime};
// use pyo3::prelude::*;
// use pyo3::types::{PyString, PyTuple};
// use std::{collections::HashMap, ops::Deref};
// use ustr::Ustr;

// /// PyO3 compatible Foreign Function Interface (FFI) for an underlying [`TestClock`].
// ///
// /// This struct wraps `TestClock` in a way that makes it possible to create
// /// Python bindings for it.
// ///
// /// It implements the `Deref` trait, allowing instances of `TestClock_API` to be
// /// dereferenced to `TestClock`, providing access to `TestClock`'s methods without
// /// having to manually access the underlying `TestClock` instance.
// #[allow(non_camel_case_types)]
// #[pyo3::pyclass(
//     module = "nautilus_trader.core.nautilus_pyo3.common",
//     name = "TestClock"
// )]
// pub struct TestClock_Py(Box<TestClock>);

// #[pymethods]
// impl TestClock_Py {
//     #[new]
//     fn py_new() -> Self {
//         TestClock_Py(Box::new(TestClock::new()))
//     }

//     fn advance_time(&mut self, to_time_ns: u64, set_time: bool) -> Vec<TimeEvent> {
//         self.0.advance_time(to_time_ns.into(), set_time)
//     }

//     fn match_handlers(&self, events: Vec<TimeEvent>) -> Vec<TimeEventHandler> {
//         self.0.match_handlers(events)
//     }

//     fn register_default_handler(&mut self, callback: EventHandler) {
//         self.0.register_default_handler(callback)
//     }

//     fn set_time_alert_ns(
//         &mut self,
//         name: &str,
//         alert_time_ns: u64,
//         callback: Option<EventHandler>,
//     ) {
//         self.0
//             .set_time_alert_ns(name, alert_time_ns.into(), callback)
//     }

//     fn set_timer_ns(
//         &mut self,
//         name: &str,
//         interval_ns: u64,
//         start_time_ns: u64,
//         stop_time_ns: Option<u64>,
//         callback: Option<EventHandler>,
//     ) {
//         self.0.set_timer_ns(
//             name,
//             interval_ns,
//             start_time_ns.into(),
//             stop_time_ns.map(UnixNanos::from),
//             callback,
//         )
//     }

//     fn next_time_ns(&self, name: &str) -> UnixNanos {
//         self.next_time_ns(name)
//     }

//     fn cancel_timer(&mut self, name: &str) {
//         self.cancel_timer(name)
//     }

//     fn cancel_timers(&mut self) {
//         self.cancel_timers()
//     }
// }

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use nautilus_core::nanos::UnixNanos;
    use pyo3::{prelude::*, types::PyList};
    use rstest::*;

    use super::*;
    use crate::{
        clock::{Clock, TestClock},
        python::timer::TimeEventCallback_Py,
        timer::{ShareableTimeEventCallback, TimeEventCallback},
    };

    #[fixture]
    pub fn test_clock() -> TestClock {
        TestClock::new()
    }

    pub fn test_callback() -> ShareableTimeEventCallback {
        Python::with_gil(|py| {
            let py_list = PyList::empty(py);
            let py_append = Py::from(py_list.getattr("append").unwrap());
            let callback: Rc<dyn TimeEventCallback> =
                Rc::new(TimeEventCallback_Py::new(py_append.into_py(py)));
            ShareableTimeEventCallback(callback)
        })
    }

    #[rstest]
    fn test_set_timer_ns_py(mut test_clock: TestClock) {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let callback = test_callback();
            test_clock.register_default_handler(callback);

            let timer_name = "TEST_TIME1";
            test_clock.set_timer_ns(timer_name, 10, 0.into(), None, None);

            assert_eq!(test_clock.timer_names(), [timer_name]);
            assert_eq!(test_clock.timer_count(), 1);
        });
    }

    #[rstest]
    fn test_cancel_timer(mut test_clock: TestClock) {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let callback = test_callback();
            test_clock.register_default_handler(callback);

            let timer_name = "TEST_TIME1";
            test_clock.set_timer_ns(timer_name, 10, 0.into(), None, None);
            test_clock.cancel_timer(timer_name);

            assert!(test_clock.timer_names().is_empty());
            assert_eq!(test_clock.timer_count(), 0);
        });
    }

    #[rstest]
    fn test_cancel_timers(mut test_clock: TestClock) {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let callback = test_callback();
            test_clock.register_default_handler(callback);

            let timer_name = "TEST_TIME1";
            test_clock.set_timer_ns(timer_name, 10, 0.into(), None, None);
            test_clock.cancel_timers();

            assert!(test_clock.timer_names().is_empty());
            assert_eq!(test_clock.timer_count(), 0);
        });
    }

    #[rstest]
    fn test_advance_within_stop_time_py(mut test_clock: TestClock) {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let callback = test_callback();
            test_clock.register_default_handler(callback);

            let timer_name = "TEST_TIME1";
            test_clock.set_timer_ns(timer_name, 1, 1.into(), Some(UnixNanos::from(3)), None);
            test_clock.advance_time(2.into(), true);

            assert_eq!(test_clock.timer_names(), [timer_name]);
            assert_eq!(test_clock.timer_count(), 1);
        });
    }

    #[rstest]
    fn test_advance_time_to_stop_time_with_set_time_true(mut test_clock: TestClock) {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let callback = test_callback();
            test_clock.register_default_handler(callback);

            test_clock.set_timer_ns("TEST_TIME1", 2, 0.into(), Some(UnixNanos::from(3)), None);
            test_clock.advance_time(3.into(), true);

            assert_eq!(test_clock.timer_names().len(), 1);
            assert_eq!(test_clock.timer_count(), 1);
            assert_eq!(test_clock.get_time_ns(), 3);
        });
    }

    #[rstest]
    fn test_advance_time_to_stop_time_with_set_time_false(mut test_clock: TestClock) {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let callback = test_callback();
            test_clock.register_default_handler(callback);

            test_clock.set_timer_ns("TEST_TIME1", 2, 0.into(), Some(UnixNanos::from(3)), None);
            test_clock.advance_time(3.into(), false);

            assert_eq!(test_clock.timer_names().len(), 1);
            assert_eq!(test_clock.timer_count(), 1);
            assert_eq!(test_clock.get_time_ns(), 0);
        });
    }
}
