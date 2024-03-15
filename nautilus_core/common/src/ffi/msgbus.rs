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

use std::{
    ffi::c_char,
    ops::{Deref, DerefMut},
    str::FromStr,
};

use nautilus_core::{
    ffi::{
        cvec::CVec,
        parsing::optional_bytes_to_json,
        string::{cstr_to_bytes, cstr_to_str, cstr_to_ustr, optional_cstr_to_str},
    },
    uuid::UUID4,
};
use nautilus_model::identifiers::trader_id::TraderId;
use pyo3::{
    ffi,
    prelude::*,
    types::{PyList, PyString},
};

use crate::{
    handlers::MessageHandler,
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
#[repr(C)]
#[allow(non_camel_case_types)]
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
    instance_id_ptr: *const c_char,
    config_ptr: *const c_char,
) -> MessageBus_API {
    let trader_id = TraderId::from_str(cstr_to_str(trader_id_ptr)).unwrap();
    let name = optional_cstr_to_str(name_ptr).map(|s| s.to_string());
    let instance_id = UUID4::from(cstr_to_str(instance_id_ptr));
    let config = optional_bytes_to_json(config_ptr);
    MessageBus_API(Box::new(
        MessageBus::new(trader_id, instance_id, name, config)
            .expect("Error initializing `MessageBus`"),
    ))
}

#[no_mangle]
pub extern "C" fn msgbus_drop(bus: MessageBus_API) {
    drop(bus); // Memory freed here
}

#[no_mangle]
pub extern "C" fn msgbus_trader_id(bus: &MessageBus_API) -> TraderId {
    bus.trader_id
}

#[no_mangle]
pub extern "C" fn msgbus_endpoints(bus: &MessageBus_API) -> *mut ffi::PyObject {
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
pub extern "C" fn msgbus_topics(bus: &MessageBus_API) -> *mut ffi::PyObject {
    Python::with_gil(|py| -> Py<PyList> {
        let topics: Vec<Py<PyString>> = bus
            .subscriptions()
            .into_iter()
            .map(|s| PyString::new(py, s.topic.as_str()).into())
            .collect();
        PyList::new(py, topics).into()
    })
    .as_ptr()
}

#[no_mangle]
pub extern "C" fn msgbus_correlation_ids(bus: &MessageBus_API) -> *mut ffi::PyObject {
    Python::with_gil(|py| -> Py<PyList> {
        let correlation_ids: Vec<Py<PyString>> = bus
            .correlation_ids()
            .into_iter()
            .map(|id| PyString::new(py, &id.to_string()).into())
            .collect();
        PyList::new(py, correlation_ids).into()
    })
    .as_ptr()
}

/// # Safety
///
/// - Assumes `pattern_ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn msgbus_has_subscribers(
    bus: &MessageBus_API,
    pattern_ptr: *const c_char,
) -> u8 {
    let pattern = cstr_to_ustr(pattern_ptr);
    u8::from(bus.has_subscribers(pattern.as_str()))
}

#[no_mangle]
pub extern "C" fn msgbus_subscription_handler_ids(bus: &MessageBus_API) -> *mut ffi::PyObject {
    Python::with_gil(|py| -> Py<PyList> {
        let handler_ids: Vec<Py<PyString>> = bus
            .subscription_handler_ids()
            .iter()
            .map(|k| PyString::new(py, k).into())
            .collect();
        PyList::new(py, handler_ids).into()
    })
    .as_ptr()
}

#[no_mangle]
pub extern "C" fn msgbus_subscriptions(bus: &MessageBus_API) -> *mut ffi::PyObject {
    Python::with_gil(|py| -> Py<PyList> {
        let subs_info: Vec<Py<PyString>> = bus
            .subscriptions()
            .iter()
            .map(|s| PyString::new(py, &format!("{s:?}")).into())
            .collect();
        PyList::new(py, subs_info).into()
    })
    .as_ptr()
}

