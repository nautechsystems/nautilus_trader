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

use nautilus_core::{
    ffi::{
        cvec::CVec,
        string::{cstr_to_string, cstr_to_ustr, optional_cstr_to_string},
    },
    uuid::UUID4,
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
pub extern "C" fn msgbus_endpoints(bus: MessageBus_API) -> *mut ffi::PyObject {
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
pub extern "C" fn msgbus_topics(bus: MessageBus_API) -> *mut ffi::PyObject {
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
/// - Assumes `pattern_ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn msgbus_has_subscribers(
    bus: MessageBus_API,
    pattern_ptr: *const c_char,
) -> u8 {
    let pattern = cstr_to_ustr(pattern_ptr);
    bus.has_subscribers(pattern.as_str()) as u8
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
) -> *mut ffi::PyObject {
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
    let subs: Vec<&Subscription> = bus.matching_subscriptions(&pattern);

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
/// - Assumes `pattern_ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn msgbus_request_handler(
    mut bus: MessageBus_API,
    endpoint_ptr: *const c_char,
    request_id: UUID4,
) -> *mut ffi::PyObject {
    let endpoint = cstr_to_ustr(endpoint_ptr);
    let handler = bus.request_handler(&endpoint, request_id);

    if let Some(handler) = handler {
        handler.py_callback.unwrap().ptr
    } else {
        ffi::Py_None()
    }
}

/// # Safety
///
/// - Assumes `pattern_ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn msgbus_response_handler(
    mut bus: MessageBus_API,
    correlation_id: &UUID4,
) -> *mut ffi::PyObject {
    let handler = bus.response_handler(correlation_id);

    if let Some(handler) = handler {
        handler.py_callback.unwrap().ptr
    } else {
        ffi::Py_None()
    }
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
mod ffi_tests {
    use std::{ffi::CString, ptr, rc::Rc};

    use nautilus_core::message::Message;
    use pyo3::FromPyPointer;
    use rstest::*;
    use ustr::Ustr;

    use super::*;
    use crate::handlers::MessageHandler;
    // Helper function to create a MessageHandler with a PyCallableWrapper for testing
    fn create_handler() -> MessageHandler {
        let py_callable_ptr = ptr::null_mut(); // Replace with an actual PyObject pointer if needed
        let handler_id = Ustr::from("test_handler");
        MessageHandler::new(
            handler_id,
            Some(PyCallableWrapper {
                ptr: py_callable_ptr,
            }),
            None,
        )
    }

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

    #[rstest]
    fn test_msgbus_new() {
        let trader_id = TraderId::from_str("trader-001").unwrap();
        let name = CString::new("Test MessageBus").unwrap();

        // Create a new MessageBus using FFI
        let bus = unsafe { msgbus_new(trader_id.to_string().as_ptr() as *const i8, name.as_ptr()) };

        // Verify that the trader ID and name are set correctly
        assert_eq!(bus.trader_id.to_string(), "trader-001");
        assert_eq!(bus.name, "Test MessageBus");
    }

    #[ignore]
    #[rstest]
    fn test_msgbus_endpoints() {
        let mut bus = MessageBus::new(TraderId::from_str("trader-001").unwrap(), None);
        let endpoint1 = "endpoint1";
        let endpoint2 = "endpoint2";

        // Register endpoints
        bus.register(endpoint1, create_handler());
        bus.register(endpoint2, create_handler());

        // Call msgbus_endpoints to get endpoints as a Python list
        let py_list = msgbus_endpoints(MessageBus_API(Box::new(bus)));

        // Convert the Python list to a Vec of strings
        let endpoints: Vec<String> = Python::with_gil(|py| {
            let py_list = unsafe { PyList::from_owned_ptr(py, py_list) };
            py_list
                .into_iter()
                .map(|item| item.extract::<String>().unwrap())
                .collect()
        });

        // Verify that the endpoints are correctly retrieved
        assert_eq!(endpoints.len(), 2);
        assert!(endpoints.contains(&endpoint1.to_string()));
        assert!(endpoints.contains(&endpoint2.to_string()));
    }

    #[ignore]
    #[rstest]
    fn test_msgbus_topics() {
        let mut bus = MessageBus::new(TraderId::from_str("trader-001").unwrap(), None);
        let topic1 = "topic1";
        let topic2 = "topic2";

        // Subscribe to topics
        bus.subscribe(topic1, create_handler(), None);
        bus.subscribe(topic2, create_handler(), None);

        // Call msgbus_topics to get topics as a Python list
        let py_list = msgbus_topics(MessageBus_API(Box::new(bus)));

        // Convert the Python list to a Vec of strings
        let topics: Vec<String> = Python::with_gil(|py| {
            let py_list = unsafe { PyList::from_owned_ptr(py, py_list) };
            py_list
                .into_iter()
                .map(|item| item.extract::<String>().unwrap())
                .collect()
        });

        // Verify that the topics are correctly retrieved
        assert_eq!(topics.len(), 2);
        assert!(topics.contains(&topic1.to_string()));
        assert!(topics.contains(&topic2.to_string()));
    }

    #[ignore]
    #[rstest]
    fn test_msgbus_subscribe() {
        let bus = MessageBus::new(TraderId::from_str("trader-001").unwrap(), None);
        let topic = "test-topic";

        // Subscribe using FFI
        unsafe {
            let topic_ptr = CString::new(topic).unwrap().clone().as_ptr();
            let handler_id_ptr = CString::new("handler-001").unwrap().clone().as_ptr();
            msgbus_subscribe(
                MessageBus_API(Box::new(bus.clone())),
                topic_ptr,
                handler_id_ptr,
                ptr::null_mut(),
                1,
            );

            // Verify that the subscription is added
            assert!(msgbus_has_subscribers(MessageBus_API(Box::new(bus)), topic_ptr) != 0);
        }
    }

    #[ignore]
    #[rstest]
    fn test_msgbus_get_endpoint() {
        let mut bus = MessageBus::new(TraderId::from_str("trader-001").unwrap(), None);
        let endpoint = "test-endpoint";
        let handler = create_handler();

        // Register an endpoint
        bus.register(endpoint, handler.clone());

        // Call msgbus_get_endpoint to get the handler as a PyObject
        let py_callable = unsafe {
            let endpoint_ptr = CString::new(endpoint).unwrap().clone().as_ptr();
            msgbus_get_endpoint(MessageBus_API(Box::new(bus)), endpoint_ptr)
        };

        // Verify that the PyObject pointer matches the registered handler's PyObject pointer
        assert_eq!(py_callable, handler.py_callback.unwrap().ptr);
    }

    #[ignore]
    #[rstest]
    fn test_msgbus_request_handler() {
        let mut bus = MessageBus::new(TraderId::from_str("trader-001").unwrap(), None);
        let endpoint = "test-endpoint";
        let request_id = UUID4::new();

        // Register an endpoint
        bus.register(endpoint, create_handler());

        // Call msgbus_request_handler to get the handler as a PyObject
        let py_callable = unsafe {
            let endpoint_ptr = CString::new(endpoint).unwrap().clone().as_ptr();
            msgbus_request_handler(
                MessageBus_API(Box::new(bus.clone())),
                endpoint_ptr,
                request_id,
            )
        };

        // Verify that the PyObject pointer matches the registered handler's PyObject pointer
        assert_eq!(
            py_callable,
            bus.endpoints[&Ustr::from(endpoint)]
                .py_callback
                .unwrap()
                .ptr
        );
    }

    #[ignore]
    #[rstest]
    fn test_msgbus_response_handler() {
        let mut bus = MessageBus::new(TraderId::from_str("trader-001").unwrap(), None);
        let correlation_id = UUID4::new();

        // Register a response handler
        let handler = create_handler();
        bus.correlation_index
            .insert(correlation_id.clone(), handler.clone());

        // Call msgbus_response_handler to get the handler as a PyObject
        let py_callable =
            unsafe { msgbus_response_handler(MessageBus_API(Box::new(bus)), &correlation_id) };

        assert_eq!(py_callable, handler.py_callback.unwrap().ptr);
    }

    #[ignore]
    #[rstest]
    fn test_msgbus_is_matching() {
        let topic = "data.quotes.BINANCE";
        let pattern = "data.*.BINANCE";

        let result = unsafe {
            let topic_ptr = CString::new(topic).unwrap().clone().as_ptr();
            let pattern_ptr = CString::new(pattern).unwrap().clone().as_ptr();
            msgbus_is_matching(topic_ptr, pattern_ptr)
        };

        assert_eq!(result, 1);
    }
}
