// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_core::message::Message;
use ustr::Ustr;
use uuid::Uuid;

use crate::msgbus::{ShareableMessageHandler, handler::MessageHandler};

// Stub message handler which logs the data it receives
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

impl MessageHandler for StubMessageHandler {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, message: &dyn Any) {
        (self.callback)(message.downcast_ref::<Message>().unwrap().clone());
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[must_use]
#[allow(unused_must_use, reason = "TODO: Temporary to fix docs build")]
pub fn get_stub_shareable_handler(id: Option<Ustr>) -> ShareableMessageHandler {
    // TODO: This reduces the need to come up with ID strings in tests.
    // In Python we do something like `hash((self.topic, str(self.handler)))` for the hash
    // which includes the memory address, just went with a UUID4 here.
    let unique_id = id.unwrap_or_else(|| Ustr::from(&Uuid::new_v4().to_string()));
    ShareableMessageHandler(Rc::new(StubMessageHandler {
        id: unique_id,
        callback: Arc::new(|m: Message| {
            format!("{m:?}");
        }),
    }))
}

// Stub message handler which checks if handle was called
#[derive(Debug)]
pub struct CallCheckMessageHandler {
    id: Ustr,
    called: Arc<AtomicBool>,
}

impl CallCheckMessageHandler {
    #[must_use]
    pub fn was_called(&self) -> bool {
        self.called.load(Ordering::SeqCst)
    }
}

impl MessageHandler for CallCheckMessageHandler {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, _message: &dyn Any) {
        self.called.store(true, Ordering::SeqCst);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[must_use]
pub fn get_call_check_shareable_handler(id: Option<Ustr>) -> ShareableMessageHandler {
    // TODO: This reduces the need to come up with ID strings in tests.
    // In Python we do something like `hash((self.topic, str(self.handler)))` for the hash
    // which includes the memory address, just went with a UUID4 here.
    let unique_id = id.unwrap_or_else(|| Ustr::from(&Uuid::new_v4().to_string()));
    ShareableMessageHandler(Rc::new(CallCheckMessageHandler {
        id: unique_id,
        called: Arc::new(AtomicBool::new(false)),
    }))
}

/// Returns whether the given `CallCheckMessageHandler` has been invoked at least once.
///
/// # Panics
///
/// Panics if the provided `handler` is not a `CallCheckMessageHandler`.
#[must_use]
pub fn check_handler_was_called(call_check_handler: ShareableMessageHandler) -> bool {
    call_check_handler
        .0
        .as_ref()
        .as_any()
        .downcast_ref::<CallCheckMessageHandler>()
        .unwrap()
        .was_called()
}

// Handler which saves the messages it receives
#[derive(Debug, Clone)]
pub struct MessageSavingHandler<T> {
    id: Ustr,
    messages: Rc<RefCell<Vec<T>>>,
}

impl<T: Clone + 'static> MessageSavingHandler<T> {
    #[must_use]
    pub fn get_messages(&self) -> Vec<T> {
        self.messages.borrow().clone()
    }
}

impl<T: Clone + 'static> MessageHandler for MessageSavingHandler<T> {
    fn id(&self) -> Ustr {
        self.id
    }

    /// Handles an incoming message by saving it.
    ///
    /// # Panics
    ///
    /// Panics if the provided `message` is not of the expected type `T`.
    fn handle(&self, message: &dyn Any) {
        let mut messages = self.messages.borrow_mut();
        match message.downcast_ref::<T>() {
            Some(m) => messages.push(m.clone()),
            None => panic!("MessageSavingHandler: message type mismatch {message:?}"),
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[must_use]
pub fn get_message_saving_handler<T: Clone + 'static>(id: Option<Ustr>) -> ShareableMessageHandler {
    // TODO: This reduces the need to come up with ID strings in tests.
    // In Python we do something like `hash((self.topic, str(self.handler)))` for the hash
    // which includes the memory address, just went with a UUID4 here.
    let unique_id = id.unwrap_or_else(|| Ustr::from(&Uuid::new_v4().to_string()));
    ShareableMessageHandler(Rc::new(MessageSavingHandler::<T> {
        id: unique_id,
        messages: Rc::new(RefCell::new(Vec::new())),
    }))
}

/// Retrieves the messages saved by a [`MessageSavingHandler`].
///
/// # Panics
///
/// Panics if the provided `handler` is not a `MessageSavingHandler<T>`.
#[must_use]
pub fn get_saved_messages<T: Clone + 'static>(handler: ShareableMessageHandler) -> Vec<T> {
    handler
        .0
        .as_ref()
        .as_any()
        .downcast_ref::<MessageSavingHandler<T>>()
        .unwrap()
        .get_messages()
}
