use std::rc::Rc;

use pyo3::{pymethods, PyObject, PyRef, PyRefMut};
use ustr::Ustr;

use crate::msgbus::{MessageBus, ShareableMessageHandler};

use super::python_handler::PythonMessageHandler;

#[pymethods]
impl MessageBus {
    /// Sends a message to a an endpoint
    #[pyo3(name = "send")]
    pub fn send_py(&self, endpoint: &str, message: PyObject) {
        if let Some(handler) = self.get_endpoint(&Ustr::from(endpoint)) {
            handler.0.handle(&message)
        }
    }

    /// Publish a message to a topic
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
        self.register(endpoint, handler);
    }

    /// Subscribes the given `handler` to the `topic`.
    #[pyo3(name = "subscribe")]
    pub fn subscribe_py(
        mut slf: PyRefMut<'_, Self>,
        topic: &str,
        handler: PythonMessageHandler,
        priority: Option<u8>,
    ) {
        // Updates value if key already exists
        let handler = ShareableMessageHandler(Rc::new(handler));
        slf.subscribe(topic, handler, priority);
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
        self.unsubscribe(topic, handler);
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
        self.deregister(endpoint);
    }
}
