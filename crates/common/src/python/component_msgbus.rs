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

//! Component-owned subscriptions to the runtime Python message bus.

use std::{
    any::Any,
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

use nautilus_core::{
    UUID4,
    python::{to_pyruntime_err, to_pytype_err, to_pyvalue_err},
};
use pyo3::prelude::*;

use super::msgbus::{PyCallableHandler, PyMessage};
use crate::msgbus::{
    self, MStr, MessageBus, Pattern, ShareableMessageHandler, Topic, TypedHandler,
    try_get_message_bus, typed_handler::Handler,
};

/// Owns a component's Python subscriptions without owning the runtime bus.
#[derive(Debug, Default)]
pub struct ComponentMessageBus {
    bus: RefCell<Weak<RefCell<MessageBus>>>,
    subscriptions: RefCell<Vec<Subscription>>,
    clearing: Cell<bool>,
}

impl ComponentMessageBus {
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
        msgbus::publish_any(topic, &PyMessage(message));
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
        msgbus::subscribe_any(pattern, handler.clone(), Some(priority));
        self.subscriptions.borrow_mut().push(Subscription {
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
struct Subscription {
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
    use nautilus_model::{
        data::QuoteTick,
        identifiers::{ActorId, ComponentId, TraderId},
        types::{Price, Quantity},
    };
    use pyo3::{ffi::c_str, types::PyDict};
    use rstest::rstest;

    use super::*;
    use crate::{
        actor::registry::with_actor_registry,
        cache::Cache,
        clock::TestClock,
        component::{release_component_subscriptions, with_component_registry},
        msgbus::{get_message_bus, set_message_bus},
        python::{actor::PyDataActor, wrappers::release_python_wrapper},
    };

    #[rstest]
    fn test_component_message_bus_object_identity_nested_delivery_and_subscription_ownership() {
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
    fn test_component_message_bus_callable_identity_and_reference_release() {
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
    fn test_component_message_bus_lifecycle_and_foreign_thread_errors() {
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
    fn test_component_message_bus_replacement_rejected_and_original_cleaned() {
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
    fn test_component_message_bus_missing_runtime_does_not_create_one() {
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
    fn test_component_message_bus_input_errors_and_existing_routes() {
        Python::initialize();
        Python::attach(|py| {
            let bus = Rc::new(RefCell::new(MessageBus::default()));
            set_message_bus(Rc::clone(&bus));
            let endpoint_received = Rc::new(RefCell::new(Vec::<i32>::new()));
            let received = Rc::clone(&endpoint_received);
            let endpoint_handler = ShareableMessageHandler::from_typed(move |value: &i32| {
                received.borrow_mut().push(*value);
            });
            msgbus::register_any("app.endpoint".into(), endpoint_handler);
            let quotes_received = Rc::new(RefCell::new(Vec::new()));
            let received = Rc::clone(&quotes_received);
            let quote_handler =
                TypedHandler::from(move |quote: &QuoteTick| received.borrow_mut().push(*quote));
            msgbus::subscribe_quotes("app.quotes".into(), quote_handler.clone(), None);
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
            msgbus::send_any("app.endpoint".into(), &73_i32);
            let quote = QuoteTick::new(
                "BTCUSDT.BINANCE".into(),
                Price::from("101.25"),
                Price::from("102.50"),
                Quantity::from("3.75"),
                Quantity::from("4.50"),
                123.into(),
                456.into(),
            );
            msgbus::publish_quote("app.quotes".into(), &quote);
            assert_eq!(*endpoint_received.borrow(), vec![73]);
            assert_eq!(*quotes_received.borrow(), vec![quote]);
            assert!(Rc::ptr_eq(&bus, &get_message_bus()));
            msgbus::unsubscribe_quotes("app.quotes".into(), &quote_handler);
            msgbus::deregister_any("app.endpoint".into());
            locals.clear();
            release_actor("MESSAGE-ERRORS");
        });
    }

    #[rstest]
    fn test_component_message_bus_failed_disposal_and_finalizer_reentry() {
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
    fn test_component_message_bus_snapshot_delivery_survives_unsubscription_and_error() {
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
