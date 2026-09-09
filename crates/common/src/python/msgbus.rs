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
//! [`PyMessageBusScope`] owns each component's Python subscriptions.

use std::{
    any::Any,
    cell::{Cell, RefCell},
    fmt::Debug,
    rc::{Rc, Weak},
    sync::LazyLock,
};

use ahash::AHashMap;
use nautilus_core::{
    UUID4,
    python::{to_pyruntime_err, to_pytype_err, to_pyvalue_err},
};
use nautilus_model::identifiers::TraderId;
use pyo3::{Py, Python, prelude::*, types::PyBytes};
use ustr::Ustr;

use crate::{
    enums::SerializationEncoding,
    msgbus::{
        self as msgbus_api, BusMessage, MessageBus, MessageBusBackingFactory, MessageBusConfig,
        core::Subscription,
        get_message_bus,
        matching::is_matching,
        mstr::{Endpoint, MStr, Pattern, Topic},
        try_get_message_bus,
        typed_handler::{Handler, ShareableMessageHandler, TypedHandler},
    },
    python::{
        config_error_to_pyvalue_err,
        factory::{FactoryExtractor, FactoryRegistry},
    },
};

/// Function type for extracting a Python object into a boxed message bus backing factory.
pub type MessageBusFactoryExtractor = FactoryExtractor<dyn MessageBusBackingFactory>;

/// Registry for Python message bus backing factory extractors.
#[derive(Debug)]
pub struct MessageBusFactoryRegistry {
    inner: FactoryRegistry<dyn MessageBusBackingFactory>,
}

impl MessageBusFactoryRegistry {
    /// Creates an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: FactoryRegistry::new("message bus factory"),
        }
    }

    // panics-doc-ok (transitive via FactoryRegistry mutex locking)
    /// Registers an extractor for a Python factory type name.
    ///
    /// # Errors
    ///
    /// Returns an error if a different extractor is already registered for the type name.
    pub fn register(
        &self,
        type_name: String,
        extractor: MessageBusFactoryExtractor,
    ) -> anyhow::Result<()> {
        self.inner.register(type_name, extractor)
    }

    // panics-doc-ok (transitive via FactoryRegistry mutex locking)
    /// Extracts a Python object into a boxed message bus backing factory.
    ///
    /// # Errors
    ///
    /// Returns an error if no extractor is registered for the Python type or extraction fails.
    pub fn extract(
        &self,
        py: Python<'_>,
        factory: Py<PyAny>,
    ) -> PyResult<Box<dyn MessageBusBackingFactory>> {
        self.inner.extract(py, factory)
    }
}

impl Default for MessageBusFactoryRegistry {
    fn default() -> Self {
        Self::new()
    }
}

static GLOBAL_MSGBUS_FACTORY_REGISTRY: LazyLock<MessageBusFactoryRegistry> =
    LazyLock::new(MessageBusFactoryRegistry::new);

