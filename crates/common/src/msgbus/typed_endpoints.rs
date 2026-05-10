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

//! Type-safe endpoint mapping for point-to-point messaging.
//!
//! This module provides [`EndpointMap<T>`] for registering handlers at named
//! endpoints and sending typed messages directly to them.

use std::fmt::Debug;

use indexmap::IndexMap;

use super::{
    mstr::{Endpoint, MStr},
    typed_handler::{TypedHandler, TypedIntoHandler},
};

/// Maps endpoints to typed handlers for point-to-point messaging.
///
/// Provides O(1) lookup for registered endpoints and type-safe message dispatch.
#[derive(Debug)]
pub struct EndpointMap<T: 'static> {
    handlers: IndexMap<MStr<Endpoint>, TypedHandler<T>>,
}

impl<T: 'static> Default for EndpointMap<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: 'static> EndpointMap<T> {
    /// Creates a new empty endpoint map.
    #[must_use]
    pub fn new() -> Self {
        Self {
            handlers: IndexMap::new(),
        }
    }

    /// Returns the number of registered endpoints.
    #[must_use]
    pub fn len(&self) -> usize {
        self.handlers.len()
    }

    /// Returns whether there are any registered endpoints.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }

    /// Returns all registered endpoint addresses.
    #[must_use]
    pub fn endpoints(&self) -> Vec<&str> {
        self.handlers.keys().map(|e| e.as_str()).collect()
    }

    /// Returns whether an endpoint is registered.
    #[must_use]
    pub fn is_registered(&self, endpoint: MStr<Endpoint>) -> bool {
        self.handlers.contains_key(&endpoint)
    }

    /// Registers a handler at an endpoint.
    ///
    /// If the endpoint already has a handler, it will be replaced.
    pub fn register(&mut self, endpoint: MStr<Endpoint>, handler: TypedHandler<T>) {
        log::debug!(
            "Registering endpoint '{endpoint}' with handler ID {}",
            handler.id()
        );
        self.handlers.insert(endpoint, handler);
    }

    /// Deregisters the handler at an endpoint.
    pub fn deregister(&mut self, endpoint: MStr<Endpoint>) {
        log::debug!("Deregistering endpoint '{endpoint}'");
        self.handlers.shift_remove(&endpoint);
    }

    /// Gets the handler registered at an endpoint.
    #[must_use]
    pub fn get(&self, endpoint: MStr<Endpoint>) -> Option<&TypedHandler<T>> {
        self.handlers.get(&endpoint)
    }

    /// Sends a message to an endpoint.
    ///
    /// Logs an error if no handler is registered for the endpoint.
    pub fn send(&self, endpoint: MStr<Endpoint>, message: &T) {
        if let Some(handler) = self.handlers.get(&endpoint) {
            handler.handle(message);
        } else {
            log::error!("send: no registered endpoint '{endpoint}'");
        }
    }

    /// Sends a message to an endpoint, returning whether a handler was found.
    #[must_use]
    pub fn try_send(&self, endpoint: MStr<Endpoint>, message: &T) -> bool {
        if let Some(handler) = self.handlers.get(&endpoint) {
            handler.handle(message);
            true
        } else {
            false
        }
    }

    /// Clears all registered endpoints.
    pub fn clear(&mut self) {
        self.handlers.clear();
    }
}

/// Maps endpoints to ownership-based typed handlers for point-to-point messaging.
///
/// Unlike [`EndpointMap`] which borrows messages, this map transfers ownership
/// of messages to handlers, enabling zero-copy processing.
#[derive(Debug)]
pub struct IntoEndpointMap<T: 'static> {
    handlers: IndexMap<MStr<Endpoint>, TypedIntoHandler<T>>,
}

impl<T: 'static> Default for IntoEndpointMap<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: 'static> IntoEndpointMap<T> {
    /// Creates a new empty endpoint map.
    #[must_use]
    pub fn new() -> Self {
        Self {
            handlers: IndexMap::new(),
        }
    }

    /// Returns the number of registered endpoints.
    #[must_use]
    pub fn len(&self) -> usize {
        self.handlers.len()
    }

    /// Returns whether there are any registered endpoints.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }

    /// Returns all registered endpoint addresses.
    #[must_use]
    pub fn endpoints(&self) -> Vec<&str> {
        self.handlers.keys().map(|e| e.as_str()).collect()
    }

    /// Returns whether an endpoint is registered.
    #[must_use]
    pub fn is_registered(&self, endpoint: MStr<Endpoint>) -> bool {
        self.handlers.contains_key(&endpoint)
    }

