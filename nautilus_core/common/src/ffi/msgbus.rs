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
    collections::HashMap,
    ffi::c_char,
    ops::{Deref, DerefMut},
};

use nautilus_core::{ffi::string::cstr_to_string, uuid::UUID4};
use pyo3::ffi::PyObject;

use crate::msgbus::MessageBus;

#[allow(dead_code)] // Temporary for development
pub struct PythonSwitchboard {
    subscriptions: HashMap<String, PyObject>,
    patterns: HashMap<String, Vec<PyObject>>,
    endpoints: HashMap<String, PyObject>,
    correlation_index: HashMap<UUID4, PyObject>,
}

impl PythonSwitchboard {
    pub fn new() -> Self {
        Self {
            subscriptions: HashMap::new(),
            patterns: HashMap::new(),
            endpoints: HashMap::new(),
            correlation_index: HashMap::new(),
        }
    }
}

impl Default for PythonSwitchboard {
    fn default() -> Self {
        Self::new()
    }
}

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
pub struct MessageBus_API {
    inner: Box<MessageBus>,
    switchboard: Box<PythonSwitchboard>,
}

impl Deref for MessageBus_API {
    type Target = MessageBus;

    fn deref(&self) -> &Self::Target {
        self.inner.deref()
    }
}

impl DerefMut for MessageBus_API {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner.deref_mut()
    }
}

/// # Safety
///
/// - Assumes `name_ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn test_msgbus_new(name_ptr: *const c_char) -> MessageBus_API {
    let name = cstr_to_string(name_ptr);
    MessageBus_API {
        inner: Box::new(MessageBus::new(&name)),
        switchboard: Box::new(PythonSwitchboard::new()),
    }
}
