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

//! Socket state publication and reconnect control for live clients.

use std::{
    cell::RefCell,
    fmt::Debug,
    sync::{
        Arc, Mutex, PoisonError, Weak,
        atomic::{AtomicU64, Ordering},
    },
};

use ahash::{AHashMap, AHashSet};
use nautilus_common::{
    live::runner::try_get_system_event_sender,
    messages::{
        SystemEvent,
        system::{SocketState as SystemSocketState, SocketStateChange},
    },
};
use nautilus_model::identifiers::{ClientId, Venue};
pub use nautilus_network::mode::ReconnectRequestOutcome as SocketReconnectRequestOutcome;
use nautilus_network::{SocketState, SocketStateSink, mode::ReconnectRequestOutcome};
use ustr::Ustr;

thread_local! {
    static SOCKET_REGISTRARS: RefCell<Vec<SocketReconnectRegistrar>> = const { RefCell::new(Vec::new()) };
}

/// Outcome from resolving a client socket through a live node registry.
#[derive(Clone, Debug)]
pub enum SocketReconnectLookup {
    /// No client with the requested ID belongs to the live node.
    ClientNotFound,
    /// The client has no controller-reconnectable sockets.
    Unsupported,
    /// The client supports socket reconnects but not for the requested endpoint.
    EndpointNotFound,
    /// More than one client surface owns the requested endpoint.
    AmbiguousEndpoint,
    /// The endpoint resolved to one reconnect handle.
    Handle(SocketReconnectHandle),
}

/// Cloneable control handle for one registered socket endpoint.
#[derive(Clone)]
pub struct SocketReconnectHandle {
    request: Arc<dyn Fn() -> ReconnectRequestOutcome + Send + Sync>,
}

impl SocketReconnectHandle {
    fn new<F>(request: F) -> Self
    where
        F: Fn() -> ReconnectRequestOutcome + Send + Sync + 'static,
    {
        Self {
            request: Arc::new(request),
        }
    }

    /// Requests reconnect of the registered endpoint.
    #[must_use]
    pub fn request_reconnect(&self) -> ReconnectRequestOutcome {
        (self.request)()
    }
}

impl Debug for SocketReconnectHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(SocketReconnectHandle))
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct SocketEndpoint {
    client_id: ClientId,
    endpoint: Ustr,
}

#[derive(Debug)]
struct RegistryEntry {
    generation: u64,
    handle: SocketReconnectHandle,
    request: Arc<Mutex<Option<SocketReconnectHandle>>>,
}

#[derive(Debug, Default)]
struct RegistryInner {
    clients: AHashSet<ClientId>,
    supported: AHashSet<ClientId>,
    entries: AHashMap<SocketEndpoint, AHashMap<u64, RegistryEntry>>,
    owners: AHashMap<ClientId, AHashSet<u64>>,
    next_generation: u64,
    next_owner: u64,
}

/// Registry of reconnectable socket endpoints owned by one live node.
#[derive(Debug, Default)]
pub struct SocketReconnectRegistry {
    inner: Arc<Mutex<RegistryInner>>,
}

impl SocketReconnectRegistry {
    /// Makes this registry available while live client factories construct their clients.
    pub fn scope<T>(&self, f: impl FnOnce() -> T) -> T {
        SOCKET_REGISTRARS.with(|registrars| {
            registrars.borrow_mut().push(self.registrar());
        });
        let _scope = SocketRegistryScope;
        f()
    }

    #[cfg(any(feature = "node", test))]
    pub(crate) fn register_client(&self, client_id: ClientId) {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clients
            .insert(client_id);
    }

    /// Resolves one logical socket endpoint.
    #[must_use]
    pub fn get(&self, client_id: ClientId, endpoint: Ustr) -> SocketReconnectLookup {
        let inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let key = SocketEndpoint {
            client_id,
            endpoint,
        };

        match inner.entries.get(&key) {
            Some(entries) if entries.len() > 1 => SocketReconnectLookup::AmbiguousEndpoint,
            Some(entries) => entries
                .values()
                .next()
                .map_or(SocketReconnectLookup::EndpointNotFound, |entry| {
                    SocketReconnectLookup::Handle(entry.handle.clone())
                }),
            None if inner.supported.contains(&client_id) => SocketReconnectLookup::EndpointNotFound,
            None if inner.clients.contains(&client_id) => SocketReconnectLookup::Unsupported,
            None => SocketReconnectLookup::ClientNotFound,
        }
    }

