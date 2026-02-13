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

//! Compile-time type-safe message handler infrastructure.
//!
//! This module provides generic handler traits and types that enable type-safe
//! message dispatch without runtime downcasting for built-in message types.

use std::{any::Any, fmt::Debug, marker::PhantomData, rc::Rc};

use nautilus_core::UUID4;
use ustr::Ustr;

/// Compile-time type-safe message handler trait.
///
/// Provides zero-cost dispatch for statically typed messages. Can also be used
/// with `dyn Any` for dynamic dispatch when type flexibility is needed.
pub trait Handler<T: ?Sized>: 'static {
    /// Returns the unique identifier for this handler.
    fn id(&self) -> Ustr;

    /// Handles a message of type `T`.
    fn handle(&self, message: &T);
}

impl<T: ?Sized, H: Handler<T>> Handler<T> for Rc<H> {
    fn id(&self) -> Ustr {
        (**self).id()
    }

    fn handle(&self, message: &T) {
        (**self).handle(message);
    }
}

/// A shareable wrapper for typed handlers.
///
/// Provides reference-counted access to handlers. Supports both concrete types
/// for zero-cost dispatch and `dyn Any` for dynamic dispatch.
///
/// # Thread Safety
///
/// Uses `Rc` intentionally (not `Arc`) for single-threaded use within each
/// async runtime. The MessageBus uses thread-local storage to ensure each
/// thread gets its own handlers.
pub struct TypedHandler<T: 'static + ?Sized>(pub Rc<dyn Handler<T>>);

impl<T: 'static + ?Sized> Clone for TypedHandler<T> {
    fn clone(&self) -> Self {
        Self(Rc::clone(&self.0))
    }
}

impl<T: 'static + ?Sized> TypedHandler<T> {
    /// Returns the handler ID.
    pub fn id(&self) -> Ustr {
        self.0.id()
    }

    /// Handles a message by delegating to the inner handler.
    pub fn handle(&self, message: &T) {
        self.0.handle(message);
    }
}

impl<T: 'static> TypedHandler<T> {
    /// Creates a new typed handler from any type implementing `Handler<T>`.
    pub fn new<H: Handler<T>>(handler: H) -> Self {
        Self(Rc::new(handler))
    }

    /// Creates a new typed handler from a callback function.
    pub fn from<F>(callback: F) -> Self
    where
        F: Fn(&T) + 'static,
    {
        Self::new(CallbackHandler::new(None::<&str>, callback))
    }

    /// Creates a new typed handler from a callback function with a custom ID.
    pub fn from_with_id<S: AsRef<str>, F>(id: S, callback: F) -> Self
    where
        F: Fn(&T) + 'static,
    {
        Self::new(CallbackHandler::new(Some(id), callback))
    }
}

impl<T: 'static + ?Sized> Debug for TypedHandler<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(TypedHandler))
            .field("id", &self.0.id())
            .field("type", &std::any::type_name::<T>())
            .finish()
    }
}

impl<T: 'static + ?Sized> PartialEq for TypedHandler<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0.id() == other.0.id()
    }
}

impl<T: 'static + ?Sized> Eq for TypedHandler<T> {}

impl<T: 'static + ?Sized> std::hash::Hash for TypedHandler<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.id().hash(state);
    }
}

impl From<Rc<dyn Handler<dyn Any>>> for TypedHandler<dyn Any> {
    fn from(handler: Rc<dyn Handler<dyn Any>>) -> Self {
        Self(handler)
    }
}

/// Type alias for handlers that work with dynamically-typed messages.
///
/// This replaces the legacy `MessageHandler` trait with a unified handler
/// type that supports both typed and dynamic dispatch through the same trait.
pub type ShareableMessageHandler = TypedHandler<dyn Any>;

/// Creates a new `ShareableMessageHandler` from an `Rc<dyn Handler<dyn Any>>`.
#[must_use]
pub fn shareable_handler(handler: Rc<dyn Handler<dyn Any>>) -> ShareableMessageHandler {
    TypedHandler(handler)
}

