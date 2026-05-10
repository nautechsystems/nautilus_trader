// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

//! Python bindings for the message bus, including configuration types and the
//! [`PyMessageBus`] wrapper that routes Python events through the Rust
//! thread-local [`MessageBus`] via the Any-based dispatch path.

use std::{any::Any, fmt::Debug, rc::Rc};

use ahash::AHashMap;
use nautilus_core::{UUID4, python::to_pyruntime_err};
use nautilus_model::identifiers::TraderId;
use pyo3::{Py, Python, prelude::*, types::PyBytes};
use ustr::Ustr;

use crate::{
    enums::SerializationEncoding,
    msgbus::{
        self as msgbus_api, BusMessage, MessageBus,
        core::Subscription,
        database::{DatabaseConfig, MessageBusConfig},
        get_message_bus,
        matching::is_matching,
        mstr::{Endpoint, MStr, Pattern, Topic},
        typed_handler::{Handler, ShareableMessageHandler, TypedHandler},
    },
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BusMessage {
    #[getter]
    #[pyo3(name = "topic")]
    fn py_topic(&self) -> String {
        self.topic.to_string()
    }

    #[getter]
    #[pyo3(name = "payload")]
    fn py_payload(&self, py: Python<'_>) -> Py<PyBytes> {
        PyBytes::new(py, self.payload.as_ref()).into()
    }

    fn __repr__(&self) -> String {
        format!("{}('{}')", stringify!(BusMessage), self)
    }

    fn __str__(&self) -> String {
        self.to_string()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl DatabaseConfig {
    /// Configuration for database connections.
    ///
    /// # Notes
    ///
    /// If `database_type` is `"redis"`, it requires Redis version 6.2 or higher for correct operation.
    #[new]
    #[expect(clippy::too_many_arguments)]
    #[pyo3(signature = (database_type=None, host=None, port=None, username=None, password=None, ssl=None, connection_timeout=None, response_timeout=None, number_of_retries=None, exponent_base=None, max_delay=None, factor=None))]
    fn py_new(
        database_type: Option<String>,
        host: Option<String>,
        port: Option<u16>,
        username: Option<String>,
        password: Option<String>,
        ssl: Option<bool>,
        connection_timeout: Option<u16>,
        response_timeout: Option<u16>,
        number_of_retries: Option<usize>,
        exponent_base: Option<u64>,
        max_delay: Option<u64>,
        factor: Option<u64>,
    ) -> Self {
        let default = Self::default();
        Self {
            database_type: database_type.unwrap_or(default.database_type),
            host,
            port,
            username,
            password,
            ssl: ssl.unwrap_or(default.ssl),
            connection_timeout: connection_timeout.unwrap_or(default.connection_timeout),
            response_timeout: response_timeout.unwrap_or(default.response_timeout),
            number_of_retries: number_of_retries.unwrap_or(default.number_of_retries),
            exponent_base: exponent_base.unwrap_or(default.exponent_base),
            max_delay: max_delay.unwrap_or(default.max_delay),
            factor: factor.unwrap_or(default.factor),
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }

    #[getter]
    fn database_type(&self) -> &str {
        &self.database_type
    }

    #[getter]
    fn host(&self) -> Option<&str> {
        self.host.as_deref()
    }

    #[getter]
    fn port(&self) -> Option<u16> {
        self.port
    }

    #[getter]
    fn username(&self) -> Option<&str> {
        self.username.as_deref()
    }

    #[getter]
    fn password(&self) -> Option<&str> {
        self.password.as_deref()
    }

    #[getter]
    fn ssl(&self) -> bool {
        self.ssl
    }

    #[getter]
    fn connection_timeout(&self) -> u16 {
        self.connection_timeout
    }

    #[getter]
    fn response_timeout(&self) -> u16 {
        self.response_timeout
    }

    #[getter]
    fn number_of_retries(&self) -> usize {
        self.number_of_retries
    }

    #[getter]
    fn exponent_base(&self) -> u64 {
        self.exponent_base
    }

    #[getter]
    fn max_delay(&self) -> u64 {
        self.max_delay
    }

    #[getter]
    fn factor(&self) -> u64 {
        self.factor
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl MessageBusConfig {
    /// Configuration for `MessageBus` instances.
    #[new]
    #[expect(clippy::too_many_arguments)]
    #[pyo3(signature = (database=None, encoding=None, timestamps_as_iso8601=None, buffer_interval_ms=None, autotrim_mins=None, use_trader_prefix=None, use_trader_id=None, use_instance_id=None, streams_prefix=None, stream_per_topic=None, external_streams=None, types_filter=None, heartbeat_interval_secs=None))]
    fn py_new(
        database: Option<DatabaseConfig>,
        encoding: Option<SerializationEncoding>,
        timestamps_as_iso8601: Option<bool>,
        buffer_interval_ms: Option<u32>,
        autotrim_mins: Option<u32>,
        use_trader_prefix: Option<bool>,
        use_trader_id: Option<bool>,
        use_instance_id: Option<bool>,
        streams_prefix: Option<String>,
        stream_per_topic: Option<bool>,
        external_streams: Option<Vec<String>>,
        types_filter: Option<Vec<String>>,
        heartbeat_interval_secs: Option<u16>,
    ) -> Self {
        let default = Self::default();
        Self {
            database,
            encoding: encoding.unwrap_or(default.encoding),
            timestamps_as_iso8601: timestamps_as_iso8601.unwrap_or(default.timestamps_as_iso8601),
            buffer_interval_ms,
            autotrim_mins,
            use_trader_prefix: use_trader_prefix.unwrap_or(default.use_trader_prefix),
            use_trader_id: use_trader_id.unwrap_or(default.use_trader_id),
            use_instance_id: use_instance_id.unwrap_or(default.use_instance_id),
            streams_prefix: streams_prefix.unwrap_or(default.streams_prefix),
            stream_per_topic: stream_per_topic.unwrap_or(default.stream_per_topic),
            external_streams,
            types_filter,
            heartbeat_interval_secs,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }

    #[getter]
    fn database(&self) -> Option<DatabaseConfig> {
        self.database.clone()
    }

    #[getter]
    fn encoding(&self) -> SerializationEncoding {
        self.encoding
    }

    #[getter]
    fn timestamps_as_iso8601(&self) -> bool {
        self.timestamps_as_iso8601
    }

    #[getter]
    fn buffer_interval_ms(&self) -> Option<u32> {
        self.buffer_interval_ms
    }

    #[getter]
    fn autotrim_mins(&self) -> Option<u32> {
        self.autotrim_mins
    }

    #[getter]
    fn use_trader_prefix(&self) -> bool {
        self.use_trader_prefix
    }

    #[getter]
    fn use_trader_id(&self) -> bool {
        self.use_trader_id
    }

    #[getter]
    fn use_instance_id(&self) -> bool {
        self.use_instance_id
    }

    #[getter]
    fn streams_prefix(&self) -> &str {
        &self.streams_prefix
    }

    #[getter]
    fn stream_per_topic(&self) -> bool {
        self.stream_per_topic
    }

    #[getter]
    fn external_streams(&self) -> Option<Vec<String>> {
        self.external_streams.clone()
    }

    #[getter]
    fn types_filter(&self) -> Option<Vec<String>> {
        self.types_filter.clone()
    }

    #[getter]
    fn heartbeat_interval_secs(&self) -> Option<u16> {
        self.heartbeat_interval_secs
    }
}

/// Wraps a Python object so it can travel through the Rust Any-based message bus.
pub struct PyMessage(pub Py<PyAny>);

impl Debug for PyMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple(stringify!(PyMessage))
            .field(&"<PyObject>")
            .finish()
    }
}

/// Adapts a Python callable as a [`ShareableMessageHandler`].
///
/// Expects messages to be [`PyMessage`] instances. Acquires the GIL and calls
/// the Python callable with the inner Python object.
pub struct PyCallableHandler {
    id: Ustr,
    callable: Py<PyAny>,
}

impl Debug for PyCallableHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(PyCallableHandler))
            .field("id", &self.id)
            .finish()
    }
}

impl PyCallableHandler {
    /// Creates a new handler from a Python callable.
    ///
    /// The handler ID is derived from `repr(callable)` for stable identity
    /// across subscribe/unsubscribe calls.
    pub fn new(py: Python<'_>, callable: Py<PyAny>) -> PyResult<Self> {
        let repr_str = callable.bind(py).repr()?.to_string();
        let id = Ustr::from(&repr_str);
        Ok(Self { id, callable })
    }
}

impl Handler<dyn Any> for PyCallableHandler {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, message: &dyn Any) {
        if let Some(py_msg) = message.downcast_ref::<PyMessage>() {
            Python::attach(|py| {
                if let Err(e) = self.callable.call1(py, (&py_msg.0,)) {
                    log::error!("Python handler {id} failed: {e}", id = self.id);
                }
            });
        } else {
            log::error!(
                "Python handler {id} received non-PyMessage type",
                id = self.id
            );
        }
    }
}

fn make_handler(py: Python<'_>, callable: Py<PyAny>) -> PyResult<ShareableMessageHandler> {
    let handler = PyCallableHandler::new(py, callable)?;
    Ok(TypedHandler(Rc::new(handler) as Rc<dyn Handler<dyn Any>>))
}

/// Python message bus backed by the Rust thread-local [`MessageBus`].
///
/// Provides the same API as the legacy Cython `MessageBus` while routing all
/// messages through the single Rust bus. Python custom events travel through
/// the Any-based dispatch path via [`PyMessage`] wrappers.
#[pyclass(
    module = "nautilus_trader.core.nautilus_pyo3.common",
    name = "MessageBus",
    unsendable
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.common")]
pub struct PyMessageBus {
    trader_id: TraderId,
    instance_id: UUID4,
    name: String,
    has_backing: bool,
    serializer: Option<Py<PyAny>>,
    database: Option<Py<PyAny>>,
    listeners: Vec<Py<PyAny>>,
    types_filter: Option<Py<PyAny>>,
    streaming_types: Vec<Py<PyAny>>,
    correlation_index: AHashMap<UUID4, Py<PyAny>>,
    sent_count: u64,
    req_count: u64,
    res_count: u64,
    pub_count: u64,
}

impl Debug for PyMessageBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(PyMessageBus))
            .field("trader_id", &self.trader_id)
            .field("name", &self.name)
            .finish()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyMessageBus {
    /// Creates a new `MessageBus` instance.
    ///
    /// This creates and registers the underlying Rust `MessageBus` as the
    /// thread-local bus, then wraps it for Python access.
    #[new]
    #[pyo3(signature = (trader_id, clock=None, instance_id=None, name=None, serializer=None, database=None, config=None))]
    #[expect(clippy::too_many_arguments, clippy::needless_pass_by_value)]
    fn py_new(
        py: Python<'_>,
        trader_id: TraderId,
        clock: Option<Py<PyAny>>,
        instance_id: Option<UUID4>,
        name: Option<String>,
        serializer: Option<Py<PyAny>>,
        database: Option<Py<PyAny>>,
        config: Option<Py<PyAny>>,
    ) -> PyResult<Self> {
        let _ = clock;
        let instance_id = instance_id.unwrap_or_default();
        let bus_name = name.clone();
        let has_backing = database.is_some();

        let msgbus = MessageBus::new(trader_id, instance_id, bus_name, None);
        msgbus.register_message_bus();

        let types_filter = if let Some(ref cfg) = config {
            let tf = cfg.getattr(py, "types_filter")?;
            if tf.is_none(py) {
                None
            } else {
                // Convert to tuple for isinstance() checks
                let tuple = py
                    .import("builtins")?
                    .call_method1("tuple", (tf,))?
                    .unbind();
                Some(tuple)
            }
        } else {
            None
        };

        Ok(Self {
            trader_id,
            instance_id,
            name: name.unwrap_or_else(|| "MessageBus".to_owned()),
            has_backing,
            serializer,
            database,
            listeners: Vec::new(),
            types_filter,
            streaming_types: Vec::new(),
            correlation_index: AHashMap::new(),
            sent_count: 0,
            req_count: 0,
            res_count: 0,
            pub_count: 0,
        })
    }

    /// Returns the trader ID associated with the message bus.
    #[getter]
    #[pyo3(name = "trader_id")]
    fn py_trader_id(&self) -> TraderId {
        self.trader_id
    }

    /// Returns the instance ID associated with the message bus.
    #[getter]
    #[pyo3(name = "instance_id")]
    fn py_instance_id(&self) -> UUID4 {
        self.instance_id
    }

    /// Returns the name of the message bus.
    #[getter]
    #[pyo3(name = "name")]
    fn py_name(&self) -> &str {
        &self.name
    }

    /// Returns whether the message bus is backed by a database.
    #[getter]
    #[pyo3(name = "has_backing")]
    fn py_has_backing(&self) -> bool {
        self.has_backing
    }

    /// Returns the count of messages sent via point-to-point.
    #[getter]
    #[pyo3(name = "sent_count")]
    fn py_sent_count(&self) -> u64 {
        self.sent_count
    }

    /// Returns the count of requests made.
    #[getter]
    #[pyo3(name = "req_count")]
    fn py_req_count(&self) -> u64 {
        self.req_count
    }

    /// Returns the count of responses handled.
    #[getter]
    #[pyo3(name = "res_count")]
    fn py_res_count(&self) -> u64 {
        self.res_count
    }

    /// Returns the count of messages published.
    #[getter]
    #[pyo3(name = "pub_count")]
    fn py_pub_count(&self) -> u64 {
        self.pub_count
    }

    /// Returns all registered endpoint addresses.
    #[pyo3(name = "endpoints")]
    fn py_endpoints(&self) -> Vec<String> {
        let bus = get_message_bus();
        let bus_ref = bus.borrow();
        bus_ref.endpoints().into_iter().map(String::from).collect()
    }

    /// Returns all topics with active subscribers.
    #[pyo3(name = "topics")]
    fn py_topics(&self) -> Vec<String> {
        let bus = get_message_bus();
        let bus_ref = bus.borrow();
        let mut topics: Vec<String> = bus_ref.patterns().into_iter().map(String::from).collect();
        topics.sort();
        topics.dedup();
        topics
    }

    /// Returns subscriptions matching the given topic pattern.
    #[pyo3(name = "subscriptions")]
    #[pyo3(signature = (pattern=None))]
    fn py_subscriptions(&self, pattern: Option<&str>) -> Vec<String> {
        let bus = get_message_bus();
        let bus_ref = bus.borrow();
        let subs: Vec<&Subscription> = bus_ref.subscriptions();

        match pattern {
            Some(p) => {
                let filter = MStr::<Pattern>::pattern(p);
                subs.into_iter()
                    .filter(|s| is_matching(s.pattern.as_bytes(), filter.as_bytes()))
                    .map(|s| {
                        format!(
                            "Subscription(topic={}, handler={})",
                            s.pattern, s.handler_id
                        )
                    })
                    .collect()
            }
            None => subs
                .into_iter()
                .map(|s| {
                    format!(
                        "Subscription(topic={}, handler={})",
                        s.pattern, s.handler_id
                    )
                })
                .collect(),
        }
    }

    /// Returns whether there are subscribers for the given topic pattern.
    #[pyo3(name = "has_subscribers")]
    #[pyo3(signature = (pattern=None))]
    fn py_has_subscribers(&self, pattern: Option<&str>) -> bool {
        let bus = get_message_bus();
        let bus_ref = bus.borrow();

        match pattern {
            Some(p) => {
                let filter = MStr::<Pattern>::pattern(p);
                bus_ref
                    .subscriptions()
                    .iter()
                    .any(|s| is_matching(s.pattern.as_bytes(), filter.as_bytes()))
            }
            None => !bus_ref.subscriptions().is_empty(),
        }
    }

    /// Returns whether the given topic and handler is subscribed.
    #[pyo3(name = "is_subscribed")]
    fn py_is_subscribed(&self, py: Python<'_>, topic: &str, handler: Py<PyAny>) -> PyResult<bool> {
        let handler = make_handler(py, handler)?;
        let pattern = MStr::<Pattern>::pattern(topic);
        let sub = Subscription::new(pattern, handler, None);
        Ok(get_message_bus().borrow().subscriptions.contains(&sub))
    }

    /// Returns whether the given request ID is pending a response.
    #[pyo3(name = "is_pending_request")]
    fn py_is_pending_request(&self, request_id: UUID4) -> bool {
        self.correlation_index.contains_key(&request_id)
    }

    /// Returns whether the given type is registered for streaming.
    #[pyo3(name = "is_streaming_type")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_is_streaming_type(&self, py: Python<'_>, cls: Py<PyAny>) -> bool {
        let cls_ref = cls.bind(py);
        self.streaming_types.iter().any(|t| t.bind(py).is(cls_ref))
    }

    /// Returns all types registered for streaming.
    #[pyo3(name = "streaming_types")]
    fn py_streaming_types(&self, py: Python<'_>) -> Vec<Py<PyAny>> {
        self.streaming_types
            .iter()
            .map(|t| t.clone_ref(py))
            .collect()
    }

    /// Registers a handler at the given endpoint address.
    #[pyo3(name = "register")]
    fn py_register(&self, py: Python<'_>, endpoint: &str, handler: Py<PyAny>) -> PyResult<()> {
        let handler = make_handler(py, handler)?;
        let endpoint = MStr::<Endpoint>::from(endpoint);
        msgbus_api::register_any(endpoint, handler);
        Ok(())
    }

    /// Deregisters the handler from the given endpoint address.
    #[pyo3(name = "deregister")]
    #[pyo3(signature = (endpoint, handler=None))]
    #[expect(clippy::needless_pass_by_value)]
    fn py_deregister(&self, endpoint: &str, handler: Option<Py<PyAny>>) {
        let _ = handler;
        let endpoint = MStr::<Endpoint>::from(endpoint);
        msgbus_api::deregister_any(endpoint);
    }

    /// Sends a message to the given endpoint address.
    #[pyo3(name = "send")]
    fn py_send(&mut self, endpoint: &str, msg: Py<PyAny>) {
        let endpoint = MStr::<Endpoint>::from(endpoint);
        let py_msg = PyMessage(msg);
        msgbus_api::send_any(endpoint, &py_msg);
        self.sent_count += 1;
    }

    /// Sends a request to the given endpoint with correlation tracking.
    #[pyo3(name = "request")]
    fn py_request(&mut self, py: Python<'_>, endpoint: &str, request: Py<PyAny>) -> PyResult<()> {
        let request_ref = request.bind(py);

        let request_id: UUID4 = request_ref.getattr("id")?.extract()?;
        let callback = request_ref.getattr("callback")?;

        if self.correlation_index.contains_key(&request_id) {
            log::error!(
                "Cannot handle request: duplicate ID {request_id} found in correlation index"
            );
            return Ok(());
        }

        if !callback.is_none() {
            self.correlation_index.insert(request_id, callback.unbind());
        }

        let endpoint = MStr::<Endpoint>::from(endpoint);
        let py_msg = PyMessage(request);
        msgbus_api::send_any(endpoint, &py_msg);
        self.req_count += 1;

        Ok(())
    }

    /// Handles a response by invoking the correlated callback.
    #[pyo3(name = "response")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_response(&mut self, py: Python<'_>, response: Py<PyAny>) -> PyResult<()> {
        let correlation_id: UUID4 = response.getattr(py, "correlation_id")?.extract(py)?;

        if let Some(callback) = self.correlation_index.remove(&correlation_id) {
            callback.call1(py, (&response,))?;
        } else {
            log::debug!("No callback for correlation_id {correlation_id}");
        }

        self.res_count += 1;
        Ok(())
    }

    /// Subscribes to the given topic with the given handler.
    #[pyo3(name = "subscribe")]
    #[pyo3(signature = (topic, handler, priority=0))]
    fn py_subscribe(
        &self,
        py: Python<'_>,
        topic: &str,
        handler: Py<PyAny>,
        priority: u8,
    ) -> PyResult<()> {
        let handler = make_handler(py, handler)?;
        let pattern = MStr::<Pattern>::pattern(topic);
        msgbus_api::subscribe_any(pattern, handler, Some(priority));
        Ok(())
    }

    /// Unsubscribes the given handler from the given topic.
    #[pyo3(name = "unsubscribe")]
    fn py_unsubscribe(&self, py: Python<'_>, topic: &str, handler: Py<PyAny>) -> PyResult<()> {
        let handler = make_handler(py, handler)?;
        let pattern = MStr::<Pattern>::pattern(topic);
        msgbus_api::unsubscribe_any(pattern, &handler);
        Ok(())
    }

    /// Publishes a message for the given topic.
    #[pyo3(name = "publish")]
    #[pyo3(signature = (topic, msg, external_pub=true))]
    #[expect(clippy::needless_pass_by_value)]
    fn py_publish(
        &mut self,
        py: Python<'_>,
        topic: &str,
        msg: Py<PyAny>,
        external_pub: bool,
    ) -> PyResult<()> {
        let topic_mstr = MStr::<Topic>::topic(topic).map_err(to_pyruntime_err)?;

        let py_msg = PyMessage(msg.clone_ref(py));
        msgbus_api::publish_any(topic_mstr, &py_msg);

        if external_pub {
            self.publish_external(py, topic, &msg)?;
        }

        self.pub_count += 1;
        Ok(())
    }

    /// Disposes of the message bus, clearing all state.
    #[pyo3(name = "dispose")]
    fn py_dispose(&mut self, py: Python<'_>) -> PyResult<()> {
        log::debug!("Closing message bus");

        get_message_bus().borrow_mut().dispose();

        self.correlation_index.clear();
        self.listeners.clear();
        self.streaming_types.clear();

        if let Some(ref database) = self.database {
            let db = database.bind(py);
            if !db.call_method0("is_closed")?.extract::<bool>()? {
                db.call_method0("close")?;
            }
        }

        log::info!("Closed message bus");
        Ok(())
    }

    /// Registers a type for external-to-internal message streaming.
    #[pyo3(name = "add_streaming_type")]
    fn py_add_streaming_type(&mut self, cls: Py<PyAny>) {
        self.streaming_types.push(cls);
    }

    /// Adds a listener to the message bus.
    #[pyo3(name = "add_listener")]
    fn py_add_listener(&mut self, listener: Py<PyAny>) {
        self.listeners.push(listener);
    }
}

