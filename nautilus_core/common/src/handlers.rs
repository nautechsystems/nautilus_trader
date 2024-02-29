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

#[cfg(not(feature = "python"))]
use std::ffi::c_char;
use std::{fmt, sync::Arc};

#[cfg(not(feature = "python"))]
use nautilus_core::message::Message;
#[cfg(feature = "python")]
use pyo3::prelude::*;
use ustr::Ustr;

use crate::timer::TimeEvent;

#[allow(dead_code)]
#[derive(Clone)]
pub struct SafeMessageCallback {
    #[cfg(not(feature = "python"))]
    pub callback: Arc<dyn Fn(Message) + Send>,
    #[cfg(feature = "python")]
    callback: PyObject,
}

unsafe impl Send for SafeMessageCallback {}
unsafe impl Sync for SafeMessageCallback {}

#[allow(dead_code)]
#[derive(Clone)]
pub struct SafeTimeEventCallback {
    pub callback: Arc<dyn Fn(TimeEvent) + Send>,
}

unsafe impl Send for SafeTimeEventCallback {}
unsafe impl Sync for SafeTimeEventCallback {}

// TODO: Make this more generic
#[derive(Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.common")
)]
pub struct MessageHandler {
    pub handler_id: Ustr,
    _callback: Option<SafeMessageCallback>,
}

impl MessageHandler {
    #[must_use]
    pub fn new(handler_id: Ustr, callback: Option<SafeMessageCallback>) -> Self {
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

#[derive(Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.common")
)]
pub struct EventHandler {
    #[cfg(not(feature = "python"))]
    pub callback: SafeTimeEventCallback,
    #[cfg(feature = "python")]
    pub callback: PyObject,
}

impl EventHandler {
    #[cfg(not(feature = "python"))]
    #[must_use]
    pub fn new(callback: SafeTimeEventCallback) -> Self {
        Self { callback }
    }

    #[cfg(feature = "python")]
    #[must_use]
    pub fn new(callback: PyObject) -> Self {
        Self { callback }
    }

    #[cfg(not(feature = "python"))]
    #[must_use]
    pub fn as_ptr(self) -> *mut c_char {
        // TODO: Temporary hack for conditional compilation
        std::ptr::null_mut()
    }

    #[cfg(feature = "python")]
    #[must_use]
    pub fn as_ptr(self) -> *mut pyo3::ffi::PyObject {
        self.callback.as_ptr()
    }
}
