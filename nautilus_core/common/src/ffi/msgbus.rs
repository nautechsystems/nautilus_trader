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
    string::{cstr_to_string, optional_cstr_to_string},
};
use nautilus_model::identifiers::trader_id::TraderId;
use pyo3::{
    ffi,
    prelude::*,
    types::{PyList, PyString},
    AsPyPointer, Python,
};
use ustr::Ustr;

use crate::msgbus::MessageBus;

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
/// - Assumes `endpoint_ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn msgbus_get_endpoint(
    bus: MessageBus_API,
    endpoint_ptr: *const c_char,
) -> *const ffi::PyObject {
    let endpoint = cstr_to_string(endpoint_ptr);
    match bus.get_endpoint(&endpoint) {
        Some(handler) => handler.clone().as_ptr(),
        None => ffi::Py_None(),
    }
}

/// # Safety
///
/// - Assumes `pattern_ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn msgbus_get_matching_handlers(
    mut bus: MessageBus_API,
    pattern_ptr: *const c_char,
) -> CVec {
    let pattern = cstr_to_string(pattern_ptr);
    // TODO: Avoid clone and take direct pointer
    (*bus.get_matching_handlers(&Ustr::from(&pattern)))
        .clone()
        .into()
}

#[allow(clippy::drop_non_drop)]
#[no_mangle]
pub extern "C" fn vec_msgbus_handlers_drop(v: CVec) {
    let CVec { ptr, len, cap } = v;
    let data: Vec<ffi::PyObject> =
        unsafe { Vec::from_raw_parts(ptr.cast::<ffi::PyObject>(), len, cap) };
    drop(data); // Memory freed here
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use nautilus_core::message::Message;
    use rstest::*;

    use super::*;
    use crate::handlers::MessageHandler;

    #[rstest]
    fn test_subscribe_python_callable() {
        let trader_id = TraderId::from("trader-001");
        let topic = "my-topic".to_string();

        // TODO: Create a Python list and pass the message in a closure to the `append` method
        let callback = Rc::new(|_m: Message| Python::with_gil(|_| {}));
        let handler = MessageHandler::new(None, Some(callback));

        let mut msgbus = MessageBus::new(trader_id, None);
        msgbus.subscribe(&topic, handler.clone(), "id_of_method", None);

        assert_eq!(msgbus.topics(), vec![topic]);
    }
}