    /// Returns the reconnect handle when exactly one owner registered `endpoint`.
    #[must_use]
    pub fn handle(&self, client_id: ClientId, endpoint: Ustr) -> Option<SocketReconnectHandle> {
        match self.get(client_id, endpoint) {
            SocketReconnectLookup::Handle(handle) => Some(handle),
            _ => None,
        }
    }

    fn registrar(&self) -> SocketReconnectRegistrar {
        SocketReconnectRegistrar {
            registry: Arc::downgrade(&self.inner),
        }
    }
}

struct SocketRegistryScope;

impl Drop for SocketRegistryScope {
    fn drop(&mut self) {
        SOCKET_REGISTRARS.with(|registrars| {
            registrars.borrow_mut().pop();
        });
    }
}

#[derive(Clone, Debug)]
struct SocketReconnectRegistrar {
    registry: Weak<Mutex<RegistryInner>>,
}

impl SocketReconnectRegistrar {
    fn owner(&self, client_id: ClientId) -> Option<SocketReconnectOwner> {
        let registry = self.registry.upgrade()?;
        let owner_id = {
            let mut inner = registry.lock().unwrap_or_else(PoisonError::into_inner);
            inner.next_owner = inner.next_owner.wrapping_add(1).max(1);
            let owner_id = inner.next_owner;
            inner.supported.insert(client_id);
            inner.owners.entry(client_id).or_default().insert(owner_id);
            owner_id
        };

        Some(SocketReconnectOwner(Arc::new(SocketReconnectOwnerInner {
            registry: Arc::downgrade(&registry),
            client_id,
            owner_id,
        })))
    }
}

#[derive(Clone, Debug)]
struct SocketReconnectOwner(Arc<SocketReconnectOwnerInner>);

impl SocketReconnectOwner {
    fn register(
        &self,
        endpoint: Ustr,
        handle: SocketReconnectHandle,
    ) -> (Option<SocketReconnectRegistration>, Option<RegistryEntry>) {
        let Some(registry) = self.0.registry.upgrade() else {
            return (None, None);
        };
        let key = SocketEndpoint {
            client_id: self.0.client_id,
            endpoint,
        };

        let replaced = remove_entry(&registry, key, self.0.owner_id, None);

        let request = Arc::new(Mutex::new(Some(handle)));
        let guarded_request = Arc::clone(&request);
        let registry_ref = Arc::downgrade(&registry);
        let guarded_handle = SocketReconnectHandle::new(move || {
            let Some(_registry) = registry_ref.upgrade() else {
                return ReconnectRequestOutcome::Closed;
            };
            let request = guarded_request
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            request
                .as_ref()
                .map_or(ReconnectRequestOutcome::Closed, |handle| {
                    handle.request_reconnect()
                })
        });
        let generation = {
            let mut inner = registry.lock().unwrap_or_else(PoisonError::into_inner);
            inner.next_generation = inner.next_generation.wrapping_add(1).max(1);
            let generation = inner.next_generation;
            inner.entries.entry(key).or_default().insert(
                self.0.owner_id,
                RegistryEntry {
                    generation,
                    handle: guarded_handle,
                    request,
                },
            );
            generation
        };

        (
            Some(SocketReconnectRegistration {
                registry: Arc::downgrade(&registry),
                key,
                owner_id: self.0.owner_id,
                generation,
            }),
            replaced,
        )
    }

    fn remove(&self, endpoint: Ustr) -> Option<RegistryEntry> {
        let registry = self.0.registry.upgrade()?;
        let key = SocketEndpoint {
            client_id: self.0.client_id,
            endpoint,
        };
        remove_entry(&registry, key, self.0.owner_id, None)
    }
}

#[derive(Debug)]
struct SocketReconnectOwnerInner {
    registry: Weak<Mutex<RegistryInner>>,
    client_id: ClientId,
    owner_id: u64,
}

impl Drop for SocketReconnectOwnerInner {
    fn drop(&mut self) {
        let Some(registry) = self.registry.upgrade() else {
            return;
        };
        let removed = {
            let mut inner = registry.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(owners) = inner.owners.get_mut(&self.client_id) {
                owners.remove(&self.owner_id);
                if owners.is_empty() {
                    inner.owners.remove(&self.client_id);
                }
            }

            let mut removed = Vec::new();
            inner.entries.retain(|key, entries| {
                if key.client_id == self.client_id
                    && let Some(entry) = entries.remove(&self.owner_id)
                {
                    removed.push(entry);
                }
                !entries.is_empty()
            });
            removed
        };

        for entry in removed {
            deactivate(Some(entry));
        }
    }
}

