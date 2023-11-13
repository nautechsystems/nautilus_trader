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
    ffi::c_char,
    ops::{Deref, DerefMut},
    str::FromStr,
};

use nautilus_core::ffi::{
    cvec::CVec,
    string::{cstr_to_string, cstr_to_ustr, optional_cstr_to_string},
};
use nautilus_model::identifiers::trader_id::TraderId;
use pyo3::{
    ffi,
    prelude::*,
    types::{PyList, PyString},
    AsPyPointer, Python,
};

use crate::{
    handlers::{MessageHandler, PyCallableWrapper},
    msgbus::{is_matching, MessageBus, Subscription},
};

/// Provides a C compatible Foreign Function Interface (FFI) for an underlying [`MessageBus`].
///
/// This struct wraps `MessageBus` in a way that makes it compatible with C function
/// calls, enabling interaction with `MessageBus` in a C environment.
///
/// It implements the `Deref` trait, allowing instances of `MessageBus_API` to be
/// dereferenced to `MessageBus`, providing access to `TestClock`'s methods without
/// having to manually access the underlying `MessageBus` instance.
#[allow(non_camel_case_types)]
#[repr(C)]
pub struct MessageBus_API(Box<MessageBus>);

impl Deref for MessageBus_API {
    type Target = MessageBus;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for MessageBus_API {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// # Safety
///
/// - Assumes `trader_id_ptr` is a valid C string pointer.
/// - Assumes `name_ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn msgbus_new(
    trader_id_ptr: *const c_char,
    name_ptr: *const c_char,
) -> MessageBus_API {
    let trader_id = TraderId::from_str(&cstr_to_string(trader_id_ptr)).unwrap();
    let name = optional_cstr_to_string(name_ptr);
    MessageBus_API(Box::new(MessageBus::new(trader_id, name)))
}

#[no_mangle]
pub extern "C" fn msgbus_endpoints(bus: MessageBus_API) -> *const ffi::PyObject {
    Python::with_gil(|py| -> Py<PyList> {
        let endpoints: Vec<Py<PyString>> = bus
            .endpoints()
            .into_iter()
            .map(|k| PyString::new(py, k).into())
            .collect();
        PyList::new(py, endpoints).into()
    })
    .as_ptr()
}

#[no_mangle]
pub extern "C" fn msgbus_topics(bus: MessageBus_API) -> *const ffi::PyObject {
    Python::with_gil(|py| -> Py<PyList> {
        let topics: Vec<Py<PyString>> = bus
            .endpoints()
            .into_iter()
            .map(|k| PyString::new(py, k).into())
            .collect();
        PyList::new(py, topics).into()
    })
    .as_ptr()
}

/// # Safety
///
/// - Assumes `handler_id_ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn msgbus_subscribe(
    mut bus: MessageBus_API,
    topic_ptr: *const c_char,
    handler_id_ptr: *const c_char,
    py_callable_ptr: *mut ffi::PyObject,
    priority: u8,
) {
    let topic = cstr_to_ustr(topic_ptr);
    let handler_id = cstr_to_ustr(handler_id_ptr);
    let py_callable = PyCallableWrapper {
        ptr: py_callable_ptr,
    };
    let handler = MessageHandler::new(handler_id, Some(py_callable), None);

    bus.subscribe(&topic, handler, Some(priority));
}

/// # Safety
///
/// - Assumes `endpoint_ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn msgbus_get_endpoint(
    bus: MessageBus_API,
    endpoint_ptr: *const c_char,
) -> *const ffi::PyObject {
    let endpoint = cstr_to_ustr(endpoint_ptr);

    match bus.get_endpoint(&endpoint) {
        Some(handler) => handler.py_callback.unwrap().ptr,
        None => ffi::Py_None(),
    }
}

/// # Safety
///
/// - Assumes `pattern_ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn msgbus_get_matching_callables(
    mut bus: MessageBus_API,
    pattern_ptr: *const c_char,
) -> CVec {
    let pattern = cstr_to_ustr(pattern_ptr);
    let subs: Vec<&Subscription> = bus.get_matching_subscriptions(&pattern);

    subs.iter()
        .map(|s| s.handler.py_callback.unwrap())
        .collect::<Vec<PyCallableWrapper>>()
        .into()
}

#[allow(clippy::drop_non_drop)]
#[no_mangle]
pub extern "C" fn vec_pycallable_drop(v: CVec) {
    let CVec { ptr, len, cap } = v;
    let data: Vec<PyCallableWrapper> =
        unsafe { Vec::from_raw_parts(ptr.cast::<PyCallableWrapper>(), len, cap) };
    drop(data); // Memory freed here
}

/// # Safety
///
/// - Assumes `topic_ptr` is a valid C string pointer.
/// - Assumes `pattern_ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn msgbus_is_matching(
    topic_ptr: *const c_char,
    pattern_ptr: *const c_char,
) -> u8 {
    let topic = cstr_to_ustr(topic_ptr);
    let pattern = cstr_to_ustr(pattern_ptr);

    is_matching(&topic, &pattern) as u8
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use nautilus_core::message::Message;
    use rstest::*;
    use ustr::Ustr;

    use super::*;
    use crate::handlers::MessageHandler;

    #[rstest]
    fn test_subscribe_rust_handler() {
        let trader_id = TraderId::from("trader-001");
        let topic = "my-topic".to_string();

        // TODO: Create a Python list and pass the message in a closure to the `append` method
        let callback = Rc::new(|_m: Message| Python::with_gil(|_| {}));
        let handler_id = Ustr::from("id_of_method");
        let handler = MessageHandler::new(handler_id, None, Some(callback));

        let mut msgbus = MessageBus::new(trader_id, None);
        msgbus.subscribe(&topic, handler, None);

        assert!(msgbus.has_subscribers(&topic));
        assert_eq!(msgbus.topics(), vec![topic]);
    }
}
