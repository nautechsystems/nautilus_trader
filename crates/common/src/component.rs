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

//! Component system for managing stateful system entities.
//!
//! This module provides the component framework for managing the lifecycle and state
//! of system entities. Components have defined states (pre-initialized, ready, running,
//! stopped, etc.) and provide a consistent interface for state management and transitions.

#![allow(unsafe_code)]

use std::{
    cell::{RefCell, UnsafeCell},
    fmt::Debug,
    rc::Rc,
};

use ahash::{AHashMap, AHashSet};
use nautilus_model::identifiers::{ComponentId, TraderId};
use thiserror::Error;
use ustr::Ustr;

use crate::{
    actor::{Actor, registry::with_actor_registry},
    cache::Cache,
    clock::Clock,
    enums::{ComponentState, ComponentTrigger},
};

/// Failure to acquire access to component state.
///
/// A conflict identifies the requested access, not the holder or its call stack.
/// Callback reentry can cause a conflict, but is not the only possible cause.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ComponentAccessError {
    /// Registration has not supplied the requested resource.
    #[error("Cannot access {resource} during {operation}: the actor is not registered")]
    NotRegistered {
        /// The resource being accessed.
        resource: &'static str,
        /// The attempted operation.
        operation: &'static str,
    },
    /// Shared access conflicts with an existing exclusive borrow.
    #[error(
        "Cannot read {resource} during {operation}: it is already mutably borrowed. Release the existing borrow before accessing it again; callback reentry can cause this conflict"
    )]
    ReadConflict {
        /// The resource being accessed.
        resource: &'static str,
        /// The attempted operation.
        operation: &'static str,
    },
    /// Exclusive access conflicts with an existing shared or exclusive borrow.
    #[error(
        "Cannot modify {resource} during {operation}: it is already borrowed. Release existing borrows before accessing it mutably; callback reentry can cause this conflict"
    )]
    WriteConflict {
        /// The resource being accessed.
        resource: &'static str,
        /// The attempted operation.
        operation: &'static str,
    },
}

/// Components have state and lifecycle management capabilities.
pub trait Component {
    /// Returns the unique identifier for this component.
    fn component_id(&self) -> ComponentId;

    /// Returns the current state of the component.
    fn state(&self) -> ComponentState;

    /// Transition the component with the state trigger.
    ///
    /// # Errors
    ///
    /// Returns an error if the `trigger` is an invalid transition from the current state.
    fn transition_state(&mut self, trigger: ComponentTrigger) -> anyhow::Result<()>;

    /// Returns whether the component is ready.
    fn is_ready(&self) -> bool {
        self.state() == ComponentState::Ready
    }

    /// Returns whether the component is *not* running.
    fn not_running(&self) -> bool {
        !self.is_running()
    }

    /// Returns whether the component is running.
    fn is_running(&self) -> bool {
        self.state() == ComponentState::Running
    }

    /// Returns whether the component is stopped.
    fn is_stopped(&self) -> bool {
        self.state() == ComponentState::Stopped
    }

    /// Returns whether the component has been degraded.
    fn is_degraded(&self) -> bool {
        self.state() == ComponentState::Degraded
    }

    /// Returns whether the component has been faulted.
    fn is_faulted(&self) -> bool {
        self.state() == ComponentState::Faulted
    }

    /// Returns whether the component has been disposed.
    fn is_disposed(&self) -> bool {
        self.state() == ComponentState::Disposed
    }

    /// Registers the component with a system.
    ///
    /// # Errors
    ///
    /// Returns an error if the component fails to register.
    fn register(
        &mut self,
        trader_id: TraderId,
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
    ) -> anyhow::Result<()>;

    /// Initializes the component.
    ///
    /// # Errors
    ///
    /// Returns an error if the initialization state transition fails.
    fn initialize(&mut self) -> anyhow::Result<()> {
        self.transition_state(ComponentTrigger::Initialize)
    }

    /// Starts the component.
    ///
    /// # Errors
    ///
    /// Returns an error if the component fails to start.
    fn start(&mut self) -> anyhow::Result<()> {
        self.transition_state(ComponentTrigger::Start)?; // -> Starting

        if let Err(e) = self.on_start() {
            log_error(self.component_id(), &e);
            return Err(e); // Halt state transition
        }

        self.transition_state(ComponentTrigger::StartCompleted)?;

        Ok(())
    }

    /// Stops the component.
    ///
    /// # Errors
    ///
    /// Returns an error if the component fails to stop.
    fn stop(&mut self) -> anyhow::Result<()> {
        self.transition_state(ComponentTrigger::Stop)?; // -> Stopping

        if let Err(e) = self.on_stop() {
            log_error(self.component_id(), &e);
            return Err(e); // Halt state transition
        }

        self.transition_state(ComponentTrigger::StopCompleted)?;

        Ok(())
    }

    /// Resumes the component.
    ///
    /// # Errors
    ///
    /// Returns an error if the component fails to resume.
    fn resume(&mut self) -> anyhow::Result<()> {
        self.transition_state(ComponentTrigger::Resume)?; // -> Resuming

        if let Err(e) = self.on_resume() {
            log_error(self.component_id(), &e);
            return Err(e); // Halt state transition
        }

        self.transition_state(ComponentTrigger::ResumeCompleted)?;

        Ok(())
    }

