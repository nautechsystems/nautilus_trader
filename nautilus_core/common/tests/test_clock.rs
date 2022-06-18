use nautilus_common::clock::{new_test_clock, set_time_alert_ns};
use pyo3::{prelude::*, types::*};
use std::str::FromStr;

#[test]
fn test_clock_advance() {
    pyo3::prepare_freethreaded_python();
    let mut test_clock = Python::with_gil(|py| {
        let dummy = PyDict::new(py).into();
        new_test_clock(0, dummy)
    });

    assert_eq!(test_clock.time_ns, 0);
    let timer_name = "tringtring";

    let (name, callback) = Python::with_gil(|py| {
        let name = PyString::new(py, timer_name).into();
        let dummy = Some(PyDict::new(py).into());
        (name, dummy)
    });

    unsafe {
        set_time_alert_ns(&mut test_clock, name, 2_000, callback);
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
    let mut test_clock = Python::with_gil(|py| {
        let dummy = PyDict::new(py).into();
        new_test_clock(0, dummy)
    });

    let (name, callback, pymod): (PyObject, PyObject, PyObject) = Python::with_gil(|py| {
        let code = include_str!("./data/callback.py");
        let pymod = PyModule::from_code(py, &code, "humpty", "dumpty").unwrap();
        let name = PyString::new(py, "brrrringbrrring");
        let callback = pymod.getattr("increment").unwrap();
        (name.into(), callback.into(), pymod.into())
    });

    unsafe {
        set_time_alert_ns(&mut test_clock, name, 2_000, Some(callback));
    }

    let events = test_clock.advance_time(3_000);
    events
        .into_iter()
        .for_each(|time_event_handler| time_event_handler.handle());

    let count: u64 =
        Python::with_gil(|py| pymod.getattr(py, "count").unwrap().extract(py).unwrap());

    assert_eq!(count, 1);
}
