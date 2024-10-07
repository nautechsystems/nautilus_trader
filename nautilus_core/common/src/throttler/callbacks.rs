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

use std::{cell::RefCell, rc::Rc};

use super::inner::InnerThrottler;
use crate::timer::{TimeEvent, TimeEventCallback};

/// Stop rate limiting messages
pub struct ThrottlerResume<T, F> {
    inner: Rc<RefCell<InnerThrottler<T, F>>>,
}

impl<T, F> ThrottlerResume<T, F> {
    pub const fn new(inner: Rc<RefCell<InnerThrottler<T, F>>>) -> Self {
        Self { inner }
    }
}

impl<T: 'static, F: Fn(T) + 'static> From<ThrottlerResume<T, F>> for TimeEventCallback {
    fn from(value: ThrottlerResume<T, F>) -> Self {
        Self::Rust(Rc::new(move |_event: TimeEvent| {
            value.inner.borrow_mut().is_limiting = false;
        }))
    }
}

/// Process buffered messages.
#[derive(Clone)]
pub struct ThrottlerProcess<T, F> {
    inner: Rc<RefCell<InnerThrottler<T, F>>>,
}

impl<T, F> ThrottlerProcess<T, F> {
    pub const fn new(inner: Rc<RefCell<InnerThrottler<T, F>>>) -> Self {
        Self { inner }
    }
}

impl<T: 'static, F: Fn(T) + 'static> From<ThrottlerProcess<T, F>> for TimeEventCallback {
    fn from(value: ThrottlerProcess<T, F>) -> Self {
        Self::Rust(Rc::new(move |_event: TimeEvent| {
            let process_clone = ThrottlerProcess {
                inner: value.inner.clone(),
            };
            let mut core = value.inner.borrow_mut();
            while let Some(msg) = core.buffer.pop_back() {
                core.send_msg(msg);

                // Set timer to process more buffered messages
                // if interval limit reached and there are more
                // buffered messages to process
                if !core.buffer.is_empty() && core.delta_next() > 0 {
                    core.is_limiting = true;
                    core.set_timer(Some(process_clone.into()));
                    return;
                }
            }

            core.is_limiting = false;
        }))
    }
}