    /// Degrades the component.
    ///
    /// # Errors
    ///
    /// Returns an error if the component fails to degrade.
    fn degrade(&mut self) -> anyhow::Result<()> {
        self.transition_state(ComponentTrigger::Degrade)?; // -> Degrading

        if let Err(e) = self.on_degrade() {
            log_error(self.component_id(), &e);
            return Err(e); // Halt state transition
        }

        self.transition_state(ComponentTrigger::DegradeCompleted)?;

        Ok(())
    }

    /// Faults the component.
    ///
    /// # Errors
    ///
    /// Returns an error if the component fails to fault.
    ///
    /// # Notes
    ///
    /// Subscriptions are released whether or not `on_fault` succeeds. This applies to faults
    /// initiated through this method; a failed `on_dispose` reaches `Faulted` without invoking
    /// `on_fault` and retains subscriptions until a later retirement.
    fn fault(&mut self) -> anyhow::Result<()> {
        self.transition_state(ComponentTrigger::Fault)?; // -> Faulting

        let result = self.on_fault();
        self.release_subscriptions();

        if let Err(e) = result {
            log_error(self.component_id(), &e);
            return Err(e); // Halt state transition
        }

        self.transition_state(ComponentTrigger::FaultCompleted)?;

        Ok(())
    }

    /// Resets the component to its initial state.
    ///
    /// # Errors
    ///
    /// Returns an error if the component fails to reset.
    ///
    /// # Notes
    ///
    /// A successful reset releases retained subscriptions so the component can acquire fresh
    /// subscriptions when it next starts. A failing `on_reset` retains subscriptions and leaves
    /// the component in `Resetting`.
    fn reset(&mut self) -> anyhow::Result<()> {
        self.transition_state(ComponentTrigger::Reset)?; // -> Resetting

        if let Err(e) = self.on_reset() {
            log_error(self.component_id(), &e);
            return Err(e); // Halt state transition
        }

        self.release_subscriptions();
        self.transition_state(ComponentTrigger::ResetCompleted)?;

        Ok(())
    }

    /// Disposes of the component, releasing any resources.
    ///
    /// # Errors
    ///
    /// Returns an error if the component fails to dispose.
    ///
    /// # Notes
    ///
    /// A failing `on_dispose` moves the component to `Faulted` and returns the error without
    /// releasing subscriptions. The trader keeps its registry entries, clock, bookkeeping, and
    /// retained Python wrapper, if any, so the failed retirement leaves the component reachable
    /// and can be retried without a partially dismantled registration.
    ///
    /// `on_fault` does not run, since invoking a second user hook immediately after `on_dispose`
    /// failed can fail again.
    fn dispose(&mut self) -> anyhow::Result<()> {
        self.transition_state(ComponentTrigger::Dispose)?; // -> Disposing

        if let Err(e) = self.on_dispose() {
            log_error(self.component_id(), &e);

            self.transition_state(ComponentTrigger::Fault)?; // -> Faulting
            self.transition_state(ComponentTrigger::FaultCompleted)?; // -> Faulted

            return Err(e);
        }

        self.release_subscriptions();
        self.transition_state(ComponentTrigger::DisposeCompleted)?;

        Ok(())
    }

    /// Releases the message bus registrations this component installed.
    ///
    /// Runs after successful `on_reset` and `on_dispose` hooks, after `on_fault` returns, and during
    /// explicit retirement cleanup. An override must handle every route and be idempotent so a
    /// failed disposal can release its subscriptions during a later retirement.
    fn release_subscriptions(&mut self) {}

    /// Actions to be performed on start.
    ///
    /// # Errors
    ///
    /// Returns an error if starting the actor fails.
    fn on_start(&mut self) -> anyhow::Result<()> {
        log::warn!(
            "The `on_start` handler was called when not overridden, \
            it's expected that any actions required when stopping the component \
            occur here, such as unsubscribing from data",
        );
        Ok(())
    }

    /// Actions to be performed on stop.
    ///
    /// # Errors
    ///
    /// Returns an error if stopping the actor fails.
    fn on_stop(&mut self) -> anyhow::Result<()> {
        log::warn!(
            "The `on_stop` handler was called when not overridden, \
            it's expected that any actions required when stopping the component \
            occur here, such as unsubscribing from data",
        );
        Ok(())
    }

    /// Actions to be performed on resume.
    ///
    /// # Errors
    ///
    /// Returns an error if resuming the actor fails.
    fn on_resume(&mut self) -> anyhow::Result<()> {
        log::warn!(
            "The `on_resume` handler was called when not overridden, \
            it's expected that any actions required when resuming the component \
            following a stop occur here"
        );
        Ok(())
    }

    /// Actions to be performed on reset.
    ///
    /// # Errors
    ///
    /// Returns an error if resetting the actor fails.
    fn on_reset(&mut self) -> anyhow::Result<()> {
        log::warn!(
            "The `on_reset` handler was called when not overridden, \
            it's expected that any actions required when resetting the component \
            occur here, such as resetting indicators and other state"
        );
        Ok(())
    }