/// Returns the global Python message bus backing factory registry.
#[must_use]
pub fn get_global_msgbus_factory_registry() -> &'static MessageBusFactoryRegistry {
    &GLOBAL_MSGBUS_FACTORY_REGISTRY
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BusMessage {
    #[getter]
    #[pyo3(name = "topic")]
    fn py_topic(&self) -> String {
        self.topic.to_string()
    }

    #[getter]
    #[pyo3(name = "payload_type")]
    fn py_payload_type(&self) -> String {
        self.payload_type.to_string()
    }

    #[getter]
    #[pyo3(name = "payload")]
    fn py_payload(&self, py: Python<'_>) -> Py<PyBytes> {
        PyBytes::new(py, self.payload.as_ref()).into()
    }

    #[getter]
    #[pyo3(name = "encoding")]
    fn py_encoding(&self) -> SerializationEncoding {
        self.encoding
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
impl MessageBusConfig {
    /// Configuration for `MessageBus` instances.
    #[new]
    #[expect(clippy::too_many_arguments)]
    #[pyo3(signature = (encoding=None, encoding_market_data=None, encoding_builtin=None, timestamps_as_iso8601=None, buffer_interval_ms=None, autotrim_mins=None, autotrim_maxlen=None, use_trader_prefix=None, use_trader_id=None, use_instance_id=None, streams_prefix=None, stream_per_topic=None, external_streams=None, types_filter=None, heartbeat_interval_secs=None))]
    fn py_new(
        encoding: Option<SerializationEncoding>,
        encoding_market_data: Option<SerializationEncoding>,
        encoding_builtin: Option<SerializationEncoding>,
        timestamps_as_iso8601: Option<bool>,
        buffer_interval_ms: Option<u32>,
        autotrim_mins: Option<u32>,
        autotrim_maxlen: Option<u32>,
        use_trader_prefix: Option<bool>,
        use_trader_id: Option<bool>,
        use_instance_id: Option<bool>,
        streams_prefix: Option<String>,
        stream_per_topic: Option<bool>,
        external_streams: Option<Vec<String>>,
        types_filter: Option<Vec<String>>,
        heartbeat_interval_secs: Option<u16>,
    ) -> PyResult<Self> {
        let default = Self::default();
        let config = Self {
            encoding: encoding.unwrap_or(default.encoding),
            encoding_market_data,
            encoding_builtin,
            timestamps_as_iso8601: timestamps_as_iso8601.unwrap_or(default.timestamps_as_iso8601),
            buffer_interval_ms,
            autotrim_mins,
            autotrim_maxlen,
            use_trader_prefix: use_trader_prefix.unwrap_or(default.use_trader_prefix),
            use_trader_id: use_trader_id.unwrap_or(default.use_trader_id),
            use_instance_id: use_instance_id.unwrap_or(default.use_instance_id),
            streams_prefix: streams_prefix.unwrap_or(default.streams_prefix),
            stream_per_topic: stream_per_topic.unwrap_or(default.stream_per_topic),
            external_streams,
            types_filter,
            heartbeat_interval_secs,
        };

        config.validate().map_err(config_error_to_pyvalue_err)?;
        Ok(config)
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }

    #[getter]
    fn encoding(&self) -> SerializationEncoding {
        self.encoding
    }

    #[getter]
    fn encoding_market_data(&self) -> Option<SerializationEncoding> {
        self.encoding_market_data
    }

    #[getter]
    fn encoding_builtin(&self) -> Option<SerializationEncoding> {
        self.encoding_builtin
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
    fn autotrim_maxlen(&self) -> Option<u32> {
        self.autotrim_maxlen
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
        Ok(Self::with_id(id, callable))
    }

    pub(crate) fn with_id(id: Ustr, callable: Py<PyAny>) -> Self {
        Self { id, callable }
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
/// Publish, subscribe, and request/response calls from Python route through the
/// single Rust bus. Python custom events travel through the Any-based dispatch
/// path via [`PyMessage`] wrappers.
#[pyclass(module = "nautilus_trader.common", name = "MessageBus", unsendable)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.common")]
pub struct PyMessageBus {
    trader_id: TraderId,
    instance_id: UUID4,
    name: String,
    has_backing: bool,
    serializer: Option<Py<PyAny>>,
    backing: Option<Py<PyAny>>,
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
    #[pyo3(signature = (trader_id, clock=None, instance_id=None, name=None, serializer=None, backing=None, config=None))]
    #[expect(clippy::too_many_arguments, clippy::needless_pass_by_value)]
    fn py_new(
        py: Python<'_>,
        trader_id: TraderId,
        clock: Option<Py<PyAny>>,
        instance_id: Option<UUID4>,
        name: Option<String>,
        serializer: Option<Py<PyAny>>,
        backing: Option<Py<PyAny>>,
        config: Option<Py<PyAny>>,
    ) -> PyResult<Self> {
        let _ = clock;
        let instance_id = instance_id.unwrap_or_default();
        let bus_name = name.clone();
        let has_backing = backing.is_some();

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
            backing,
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

    /// Returns whether the message bus has an external backing.
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
    fn py_subscriptions(&self, pattern: Option<&str>) -> PyResult<Vec<String>> {
        let filter = pattern.map(parse_pattern).transpose()?;

        let bus = get_message_bus();
        let bus_ref = bus.borrow();
        let subs: Vec<&Subscription> = bus_ref.subscriptions();

        Ok(subs
            .into_iter()
            .filter(|s| filter.is_none_or(|f| is_matching(s.pattern.as_bytes(), f.as_bytes())))
            .map(|s| {
                format!(
                    "Subscription(topic={}, handler={})",
                    s.pattern, s.handler_id
                )
            })
            .collect())
    }

    /// Returns whether there are subscribers for the given topic pattern.
    #[pyo3(name = "has_subscribers")]
    #[pyo3(signature = (pattern=None))]
    fn py_has_subscribers(&self, pattern: Option<&str>) -> PyResult<bool> {
        let filter = pattern.map(parse_pattern).transpose()?;

        let bus = get_message_bus();
        let bus_ref = bus.borrow();

        Ok(match filter {
            Some(filter) => bus_ref
                .subscriptions()
                .iter()
                .any(|s| is_matching(s.pattern.as_bytes(), filter.as_bytes())),
            None => !bus_ref.subscriptions().is_empty(),
        })
    }

    /// Returns whether the given topic and handler is subscribed.
    #[pyo3(name = "is_subscribed")]
    fn py_is_subscribed(&self, py: Python<'_>, topic: &str, handler: Py<PyAny>) -> PyResult<bool> {
        let pattern = parse_pattern(topic)?;
        let handler = make_handler(py, handler)?;
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
        let endpoint = parse_endpoint(endpoint)?;
        let handler = make_handler(py, handler)?;
        msgbus_api::register_any(endpoint, handler);
        Ok(())
    }

    /// Deregisters the handler from the given endpoint address.
    #[pyo3(name = "deregister")]
    #[pyo3(signature = (endpoint, handler=None))]
    #[expect(clippy::needless_pass_by_value)]
    fn py_deregister(&self, endpoint: &str, handler: Option<Py<PyAny>>) -> PyResult<()> {
        let _ = handler;
        let endpoint = parse_endpoint(endpoint)?;
        msgbus_api::deregister_any(endpoint);
        Ok(())
    }

    /// Sends a message to the given endpoint address.
    #[pyo3(name = "send")]
    fn py_send(&mut self, endpoint: &str, msg: Py<PyAny>) -> PyResult<()> {
        let endpoint = parse_endpoint(endpoint)?;
        let py_msg = PyMessage(msg);
        msgbus_api::send_any(endpoint, &py_msg);
        self.sent_count += 1;
        Ok(())
    }

    /// Sends a request to the given endpoint with correlation tracking.
    #[pyo3(name = "request")]
    fn py_request(&mut self, py: Python<'_>, endpoint: &str, request: Py<PyAny>) -> PyResult<()> {
        let endpoint = parse_endpoint(endpoint)?;
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
        priority: u32,
    ) -> PyResult<()> {
        let pattern = parse_pattern(topic)?;
        let handler = make_handler(py, handler)?;
        msgbus_api::subscribe_any(pattern, handler, Some(priority));
        Ok(())
    }

    /// Unsubscribes the given handler from the given topic.
    #[pyo3(name = "unsubscribe")]
    fn py_unsubscribe(&self, py: Python<'_>, topic: &str, handler: Py<PyAny>) -> PyResult<()> {
        let pattern = parse_pattern(topic)?;
        let handler = make_handler(py, handler)?;
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

        if let Some(ref backing) = self.backing {
            let db = backing.bind(py);
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

        if let Some(ref backing) = self.backing {
            let db = backing.bind(py);
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

fn parse_endpoint(endpoint: &str) -> PyResult<MStr<Endpoint>> {
    MStr::<Endpoint>::endpoint(endpoint).map_err(to_pyvalue_err)
}

fn parse_pattern(pattern: &str) -> PyResult<MStr<Pattern>> {
    MStr::<Pattern>::pattern_checked(pattern).map_err(to_pyvalue_err)
}

/// Owns a component's Python subscriptions without owning the runtime bus.
#[derive(Debug, Default)]
pub struct PyMessageBusScope {
    bus: RefCell<Weak<RefCell<MessageBus>>>,
    subscriptions: RefCell<Vec<ScopedSubscription>>,
    clearing: Cell<bool>,
}

impl PyMessageBusScope {
    pub(crate) fn register(&self) {
        *self.bus.borrow_mut() =
            try_get_message_bus().map_or_else(Weak::new, |bus| Rc::downgrade(&bus));
    }

    /// Publishes the original Python object synchronously on an application topic.
    ///
    /// Handlers receive the same object. Nested publication finishes before the outer call
    /// returns. Python handler exceptions are logged and do not interrupt other handlers.
    /// This does not serialize or externally publish the object.
    ///
    /// # Errors
    ///
    /// Returns an error if the topic is invalid or the registered runtime bus is no longer active.
    pub fn publish_message(&self, topic: &str, message: Py<PyAny>) -> PyResult<()> {
        let topic = MStr::<Topic>::topic(topic).map_err(to_pyvalue_err)?;
        self.active_bus()?;
        msgbus_api::publish_any(topic, &PyMessage(message));
        Ok(())
    }

    /// Subscribes a callable to an application topic pattern.
    ///
    /// Higher priorities run first. Repeating a pattern and callable within this component is
    /// a no-op, even with a different priority. Unsubscribe first to change priority.
    /// Python-defined bound methods are identified by their receiver and function; other callables
    /// use object identity. Subscriptions owned by other components remain independent.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid pattern, a non-callable handler, or an inactive runtime bus.
    pub fn subscribe_topic(
        &self,
        py: Python<'_>,
        topic: &str,
        handler: Py<PyAny>,
        priority: u32,
    ) -> PyResult<()> {
        let pattern = MStr::<Pattern>::pattern_checked(topic).map_err(to_pyvalue_err)?;
        let identity = CallableIdentity::new(handler.bind(py))?;
        self.active_bus()?;

        if self
            .subscriptions
            .borrow()
            .iter()
            .any(|sub| sub.pattern == pattern && sub.identity == identity)
        {
            return Ok(());
        }

        let id = format!("python-component:{}", UUID4::new()).into();
        let handler = TypedHandler(
            Rc::new(PyCallableHandler::with_id(id, handler)) as Rc<dyn Handler<dyn Any>>
        );
        msgbus_api::subscribe_any(pattern, handler.clone(), Some(priority));

        self.subscriptions.borrow_mut().push(ScopedSubscription {
            pattern,
            identity,
            handler,
        });

        Ok(())
    }

    /// Removes this component's exact topic pattern and callable subscription.
    ///
    /// An absent subscription is a no-op. A callback already snapshotted by a synchronous
    /// publication may still run in that publication.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid pattern, a non-callable handler, or an inactive runtime bus.
    pub fn unsubscribe_topic(&self, topic: &str, handler: &Bound<'_, PyAny>) -> PyResult<()> {
        let pattern = MStr::<Pattern>::pattern_checked(topic).map_err(to_pyvalue_err)?;
        let identity = CallableIdentity::new(handler)?;
        let bus = self.active_bus()?;

        let removed = {
            let mut subscriptions = self.subscriptions.borrow_mut();
            subscriptions
                .iter()
                .position(|sub| sub.pattern == pattern && sub.identity == identity)
                .map(|index| subscriptions.remove(index))
        };

        if let Some(sub) = removed {
            bus.borrow_mut().unsubscribe_any(sub.pattern, &sub.handler);
        }

        Ok(())
    }

    pub(crate) fn invalidate(&self) {
        *self.bus.borrow_mut() = Weak::new();
    }

    pub(crate) fn clear(&self) {
        if self.clearing.replace(true) {
            return;
        }

        let subscriptions = std::mem::take(&mut *self.subscriptions.borrow_mut());

        if let Some(bus) = self.bus.borrow().upgrade() {
            let mut bus = bus.borrow_mut();
            for sub in &subscriptions {
                bus.unsubscribe_any(sub.pattern, &sub.handler);
            }
        }

        // Callable finalizers may re-enter Python; release them after all bookkeeping borrows
        drop(subscriptions);
        self.clearing.set(false);
    }

    fn active_bus(&self) -> PyResult<Rc<RefCell<MessageBus>>> {
        if self.clearing.get() {
            return Err(to_pyruntime_err("Component is releasing subscriptions"));
        }

        let registered =
            self.bus.borrow().upgrade().ok_or_else(|| {
                to_pyruntime_err("Component's registered message bus is unavailable")
            })?;
        let active = try_get_message_bus()
            .ok_or_else(|| to_pyruntime_err("No runtime message bus is active on this thread"))?;

        if !Rc::ptr_eq(&registered, &active) {
            return Err(to_pyruntime_err(
                "Component's registered message bus has been replaced",
            ));
        }

        Ok(registered)
    }
}

#[derive(Debug)]
struct ScopedSubscription {
    pattern: MStr<Pattern>,
    identity: CallableIdentity,
    handler: ShareableMessageHandler,
}

#[derive(Debug, PartialEq, Eq)]
struct CallableIdentity {
    receiver: usize,
    function: Option<usize>,
}

impl CallableIdentity {
    fn new(callable: &Bound<'_, PyAny>) -> PyResult<Self> {
        if !callable.is_callable() {
            return Err(to_pytype_err("handler must be callable"));
        }

        let method_type = callable.py().import("types")?.getattr("MethodType")?;
        if callable.get_type().is(&method_type) {
            return Ok(Self {
                receiver: callable.getattr("__self__")?.as_ptr() as usize,
                function: Some(callable.getattr("__func__")?.as_ptr() as usize),
            });
        }

        Ok(Self {
            receiver: callable.as_ptr() as usize,
            function: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::any::Any;

    use nautilus_model::{
        data::QuoteTick,
        identifiers::{ActorId, ComponentId},
        types::{Price, Quantity},
    };
    use pyo3::{exceptions::PyValueError, ffi::c_str, types::PyDict};
    use rstest::rstest;

    use super::*;
    use crate::{
        actor::registry::with_actor_registry,
        cache::Cache,
        clock::TestClock,
        component::{release_component_subscriptions, with_component_registry},
        msgbus::set_message_bus,
        python::{actor::PyDataActor, wrappers::release_python_wrapper},
    };

    #[rstest]
    fn test_message_bus_factory_registry_compatibility_constructors() {
        let registry = MessageBusFactoryRegistry::new();
        let default_registry = MessageBusFactoryRegistry::default();

        assert_eq!(format!("{registry:?}"), format!("{default_registry:?}"));
        assert!(format!("{registry:?}").contains("message bus factory"));
    }

    #[rstest]
    fn message_bus_config_py_new_maps_validate_error_to_value_error() {
        pyo3::Python::initialize();
        Python::attach(|py| {
            let err = MessageBusConfig::py_new(
                Some(SerializationEncoding::Json),
                None,
                Some(SerializationEncoding::Capnp),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap_err();

            assert!(err.is_instance_of::<PyValueError>(py));
            assert_eq!(
                err.value(py).to_string(),
                format!(
                    "MessageBusConfig.encoding_builtin has unsupported value: {} is not supported by AccountState, OrderEventAny, PositionEvent, PortfolioSnapshot",
                    SerializationEncoding::Capnp
                )
            );
        });
    }

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
    #[rstest]
    fn test_py_message_bus_scope_object_identity_nested_delivery_and_subscription_ownership() {
        Python::initialize();
        Python::attach(|py| {
            set_message_bus(Rc::new(RefCell::new(MessageBus::default())));
            let first = registered_actor(py, "MESSAGE-FIRST");
            let second = registered_actor(py, "MESSAGE-SECOND");
            let locals = PyDict::new(py);
            locals.set_item("first", &first).unwrap();
            locals.set_item("second", &second).unwrap();
            py.run(
                c_str!(
                    r#"
message = {"quantity": 73, "labels": ["alpha", "beta"]}
seen = []
def high(value):
    seen.append(("high", value))
    first.publish_message("app.nested", value)
    seen.append(("returned", value))
def nested(value):
    seen.append(("nested", value))
def low(value):
    seen.append(("low", value))
first.subscribe_topic("app.outer", high, 50)
first.subscribe_topic("app.outer", high, 100)
second.subscribe_topic("app.*", low, 2)
second.subscribe_topic("app.nested", nested, 20)
first.publish_message("app.outer", message)
assert [name for name, _ in seen] == ["high", "nested", "low", "returned", "low"]
assert all(value is message for _, value in seen)
assert message == {"quantity": 73, "labels": ["alpha", "beta"]}
first.unsubscribe_topic("app.outer", high)
first.unsubscribe_topic("app.outer", high)
seen.clear()
first.publish_message("app.outer", message)
assert seen == [("low", message)]
first.subscribe_topic("app.shared", low)
second.subscribe_topic("app.shared", low)
first.unsubscribe_topic("app.shared", low)
seen.clear()
first.publish_message("app.shared", message)
assert seen == [("low", message), ("low", message)]
for owner, peer in [(first, second), (second, first)]:
    owner.unsubscribe_topic("isolated.shared", low)
    owner.subscribe_topic("isolated.shared", low)
    peer.subscribe_topic("isolated.shared", low)
    owner.unsubscribe_topic("isolated.shared", low)
    seen.clear()
    first.publish_message("isolated.shared", message)
    assert seen == [("low", message)]
    peer.unsubscribe_topic("isolated.shared", low)
    seen.clear()
    first.publish_message("isolated.shared", message)
    assert seen == []
"#
                ),
                Some(&locals),
                None,
            )
            .unwrap();
            first.call_method0("dispose").unwrap();
            second.call_method0("dispose").unwrap();
            locals.clear();
            release_actor("MESSAGE-FIRST");
            release_actor("MESSAGE-SECOND");
        });
    }

    #[rstest]
    fn test_py_message_bus_scope_callable_identity_and_reference_release() {
        Python::initialize();
        Python::attach(|py| {
            set_message_bus(Rc::new(RefCell::new(MessageBus::default())));
            let actor = registered_actor(py, "MESSAGE-IDENTITY");
            let locals = PyDict::new(py);
            locals.set_item("actor", &actor).unwrap();
            py.run(
                c_str!(
                    r#"
import gc
import weakref
seen = []
class Receiver:
    def __init__(self, name): self.name = name
    def __repr__(self): raise AssertionError("repr must not run")
    def __eq__(self, other): raise AssertionError("equality must not run")
    def __call__(self, value): seen.append((self.name, value))
    def receive(self, value): seen.append((self.name, value))
left = Receiver("left")
right = Receiver("right")
actor.subscribe_topic("app.callables", left, 2)
actor.subscribe_topic("app.callables", right, 1)
actor.unsubscribe_topic("app.callables", left)
actor.subscribe_topic("app.methods", left.receive)
actor.subscribe_topic("app.methods", left.receive)
actor.unsubscribe_topic("app.methods", left.receive)
actor.publish_message("app.methods", 19)
actor.publish_message("app.callables", 23)
assert seen == [("right", 23)]
ref = weakref.ref(right)
del right
assert ref() is not None
actor.dispose()
gc.collect()
assert ref() is None
"#
                ),
                Some(&locals),
                None,
            )
            .unwrap();
            locals.clear();
            release_actor("MESSAGE-IDENTITY");
        });
    }

    #[rstest]
    fn test_py_message_bus_scope_lifecycle_and_foreign_thread_errors() {
        Python::initialize();
        Python::attach(|py| {
            set_message_bus(Rc::new(RefCell::new(MessageBus::default())));
            let actor = registered_actor(py, "MESSAGE-LIFECYCLE");
            let locals = PyDict::new(py);
            locals.set_item("actor", &actor).unwrap();
            locals
                .set_item(
                    "unregistered",
                    py.get_type::<PyDataActor>().call0().unwrap(),
                )
                .unwrap();
            py.run(
                c_str!(
                    r#"
import threading
seen = []
def handler(value): seen.append(value)
def assert_runtime_errors(component):

    for name, args in [("publish_message", ("app.events", 9)),
                       ("subscribe_topic", ("app.events", handler)),
                       ("unsubscribe_topic", ("app.events", handler))]:
        try: getattr(component, name)(*args)
        except RuntimeError: pass
        else: raise AssertionError(name)
assert_runtime_errors(unregistered)
errors = []
def foreign():
    try: assert_runtime_errors(actor)
    except BaseException as error: errors.append(error)
thread = threading.Thread(target=foreign)
thread.start()
thread.join()
assert errors == []
actor.on_start = lambda: actor.subscribe_topic("app.events", handler)
actor.start()
actor.stop()
actor.publish_message("app.events", 31)
actor.resume()
actor.publish_message("app.events", 37)
actor.stop()
actor.reset()
actor.publish_message("app.events", 41)
assert seen == [31, 37]
actor.subscribe_topic("app.events", handler)
actor.dispose()
assert_runtime_errors(actor)
"#
                ),
                Some(&locals),
                None,
            )
            .unwrap();
            assert_eq!(get_message_bus().borrow().subscriptions().len(), 0);
            locals.clear();
            release_actor("MESSAGE-LIFECYCLE");
        });
    }

    #[rstest]
    fn test_py_message_bus_scope_replacement_rejected_and_original_cleaned() {
        Python::initialize();
        Python::attach(|py| {
            let original = Rc::new(RefCell::new(MessageBus::default()));
            set_message_bus(Rc::clone(&original));
            let actor = registered_actor(py, "MESSAGE-REPLACED");
            let handler = py.eval(c_str!("lambda value: None"), None, None).unwrap();
            actor
                .call_method1("subscribe_topic", ("app.events", &handler))
                .unwrap();
            let replacement = Rc::new(RefCell::new(MessageBus::default()));
            set_message_bus(Rc::clone(&replacement));
            let error = actor
                .call_method1("publish_message", ("app.events", 43))
                .unwrap_err();
            assert!(error.is_instance_of::<pyo3::exceptions::PyRuntimeError>(py));
            assert_eq!(original.borrow().subscriptions().len(), 1);
            actor.call_method0("dispose").unwrap();
            assert_eq!(original.borrow().subscriptions().len(), 0);
            assert!(Rc::ptr_eq(&replacement, &get_message_bus()));
            release_actor("MESSAGE-REPLACED");
        });
    }

    #[rstest]
    fn test_py_message_bus_scope_missing_runtime_does_not_create_one() {
        Python::initialize();
        Python::attach(|py| {
            assert!(try_get_message_bus().is_none());
            let actor = registered_actor(py, "MESSAGE-MISSING");
            let error = actor
                .call_method1("publish_message", ("app.events", 47))
                .unwrap_err();
            assert!(error.is_instance_of::<pyo3::exceptions::PyRuntimeError>(py));
            assert_eq!(
                error.value(py).str().unwrap().to_str().unwrap(),
                "Component's registered message bus is unavailable"
            );
            assert!(try_get_message_bus().is_none());
            release_actor("MESSAGE-MISSING");
        });
    }

    #[rstest]
    fn test_py_message_bus_scope_input_errors_and_existing_routes() {
        Python::initialize();
        Python::attach(|py| {
            let bus = Rc::new(RefCell::new(MessageBus::default()));
            set_message_bus(Rc::clone(&bus));
            let endpoint_received = Rc::new(RefCell::new(Vec::<i32>::new()));
            let received = Rc::clone(&endpoint_received);
            let endpoint_handler = ShareableMessageHandler::from_typed(move |value: &i32| {
                received.borrow_mut().push(*value);
            });
            msgbus_api::register_any("app.endpoint".into(), endpoint_handler);
            let quotes_received = Rc::new(RefCell::new(Vec::new()));
            let received = Rc::clone(&quotes_received);
            let quote_handler =
                TypedHandler::from(move |quote: &QuoteTick| received.borrow_mut().push(*quote));
            msgbus_api::subscribe_quotes("app.quotes".into(), quote_handler.clone(), None);
            let actor = registered_actor(py, "MESSAGE-ERRORS");
            let locals = PyDict::new(py);
            locals.set_item("actor", &actor).unwrap();
            py.run(
                c_str!(
                    r#"
def handler(value): pass

for name, args, expected in [
    ("publish_message", ("", 53), ValueError),
    ("publish_message", ("app.*", 59), ValueError),
    ("subscribe_topic", ("", handler), ValueError),
    ("unsubscribe_topic", ("", handler), ValueError),
    ("subscribe_topic", ("app.events", 61), TypeError),
    ("unsubscribe_topic", ("app.events", 67), TypeError),
    ("subscribe_topic", ("app.events", handler, -1), OverflowError),
    ("subscribe_topic", ("app.events", handler, 2**32), OverflowError),
]:
    try: getattr(actor, name)(*args)
    except expected: pass
    else: raise AssertionError((name, args))
actor.subscribe_topic("app.events", handler)
actor.publish_message("app.events", 71)
actor.unsubscribe_topic("app.events", handler)
actor.dispose()
"#
                ),
                Some(&locals),
                None,
            )
            .unwrap();
            msgbus_api::send_any("app.endpoint".into(), &73_i32);
            let quote = QuoteTick::new(
                "BTCUSDT.BINANCE".into(),
                Price::from("101.25"),
                Price::from("102.50"),
                Quantity::from("3.75"),
                Quantity::from("4.50"),
                123.into(),
                456.into(),
            );
            msgbus_api::publish_quote("app.quotes".into(), &quote);
            assert_eq!(*endpoint_received.borrow(), vec![73]);
            assert_eq!(*quotes_received.borrow(), vec![quote]);
            assert!(Rc::ptr_eq(&bus, &get_message_bus()));
            msgbus_api::unsubscribe_quotes("app.quotes".into(), &quote_handler);
            msgbus_api::deregister_any("app.endpoint".into());
            locals.clear();
            release_actor("MESSAGE-ERRORS");
        });
    }

    #[rstest]
    fn test_py_message_bus_scope_failed_disposal_and_finalizer_reentry() {
        Python::initialize();
        Python::attach(|py| {
            set_message_bus(Rc::new(RefCell::new(MessageBus::default())));
            let actor = registered_actor(py, "MESSAGE-FINALIZER");
            let locals = PyDict::new(py);
            locals.set_item("actor", &actor).unwrap();
            py.run(
                c_str!(
                    r#"
seen = []
finalized = []
def handler(value): seen.append(value)
def fail(): raise ValueError("disposal refused")
actor.subscribe_topic("app.events", handler)
actor.on_dispose = fail
try: actor.dispose()
except RuntimeError as error: assert "disposal refused" in str(error)
else: raise AssertionError("disposal must fail")
actor.publish_message("app.events", 79)
assert seen == [79]
class Finalizer:
    def __call__(self, value): pass
    def __del__(self):
        try: actor.subscribe_topic("app.events", handler)
        except RuntimeError: finalized.append("rejected")
        else: finalized.append("subscribed")
actor.subscribe_topic("app.finalize", Finalizer())

"#
                ),
                Some(&locals),
                None,
            )
            .unwrap();
            release_component_subscriptions(&"MESSAGE-FINALIZER".into()).unwrap();
            assert_eq!(
                locals
                    .get_item("finalized")
                    .unwrap()
                    .unwrap()
                    .extract::<Vec<String>>()
                    .unwrap(),
                vec!["rejected"]
            );
            assert_eq!(get_message_bus().borrow().subscriptions().len(), 0);
            locals.clear();
            release_actor("MESSAGE-FINALIZER");
        });
    }

    #[rstest]
    fn test_py_message_bus_scope_snapshot_delivery_survives_unsubscription_and_error() {
        Python::initialize();
        Python::attach(|py| {
            set_message_bus(Rc::new(RefCell::new(MessageBus::default())));
            let actor = registered_actor(py, "MESSAGE-SNAPSHOT");
            let locals = PyDict::new(py);
            locals.set_item("actor", &actor).unwrap();
            py.run(
                c_str!(
                    r#"
seen = []
def high(value):
    actor.unsubscribe_topic("app.events", low)
    raise ValueError("handler failed")
def low(value): seen.append(value)
actor.subscribe_topic("app.events", high, 2)
actor.subscribe_topic("app.events", low, 1)
actor.publish_message("app.events", 83)
actor.publish_message("app.events", 89)
assert seen == [83]
actor.dispose()
"#
                ),
                Some(&locals),
                None,
            )
            .unwrap();
            locals.clear();
            release_actor("MESSAGE-SNAPSHOT");
        });
    }

    fn release_actor(id: &str) {
        release_python_wrapper(ComponentId::from(id));
        with_actor_registry(|registry| registry.remove(&id.into()));
        with_component_registry(|registry| registry.remove(&id.into()));
    }

    fn registered_actor<'py>(py: Python<'py>, id: &str) -> Bound<'py, PyDataActor> {
        let locals = PyDict::new(py);
        locals
            .set_item("DataActor", py.get_type::<PyDataActor>())
            .unwrap();
        py.run(
            c_str!(
                r#"
class Actor(DataActor):
    def on_start(self): pass
    def on_stop(self): pass
    def on_resume(self): pass
    def on_reset(self): pass
    def on_dispose(self): pass
"#
            ),
            Some(&locals),
            None,
        )
        .unwrap();
        let actor = locals
            .get_item("Actor")
            .unwrap()
            .unwrap()
            .call0()
            .unwrap()
            .cast_into::<PyDataActor>()
            .unwrap();
        {
            let mut borrowed = actor.borrow_mut();
            borrowed.set_actor_id(ActorId::from(id));
            borrowed.set_python_instance(actor.as_any()).unwrap();
            borrowed
                .register(
                    TraderId::from("TRADER-001"),
                    Rc::new(RefCell::new(TestClock::new())),
                    Rc::new(RefCell::new(Cache::default())),
                )
                .unwrap();
            borrowed.register_in_global_registries().unwrap();
        }
        actor
    }
}
