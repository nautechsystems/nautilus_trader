// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 2Nautech Systems Pty Ltd. All rights reserved.
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

use pyo3::{pymethods, PyObject, PyRefMut};
use ustr::Ustr;

use super::handler::PythonMessageHandler;
use crate::msgbus::{database::BusMessage, handler::ShareableMessageHandler, MessageBus};

#[pymethods]
impl BusMessage {
    #[getter]
    #[pyo3(name = "topic")]
    fn py_close(&mut self) -> String {
        self.topic.clone()
    }

    #[getter]
    #[pyo3(name = "payload")]
    fn py_payload(&mut self) -> &[u8] {
        self.payload.as_ref()
    }
}

#[pymethods]
impl MessageBus {
    /// Sends a message to a an endpoint.
    #[pyo3(name = "send")]
    pub fn send_py(&self, endpoint: &str, message: PyObject) {
        if let Some(handler) = self.get_endpoint(&Ustr::from(endpoint)) {
            handler.0.handle(&message);
        }
    }

    /// Publish a message to a topic.
    #[pyo3(name = "publish")]
    pub fn publish_py(&self, topic: &str, message: PyObject) {
        let topic = Ustr::from(topic);
        let matching_subs = self.matching_subscriptions(&topic);

        for sub in matching_subs {
            sub.handler.0.handle(&message);
        }
    }

    /// Registers the given `handler` for the `endpoint` address.
    #[pyo3(name = "register")]
    pub fn register_py(&mut self, endpoint: &str, handler: PythonMessageHandler) {
        // Updates value if key already exists
        let handler = ShareableMessageHandler(Rc::new(handler));
        self.register(Ustr::from(endpoint), handler);
    }

    /// Subscribes the given `handler` to the `topic`.
    ///
    /// The priority for the subscription determines the ordering of
    /// handlers receiving messages being processed, higher priority
    /// handlers will receive messages before lower priority handlers.
    ///
    /// Safety: Priority should be between 0 and 255
    ///
    /// # Warnings
    ///
    /// Assigning priority handling is an advanced feature which *shouldn't
    /// normally be needed by most users*. **Only assign a higher priority to the
    /// subscription if you are certain of what you're doing**. If an inappropriate
    /// priority is assigned then the handler may receive messages before core
    /// system components have been able to process necessary calculations and
    /// produce potential side effects for logically sound behavior.
    #[pyo3(name = "subscribe")]
    #[pyo3(signature = (topic, handler, priority=None))]
    pub fn subscribe_py(
        mut slf: PyRefMut<'_, Self>,
        topic: &str,
        handler: PythonMessageHandler,
        priority: Option<u8>,
    ) {
        // Updates value if key already exists
        let handler = ShareableMessageHandler(Rc::new(handler));
        slf.subscribe(Ustr::from(topic), handler, priority);
    }

    /// Returns whether there are subscribers for the given `pattern`.
    #[must_use]
    #[pyo3(name = "is_subscribed")]
    pub fn is_subscribed_py(&self, topic: &str, handler: PythonMessageHandler) -> bool {
        let handler = ShareableMessageHandler(Rc::new(handler));
        self.is_subscribed(topic, handler)
    }

    /// Unsubscribes the given `handler` from the `topic`.
    #[pyo3(name = "unsubscribe")]
    pub fn unsubscribe_py(&mut self, topic: &str, handler: PythonMessageHandler) {
        let handler = ShareableMessageHandler(Rc::new(handler));
        self.unsubscribe(Ustr::from(topic), handler);
    }

    /// Returns whether there are subscribers for the given `pattern`.
    #[must_use]
    #[pyo3(name = "is_registered")]
    pub fn is_registered_py(&self, endpoint: &str) -> bool {
        self.is_registered(endpoint)
    }

    /// Deregisters the given `handler` for the `endpoint` address.
    #[pyo3(name = "deregister")]
    pub fn deregister_py(&mut self, endpoint: &str) {
        // Removes entry if it exists for endpoint
        self.deregister(&Ustr::from(endpoint));
    }
}