    /// Actions to be performed on dispose.
    ///
    /// # Errors
    ///
    /// Returns an error if disposing the actor fails.
    fn on_dispose(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed on degrade.
    ///
    /// # Errors
    ///
    /// Returns an error if degrading the actor fails.
    fn on_degrade(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed on fault.
    ///
    /// # Errors
    ///
    /// Returns an error if faulting the actor fails.
    fn on_fault(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

fn log_error(component: ComponentId, e: &anyhow::Error) {
    log::error!(component = component.as_str(); "{e}");
}

#[rustfmt::skip]
impl ComponentState {
    /// Transition the state machine with the component `trigger`.
    ///
    /// # Errors
    ///
    /// Returns an error if `trigger` is invalid for the current state.
    pub fn transition(&mut self, trigger: &ComponentTrigger) -> anyhow::Result<Self> {
        let new_state = match (&self, trigger) {
            (Self::PreInitialized, ComponentTrigger::Initialize) => Self::Ready,
            (Self::Ready, ComponentTrigger::Reset) => Self::Resetting,
            (Self::Ready, ComponentTrigger::Start) => Self::Starting,
            (Self::Ready, ComponentTrigger::Dispose) => Self::Disposing,
            (Self::Resetting, ComponentTrigger::ResetCompleted) => Self::Ready,
            (Self::Starting, ComponentTrigger::StartCompleted) => Self::Running,
            (Self::Starting, ComponentTrigger::Stop) => Self::Stopping,
            (Self::Starting, ComponentTrigger::Fault) => Self::Faulting,
            (Self::Running, ComponentTrigger::Stop) => Self::Stopping,
            (Self::Running, ComponentTrigger::Degrade) => Self::Degrading,
            (Self::Running, ComponentTrigger::Fault) => Self::Faulting,
            (Self::Resuming, ComponentTrigger::Stop) => Self::Stopping,
            (Self::Resuming, ComponentTrigger::ResumeCompleted) => Self::Running,
            (Self::Resuming, ComponentTrigger::Fault) => Self::Faulting,
            (Self::Stopping, ComponentTrigger::StopCompleted) => Self::Stopped,
            (Self::Stopping, ComponentTrigger::Dispose) => Self::Disposing,
            (Self::Stopping, ComponentTrigger::Fault) => Self::Faulting,
            (Self::Stopped, ComponentTrigger::Reset) => Self::Resetting,
            (Self::Stopped, ComponentTrigger::Resume) => Self::Resuming,
            (Self::Stopped, ComponentTrigger::Dispose) => Self::Disposing,
            (Self::Stopped, ComponentTrigger::Fault) => Self::Faulting,
            (Self::Degrading, ComponentTrigger::DegradeCompleted) => Self::Degraded,
            (Self::Degraded, ComponentTrigger::Resume) => Self::Resuming,
            (Self::Degraded, ComponentTrigger::Stop) => Self::Stopping,
            (Self::Degraded, ComponentTrigger::Fault) => Self::Faulting,
            (Self::Disposing, ComponentTrigger::DisposeCompleted) => Self::Disposed,
            (Self::Disposing, ComponentTrigger::Fault) => Self::Faulting,
            (Self::Faulting, ComponentTrigger::Dispose) => Self::Disposing,
            (Self::Faulting, ComponentTrigger::FaultCompleted) => Self::Faulted,
            _ => anyhow::bail!("Invalid state trigger {self} -> {trigger}"),
        };
        Ok(new_state)
    }
}

thread_local! {
    static COMPONENT_REGISTRY: ComponentRegistry = ComponentRegistry::new();
}

/// Registry for storing components with runtime borrow tracking.
///
/// The registry tracks which components are currently mutably borrowed to prevent
/// multiple simultaneous mutable borrows (which would be undefined behavior).
pub struct ComponentRegistry {
    components: RefCell<AHashMap<Ustr, Rc<UnsafeCell<dyn Component>>>>,
    borrows: RefCell<AHashSet<Ustr>>,
}

impl Debug for ComponentRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let components_ref = self.components.borrow();
        let keys: Vec<&Ustr> = components_ref.keys().collect();
        f.debug_struct(stringify!(ComponentRegistry))
            .field("components", &keys)
            .field("active_borrows", &self.borrows.borrow().len())
            .finish()
    }
}

impl Default for ComponentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ComponentRegistry {
    pub fn new() -> Self {
        Self {
            components: RefCell::new(AHashMap::new()),
            borrows: RefCell::new(AHashSet::new()),
        }
    }

    pub fn insert(&self, id: Ustr, component: Rc<UnsafeCell<dyn Component>>) {
        self.components.borrow_mut().insert(id, component);
    }

    pub fn get(&self, id: &Ustr) -> Option<Rc<UnsafeCell<dyn Component>>> {
        self.components.borrow().get(id).cloned()
    }

    /// Removes the component with `id`, returning it when it was registered.
    pub fn remove(&self, id: &Ustr) -> Option<Rc<UnsafeCell<dyn Component>>> {
        self.components.borrow_mut().remove(id)
    }

    /// Checks if a component is currently borrowed.
    pub fn is_borrowed(&self, id: &Ustr) -> bool {
        self.borrows.borrow().contains(id)
    }

    /// Marks a component as borrowed. Returns false if already borrowed.
    fn try_borrow(&self, id: Ustr) -> bool {
        let mut borrows = self.borrows.borrow_mut();
        if borrows.contains(&id) {
            false
        } else {
            borrows.insert(id);
            true
        }
    }

    /// Releases a borrow on a component.
    fn release_borrow(&self, id: &Ustr) {
        self.borrows.borrow_mut().remove(id);
    }
}

/// Guard that releases a component borrow when dropped.
///
/// This ensures borrows are released even if the code panics during
/// a lifecycle method call.
struct BorrowGuard {
    id: Ustr,
}

impl BorrowGuard {
    fn new(id: Ustr) -> Self {
        Self { id }
    }
}

impl Drop for BorrowGuard {
    fn drop(&mut self) {
        with_component_registry(|registry| registry.release_borrow(&self.id));
    }
}

pub fn with_component_registry<R>(f: impl FnOnce(&ComponentRegistry) -> R) -> R {
    COMPONENT_REGISTRY.with(f)
}

/// Registers a component.
pub fn register_component<T>(component: T) -> Rc<UnsafeCell<T>>
where
    T: Component + 'static,
{
    let component_id = component.component_id().inner();
    let component_ref = Rc::new(UnsafeCell::new(component));

    // Register in component registry
    let component_trait_ref: Rc<UnsafeCell<dyn Component>> = component_ref.clone();
    with_component_registry(|registry| registry.insert(component_id, component_trait_ref));

    component_ref
}

/// Registers a component that also implements Actor.
pub fn register_component_actor<T>(component: T) -> Rc<UnsafeCell<T>>
where
    T: Component + Actor + 'static,
{
    let component_id = component.component_id().inner();
    let actor_id = component.id();
    let component_ref = Rc::new(UnsafeCell::new(component));

    // Register in component registry
    let component_trait_ref: Rc<UnsafeCell<dyn Component>> = component_ref.clone();
    with_component_registry(|registry| registry.insert(component_id, component_trait_ref));

    // Register in actor registry
    let actor_trait_ref: Rc<UnsafeCell<dyn Actor>> = component_ref.clone();
    with_actor_registry(|registry| registry.insert(actor_id, actor_trait_ref));

    component_ref
}

/// Safely calls `start()` on a component in the global registry.
///
/// # Errors
///
/// - Returns an error if the component is not found.
/// - Returns an error if the component is already borrowed.
/// - Returns an error if `start()` fails.
pub fn start_component(id: &Ustr) -> anyhow::Result<()> {
    let component_ref = with_component_registry(|registry| {
        let component_ref = registry
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("Component '{id}' not found in global registry"))?;

        if !registry.try_borrow(*id) {
            anyhow::bail!(
                "Component '{id}' is already mutably borrowed. \
                 This would create aliasing mutable references (undefined behavior)."
            );
        }

        Ok::<_, anyhow::Error>(component_ref)
    })?;

    let _guard = BorrowGuard::new(*id);

    // SAFETY: Borrow tracking ensures exclusive access
    unsafe {
        let component = &mut *component_ref.get();
        component.start()
    }
}

/// Returns the state of a component in the global registry.
///
/// # Errors
///
/// - Returns an error if the component is not found.
/// - Returns an error if the component is already borrowed.
pub fn component_state(id: &Ustr) -> anyhow::Result<ComponentState> {
    let component_ref = with_component_registry(|registry| {
        let component_ref = registry
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("Component '{id}' not found in global registry"))?;

        if !registry.try_borrow(*id) {
            anyhow::bail!(
                "Component '{id}' is already mutably borrowed. \
                 This would create aliasing mutable references (undefined behavior)."
            );
        }

        Ok::<_, anyhow::Error>(component_ref)
    })?;

