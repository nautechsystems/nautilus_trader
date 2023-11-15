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

use std::{fmt, rc::Rc};

use nautilus_core::message::Message;
use pyo3::{ffi, prelude::*, AsPyPointer};
use ustr::Ustr;

use crate::timer::TimeEvent;

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct PyCallableWrapper {
    pub ptr: *mut ffi::PyObject,
}

// This function only exists so that `PyCallableWrapper` is included in the definitions
#[no_mangle]
pub extern "C" fn dummy_callable(c: PyCallableWrapper) -> PyCallableWrapper {
    c
}

// TODO: Make this more generic
#[derive(Clone)]
pub struct MessageHandler {
    pub handler_id: Ustr,
    _callback: Option<Rc<dyn Fn(Message)>>,
}

impl MessageHandler {
    // TODO: Validate exactly one of these is `Some`
    pub fn new(handler_id: Ustr, callback: Option<Rc<dyn Fn(Message)>>) -> Self {
        Self {
            handler_id,
            _callback: callback,
        }
    }
}

impl PartialEq for MessageHandler {
    fn eq(&self, other: &Self) -> bool {
        self.handler_id == other.handler_id
    }
}

impl fmt::Debug for MessageHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct(stringify!(MessageHandler))
            .field("handler_id", &self.handler_id)
            .finish()
    }
}

// TODO: Make this more generic
#[derive(Clone)]
pub struct EventHandler {
    py_callback: Option<PyObject>,
    _callback: Option<Rc<dyn Fn(TimeEvent)>>,
}

impl EventHandler {
    // TODO: Validate exactly one of these is `Some`
    pub fn new(py_callback: Option<PyObject>, callback: Option<Rc<dyn Fn(TimeEvent)>>) -> Self {
        Self {
            py_callback,
            _callback: callback,
        }
    }

    pub fn as_ptr(self) -> *mut ffi::PyObject {
        // SAFETY: Will panic if `unwrap` is called on None
        self.py_callback.unwrap().as_ptr()
    }
}