/// # Safety
///
/// - Assumes `endpoint_ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn msgbus_is_registered(
    bus: &MessageBus_API,
    endpoint_ptr: *const c_char,
) -> u8 {
    let endpoint = cstr_to_str(endpoint_ptr);
    u8::from(bus.is_registered(endpoint))
}

/// # Safety
///
/// - Assumes `topic_ptr` is a valid C string pointer.
/// - Assumes `handler_id_ptr` is a valid C string pointer.
/// - Assumes `py_callable_ptr` points to a valid Python callable.
#[no_mangle]
pub unsafe extern "C" fn msgbus_is_subscribed(
    bus: &MessageBus_API,
    topic_ptr: *const c_char,
    handler_id_ptr: *const c_char,
) -> u8 {
    let topic = cstr_to_ustr(topic_ptr);
    let handler_id = cstr_to_ustr(handler_id_ptr);
    let handler = MessageHandler::new(handler_id, None);
    u8::from(bus.is_subscribed(topic.as_str(), handler))
}

/// # Safety
///
/// - Assumes `endpoint_ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn msgbus_is_pending_response(
    bus: &MessageBus_API,
    request_id: &UUID4,
) -> u8 {
    u8::from(bus.is_pending_response(request_id))
}

#[no_mangle]
pub extern "C" fn msgbus_sent_count(bus: &MessageBus_API) -> u64 {
    bus.sent_count
}

#[no_mangle]
pub extern "C" fn msgbus_req_count(bus: &MessageBus_API) -> u64 {
    bus.req_count
}

#[no_mangle]
pub extern "C" fn msgbus_res_count(bus: &MessageBus_API) -> u64 {
    bus.res_count
}

#[no_mangle]
pub extern "C" fn msgbus_pub_count(bus: &MessageBus_API) -> u64 {
    bus.pub_count
}

/// # Safety
///
/// - Assumes `endpoint_ptr` is a valid C string pointer.
/// - Assumes `handler_id_ptr` is a valid C string pointer.
/// - Assumes `py_callable_ptr` points to a valid Python callable.
#[no_mangle]
pub unsafe extern "C" fn msgbus_register(
    bus: &mut MessageBus_API,
    endpoint_ptr: *const c_char,
    handler_id_ptr: *const c_char,
) -> *const c_char {
    let endpoint = cstr_to_str(endpoint_ptr);
    let handler_id = cstr_to_ustr(handler_id_ptr);
    let handler = MessageHandler::new(handler_id, None);
    bus.register(endpoint, handler);
    handler_id.as_ptr().cast::<c_char>()
}

/// # Safety
///
/// - Assumes `endpoint_ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn msgbus_deregister(mut bus: MessageBus_API, endpoint_ptr: *const c_char) {
    let endpoint = cstr_to_str(endpoint_ptr);
    bus.deregister(endpoint);
}

/// # Safety
///
/// - Assumes `topic_ptr` is a valid C string pointer.
/// - Assumes `handler_id_ptr` is a valid C string pointer.
/// - Assumes `py_callable_ptr` points to a valid Python callable.
#[no_mangle]
pub unsafe extern "C" fn msgbus_subscribe(
    bus: &mut MessageBus_API,
    topic_ptr: *const c_char,
    handler_id_ptr: *const c_char,
    priority: u8,
) -> *const c_char {
    let topic = cstr_to_ustr(topic_ptr);
    let handler_id = cstr_to_ustr(handler_id_ptr);
    let handler = MessageHandler::new(handler_id, None);
    bus.subscribe(&topic, handler, Some(priority));
    handler_id.as_ptr().cast::<c_char>()
}