    let _guard = BorrowGuard::new(*id);

    // SAFETY: Borrow tracking ensures there is no concurrent mutable lifecycle access.
    unsafe {
        let component = &*component_ref.get();
        Ok(component.state())
    }
}

/// Safely calls `stop()` on a component in the global registry.
///
/// # Errors
///
/// - Returns an error if the component is not found.
/// - Returns an error if the component is already borrowed.
/// - Returns an error if `stop()` fails.
pub fn stop_component(id: &Ustr) -> anyhow::Result<()> {
    let component_ref = with_component_registry(|registry| {
        let component_ref = registry
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("Component '{id}' not found in global registry"))?;

        if !registry.try_borrow(*id) {
            anyhow::bail!(
                "Component '{id}' is already mutably borrowed. \
                 This would create aliasing mutable references (undefined behavior)."
            );
        }

        Ok::<_, anyhow::Error>(component_ref)
    })?;

    let _guard = BorrowGuard::new(*id);

    // SAFETY: Borrow tracking ensures exclusive access
    unsafe {
        let component = &mut *component_ref.get();
        component.stop()
    }
}

/// Safely calls `reset()` on a component in the global registry.
///
/// # Errors
///
/// - Returns an error if the component is not found.
/// - Returns an error if the component is already borrowed.
/// - Returns an error if `reset()` fails.
pub fn reset_component(id: &Ustr) -> anyhow::Result<()> {
    let component_ref = with_component_registry(|registry| {
        let component_ref = registry
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("Component '{id}' not found in global registry"))?;

        if !registry.try_borrow(*id) {
            anyhow::bail!(
                "Component '{id}' is already mutably borrowed. \
                 This would create aliasing mutable references (undefined behavior)."
            );
        }

        Ok::<_, anyhow::Error>(component_ref)
    })?;

    let _guard = BorrowGuard::new(*id);

    // SAFETY: Borrow tracking ensures exclusive access
    unsafe {
        let component = &mut *component_ref.get();
        component.reset()
    }
}

/// Safely calls `dispose()` on a component in the global registry.
///
/// # Errors
///
/// - Returns an error if the component is not found.
/// - Returns an error if the component is already borrowed.
/// - Returns an error if `dispose()` fails.
pub fn dispose_component(id: &Ustr) -> anyhow::Result<()> {
    let component_ref = with_component_registry(|registry| {
        let component_ref = registry
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("Component '{id}' not found in global registry"))?;

        if !registry.try_borrow(*id) {
            anyhow::bail!(
                "Component '{id}' is already mutably borrowed. \
                 This would create aliasing mutable references (undefined behavior)."
            );
        }

        Ok::<_, anyhow::Error>(component_ref)
    })?;

    let _guard = BorrowGuard::new(*id);

    // SAFETY: Borrow tracking ensures exclusive access
    unsafe {
        let component = &mut *component_ref.get();
        component.dispose()
    }
}