#[derive(Debug)]
struct SocketReconnectRegistration {
    registry: Weak<Mutex<RegistryInner>>,
    key: SocketEndpoint,
    owner_id: u64,
    generation: u64,
}

impl Drop for SocketReconnectRegistration {
    fn drop(&mut self) {
        let Some(registry) = self.registry.upgrade() else {
            return;
        };
        let entry = remove_entry(&registry, self.key, self.owner_id, Some(self.generation));
        deactivate(entry);
    }
}

fn remove_entry(
    registry: &Arc<Mutex<RegistryInner>>,
    key: SocketEndpoint,
    owner_id: u64,
    generation: Option<u64>,
) -> Option<RegistryEntry> {
    let mut inner = registry.lock().unwrap_or_else(PoisonError::into_inner);
    let mut entry = None;
    let mut remove_endpoint = false;

    if let Some(entries) = inner.entries.get_mut(&key) {
        let matches = entries
            .get(&owner_id)
            .is_some_and(|entry| generation.is_none_or(|value| entry.generation == value));
        if matches {
            entry = entries.remove(&owner_id);
        }
        remove_endpoint = entries.is_empty();
    }

    if remove_endpoint {
        inner.entries.remove(&key);
    }
    entry
}

fn deactivate(entry: Option<RegistryEntry>) {
    if let Some(entry) = entry {
        *entry.request.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }
}

/// Creates socket controls which share client identity and registry ownership.
#[derive(Clone, Debug)]
pub struct SocketControlFactory {
    client_id: ClientId,
    venue: Option<Venue>,
    sender: Option<tokio::sync::mpsc::UnboundedSender<SystemEvent>>,
    owner: Option<SocketReconnectOwner>,
    controls: Arc<Mutex<AHashMap<Ustr, SocketControl>>>,
}

impl SocketControlFactory {
    /// Creates a new [`SocketControlFactory`] instance.
    #[must_use]
    pub fn new(client_id: ClientId, venue: Option<Venue>) -> Self {
        let registrar = SOCKET_REGISTRARS.with(|registrars| registrars.borrow().last().cloned());
        Self::from_registrar(client_id, venue, registrar)
    }

    /// Creates a factory which registers with an explicit live socket registry.
    #[must_use]
    pub fn with_registry(
        client_id: ClientId,
        venue: Option<Venue>,
        registry: &SocketReconnectRegistry,
    ) -> Self {
        Self::from_registrar(client_id, venue, Some(registry.registrar()))
    }

    /// Creates a control for one logical socket endpoint.
    #[must_use]
    pub fn control(&self, endpoint: impl AsRef<str>) -> SocketControl {
        let endpoint = Ustr::from(endpoint.as_ref());
        let mut controls = self.controls.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(control) = controls.get(&endpoint) {
            return control.clone();
        }

        let control = SocketControl {
            publisher: SocketStatePublisher {
                client_id: self.client_id,
                venue: self.venue,
                endpoint,
                sender: self.sender.clone(),
                active_generation: Arc::new(AtomicU64::new(0)),
                publish_lock: Arc::new(Mutex::new(())),
            },
            owner: self.owner.clone(),
            generation: AtomicU64::new(0),
            registration: Mutex::new(None),
        };
        controls.insert(endpoint, control.clone());
        control
    }

    fn from_registrar(
        client_id: ClientId,
        venue: Option<Venue>,
        registrar: Option<SocketReconnectRegistrar>,
    ) -> Self {
        Self {
            client_id,
            venue,
            sender: try_get_system_event_sender(),
            owner: registrar.and_then(|registrar| registrar.owner(client_id)),
            controls: Arc::new(Mutex::new(AHashMap::new())),
        }
    }
}

#[derive(Clone, Debug)]
struct SocketStatePublisher {
    client_id: ClientId,
    venue: Option<Venue>,
    endpoint: Ustr,
    sender: Option<tokio::sync::mpsc::UnboundedSender<SystemEvent>>,
    active_generation: Arc<AtomicU64>,
    publish_lock: Arc<Mutex<()>>,
}

