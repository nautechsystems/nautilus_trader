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

use std::str::FromStr;

use pyo3::{prelude::*, types::*, AsPyPointer};

use nautilus_common::clock::{test_clock_new, test_clock_set_time_alert_ns};

#[test]
fn test_clock_advance() {
    pyo3::prepare_freethreaded_python();

    let mut clock = test_clock_new();

    let timer_name = "tringtring";
    let name = Python::with_gil(|py| PyString::new(py, timer_name).as_ptr());

    unsafe {
        test_clock_set_time_alert_ns(&mut clock, name, 2_000);
    }

    assert_eq!(clock.timers.len(), 1);
    assert_eq!(clock.timers.keys().next().unwrap().as_str(), timer_name);

    let events = clock.advance_time(3_000, true);

    assert_eq!(clock.timers.values().next().unwrap().is_expired, true);
    assert_eq!(events.len(), 1);
    assert_eq!(
        events.iter().next().unwrap().name.to_string(),
        String::from_str(timer_name).unwrap()
    );
}

#[test]
fn test_clock_event_callback() {
    pyo3::prepare_freethreaded_python();

    let mut test_clock = Python::with_gil(|_py| test_clock_new());

    let timer_name = "tringtring";
    let name = Python::with_gil(|py| PyString::new(py, timer_name).as_ptr());

    unsafe {
        test_clock_set_time_alert_ns(&mut test_clock, name, 2_000);
    }

    let events = test_clock.advance_time(3_000, true);
    assert_eq!(events.len(), 1); // TODO
}