impl ShareableMessageHandler {
    /// Creates a handler from a typed closure that internally downcasts.
    ///
    /// Use this when you need Any-based routing but want type-safe handling.
    /// The callback will only be invoked if the message downcasts successfully.
    pub fn from_typed<T, F>(f: F) -> Self
    where
        T: 'static,
        F: Fn(&T) + 'static,
    {
        TypedHandler(Rc::new(DowncastingHandler::new(None::<&str>, f)))
    }

    /// Creates a handler from an Any-typed closure.
    pub fn from_any<F>(f: F) -> Self
    where
        F: Fn(&dyn Any) + 'static,
    {
        TypedHandler(Rc::new(AnyCallbackHandler::new(None::<&str>, f)))
    }
}

/// Handler that downcasts `&dyn Any` to a concrete type before calling the callback.
struct DowncastingHandler<T, F: Fn(&T)> {
    id: Ustr,
    callback: F,
    _marker: PhantomData<T>,
}

impl<T: 'static, F: Fn(&T) + 'static> DowncastingHandler<T, F> {
    fn new<S: AsRef<str>>(id: Option<S>, callback: F) -> Self {
        let id_ustr = id.map_or_else(
            || generate_handler_id::<T, F>(&callback),
            |s| Ustr::from(s.as_ref()),
        );
        Self {
            id: id_ustr,
            callback,
            _marker: PhantomData,
        }
    }
}

impl<T: 'static, F: Fn(&T) + 'static> Handler<dyn Any> for DowncastingHandler<T, F> {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, message: &dyn Any) {
        if let Some(typed_msg) = message.downcast_ref::<T>() {
            (self.callback)(typed_msg);
        } else {
            log::error!(
                "DowncastingHandler downcast failed: expected {} got {:?}",
                std::any::type_name::<T>(),
                message.type_id()
            );
        }
    }
}

/// Handler that directly receives `&dyn Any` without downcasting.
struct AnyCallbackHandler<F: Fn(&dyn Any)> {
    id: Ustr,
    callback: F,
}

impl<F: Fn(&dyn Any) + 'static> AnyCallbackHandler<F> {
    fn new<S: AsRef<str>>(id: Option<S>, callback: F) -> Self {
        let id_ustr = id.map_or_else(
            || {
                let callback_ptr = std::ptr::from_ref(&callback);
                let uuid = UUID4::new();
                Ustr::from(&format!("<{callback_ptr:?}>-{uuid}"))
            },
            |s| Ustr::from(s.as_ref()),
        );
        Self {
            id: id_ustr,
            callback,
        }
    }
}

impl<F: Fn(&dyn Any) + 'static> Handler<dyn Any> for AnyCallbackHandler<F> {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, message: &dyn Any) {
        (self.callback)(message);
    }
}

/// A callback-based handler implementation.
///
/// This is the typed equivalent of `TypedMessageHandler`,
/// but without runtime downcasting overhead.
pub struct CallbackHandler<T, F: Fn(&T)> {
    id: Ustr,
    callback: F,
    _marker: PhantomData<T>,
}

impl<T: 'static, F: Fn(&T) + 'static> CallbackHandler<T, F> {
    /// Creates a new callback handler with an optional custom ID.
    pub fn new<S: AsRef<str>>(id: Option<S>, callback: F) -> Self {
        let id_ustr = id.map_or_else(
            || generate_handler_id(&callback),
            |s| Ustr::from(s.as_ref()),
        );

        Self {
            id: id_ustr,
            callback,
            _marker: PhantomData,
        }
    }
}

impl<T: 'static, F: Fn(&T) + 'static> Handler<T> for CallbackHandler<T, F> {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, message: &T) {
        (self.callback)(message);
    }
}

impl<T, F: Fn(&T)> Debug for CallbackHandler<T, F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(CallbackHandler))
            .field("id", &self.id)
            .field("type", &std::any::type_name::<T>())
            .finish()
    }
}

fn generate_handler_id<T: 'static + ?Sized, F: 'static + Fn(&T)>(callback: &F) -> Ustr {
    let callback_ptr = std::ptr::from_ref(callback);
    let uuid = UUID4::new();
    Ustr::from(&format!("<{callback_ptr:?}>-{uuid}"))
}

