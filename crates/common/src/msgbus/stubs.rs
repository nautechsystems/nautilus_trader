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

use std::{
    any::Any,
    cell::RefCell,
    fmt::Debug,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use ahash::AHashMap;
use nautilus_core::{UUID4, message::Message};
use ustr::Ustr;

use crate::msgbus::{
    Handler, IntoHandler, ShareableMessageHandler, TypedHandler, TypedIntoHandler,
    typed_handler::shareable_handler,
};

/// Stub handler which logs messages it receives.
#[derive(Clone)]
pub struct StubMessageHandler {
    id: Ustr,
    callback: Arc<dyn Fn(Message) + Send>,
}

impl Debug for StubMessageHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(StubMessageHandler))
            .field("id", &self.id)
            .finish()
    }
}

impl Handler<dyn Any> for StubMessageHandler {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, message: &dyn Any) {
        if let Some(msg) = message.downcast_ref::<Message>() {
            (self.callback)(msg.clone());
        }
    }
}

#[must_use]
#[allow(unused_must_use)]
pub fn get_stub_shareable_handler(id: Option<Ustr>) -> ShareableMessageHandler {
    let unique_id = id.unwrap_or_else(|| Ustr::from(UUID4::new().as_str()));
    shareable_handler(Rc::new(StubMessageHandler {
        id: unique_id,
        callback: Arc::new(|m: Message| {
            format!("{m:?}");
        }),
    }))
}

/// Handler that tracks whether it has been called.
#[derive(Debug, Clone)]
pub struct CallCheckHandler {
    id: Ustr,
    called: Arc<AtomicBool>,
}

impl CallCheckHandler {
    #[must_use]
    pub fn new(id: Option<Ustr>) -> Self {
        let unique_id = id.unwrap_or_else(|| Ustr::from(UUID4::new().as_str()));
        Self {
            id: unique_id,
            called: Arc::new(AtomicBool::new(false)),
        }
    }

    #[must_use]
    pub fn was_called(&self) -> bool {
        self.called.load(Ordering::SeqCst)
    }

    /// Returns a `ShareableMessageHandler` for registration.
    #[must_use]
    pub fn handler(&self) -> ShareableMessageHandler {
        shareable_handler(Rc::new(self.clone()))
    }
}

impl Handler<dyn Any> for CallCheckHandler {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, _message: &dyn Any) {
        self.called.store(true, Ordering::SeqCst);
    }
}

/// Creates a call-checking handler and returns both the handler for registration
/// and a clone that can be used to check if it was called.
#[must_use]
pub fn get_call_check_handler(id: Option<Ustr>) -> (ShareableMessageHandler, CallCheckHandler) {
    let checker = CallCheckHandler::new(id);
    let handler = checker.handler();
    (handler, checker)
}

/// Handler that saves messages it receives (for Any-based routing).
#[derive(Debug, Clone)]
pub struct AnySavingHandler<T> {
    id: Ustr,
    messages: Rc<RefCell<Vec<T>>>,
}

impl<T: Clone + 'static> AnySavingHandler<T> {
    #[must_use]
    pub fn new(id: Option<Ustr>) -> Self {
        let unique_id = id.unwrap_or_else(|| Ustr::from(UUID4::new().as_str()));
        Self {
            id: unique_id,
            messages: Rc::new(RefCell::new(Vec::new())),
        }
    }

    #[must_use]
    pub fn get_messages(&self) -> Vec<T> {
        self.messages.borrow().clone()
    }

    pub fn clear(&self) {
        self.messages.borrow_mut().clear();
    }

    /// Returns a `ShareableMessageHandler` for registration.
    #[must_use]
    pub fn handler(&self) -> ShareableMessageHandler {
        shareable_handler(Rc::new(self.clone()))
    }
}

impl<T: Clone + 'static> Handler<dyn Any> for AnySavingHandler<T> {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, message: &dyn Any) {
        if let Some(m) = message.downcast_ref::<T>() {
            self.messages.borrow_mut().push(m.clone());
        } else {
            log::error!(
                "AnySavingHandler: expected {} got {:?}",
                std::any::type_name::<T>(),
                message.type_id()
            );
        }
    }
}

/// Creates an Any-based message saving handler and returns both the handler
/// for registration and a clone that can be used to retrieve messages.
#[must_use]
pub fn get_any_saving_handler<T: Clone + 'static>(
    id: Option<Ustr>,
) -> (ShareableMessageHandler, AnySavingHandler<T>) {
    let saver = AnySavingHandler::new(id);
    let handler = saver.handler();
    (handler, saver)
}

// Type alias for backward compatibility
pub type MessageSavingHandler<T> = AnySavingHandler<T>;

