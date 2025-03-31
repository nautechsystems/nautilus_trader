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

//! Message handler functionality for the message bus system.
//!
//! This module provides a trait and implementations for handling messages
//! in a type-safe manner, enabling both typed and untyped message processing.

use std::{any::Any, marker::PhantomData, rc::Rc};

use ustr::Ustr;
use uuid::Uuid;

pub trait MessageHandler: Any {
    /// Returns the unique identifier for this handler.
    fn id(&self) -> Ustr;
    /// Handles a message of any type.
    fn handle(&self, message: &dyn Any);
    /// Returns this handler as a trait object.
    fn as_any(&self) -> &dyn Any;
}

pub struct TypedMessageHandler<T: 'static + ?Sized, F: Fn(&T) + 'static> {
    id: Ustr,
    callback: F,
    _phantom: PhantomData<T>,
}

impl<T: 'static, F: Fn(&T) + 'static> TypedMessageHandler<T, F> {
    /// Creates a new handler with an optional custom ID.
    pub fn new<S: AsRef<str>>(id: Option<S>, callback: F) -> Self {
        let id_ustr = id
            .map(|s| Ustr::from(s.as_ref()))
            .unwrap_or_else(generate_unique_handler_id);
        Self {
            id: id_ustr,
            callback,
            _phantom: PhantomData,
        }
    }

    /// Creates a new handler with an auto-generated ID.
    pub fn from(callback: F) -> Self {
        Self::new::<Ustr>(None, callback)
    }
}

impl<T: 'static, F: Fn(&T) + 'static> MessageHandler for TypedMessageHandler<T, F> {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, message: &dyn Any) {
        if let Some(typed_msg) = message.downcast_ref::<T>() {
            (self.callback)(typed_msg);
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl<F: Fn(&dyn Any) + 'static> TypedMessageHandler<dyn Any, F> {
    /// Creates a new handler for dynamic Any messages with an optional custom ID.
    pub fn new_any<S: AsRef<str>>(id: Option<S>, callback: F) -> Self {
        let id_ustr = id
            .map(|s| Ustr::from(s.as_ref()))
            .unwrap_or_else(generate_unique_handler_id);

        Self {
            id: id_ustr,
            callback,
            _phantom: PhantomData,
        }
    }

    /// Creates a handler for Any messages with an optional ID.
    pub fn from_any<S: AsRef<str>>(id_opt: Option<S>, callback: F) -> Self {
        Self::new_any(id_opt, callback)
    }

    /// Creates a handler for Any messages with an auto-generated ID.
    pub fn with_any(callback: F) -> Self {
        Self::new_any::<&str>(None, callback)
    }
}

impl<F: Fn(&dyn Any) + 'static> MessageHandler for TypedMessageHandler<dyn Any, F> {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, message: &dyn Any) {
        (self.callback)(message);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn generate_unique_handler_id() -> Ustr {
    Ustr::from(&Uuid::new_v4().to_string())
}

#[derive(Clone)]
#[repr(transparent)]
pub struct ShareableMessageHandler(pub Rc<dyn MessageHandler>);

impl From<Rc<dyn MessageHandler>> for ShareableMessageHandler {
    fn from(value: Rc<dyn MessageHandler>) -> Self {
        Self(value)
    }
}

// SAFETY: Message handlers cannot be sent across thread boundaries
unsafe impl Send for ShareableMessageHandler {}