fn generate_into_handler_id<T: 'static, F: 'static + Fn(T)>(callback: &F) -> Ustr {
    let callback_ptr = std::ptr::from_ref(callback);
    let uuid = UUID4::new();
    Ustr::from(&format!("<{callback_ptr:?}>-{uuid}"))
}

/// Compile-time type-safe message handler trait that takes ownership.
///
/// Unlike [`Handler<T>`] which borrows messages, this trait takes ownership
/// of messages, enabling zero-copy processing when the handler needs to store
/// or forward the message.
pub trait IntoHandler<T>: 'static {
    /// Returns the unique identifier for this handler.
    fn id(&self) -> Ustr;

    /// Handles a message of type `T`, taking ownership.
    fn handle(&self, message: T);
}

impl<T, H: IntoHandler<T>> IntoHandler<T> for Rc<H> {
    fn id(&self) -> Ustr {
        (**self).id()
    }

    fn handle(&self, message: T) {
        (**self).handle(message);
    }
}

/// A shareable wrapper for ownership-based typed handlers.
///
/// This is the ownership-based equivalent of [`TypedHandler`], used for
/// point-to-point messaging where the sender transfers ownership of the message.
///
/// # Thread Safety
///
/// Uses `Rc` intentionally (not `Arc`) for single-threaded use within each
/// async runtime. The MessageBus uses thread-local storage to ensure each
/// thread gets its own handlers.
pub struct TypedIntoHandler<T: 'static>(pub Rc<dyn IntoHandler<T>>);

impl<T: 'static> Clone for TypedIntoHandler<T> {
    fn clone(&self) -> Self {
        Self(Rc::clone(&self.0))
    }
}

impl<T: 'static> TypedIntoHandler<T> {
    /// Creates a new typed into handler from any type implementing `IntoHandler<T>`.
    pub fn new<H: IntoHandler<T>>(handler: H) -> Self {
        Self(Rc::new(handler))
    }

    /// Creates a new typed into handler from a callback function.
    pub fn from<F>(callback: F) -> Self
    where
        F: Fn(T) + 'static,
    {
        Self::new(IntoCallbackHandler::new(None::<&str>, callback))
    }

    /// Creates a new typed into handler from a callback function with a custom ID.
    pub fn from_with_id<S: AsRef<str>, F>(id: S, callback: F) -> Self
    where
        F: Fn(T) + 'static,
    {
        Self::new(IntoCallbackHandler::new(Some(id), callback))
    }

    /// Returns the handler ID.
    pub fn id(&self) -> Ustr {
        self.0.id()
    }

    /// Handles a message by delegating to the inner handler, taking ownership.
    pub fn handle(&self, message: T) {
        self.0.handle(message);
    }
}

impl<T: 'static> Debug for TypedIntoHandler<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(TypedIntoHandler))
            .field("id", &self.0.id())
            .field("type", &std::any::type_name::<T>())
            .finish()
    }
}

impl<T: 'static> PartialEq for TypedIntoHandler<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0.id() == other.0.id()
    }
}

impl<T: 'static> Eq for TypedIntoHandler<T> {}

impl<T: 'static> std::hash::Hash for TypedIntoHandler<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.id().hash(state);
    }
}

/// A callback-based handler implementation that takes ownership.
///
/// This is the ownership-based equivalent of `CallbackHandler`.
pub struct IntoCallbackHandler<T, F: Fn(T)> {
    id: Ustr,
    callback: F,
    _marker: PhantomData<T>,
}

impl<T: 'static, F: Fn(T) + 'static> IntoCallbackHandler<T, F> {
    /// Creates a new into callback handler with an optional custom ID.
    pub fn new<S: AsRef<str>>(id: Option<S>, callback: F) -> Self {
        let id_ustr = id.map_or_else(
            || generate_into_handler_id(&callback),
            |s| Ustr::from(s.as_ref()),
        );

        Self {
            id: id_ustr,
            callback,
            _marker: PhantomData,
        }
    }
}

impl<T: 'static, F: Fn(T) + 'static> IntoHandler<T> for IntoCallbackHandler<T, F> {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, message: T) {
        (self.callback)(message);
    }
}

