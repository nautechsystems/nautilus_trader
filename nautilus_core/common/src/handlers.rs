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

use std::rc::Rc;

use nautilus_core::message::Message;
use pyo3::{ffi, prelude::*, AsPyPointer};

use crate::timer::TimeEvent;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct PyCallableWrapper {
    pub ptr: *mut ffi::PyObject,
}

// TODO: Make this more generic
#[derive(Clone)]
pub struct MessageHandler {
    pub callback_py: Option<PyCallableWrapper>,
    _callback: Option<Rc<dyn Fn(Message)>>,
}

impl MessageHandler {
    // TODO: Validate exactly one of these is `Some`
    pub fn new(callback_py: Option<PyObject>, callback: Option<Rc<dyn Fn(Message)>>) -> Self {
        let callback_py = callback_py.map(|callable| PyCallableWrapper {
            ptr: callable.as_ptr(),
        });

        Self {
            callback_py,
            _callback: callback,
        }
    }

    pub fn as_ptr(self) -> *mut pyo3::ffi::PyObject {
        // SAFETY: Will panic if `unwrap` is called on None
        self.callback_py.unwrap().ptr
    }
}

// TODO: Make this more generic
#[derive(Clone)]
pub struct EventHandler {
    callback_py: Option<PyObject>,
    _callback: Option<Rc<dyn Fn(TimeEvent)>>,
}

impl EventHandler {
    // TODO: Validate exactly one of these is `Some`
    pub fn new(callback_py: Option<PyObject>, callback: Option<Rc<dyn Fn(TimeEvent)>>) -> Self {
        Self {
            callback_py,
            _callback: callback,
        }
    }

    pub fn as_ptr(self) -> *mut pyo3::ffi::PyObject {
        // SAFETY: Will panic if `unwrap` is called on None
        self.callback_py.unwrap().as_ptr()
    }
}
