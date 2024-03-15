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

////////////////////////////////////////////////////////////////////////////////
// Stubs
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
pub mod stubs {
    use rstest::fixture;

    use crate::clock::TestClock;

    #[fixture]
    pub fn test_clock() -> TestClock {
        TestClock::new()
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use pyo3::{prelude::*, types::PyList};
    use rstest::*;
    use stubs::*;

    use super::*;
    use crate::{
        clock::{Clock, TestClock},
        handlers::EventHandler,
    };

    #[rstest]
    fn test_set_timer_ns_py(mut test_clock: TestClock) {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let py_list = PyList::empty(py);
            let py_append = Py::from(py_list.getattr("append").unwrap());
            let handler = EventHandler::new(py_append);
            test_clock.register_default_handler(handler);

            let timer_name = "TEST_TIME1";
            test_clock.set_timer_ns(timer_name, 10, 0, None, None);

            assert_eq!(test_clock.timer_names(), [timer_name]);
            assert_eq!(test_clock.timer_count(), 1);
        });
    }

    #[rstest]
    fn test_cancel_timer(mut test_clock: TestClock) {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let py_list = PyList::empty(py);
            let py_append = Py::from(py_list.getattr("append").unwrap());
            let handler = EventHandler::new(py_append);
            test_clock.register_default_handler(handler);

            let timer_name = "TEST_TIME1";
            test_clock.set_timer_ns(timer_name, 10, 0, None, None);
            test_clock.cancel_timer(timer_name);

            assert!(test_clock.timer_names().is_empty());
            assert_eq!(test_clock.timer_count(), 0);
        });
    }

    #[rstest]
    fn test_cancel_timers(mut test_clock: TestClock) {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let py_list = PyList::empty(py);
            let py_append = Py::from(py_list.getattr("append").unwrap());
            let handler = EventHandler::new(py_append);
            test_clock.register_default_handler(handler);

            let timer_name = "TEST_TIME1";
            test_clock.set_timer_ns(timer_name, 10, 0, None, None);
            test_clock.cancel_timers();

            assert!(test_clock.timer_names().is_empty());
            assert_eq!(test_clock.timer_count(), 0);
        });
    }

    #[rstest]
    fn test_advance_within_stop_time_py(mut test_clock: TestClock) {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let py_list = PyList::empty(py);
            let py_append = Py::from(py_list.getattr("append").unwrap());
            let handler = EventHandler::new(py_append);
            test_clock.register_default_handler(handler);

            let timer_name = "TEST_TIME1";
            test_clock.set_timer_ns(timer_name, 1, 1, Some(3), None);
            test_clock.advance_time(2, true);

            assert_eq!(test_clock.timer_names(), [timer_name]);
            assert_eq!(test_clock.timer_count(), 1);
        });
    }

    #[rstest]
    fn test_advance_time_to_stop_time_with_set_time_true(mut test_clock: TestClock) {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let py_list = PyList::empty(py);
            let py_append = Py::from(py_list.getattr("append").unwrap());
            let handler = EventHandler::new(py_append);
            test_clock.register_default_handler(handler);

            test_clock.set_timer_ns("TEST_TIME1", 2, 0, Some(3), None);
            test_clock.advance_time(3, true);

            assert_eq!(test_clock.timer_names().len(), 1);
            assert_eq!(test_clock.timer_count(), 1);
            assert_eq!(test_clock.get_time_ns(), 3);
        });
    }

    #[rstest]
    fn test_advance_time_to_stop_time_with_set_time_false(mut test_clock: TestClock) {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let py_list = PyList::empty(py);
            let py_append = Py::from(py_list.getattr("append").unwrap());
            let handler = EventHandler::new(py_append);
            test_clock.register_default_handler(handler);

            test_clock.set_timer_ns("TEST_TIME1", 2, 0, Some(3), None);
            test_clock.advance_time(3, false);

            assert_eq!(test_clock.timer_names().len(), 1);
            assert_eq!(test_clock.timer_count(), 1);
            assert_eq!(test_clock.get_time_ns(), 0);
        });
    }
}