impl<T, F: Fn(T)> Debug for IntoCallbackHandler<T, F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(IntoCallbackHandler))
            .field("id", &self.id)
            .field("type", &std::any::type_name::<T>())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_typed_handler_from_fn() {
        let received = Rc::new(RefCell::new(Vec::new()));
        let received_clone = received.clone();

        let handler = TypedHandler::from(move |msg: &String| {
            received_clone.borrow_mut().push(msg.clone());
        });

        handler.handle(&"test1".to_string());
        handler.handle(&"test2".to_string());

        assert_eq!(*received.borrow(), vec!["test1", "test2"]);
    }

    #[rstest]
    fn test_typed_handler_with_custom_id() {
        let handler = TypedHandler::from_with_id("custom-id", |_msg: &i32| {});

        assert_eq!(handler.id().as_str(), "custom-id");
    }

    #[rstest]
    fn test_typed_handler_equality() {
        let handler1 = TypedHandler::from_with_id("same-id", |_: &u32| {});
        let handler2 = TypedHandler::from_with_id("same-id", |_: &u32| {});
        let handler3 = TypedHandler::from_with_id("different-id", |_: &u32| {});

        assert_eq!(handler1, handler2);
        assert_ne!(handler1, handler3);
    }

    #[rstest]
    fn test_typed_handler_hash() {
        use std::collections::HashSet;

        let handler1 = TypedHandler::from_with_id("id-a", |_: &u32| {});
        let handler2 = TypedHandler::from_with_id("id-a", |_: &u32| {});
        let handler3 = TypedHandler::from_with_id("id-b", |_: &u32| {});

        let mut set = HashSet::new();
        set.insert(handler1);

        // Same ID should be considered duplicate
        assert!(!set.insert(handler2));
        // Different ID should be added
        assert!(set.insert(handler3));
        assert_eq!(set.len(), 2);
    }

    #[rstest]
    fn test_typed_handler_debug() {
        let handler = TypedHandler::from_with_id("debug-test", |_: &String| {});
        let debug_str = format!("{handler:?}");

        assert!(debug_str.contains("TypedHandler"));
        assert!(debug_str.contains("debug-test"));
    }

    // Tests for Handler<T> impl for Rc<H>
    struct TestHandler {
        id: Ustr,
        call_count: RefCell<usize>,
    }

    impl TestHandler {
        fn new(id: &str) -> Self {
            Self {
                id: Ustr::from(id),
                call_count: RefCell::new(0),
            }
        }
    }

    impl Handler<i32> for TestHandler {
        fn id(&self) -> Ustr {
            self.id
        }

        fn handle(&self, _message: &i32) {
            *self.call_count.borrow_mut() += 1;
        }
    }

    #[rstest]
    fn test_rc_handler_delegates_id() {
        let handler = TestHandler::new("rc-test-id");
        let rc_handler: Rc<TestHandler> = Rc::new(handler);

        assert_eq!(rc_handler.id(), Ustr::from("rc-test-id"));
    }

    #[rstest]
    fn test_rc_handler_delegates_handle() {
        let handler = TestHandler::new("rc-handle-test");
        let rc_handler: Rc<TestHandler> = Rc::new(handler);

        rc_handler.handle(&42);
        rc_handler.handle(&100);

        assert_eq!(*rc_handler.call_count.borrow(), 2);
    }

    #[rstest]
    fn test_rc_handler_shared_state() {
        let handler = TestHandler::new("shared-state");
        let rc1: Rc<TestHandler> = Rc::new(handler);
        let rc2 = rc1.clone();

        // Both Rc's point to same handler
        rc1.handle(&1);
        rc2.handle(&2);
        rc1.handle(&3);

        // All calls should be counted on the same handler
        assert_eq!(*rc1.call_count.borrow(), 3);
        assert_eq!(*rc2.call_count.borrow(), 3);
    }

    #[rstest]
    fn test_typed_handler_from_rc() {
        let handler = Rc::new(TestHandler::new("from-rc-test"));
        let typed: TypedHandler<i32> = TypedHandler::new(handler.clone());

        typed.handle(&42);

        assert_eq!(*handler.call_count.borrow(), 1);
        assert_eq!(typed.id(), Ustr::from("from-rc-test"));
    }
}
