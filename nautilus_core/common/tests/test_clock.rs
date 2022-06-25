// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_common::clock::{test_clock_new, test_clock_set_time_alert_ns};
use pyo3::{prelude::*, types::*};
use std::str::FromStr;

#[test]
fn test_clock_advance() {
    pyo3::prepare_freethreaded_python();

    let mut test_clock = Python::with_gil(|_py| test_clock_new());

    assert_eq!(test_clock.time_ns, 0);
    let timer_name = "tringtring";

    let (name, callback) = Python::with_gil(|py| {
        let name = PyString::new(py, timer_name).into();
        let dummy = Some(PyDict::new(py).into());
        (name, dummy)
    });

    unsafe {
        test_clock_set_time_alert_ns(&mut test_clock, name, 2_000, callback);
    }

    assert_eq!(test_clock.timers.len(), 1);
    assert_eq!(
        test_clock.timers.keys().next().unwrap().as_str(),
        timer_name
    );

    let events = test_clock.advance_time(3_000);

    assert_eq!(test_clock.timers.values().next().unwrap().is_expired, true);
    assert_eq!(events.len(), 1);
    assert_eq!(
        events.iter().next().unwrap().event.name.to_string(),
        String::from_str(timer_name).unwrap()
    );
}

#[test]
fn test_clock_even_callback() {
    pyo3::prepare_freethreaded_python();

    let mut test_clock = Python::with_gil(|_py| test_clock_new());

    let (name, callback, pymod): (PyObject, PyObject, PyObject) = Python::with_gil(|py| {
        let code = include_str!("callback.py");
        let pymod = PyModule::from_code(py, &code, "humpty", "dumpty").unwrap();
        let name = PyString::new(py, "brrrringbrrring");
        let callback = pymod.getattr("increment").unwrap();
        (name.into(), callback.into(), pymod.into())
    });

    unsafe {
        test_clock_set_time_alert_ns(&mut test_clock, name, 2_000, Some(callback));
    }

    let events = test_clock.advance_time(3_000);
    events
        .into_iter()
        .for_each(|time_event_handler| time_event_handler.handle());

    let count: u64 =
        Python::with_gil(|py| pymod.getattr(py, "count").unwrap().extract(py).unwrap());

    assert_eq!(count, 1);
}