impl SocketStatePublisher {
    fn publish(&self, state: SocketState) {
        let Some(sender) = &self.sender else {
            return;
        };
        let state = match state {
            SocketState::Connected => SystemSocketState::Connected,
            SocketState::Disconnected => SystemSocketState::Disconnected,
        };
        let change = SocketStateChange::new(self.client_id, self.venue, self.endpoint, state);
        if let Err(e) = sender.send(SystemEvent::SocketState(change)) {
            log::error!("Failed to emit socket state change: {e}");
        }
    }

    fn is_current(&self, generation: u64) -> bool {
        self.active_generation.load(Ordering::Acquire) == generation
    }

    fn publish_if_current<F>(&self, generation: u64, state: SocketState, on_state: &F)
    where
        F: Fn(SocketState),
    {
        let _guard = self
            .publish_lock
            .lock()
            .unwrap_or_else(PoisonError::into_inner);

        if self.is_current(generation) {
            on_state(state);
            self.publish(state);
        }
    }
}

/// State publisher and reconnect registration for one logical socket endpoint.
///
/// A clone starts without ownership of the source control's active transport generation.
#[derive(Debug)]
pub struct SocketControl {
    publisher: SocketStatePublisher,
    owner: Option<SocketReconnectOwner>,
    generation: AtomicU64,
    registration: Mutex<Option<SocketReconnectRegistration>>,
}

impl Clone for SocketControl {
    fn clone(&self) -> Self {
        Self {
            publisher: self.publisher.clone(),
            owner: self.owner.clone(),
            generation: AtomicU64::new(0),
            registration: Mutex::new(None),
        }
    }
}

impl SocketControl {
    /// Creates a new [`SocketControl`] instance.
    #[must_use]
    pub fn new(client_id: ClientId, venue: Option<Venue>, endpoint: impl AsRef<str>) -> Self {
        SocketControlFactory::new(client_id, venue).control(endpoint)
    }

    /// Creates a control which registers with an explicit live socket registry.
    #[must_use]
    pub fn with_registry(
        client_id: ClientId,
        venue: Option<Venue>,
        endpoint: impl AsRef<str>,
        registry: &SocketReconnectRegistry,
    ) -> Self {
        SocketControlFactory::with_registry(client_id, venue, registry).control(endpoint)
    }

    /// Returns a sink which publishes transport state changes as system events.
    #[must_use]
    pub fn sink(&self) -> SocketStateSink {
        self.sink_with(|_| {})
    }

    /// Returns a state sink which calls `on_state` before publishing each change.
    ///
    /// `on_state` must not synchronously trigger another state change for the same endpoint.
    #[must_use]
    pub fn sink_with<F>(&self, on_state: F) -> SocketStateSink
    where
        F: Fn(SocketState) + Send + Sync + 'static,
    {
        let (registration, replaced, generation) = {
            let _guard = self
                .publisher
                .publish_lock
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let replaced = self
                .owner
                .as_ref()
                .and_then(|owner| owner.remove(self.publisher.endpoint));
            let generation = advance_generation(&self.publisher.active_generation);
            self.generation.store(generation, Ordering::Release);
            let registration = self
                .registration
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take();
            (registration, replaced, generation)
        };
        deactivate(replaced);
        drop(registration);

        let publisher = self.publisher.clone();
        SocketStateSink::new(move |state| {
            publisher.publish_if_current(generation, state, &on_state);
        })
    }

    /// Registers a reconnect request function for this endpoint's active generation.
    pub fn register<F>(&self, request: F)
    where
        F: Fn() -> ReconnectRequestOutcome + Send + Sync + 'static,
    {
        let (replaced, old_registration) = {
            let _guard = self
                .publisher
                .publish_lock
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let generation = self.generation.load(Ordering::Acquire);
            if generation == 0 || !self.publisher.is_current(generation) {
                return;
            }
            let (registration, replaced) = if let Some(owner) = &self.owner {
                owner.register(self.publisher.endpoint, SocketReconnectHandle::new(request))
            } else {
                (None, None)
            };
            let mut current = self
                .registration
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let old_registration = std::mem::replace(&mut *current, registration);
            (replaced, old_registration)
        };
        deactivate(replaced);
        drop(old_registration);
    }

    /// Removes the reconnect handle owned by this control generation.
    pub fn deregister(&self) {
        let registration = {
            let _guard = self
                .publisher
                .publish_lock
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            self.generation.store(0, Ordering::Release);
            self.registration
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take()
        };
        drop(registration);
    }
}