/// Releases subscriptions for a component in the global registry.
///
/// This is used when retiring a component whose earlier `on_dispose` failed after the framework
/// left its registration intact.
///
/// # Errors
///
/// - Returns an error if the component is not found.
/// - Returns an error if the component is already borrowed.
pub fn release_component_subscriptions(id: &Ustr) -> anyhow::Result<()> {
    let component_ref = with_component_registry(|registry| {
        let component_ref = registry
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("Component '{id}' not found in global registry"))?;

        if !registry.try_borrow(*id) {
            anyhow::bail!(
                "Component '{id}' is already mutably borrowed. \
                 This would create aliasing mutable references (undefined behavior)."
            );
        }

        Ok::<_, anyhow::Error>(component_ref)
    })?;

    let _guard = BorrowGuard::new(*id);

    // SAFETY: Borrow tracking ensures exclusive access
    unsafe {
        let component = &mut *component_ref.get();
        component.release_subscriptions();
    }

    Ok(())
}

/// Returns a component from the global registry by ID.
pub fn get_component(id: &Ustr) -> Option<Rc<UnsafeCell<dyn Component>>> {
    with_component_registry(|registry| registry.get(id))
}

/// Removes the component with `id` from the global registry.
///
/// Only the exact ID is removed, so unrelated components sharing the thread-local registry
/// are untouched.
pub fn deregister_component(id: &Ustr) {
    with_component_registry(|registry| registry.remove(id));
}

#[cfg(test)]
/// Clears the component registry (for test isolation).
pub fn clear_component_registry() {
    with_component_registry(|registry| {
        registry.components.borrow_mut().clear();
        registry.borrows.borrow_mut().clear();
    });
}

#[cfg(test)]
mod tests {
    use std::{
        any::Any,
        sync::atomic::{AtomicBool, Ordering},
    };

    use rstest::rstest;

    use super::*;

    #[derive(Debug)]
    struct TestComponent {
        id: ComponentId,
        state: ComponentState,
        should_panic: &'static AtomicBool,
        hooks_fail: bool,
        releases: usize,
    }

    impl TestComponent {
        fn new(name: &str, should_panic: &'static AtomicBool) -> Self {
            Self {
                id: ComponentId::new(name),
                state: ComponentState::Ready,
                should_panic,
                hooks_fail: false,
                releases: 0,
            }
        }

        fn register_in_global_registry(self) -> Ustr {
            let id = self.id.inner();
            let component_ref: Rc<UnsafeCell<dyn Component>> = Rc::new(UnsafeCell::new(self));
            with_component_registry(|registry| registry.insert(id, component_ref));
            id
        }
    }

    impl Actor for TestComponent {
        fn id(&self) -> Ustr {
            self.id.inner()
        }

        fn handle(&mut self, _msg: &dyn Any) {}

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    impl Component for TestComponent {
        fn component_id(&self) -> ComponentId {
            self.id
        }

        fn state(&self) -> ComponentState {
            self.state
        }

        fn transition_state(&mut self, trigger: ComponentTrigger) -> anyhow::Result<()> {
            self.state = self.state.transition(&trigger)?;
            Ok(())
        }

        fn register(
            &mut self,
            _trader_id: TraderId,
            _clock: Rc<RefCell<dyn Clock>>,
            _cache: Rc<RefCell<Cache>>,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        #[expect(clippy::panic_in_result_fn)] // Intentional panic for testing
        fn on_start(&mut self) -> anyhow::Result<()> {
            assert!(
                !self.should_panic.load(Ordering::SeqCst),
                "Intentional panic for testing"
            );

            if self.hooks_fail {
                anyhow::bail!("on_start failed");
            }

            Ok(())
        }

        fn on_stop(&mut self) -> anyhow::Result<()> {
            if self.hooks_fail {
                anyhow::bail!("on_stop failed");
            }

            Ok(())
        }

        fn on_resume(&mut self) -> anyhow::Result<()> {
            if self.hooks_fail {
                anyhow::bail!("on_resume failed");
            }

            Ok(())
        }

        fn on_reset(&mut self) -> anyhow::Result<()> {
            if self.hooks_fail {
                anyhow::bail!("on_reset failed");
            }

            Ok(())
        }

        fn on_dispose(&mut self) -> anyhow::Result<()> {
            if self.hooks_fail {
                anyhow::bail!("on_dispose failed");
            }

            Ok(())
        }

        fn on_fault(&mut self) -> anyhow::Result<()> {
            if self.hooks_fail {
                anyhow::bail!("on_fault failed");
            }

            Ok(())
        }

        fn on_degrade(&mut self) -> anyhow::Result<()> {
            if self.hooks_fail {
                anyhow::bail!("on_degrade failed");
            }

            Ok(())
        }

        fn release_subscriptions(&mut self) {
            self.releases += 1;
        }
    }

    static NO_PANIC: AtomicBool = AtomicBool::new(false);
    static DO_PANIC: AtomicBool = AtomicBool::new(true);

    #[rstest]
    fn test_component_borrow_tracking_prevents_double_borrow() {
        clear_component_registry();

        let id = Ustr::from("test-component-1");
        let component = TestComponent::new("test-component-1", &NO_PANIC);
        let component_id = component.id.inner();

        let component_ref = Rc::new(UnsafeCell::new(component));
        with_component_registry(|registry| registry.insert(component_id, component_ref));

        // First borrow via start_component should succeed
        start_component(&id).unwrap();
        assert_eq!(component_state(&id).unwrap(), ComponentState::Running);

        // Component should now be borrowable again (guard released)
        stop_component(&id).unwrap();
        assert_eq!(component_state(&id).unwrap(), ComponentState::Stopped);
    }

    #[rstest]
    fn test_component_borrow_released_after_lifecycle_call() {
        clear_component_registry();

        let id = Ustr::from("test-component-2");
        let component = TestComponent::new("test-component-2", &NO_PANIC);
        let component_id = component.id.inner();

        let component_ref = Rc::new(UnsafeCell::new(component));
        with_component_registry(|registry| registry.insert(component_id, component_ref));

        // Call start - borrow should be released after
        let _ = start_component(&id);

        // Verify not marked as borrowed
        assert!(!with_component_registry(
            |registry| registry.is_borrowed(&id)
        ));
    }

    #[rstest]
    fn test_component_borrow_released_on_panic() {
        clear_component_registry();

        let id = Ustr::from("test-component-panic");
        let component = TestComponent::new("test-component-panic", &DO_PANIC);
        let component_id = component.id.inner();

        let component_ref = Rc::new(UnsafeCell::new(component));
        with_component_registry(|registry| registry.insert(component_id, component_ref));

        // Call start which will panic - catch the panic
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = start_component(&id);
        }));
        assert!(result.is_err(), "Expected panic from on_start");