/// # Safety
///
/// - Assumes `topic_ptr` is a valid C string pointer.
/// - Assumes `handler_id_ptr` is a valid C string pointer.
/// - Assumes `py_callable_ptr` points to a valid Python callable.
#[no_mangle]
pub unsafe extern "C" fn msgbus_unsubscribe(
    bus: &mut MessageBus_API,
    topic_ptr: *const c_char,
    handler_id_ptr: *const c_char,
) {
    let topic = cstr_to_ustr(topic_ptr);
    let handler_id = cstr_to_ustr(handler_id_ptr);
    let handler = MessageHandler::new(handler_id, None);
    bus.unsubscribe(&topic, handler);
}

/// # Safety
///
/// - Assumes `endpoint_ptr` is a valid C string pointer.
/// - Returns a NULL pointer if endpoint is not registered.
#[no_mangle]
pub unsafe extern "C" fn msgbus_endpoint_callback(
    bus: &MessageBus_API,
    endpoint_ptr: *const c_char,
) -> *const c_char {
    let endpoint = cstr_to_ustr(endpoint_ptr);
    match bus.get_endpoint(&endpoint) {
        Some(handler) => handler.handler_id.as_ptr().cast::<c_char>(),
        None => std::ptr::null(),
    }
}

/// # Safety
///
/// - Assumes `pattern_ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn msgbus_matching_callbacks(
    bus: &mut MessageBus_API,
    pattern_ptr: *const c_char,
) -> CVec {
    let pattern = cstr_to_ustr(pattern_ptr);
    let subs: Vec<&Subscription> = bus.matching_subscriptions(&pattern);
    subs.iter()
        .map(|s| s.handler.handler_id.as_ptr().cast::<c_char>())
        .collect::<Vec<*const c_char>>()
        .into()
}

/// # Safety
///
/// - Assumes `endpoint_ptr` is a valid C string pointer.
/// - Potentially returns a pointer to `Py_None`.
#[no_mangle]
pub unsafe extern "C" fn msgbus_request_callback(
    bus: &mut MessageBus_API,
    endpoint_ptr: *const c_char,
    request_id: UUID4,
    handler_id_ptr: *const c_char,
) -> *const c_char {
    let endpoint = cstr_to_ustr(endpoint_ptr);
    let handler_id = cstr_to_ustr(handler_id_ptr);
    let handler = MessageHandler::new(handler_id, None);
    match bus.request_handler(&endpoint, request_id, handler) {
        Some(handler) => handler.handler_id.as_ptr().cast::<c_char>(),
        None => std::ptr::null(),
    }
}

/// # Safety
///
/// - Potentially returns a pointer to `Py_None`.
#[no_mangle]
pub unsafe extern "C" fn msgbus_response_callback(
    bus: &mut MessageBus_API,
    correlation_id: &UUID4,
) -> *const c_char {
    match bus.response_handler(correlation_id) {
        Some(handler) => handler.handler_id.as_ptr().cast::<c_char>(),
        None => std::ptr::null(),
    }
}

/// # Safety
///
/// - Potentially returns a pointer to `Py_None`.
#[no_mangle]
pub unsafe extern "C" fn msgbus_correlation_id_handler(
    bus: &mut MessageBus_API,
    correlation_id: &UUID4,
) -> *const c_char {
    match bus.correlation_id_handler(correlation_id) {
        Some(handler) => handler.handler_id.as_ptr().cast::<c_char>(),
        None => std::ptr::null(),
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
    u8::from(is_matching(&topic, &pattern))
}

/// # Safety
///
/// - Assumes `topic_ptr` is a valid C string pointer.
/// - Assumes `handler_id_ptr` is a valid C string pointer.
/// - Assumes `py_callable_ptr` points to a valid Python callable.
#[no_mangle]
pub unsafe extern "C" fn msgbus_publish_external(
    bus: &mut MessageBus_API,
    topic_ptr: *const c_char,
    payload_ptr: *const c_char,
) {
    let topic = cstr_to_str(topic_ptr);
    let payload = cstr_to_bytes(payload_ptr);
    bus.publish_external(topic.to_string(), payload);
}
