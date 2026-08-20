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

//! Endpoint-level socket reconnect registration.

use std::{
    fmt::Debug,
    sync::{Arc, Mutex, PoisonError, Weak},
};

use ahash::AHashMap;
use ustr::Ustr;

/// Outcome returned by an endpoint reconnect handle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SocketReconnectRequestOutcome {
    /// The active transport entered reconnect mode.
    Accepted,
    /// The transport is already reconnecting.
    AlreadyReconnecting,
    /// The transport is disconnecting.
    Disconnected,
    /// The transport is permanently closed.
    Closed,
    /// The transport does not support controller-owned reconnects.
    Unsupported,
}

/// Cloneable control handle for one registered socket endpoint.
#[derive(Clone)]
pub struct SocketReconnectHandle {
    request: Arc<dyn Fn() -> SocketReconnectRequestOutcome + Send + Sync>,
}

impl SocketReconnectHandle {
    /// Creates a handle from a synchronous reconnect request function.
    #[must_use]
    pub fn new<F>(request: F) -> Self
    where
        F: Fn() -> SocketReconnectRequestOutcome + Send + Sync + 'static,
    {
        Self {
            request: Arc::new(request),
        }
    }

    /// Requests reconnect of the registered endpoint.
    #[must_use]
    pub fn request_reconnect(&self) -> SocketReconnectRequestOutcome {
        (self.request)()
    }
}

impl Debug for SocketReconnectHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(SocketReconnectHandle))
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
struct RegistryEntry {
    generation: u64,
    handle: SocketReconnectHandle,
}

#[derive(Debug, Default)]
struct RegistryInner {
    entries: AHashMap<Ustr, RegistryEntry>,
    next_generation: u64,
}

/// Registry of independently reconnectable socket endpoints for one client.
#[derive(Clone, Debug, Default)]
pub struct SocketReconnectRegistry {
    inner: Arc<Mutex<RegistryInner>>,
}

impl SocketReconnectRegistry {
    /// Registers or replaces an endpoint handle.
    ///
    /// Dropping the returned registration removes the endpoint only if it still owns the current
    /// generation. This prevents an old transport from removing a newer replacement.
    #[must_use]
    pub fn register(
        &self,
        endpoint: Ustr,
        handle: SocketReconnectHandle,
    ) -> SocketReconnectRegistration {
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        inner.next_generation = inner.next_generation.wrapping_add(1).max(1);
        let generation = inner.next_generation;
        inner
            .entries
            .insert(endpoint, RegistryEntry { generation, handle });

        SocketReconnectRegistration {
            registry: Arc::downgrade(&self.inner),
            endpoint,
            generation,
        }
    }

    /// Returns the current handle for `endpoint`.
    #[must_use]
    pub fn get(&self, endpoint: Ustr) -> Option<SocketReconnectHandle> {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entries
            .get(&endpoint)
            .map(|entry| entry.handle.clone())
    }
}

/// Generation-bound ownership of one registry entry.
pub struct SocketReconnectRegistration {
    registry: Weak<Mutex<RegistryInner>>,
    endpoint: Ustr,
    generation: u64,
}

impl Debug for SocketReconnectRegistration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(SocketReconnectRegistration))
            .field("endpoint", &self.endpoint)
            .field("generation", &self.generation)
            .finish()
    }
}

impl Drop for SocketReconnectRegistration {
    fn drop(&mut self) {
        let Some(registry) = self.registry.upgrade() else {
            return;
        };
        let mut inner = registry.lock().unwrap_or_else(PoisonError::into_inner);

        if inner
            .entries
            .get(&self.endpoint)
            .is_some_and(|entry| entry.generation == self.generation)
        {
            inner.entries.remove(&self.endpoint);
        }
    }
}

/// Result of resolving a socket endpoint through one engine.
#[derive(Clone, Debug)]
pub enum SocketReconnectLookup {
    /// The engine does not own the requested client.
    ClientNotFound,
    /// The client does not expose endpoint reconnect controls.
    Unsupported,
    /// The client does not own the requested endpoint.
    EndpointNotFound,
    /// The engine resolved the requested endpoint.
    Handle(SocketReconnectHandle),
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use rstest::rstest;

    use super::*;

    fn counting_handle(count: Arc<AtomicUsize>) -> SocketReconnectHandle {
        SocketReconnectHandle::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
            SocketReconnectRequestOutcome::Accepted
        })
    }

    #[rstest]
    fn requests_are_isolated_by_endpoint() {
        let registry = SocketReconnectRegistry::default();
        let first = Arc::new(AtomicUsize::new(0));
        let second = Arc::new(AtomicUsize::new(0));
        let _first_registration =
            registry.register(Ustr::from("first"), counting_handle(Arc::clone(&first)));
        let _second_registration =
            registry.register(Ustr::from("second"), counting_handle(Arc::clone(&second)));

        let outcome = registry
            .get(Ustr::from("first"))
            .expect("first endpoint should be registered")
            .request_reconnect();

        assert_eq!(outcome, SocketReconnectRequestOutcome::Accepted);
        assert_eq!(first.load(Ordering::SeqCst), 1);
        assert_eq!(second.load(Ordering::SeqCst), 0);
        assert!(registry.get(Ustr::from("unknown")).is_none());
    }

    #[rstest]
    fn old_registration_cannot_remove_replacement() {
        let registry = SocketReconnectRegistry::default();
        let old = Arc::new(AtomicUsize::new(0));
        let current = Arc::new(AtomicUsize::new(0));
        let old_registration =
            registry.register(Ustr::from("market"), counting_handle(Arc::clone(&old)));
        let _current_registration =
            registry.register(Ustr::from("market"), counting_handle(Arc::clone(&current)));

        drop(old_registration);
        let outcome = registry
            .get(Ustr::from("market"))
            .expect("replacement should remain registered")
            .request_reconnect();

        assert_eq!(outcome, SocketReconnectRequestOutcome::Accepted);
        assert_eq!(old.load(Ordering::SeqCst), 0);
        assert_eq!(current.load(Ordering::SeqCst), 1);
    }
}