        // Borrow should still be released due to BorrowGuard drop
        assert!(
            !with_component_registry(|registry| registry.is_borrowed(&id)),
            "Borrow was not released after panic"
        );
    }

    #[rstest]
    #[case(ComponentState::PreInitialized, ComponentTrigger::Start)]
    #[case(ComponentState::Ready, ComponentTrigger::Resume)]
    #[case(ComponentState::Running, ComponentTrigger::Start)]
    #[case(ComponentState::Stopped, ComponentTrigger::Stop)]
    #[case(ComponentState::Disposed, ComponentTrigger::Dispose)]
    #[case(ComponentState::Faulted, ComponentTrigger::Fault)]
    fn test_transition_rejects_invalid_trigger(
        #[case] state: ComponentState,
        #[case] trigger: ComponentTrigger,
    ) {
        let mut current = state;

        let error = current.transition(&trigger).unwrap_err();

        assert_eq!(
            error.to_string(),
            format!("Invalid state trigger {state} -> {trigger}")
        );
        assert_eq!(current, state, "Rejected trigger must not mutate the state");
    }

    /// Covers every arm of the transition table, so deleting one is a failing test.
    #[rstest]
    #[case(
        ComponentState::PreInitialized,
        ComponentTrigger::Initialize,
        ComponentState::Ready
    )]
    #[case(
        ComponentState::Ready,
        ComponentTrigger::Reset,
        ComponentState::Resetting
    )]
    #[case(
        ComponentState::Ready,
        ComponentTrigger::Start,
        ComponentState::Starting
    )]
    #[case(
        ComponentState::Ready,
        ComponentTrigger::Dispose,
        ComponentState::Disposing
    )]
    #[case(
        ComponentState::Resetting,
        ComponentTrigger::ResetCompleted,
        ComponentState::Ready
    )]
    #[case(
        ComponentState::Starting,
        ComponentTrigger::StartCompleted,
        ComponentState::Running
    )]
    #[case(
        ComponentState::Starting,
        ComponentTrigger::Stop,
        ComponentState::Stopping
    )]
    #[case(
        ComponentState::Starting,
        ComponentTrigger::Fault,
        ComponentState::Faulting
    )]
    #[case(
        ComponentState::Running,
        ComponentTrigger::Stop,
        ComponentState::Stopping
    )]
    #[case(
        ComponentState::Running,
        ComponentTrigger::Degrade,
        ComponentState::Degrading
    )]
    #[case(
        ComponentState::Running,
        ComponentTrigger::Fault,
        ComponentState::Faulting
    )]
    #[case(
        ComponentState::Resuming,
        ComponentTrigger::Stop,
        ComponentState::Stopping
    )]
    #[case(
        ComponentState::Resuming,
        ComponentTrigger::ResumeCompleted,
        ComponentState::Running
    )]
    #[case(
        ComponentState::Resuming,
        ComponentTrigger::Fault,
        ComponentState::Faulting
    )]
    #[case(
        ComponentState::Stopping,
        ComponentTrigger::StopCompleted,
        ComponentState::Stopped
    )]
    #[case(
        ComponentState::Stopping,
        ComponentTrigger::Dispose,
        ComponentState::Disposing
    )]
    #[case(
        ComponentState::Stopping,
        ComponentTrigger::Fault,
        ComponentState::Faulting
    )]
    #[case(
        ComponentState::Stopped,
        ComponentTrigger::Reset,
        ComponentState::Resetting
    )]
    #[case(
        ComponentState::Stopped,
        ComponentTrigger::Resume,
        ComponentState::Resuming
    )]
    #[case(
        ComponentState::Stopped,
        ComponentTrigger::Dispose,
        ComponentState::Disposing
    )]
    #[case(
        ComponentState::Stopped,
        ComponentTrigger::Fault,
        ComponentState::Faulting
    )]
    #[case(
        ComponentState::Degrading,
        ComponentTrigger::DegradeCompleted,
        ComponentState::Degraded
    )]
    #[case(
        ComponentState::Degraded,
        ComponentTrigger::Resume,
        ComponentState::Resuming
    )]
    #[case(
        ComponentState::Degraded,
        ComponentTrigger::Stop,
        ComponentState::Stopping
    )]
    #[case(
        ComponentState::Degraded,
        ComponentTrigger::Fault,
        ComponentState::Faulting
    )]
    #[case(
        ComponentState::Disposing,
        ComponentTrigger::DisposeCompleted,
        ComponentState::Disposed
    )]
    #[case(
        ComponentState::Disposing,
        ComponentTrigger::Fault,
        ComponentState::Faulting
    )]
    #[case(
        ComponentState::Faulting,
        ComponentTrigger::Dispose,
        ComponentState::Disposing
    )]
    #[case(
        ComponentState::Faulting,
        ComponentTrigger::FaultCompleted,
        ComponentState::Faulted
    )]
    fn test_transition_accepts_valid_trigger(
        #[case] state: ComponentState,
        #[case] trigger: ComponentTrigger,
        #[case] expected: ComponentState,
    ) {
        let mut current = state;

        assert_eq!(current.transition(&trigger).unwrap(), expected);
    }

    #[rstest]
    fn test_state_predicates_match_the_current_state() {
        let mut component = TestComponent::new("predicates", &NO_PANIC);

        assert!(component.is_ready());
        assert!(component.not_running());

        component.start().unwrap();
        assert!(component.is_running());
        assert!(!component.not_running());
        assert!(!component.is_ready());

        component.stop().unwrap();
        assert!(component.is_stopped());
        assert!(!component.is_running());

        component.resume().unwrap();
        component.degrade().unwrap();
        assert!(component.is_degraded());
        assert!(!component.is_stopped());

        component.fault().unwrap();
        assert!(component.is_faulted());
        assert!(!component.is_degraded());
        assert!(!component.is_disposed());
    }

    #[rstest]
    fn test_is_disposed_only_after_disposal() {
        let mut component = TestComponent::new("disposable", &NO_PANIC);

        assert!(!component.is_disposed());

        component.dispose().unwrap();

        assert!(component.is_disposed());
        assert!(!component.is_ready());
        assert!(!component.is_faulted());
    }

    #[rstest]
    fn test_degrade_halts_transition_when_on_degrade_fails() {
        let mut component = TestComponent::new("failing-degrade", &NO_PANIC);
        component.start().unwrap();
        component.hooks_fail = true;

        let error = component.degrade().unwrap_err();

        assert_eq!(error.to_string(), "on_degrade failed");
        assert_eq!(component.state(), ComponentState::Degrading);
    }

    #[rstest]
    fn test_initialize_advances_from_pre_initialized() {
        let mut component = TestComponent::new("initializing", &NO_PANIC);
        component.state = ComponentState::PreInitialized;

        component.initialize().unwrap();

        assert_eq!(component.state(), ComponentState::Ready);
    }

    #[rstest]
    fn test_start_halts_transition_when_on_start_fails() {
        let mut component = TestComponent::new("failing-start", &NO_PANIC);
        component.hooks_fail = true;

        let error = component.start().unwrap_err();

        assert_eq!(error.to_string(), "on_start failed");
        assert_eq!(component.state(), ComponentState::Starting);
    }

    #[rstest]
    fn test_stop_halts_transition_when_on_stop_fails() {
        let mut component = TestComponent::new("failing-stop", &NO_PANIC);
        component.start().unwrap();
        component.hooks_fail = true;

        let error = component.stop().unwrap_err();

        assert_eq!(error.to_string(), "on_stop failed");
        assert_eq!(component.state(), ComponentState::Stopping);
    }

    #[rstest]
    fn test_resume_halts_transition_when_on_resume_fails() {
        let mut component = TestComponent::new("failing-resume", &NO_PANIC);
        component.start().unwrap();
        component.stop().unwrap();
        component.hooks_fail = true;

        let error = component.resume().unwrap_err();

        assert_eq!(error.to_string(), "on_resume failed");
        assert_eq!(component.state(), ComponentState::Resuming);
    }

    #[rstest]
    fn test_reset_retains_subscriptions_when_on_reset_fails() {
        let mut component = TestComponent::new("failing-reset", &NO_PANIC);
        component.hooks_fail = true;

        let error = component.reset().unwrap_err();

        assert_eq!(error.to_string(), "on_reset failed");
        assert_eq!(component.state(), ComponentState::Resetting);
        assert_eq!(component.releases, 0);
    }

    #[rstest]
    fn test_reset_releases_subscriptions_on_success() {
        let mut component = TestComponent::new("resetting", &NO_PANIC);

        component.reset().unwrap();

        assert_eq!(component.state(), ComponentState::Ready);
        assert_eq!(component.releases, 1);
    }

    #[rstest]
    fn test_dispose_faults_and_retains_subscriptions_when_on_dispose_fails() {
        let mut component = TestComponent::new("failing-dispose", &NO_PANIC);
        component.hooks_fail = true;

        let error = component.dispose().unwrap_err();

        assert_eq!(error.to_string(), "on_dispose failed");
        assert_eq!(component.state(), ComponentState::Faulted);
        assert_eq!(component.releases, 0);
    }

    #[rstest]
    fn test_dispose_releases_subscriptions_on_success() {
        let mut component = TestComponent::new("disposing", &NO_PANIC);

        component.dispose().unwrap();

        assert_eq!(component.state(), ComponentState::Disposed);
        assert_eq!(component.releases, 1);
    }

    #[rstest]
    fn test_fault_releases_subscriptions_even_when_on_fault_fails() {
        let mut component = TestComponent::new("failing-fault", &NO_PANIC);
        component.start().unwrap();
        component.hooks_fail = true;

        let error = component.fault().unwrap_err();

        assert_eq!(error.to_string(), "on_fault failed");
        assert_eq!(component.state(), ComponentState::Faulting);
        assert_eq!(component.releases, 1);
    }

    #[rstest]
    fn test_fault_releases_subscriptions_on_success() {
        let mut component = TestComponent::new("faulting", &NO_PANIC);
        component.start().unwrap();

        component.fault().unwrap();

        assert_eq!(component.state(), ComponentState::Faulted);
        assert_eq!(component.releases, 1);
    }

    #[rstest]
    fn test_registry_entry_points_reject_unknown_component() {
        clear_component_registry();

        let id = Ustr::from("absent-component");

        for (name, entry_point) in registry_entry_points() {
            let error = entry_point(&id).unwrap_err();
            assert_eq!(
                error.to_string(),
                "Component 'absent-component' not found in global registry",
                "unexpected error from {name}"
            );
        }

        let error = component_state(&id).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Component 'absent-component' not found in global registry"
        );
    }

    #[rstest]
    fn test_registry_entry_points_reject_borrowed_component() {
        clear_component_registry();

        let id = TestComponent::new("borrowed-component", &NO_PANIC).register_in_global_registry();
        assert!(with_component_registry(|registry| registry.try_borrow(id)));

        for (name, entry_point) in registry_entry_points() {
            let error = entry_point(&id).unwrap_err();
            assert!(
                error
                    .to_string()
                    .starts_with("Component 'borrowed-component' is already mutably borrowed."),
                "unexpected error from {name}: {error}"
            );
        }

        let error = component_state(&id).unwrap_err();
        assert!(
            error
                .to_string()
                .starts_with("Component 'borrowed-component' is already mutably borrowed.")
        );

        // The rejected calls must leave the original borrow in place
        assert!(with_component_registry(|registry| registry.is_borrowed(&id)));
    }

    type RegistryEntryPoint = (&'static str, fn(&Ustr) -> anyhow::Result<()>);

    /// Lifecycle entry points that resolve a component from the global registry.
    fn registry_entry_points() -> [RegistryEntryPoint; 5] {
        [
            ("start", start_component),
            ("stop", stop_component),
            ("reset", reset_component),
            ("dispose", dispose_component),
            ("release", release_component_subscriptions),
        ]
    }

    #[rstest]
    fn test_registry_entry_points_drive_the_full_lifecycle() {
        clear_component_registry();

        let id = TestComponent::new("lifecycle-component", &NO_PANIC).register_in_global_registry();

        assert_eq!(component_state(&id).unwrap(), ComponentState::Ready);

        start_component(&id).unwrap();
        assert_eq!(component_state(&id).unwrap(), ComponentState::Running);

        stop_component(&id).unwrap();
        assert_eq!(component_state(&id).unwrap(), ComponentState::Stopped);

        reset_component(&id).unwrap();
        assert_eq!(component_state(&id).unwrap(), ComponentState::Ready);

        dispose_component(&id).unwrap();
        assert_eq!(component_state(&id).unwrap(), ComponentState::Disposed);

        assert!(!with_component_registry(
            |registry| registry.is_borrowed(&id)
        ));
    }

    #[rstest]
    fn test_release_component_subscriptions_invokes_the_component_hook() {
        clear_component_registry();

        let component = TestComponent::new("releasing-component", &NO_PANIC);
        let id = component.id.inner();
        let component_ref = Rc::new(UnsafeCell::new(component));
        let observed = component_ref.clone();
        with_component_registry(|registry| {
            registry.insert(id, component_ref as Rc<UnsafeCell<dyn Component>>);
        });

        release_component_subscriptions(&id).unwrap();
        release_component_subscriptions(&id).unwrap();

        // SAFETY: no lifecycle call is in flight, so no other reference exists
        assert_eq!(unsafe { (*observed.get()).releases }, 2);
    }

    #[rstest]
    fn test_deregister_component_removes_only_the_named_component() {
        clear_component_registry();

        let kept = TestComponent::new("kept-component", &NO_PANIC).register_in_global_registry();
        let removed =
            TestComponent::new("removed-component", &NO_PANIC).register_in_global_registry();

        deregister_component(&removed);

        assert!(get_component(&removed).is_none());
        assert!(get_component(&kept).is_some());
        assert_eq!(component_state(&kept).unwrap(), ComponentState::Ready);
    }

    #[rstest]
    fn test_component_registry_debug_reports_components_and_borrows() {
        clear_component_registry();

        let id = TestComponent::new("debug-component", &NO_PANIC).register_in_global_registry();
        assert!(with_component_registry(|registry| registry.try_borrow(id)));

        let debug = with_component_registry(|registry| format!("{registry:?}"));

        assert!(debug.contains("debug-component"), "{debug}");
        assert!(debug.contains("active_borrows: 1"), "{debug}");
    }

    #[rstest]
    fn test_registry_remove_returns_the_registered_component() {
        clear_component_registry();

        let id = TestComponent::new("removable", &NO_PANIC).register_in_global_registry();

        assert!(with_component_registry(|registry| registry.remove(&id)).is_some());
        assert!(with_component_registry(|registry| registry.remove(&id)).is_none());
    }
}