impl PyMessageBus {
    fn publish_external(&self, py: Python<'_>, topic: &str, msg: &Py<PyAny>) -> PyResult<()> {
        if let Some(ref filter) = self.types_filter {
            let is_excluded = py
                .import("builtins")?
                .call_method1("isinstance", (msg, filter))?
                .extract::<bool>()?;

            if is_excluded {
                return Ok(());
            }
        }

        // Serialize: raw bytes pass through, other types need a serializer
        let msg_ref = msg.bind(py);
        let payload: Py<PyAny> = if msg_ref.is_instance_of::<pyo3::types::PyBytes>() {
            msg.clone_ref(py)
        } else if let Some(ref serializer) = self.serializer {
            serializer.call_method1(py, "serialize", (msg,))?
        } else {
            return Ok(());
        };

        if let Some(ref database) = self.database {
            let db = database.bind(py);
            if !db.call_method0("is_closed")?.extract::<bool>()? {
                db.call_method1("publish", (topic, &payload))?;
            }
        }

        for listener in &self.listeners {
            let l = listener.bind(py);
            if l.call_method0("is_closed")?.extract::<bool>()? {
                continue;
            }
            l.call_method1("publish", (topic, &payload))?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::any::Any;

    use pyo3::ffi::c_str;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_py_message_downcast() {
        pyo3::Python::initialize();
        Python::attach(|py| {
            let py_obj = py.eval(c_str!("42"), None, None).unwrap();
            let msg = PyMessage(py_obj.unbind());

            let any_ref: &dyn Any = &msg;
            let downcasted = any_ref.downcast_ref::<PyMessage>();
            assert!(downcasted.is_some());

            let inner = &downcasted.unwrap().0;
            let value: i64 = inner.extract(py).unwrap();
            assert_eq!(value, 42);
        });
    }

    #[rstest]
    fn test_py_callable_handler_id_stability() {
        pyo3::Python::initialize();
        Python::attach(|py| {
            let callable = py.eval(c_str!("lambda x: x"), None, None).unwrap().unbind();

            let handler1 = PyCallableHandler::new(py, callable.clone_ref(py)).unwrap();
            let handler2 = PyCallableHandler::new(py, callable).unwrap();

            assert_eq!(handler1.id(), handler2.id());
        });
    }

    #[rstest]
    fn test_py_callable_handler_dispatch() {
        pyo3::Python::initialize();
        Python::attach(|py| {
            let main = py.import("__main__").unwrap();
            let globals = main.dict();
            py.run(
                c_str!("results = []\ndef handler(x): results.append(x)"),
                Some(&globals),
                None,
            )
            .unwrap();

            let handler_fn = globals.get_item("handler").unwrap().unwrap().unbind();
            let handler = PyCallableHandler::new(py, handler_fn).unwrap();

            let py_obj = py.eval(c_str!("'hello'"), None, None).unwrap();
            let msg = PyMessage(py_obj.unbind());

            let any_ref: &dyn Any = &msg;
            handler.handle(any_ref);

            let results = globals.get_item("results").unwrap().unwrap();
            let len: usize = results.len().unwrap();
            assert_eq!(len, 1);
        });
    }
}