impl Drop for SocketControl {
    fn drop(&mut self) {
        self.deregister();
    }
}

fn advance_generation(counter: &AtomicU64) -> u64 {
    let mut current = counter.load(Ordering::Relaxed);
    loop {
        let next = current.wrapping_add(1).max(1);
        match counter.compare_exchange_weak(current, next, Ordering::Release, Ordering::Relaxed) {
            Ok(_) => return next,
            Err(actual) => current = actual,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc, Barrier,
            atomic::{AtomicUsize, Ordering},
        },
        thread,
        time::{Duration, Instant},
    };

    use rstest::rstest;

    use super::*;

    const ENDPOINT: &str = "test-streams";

    fn control(registry: &SocketReconnectRegistry) -> SocketControl {
        SocketControl::with_registry(
            ClientId::from("TEST"),
            Some(Venue::from("TEST")),
            ENDPOINT,
            registry,
        )
    }

    fn handle(registry: &SocketReconnectRegistry) -> SocketReconnectHandle {
        let SocketReconnectLookup::Handle(handle) =
            registry.get(ClientId::from("TEST"), Ustr::from(ENDPOINT))
        else {
            panic!("test socket endpoint should be registered");
        };
        handle
    }

    #[rstest]
    fn publishes_endpoint_state() {
        let registry = SocketReconnectRegistry::default();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut control = control(&registry);
        control.publisher.sender = Some(sender);
        let _sink = control.sink();

        control.publisher.publish(SocketState::Connected);
        control.publisher.publish(SocketState::Disconnected);

        let SystemEvent::SocketState(connected) = receiver.try_recv().unwrap();
        let SystemEvent::SocketState(disconnected) = receiver.try_recv().unwrap();
        assert_eq!(connected.client_id, ClientId::from("TEST"));
        assert_eq!(connected.venue, Some(Venue::from("TEST")));
        assert_eq!(connected.endpoint, Ustr::from(ENDPOINT));
        assert_eq!(connected.state, SystemSocketState::Connected);
        assert_eq!(disconnected.client_id, ClientId::from("TEST"));
        assert_eq!(disconnected.venue, Some(Venue::from("TEST")));
        assert_eq!(disconnected.endpoint, Ustr::from(ENDPOINT));
        assert_eq!(disconnected.state, SystemSocketState::Disconnected);
    }

    #[rstest]
    fn replacement_sink_suppresses_stale_transport_state() {
        let registry = SocketReconnectRegistry::default();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut first = control(&registry);
        first.publisher.sender = Some(sender.clone());
        let mut replacement = first.clone();
        replacement.publisher.sender = Some(sender);
        let _stale_sink = first.sink();
        let stale_generation = first.generation.load(Ordering::Acquire);
        let _current_sink = replacement.sink();
        let current_generation = replacement.generation.load(Ordering::Acquire);

        first
            .publisher
            .publish_if_current(stale_generation, SocketState::Disconnected, &|_| {});
        replacement.publisher.publish_if_current(
            current_generation,
            SocketState::Connected,
            &|_| {},
        );

        let SystemEvent::SocketState(change) = receiver.try_recv().unwrap();
        assert_eq!(change.state, SystemSocketState::Connected);
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    #[case(ReconnectRequestOutcome::Accepted)]
    #[case(ReconnectRequestOutcome::AlreadyReconnecting)]
    #[case(ReconnectRequestOutcome::Disconnected)]
    #[case(ReconnectRequestOutcome::Closed)]
    #[case(ReconnectRequestOutcome::Unsupported)]
    fn register_preserves_transport_outcome(#[case] expected: ReconnectRequestOutcome) {
        let registry = SocketReconnectRegistry::default();
        let control = control(&registry);
        let _sink = control.sink();
        control.register(move || expected);

        assert_eq!(handle(&registry).request_reconnect(), expected);
    }

    #[rstest]
    fn replacement_invalidates_stale_handle() {
        let registry = SocketReconnectRegistry::default();
        let factory = SocketControlFactory::with_registry(
            ClientId::from("TEST"),
            Some(Venue::from("TEST")),
            &registry,
        );
        let first = factory.control(ENDPOINT);
        let _first_sink = first.sink();
        first.register(|| ReconnectRequestOutcome::Accepted);
        let stale_handle = handle(&registry);
        let replacement = factory.control(ENDPOINT);
        let _replacement_sink = replacement.sink();
        replacement.register(|| ReconnectRequestOutcome::AlreadyReconnecting);

        first.register(|| ReconnectRequestOutcome::Disconnected);
        first.deregister();

        assert_eq!(
            stale_handle.request_reconnect(),
            ReconnectRequestOutcome::Closed
        );
        assert_eq!(
            handle(&registry).request_reconnect(),
            ReconnectRequestOutcome::AlreadyReconnecting
        );
    }

    #[rstest]
    fn replacement_waits_for_an_inflight_request_that_publishes_state() {
        let registry = SocketReconnectRegistry::default();
        let factory = SocketControlFactory::with_registry(
            ClientId::from("TEST"),
            Some(Venue::from("TEST")),
            &registry,
        );
        let first = factory.control(ENDPOINT);
        let _first_sink = first.sink();
        let generation = first.generation.load(Ordering::Acquire);
        let publisher = first.publisher.clone();
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let request_entered = Arc::clone(&entered);
        let request_release = Arc::clone(&release);
        first.register(move || {
            request_entered.wait();
            request_release.wait();
            publisher.publish_if_current(generation, SocketState::Disconnected, &|_| {});
            ReconnectRequestOutcome::Accepted
        });
        let stale_handle = handle(&registry);
        let request_handle = stale_handle.clone();
        let request = thread::spawn(move || request_handle.request_reconnect());
        entered.wait();

        let replacement = factory.control(ENDPOINT);
        let (replaced_tx, replaced_rx) = std::sync::mpsc::channel();

        let replace = thread::spawn(move || {
            let _replacement_sink = replacement.sink();
            replaced_tx.send(()).unwrap();
        });
        let deadline = Instant::now() + Duration::from_secs(1);

        while !matches!(
            registry.get(ClientId::from("TEST"), Ustr::from(ENDPOINT)),
            SocketReconnectLookup::EndpointNotFound
        ) {
            assert!(
                Instant::now() < deadline,
                "replacement did not remove the stale entry"
            );
            thread::yield_now();
        }

        assert!(replaced_rx.try_recv().is_err());
        release.wait();
        assert_eq!(request.join().unwrap(), ReconnectRequestOutcome::Accepted);
        replace.join().unwrap();
        assert!(replaced_rx.try_recv().is_ok());
        assert_eq!(
            stale_handle.request_reconnect(),
            ReconnectRequestOutcome::Closed
        );
    }

    #[rstest]
    fn reregistration_waits_for_an_inflight_request_that_publishes_state() {
        let registry = SocketReconnectRegistry::default();
        let control = Arc::new(control(&registry));
        let _sink = control.sink();
        let generation = control.generation.load(Ordering::Acquire);
        let publisher = control.publisher.clone();
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let request_entered = Arc::clone(&entered);
        let request_release = Arc::clone(&release);
        control.register(move || {
            request_entered.wait();
            request_release.wait();
            publisher.publish_if_current(generation, SocketState::Disconnected, &|_| {});
            ReconnectRequestOutcome::Accepted
        });
        let stale_handle = handle(&registry);
        let key = SocketEndpoint {
            client_id: ClientId::from("TEST"),
            endpoint: Ustr::from(ENDPOINT),
        };
        let old_generation = registry
            .inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entries
            .get(&key)
            .and_then(|entries| entries.values().next())
            .map(|entry| entry.generation)
            .unwrap();
        let request_handle = stale_handle.clone();
        let request = thread::spawn(move || request_handle.request_reconnect());
        entered.wait();

        let replacement = Arc::clone(&control);
        let (registered_tx, registered_rx) = std::sync::mpsc::channel();

        let register = thread::spawn(move || {
            replacement.register(|| ReconnectRequestOutcome::AlreadyReconnecting);
            registered_tx.send(()).unwrap();
        });
        let deadline = Instant::now() + Duration::from_secs(1);

        loop {
            let old_entry_is_registered = registry
                .inner
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .entries
                .get(&key)
                .and_then(|entries| entries.values().next())
                .is_some_and(|entry| entry.generation == old_generation);
            if !old_entry_is_registered {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "replacement reconnect handle was not registered"
            );
            thread::yield_now();
        }

        assert!(registered_rx.try_recv().is_err());
        release.wait();
        assert_eq!(request.join().unwrap(), ReconnectRequestOutcome::Accepted);
        register.join().unwrap();
        assert!(registered_rx.try_recv().is_ok());
        assert_eq!(
            stale_handle.request_reconnect(),
            ReconnectRequestOutcome::Closed
        );
        assert_eq!(
            handle(&registry).request_reconnect(),
            ReconnectRequestOutcome::AlreadyReconnecting
        );
    }

    #[rstest]
    fn registry_drop_revokes_a_retained_handle() {
        let registry = SocketReconnectRegistry::default();
        let control = control(&registry);
        let _sink = control.sink();
        control.register(|| ReconnectRequestOutcome::Accepted);
        let retained = handle(&registry);

        drop(registry);

        assert_eq!(
            retained.request_reconnect(),
            ReconnectRequestOutcome::Closed
        );
    }

    #[rstest]
    fn deregister_prevents_delayed_registration() {
        let registry = SocketReconnectRegistry::default();
        let control = control(&registry);
        let _sink = control.sink();

        control.deregister();
        control.register(|| ReconnectRequestOutcome::Accepted);

        assert!(matches!(
            registry.get(ClientId::from("TEST"), Ustr::from(ENDPOINT)),
            SocketReconnectLookup::EndpointNotFound
        ));
    }

    #[rstest]
    fn ambiguous_endpoint_does_not_invoke_either_owner() {
        let registry = SocketReconnectRegistry::default();
        let first_count = Arc::new(AtomicUsize::new(0));
        let second_count = Arc::new(AtomicUsize::new(0));
        let first = control(&registry);
        let second = control(&registry);
        let _first_sink = first.sink();
        let _second_sink = second.sink();
        let first_callback = Arc::clone(&first_count);
        first.register(move || {
            first_callback.fetch_add(1, Ordering::SeqCst);
            ReconnectRequestOutcome::Accepted
        });
        let second_callback = Arc::clone(&second_count);
        second.register(move || {
            second_callback.fetch_add(1, Ordering::SeqCst);
            ReconnectRequestOutcome::Accepted
        });

        assert!(matches!(
            registry.get(ClientId::from("TEST"), Ustr::from(ENDPOINT)),
            SocketReconnectLookup::AmbiguousEndpoint
        ));
        assert_eq!(first_count.load(Ordering::SeqCst), 0);
        assert_eq!(second_count.load(Ordering::SeqCst), 0);
    }

    #[rstest]
    fn distinguishes_unknown_unsupported_and_missing_endpoint() {
        let registry = SocketReconnectRegistry::default();
        registry.register_client(ClientId::from("UNSUPPORTED"));
        let _control = control(&registry);

        assert!(matches!(
            registry.get(ClientId::from("UNKNOWN"), Ustr::from(ENDPOINT)),
            SocketReconnectLookup::ClientNotFound
        ));
        assert!(matches!(
            registry.get(ClientId::from("UNSUPPORTED"), Ustr::from(ENDPOINT)),
            SocketReconnectLookup::Unsupported
        ));
        assert!(matches!(
            registry.get(ClientId::from("TEST"), Ustr::from("unknown")),
            SocketReconnectLookup::EndpointNotFound
        ));
    }

    #[rstest]
    fn scope_restores_the_prior_registry() {
        let outer = SocketReconnectRegistry::default();
        let inner = SocketReconnectRegistry::default();
        let (outer_control, inner_control) = outer.scope(|| {
            let outer_control =
                SocketControl::new(ClientId::from("OUTER"), Some(Venue::from("TEST")), ENDPOINT);
            let inner_control = inner.scope(|| {
                SocketControl::new(ClientId::from("INNER"), Some(Venue::from("TEST")), ENDPOINT)
            });
            let _sink = inner_control.sink();
            inner_control.register(|| ReconnectRequestOutcome::Accepted);
            (outer_control, inner_control)
        });
        let _sink = outer_control.sink();
        outer_control.register(|| ReconnectRequestOutcome::Accepted);

        assert!(matches!(
            outer.get(ClientId::from("OUTER"), Ustr::from(ENDPOINT)),
            SocketReconnectLookup::Handle(_)
        ));
        assert!(matches!(
            inner.get(ClientId::from("INNER"), Ustr::from(ENDPOINT)),
            SocketReconnectLookup::Handle(_)
        ));
        drop(inner_control);
    }
}