/// Typed handler which saves the messages it receives (no downcast needed).
#[derive(Debug, Clone)]
pub struct TypedMessageSavingHandler<T> {
    id: Ustr,
    messages: Rc<RefCell<Vec<T>>>,
}

impl<T: Clone + 'static> TypedMessageSavingHandler<T> {
    #[must_use]
    pub fn new(id: Option<Ustr>) -> Self {
        let unique_id = id.unwrap_or_else(|| Ustr::from(UUID4::new().as_str()));
        Self {
            id: unique_id,
            messages: Rc::new(RefCell::new(Vec::new())),
        }
    }

    #[must_use]
    pub fn get_messages(&self) -> Vec<T> {
        self.messages.borrow().clone()
    }

    /// Returns a `TypedHandler` that can be used for subscriptions.
    #[must_use]
    pub fn handler(&self) -> TypedHandler<T> {
        TypedHandler::new(self.clone())
    }
}

impl<T: Clone + 'static> Handler<T> for TypedMessageSavingHandler<T> {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, message: &T) {
        self.messages.borrow_mut().push(message.clone());
    }
}

/// Creates a typed message saving handler and returns both the handler for subscriptions
/// and a clone that can be used to retrieve messages.
#[must_use]
pub fn get_typed_message_saving_handler<T: Clone + 'static>(
    id: Option<Ustr>,
) -> (TypedHandler<T>, TypedMessageSavingHandler<T>) {
    let saving_handler = TypedMessageSavingHandler::new(id);
    let typed_handler = saving_handler.handler();
    (typed_handler, saving_handler)
}

/// Ownership-based typed handler which saves the messages it receives.
///
/// Unlike [`TypedMessageSavingHandler`] which borrows messages, this handler
/// takes ownership which is required for `IntoEndpointMap` endpoints.
#[derive(Debug, Clone)]
pub struct TypedIntoMessageSavingHandler<T> {
    id: Ustr,
    messages: Rc<RefCell<Vec<T>>>,
}

impl<T: 'static> TypedIntoMessageSavingHandler<T> {
    #[must_use]
    pub fn new(id: Option<Ustr>) -> Self {
        let unique_id = id.unwrap_or_else(|| Ustr::from(UUID4::new().as_str()));
        Self {
            id: unique_id,
            messages: Rc::new(RefCell::new(Vec::new())),
        }
    }

    #[must_use]
    pub fn get_messages(&self) -> Vec<T>
    where
        T: Clone,
    {
        self.messages.borrow().clone()
    }

    /// Returns a `TypedIntoHandler` that can be used for endpoint registration.
    #[must_use]
    pub fn handler(&self) -> TypedIntoHandler<T> {
        TypedIntoHandler::new(Self {
            id: self.id,
            messages: self.messages.clone(),
        })
    }

    pub fn clear(&self) {
        self.messages.borrow_mut().clear();
    }
}

impl<T: 'static> IntoHandler<T> for TypedIntoMessageSavingHandler<T> {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, message: T) {
        self.messages.borrow_mut().push(message);
    }
}

/// Creates an ownership-based typed message saving handler and returns both the handler
/// for endpoint registration and a clone that can be used to retrieve messages.
#[must_use]
pub fn get_typed_into_message_saving_handler<T: 'static>(
    id: Option<Ustr>,
) -> (TypedIntoHandler<T>, TypedIntoMessageSavingHandler<T>) {
    let saving_handler = TypedIntoMessageSavingHandler::new(id);
    let typed_handler = saving_handler.handler();
    (typed_handler, saving_handler)
}

// Legacy API for tests that use the old pattern with thread_local storage.
// These wrap AnySavingHandler in thread_local for simpler test usage.

thread_local! {
    static SAVING_HANDLERS: RefCell<AHashMap<Ustr, Box<dyn std::any::Any>>> = RefCell::new(AHashMap::new());
}

/// Creates a message saving handler and stores it for later retrieval.
#[must_use]
pub fn get_message_saving_handler<T: Clone + 'static>(id: Option<Ustr>) -> ShareableMessageHandler {
    let (handler, saver) = get_any_saving_handler::<T>(id);
    let handler_id = handler.0.id();
    SAVING_HANDLERS.with(|handlers| {
        handlers.borrow_mut().insert(handler_id, Box::new(saver));
    });
    handler
}

/// Retrieves saved messages from a handler created by `get_message_saving_handler`.
#[must_use]
pub fn get_saved_messages<T: Clone + 'static>(handler: ShareableMessageHandler) -> Vec<T> {
    let handler_id = handler.0.id();
    SAVING_HANDLERS.with(|handlers| {
        let handlers = handlers.borrow();
        if let Some(saver) = handlers.get(&handler_id)
            && let Some(saver) = saver.downcast_ref::<AnySavingHandler<T>>()
        {
            return saver.get_messages();
        }
        Vec::new()
    })
}