    /// Registers a handler at an endpoint.
    ///
    /// If the endpoint already has a handler, it will be replaced.
    pub fn register(&mut self, endpoint: MStr<Endpoint>, handler: TypedIntoHandler<T>) {
        log::debug!(
            "Registering endpoint '{endpoint}' with handler ID {}",
            handler.id()
        );
        self.handlers.insert(endpoint, handler);
    }

    /// Deregisters the handler at an endpoint.
    pub fn deregister(&mut self, endpoint: MStr<Endpoint>) {
        log::debug!("Deregistering endpoint '{endpoint}'");
        self.handlers.shift_remove(&endpoint);
    }

    /// Gets the handler registered at an endpoint.
    #[must_use]
    pub fn get(&self, endpoint: MStr<Endpoint>) -> Option<&TypedIntoHandler<T>> {
        self.handlers.get(&endpoint)
    }

    /// Sends a message to an endpoint, transferring ownership.
    ///
    /// Logs an error if no handler is registered for the endpoint.
    pub fn send(&self, endpoint: MStr<Endpoint>, message: T) {
        if let Some(handler) = self.handlers.get(&endpoint) {
            handler.handle(message);
        } else {
            log::error!("send: no registered endpoint '{endpoint}'");
        }
    }

    /// Sends a message to an endpoint, returning whether a handler was found.
    ///
    /// # Errors
    ///
    /// Returns `Err(message)` if no handler is registered, allowing the caller
    /// to recover the message.
    pub fn try_send(&self, endpoint: MStr<Endpoint>, message: T) -> Result<(), T> {
        if let Some(handler) = self.handlers.get(&endpoint) {
            handler.handle(message);
            Ok(())
        } else {
            Err(message)
        }
    }

    /// Clears all registered endpoints.
    pub fn clear(&mut self) {
        self.handlers.clear();
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_endpoint_map_register_and_send() {
        let mut endpoints = EndpointMap::<String>::new();
        let received = Rc::new(RefCell::new(Vec::new()));
        let received_clone = received.clone();

        let handler = TypedHandler::from(move |msg: &String| {
            received_clone.borrow_mut().push(msg.clone());
        });

        let endpoint: MStr<Endpoint> = "DataEngine.execute".into();
        endpoints.register(endpoint, handler);

        endpoints.send(endpoint, &"command1".to_string());
        endpoints.send(endpoint, &"command2".to_string());

        assert_eq!(*received.borrow(), vec!["command1", "command2"]);
    }

    #[rstest]
    fn test_endpoint_map_deregister() {
        let mut endpoints = EndpointMap::<i32>::new();
        let received = Rc::new(RefCell::new(Vec::new()));
        let received_clone = received.clone();

        let handler = TypedHandler::from(move |msg: &i32| {
            received_clone.borrow_mut().push(*msg);
        });

        let endpoint: MStr<Endpoint> = "Test.endpoint".into();
        endpoints.register(endpoint, handler);
        assert!(endpoints.is_registered(endpoint));

        endpoints.deregister(endpoint);
        assert!(!endpoints.is_registered(endpoint));

        // Should not receive (no handler)
        endpoints.send(endpoint, &42);
        assert!(received.borrow().is_empty());
    }

    #[rstest]
    fn test_endpoint_map_try_send() {
        let mut endpoints = EndpointMap::<String>::new();
        let received = Rc::new(RefCell::new(Vec::new()));
        let received_clone = received.clone();

        let handler = TypedHandler::from(move |msg: &String| {
            received_clone.borrow_mut().push(msg.clone());
        });

        let registered: MStr<Endpoint> = "Registered.endpoint".into();
        let unregistered: MStr<Endpoint> = "Unregistered.endpoint".into();

        endpoints.register(registered, handler);

        assert!(endpoints.try_send(registered, &"test".to_string()));
        assert!(!endpoints.try_send(unregistered, &"test".to_string()));

        assert_eq!(*received.borrow(), vec!["test"]);
    }

    #[rstest]
    fn test_endpoint_map_replace_handler() {
        let mut endpoints = EndpointMap::<i32>::new();
        let first_received = Rc::new(RefCell::new(false));
        let second_received = Rc::new(RefCell::new(false));

        let first_clone = first_received.clone();
        let handler1 = TypedHandler::from(move |_: &i32| {
            *first_clone.borrow_mut() = true;
        });

        let second_clone = second_received.clone();
        let handler2 = TypedHandler::from(move |_: &i32| {
            *second_clone.borrow_mut() = true;
        });

        let endpoint: MStr<Endpoint> = "Test.endpoint".into();

        endpoints.register(endpoint, handler1);
        endpoints.register(endpoint, handler2);

        endpoints.send(endpoint, &1);

        // Only second handler should receive (replaced first)
        assert!(!*first_received.borrow());
        assert!(*second_received.borrow());
    }
}
