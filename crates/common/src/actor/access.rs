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

//! Checked allocation access for ordered callback dispatch.
//!
//! Actor registry guards and component lifecycle operations do not acquire these guards.
//! Component lifecycle operations use separate ID-based tracking. Callers must prevent
//! overlapping access through raw handles, actor registry guards, or component lifecycle
//! operations; allocation-level exclusion applies only between these checked guards.
//!
//! Production dispatch does not use these primitives. Activation requires the callback and
//! runtime boundaries described in `docs/developer_guide/callback_dispatch.md`.

#![allow(
    dead_code,
    reason = "activation requires ordered callback dispatch and safe runtime drains"
)]

use std::{
    any::{Any, TypeId},
    cell::{RefCell, UnsafeCell},
    marker::PhantomData,
    ops::{Deref, DerefMut},
    rc::Rc,
};

use ahash::AHashSet;
use ustr::Ustr;

use super::{Actor, registry::get_actor};

thread_local! {
    static BORROWED: Rc<RefCell<AHashSet<*const ()>>> = Rc::default();
}

pub(super) fn is_active() -> bool {
    BORROWED
        .try_with(|borrowed| !borrowed.borrow().is_empty())
        .unwrap_or(true)
}

/// Owns an allocation and excludes other checked accesses to that allocation until it drops.
///
/// Retaining the `Rc` prevents address reuse while access is held.
#[derive(Debug)]
pub(crate) struct AllocationGuard<T: ?Sized> {
    allocation: Rc<UnsafeCell<T>>,
    borrowed: Rc<RefCell<AHashSet<*const ()>>>,
}

impl<T: ?Sized> AllocationGuard<T> {
    /// Returns a guard, or `None` when the allocation is busy or thread-local tracking has ended.
    pub(crate) fn acquire(allocation: Rc<UnsafeCell<T>>) -> Option<Self> {
        let address = Rc::as_ptr(&allocation).cast::<()>();
        BORROWED
            .try_with(|borrowed| {
                if !borrowed.borrow_mut().insert(address) {
                    return None;
                }

                Some(Self {
                    allocation,
                    borrowed: Rc::clone(borrowed),
                })
            })
            .ok()
            .flatten()
    }
}

impl<T: ?Sized> Deref for AllocationGuard<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: The guard retains the allocation and excludes competing checked access.
        // Callers must exclude overlapping access through paths that do not acquire these guards.
        unsafe { &*self.allocation.get() }
    }
}

impl<T: ?Sized> DerefMut for AllocationGuard<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: The guard retains the allocation and excludes competing checked access.
        // Callers must exclude overlapping access through paths that do not acquire these guards.
        unsafe { &mut *self.allocation.get() }
    }
}

impl<T: ?Sized> Drop for AllocationGuard<T> {
    fn drop(&mut self) {
        // Release before dropping the allocation, whose destructor may acquire other guards
        self.borrowed
            .borrow_mut()
            .remove(&Rc::as_ptr(&self.allocation).cast::<()>());
    }
}

/// Owns checked access to an actor whose concrete type is verified under the allocation guard.
///
/// Verification uses `Any` directly because an actor can override `as_any`.
#[derive(Debug)]
pub(crate) struct ActorGuard<T: Actor> {
    allocation: AllocationGuard<dyn Actor>,
    marker: PhantomData<T>,
}

impl<T: Actor> ActorGuard<T> {
    /// Acquires access before type inspection, so busy access takes precedence over a wrong type.
    pub(crate) fn acquire(actor: Rc<UnsafeCell<dyn Actor>>) -> Result<Self, ActorAccessError> {
        let allocation = AllocationGuard::acquire(actor).ok_or(ActorAccessError::Busy)?;
        let actual_type = Any::type_id(&*allocation);
        let expected_type = TypeId::of::<T>();
        if actual_type != expected_type {
            return Err(ActorAccessError::WrongType {
                expected_type,
                actual_type,
            });
        }

        Ok(Self {
            allocation,
            marker: PhantomData,
        })
    }
}

impl<T: Actor> Deref for ActorGuard<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: The allocation guard is held and the concrete type was verified on acquisition.
        // Callers must exclude overlapping access through paths that do not acquire these guards.
        unsafe { &*self.allocation.allocation.get().cast::<T>() }
    }
}

impl<T: Actor> DerefMut for ActorGuard<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: The allocation guard is held and the concrete type was verified on acquisition.
        // Callers must exclude overlapping access through paths that do not acquire these guards.
        unsafe { &mut *self.allocation.allocation.get().cast::<T>() }
    }
}

pub(crate) fn try_get_actor<T: Actor>(id: &Ustr) -> Result<ActorGuard<T>, ActorAccessError> {
    ActorGuard::acquire(get_actor(id).ok_or(ActorAccessError::Missing)?)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActorAccessError {
    Missing,
    // Includes unavailable tracking during thread-local teardown
    Busy,
    WrongType {
        expected_type: TypeId,
        actual_type: TypeId,
    },
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, panic::AssertUnwindSafe};

    use nautilus_model::identifiers::{ComponentId, TraderId};
    use rstest::rstest;

    use super::*;
    use crate::{
        actor::registry::{
            clear_actor_registry, deregister_actor, register_actor, with_actor_registry,
        },
        cache::Cache,
        clock::Clock,
        component::{Component, register_component_actor},
        enums::{ComponentState, ComponentTrigger},
    };

    #[derive(Debug)]
    struct TestActor {
        id: Ustr,
        value: i32,
    }

    impl Actor for TestActor {
        fn id(&self) -> Ustr {
            self.id
        }

        fn handle(&mut self, _msg: &dyn Any) {}

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    impl Component for TestActor {
        fn component_id(&self) -> ComponentId {
            ComponentId::new(self.id.as_str())
        }

        fn state(&self) -> ComponentState {
            ComponentState::Ready
        }

        fn transition_state(&mut self, _trigger: ComponentTrigger) -> anyhow::Result<()> {
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
    }

    #[derive(Debug)]
    struct OtherActor;

    impl Actor for OtherActor {
        fn id(&self) -> Ustr {
            Ustr::from("other")
        }

        fn handle(&mut self, _msg: &dyn Any) {}

        fn as_any(&self) -> &dyn Any {
            &0_u8
        }
    }

    #[rstest]
    fn test_actor_type_uses_allocation_identity() {
        register_actor(OtherActor);
        let actor = try_get_actor::<OtherActor>(&Ustr::from("other")).unwrap();
        assert_eq!(actor.id(), Ustr::from("other"));
    }

    #[rstest]
    fn test_actor_access_distinguishes_missing_wrong_type_and_busy() {
        clear_actor_registry();
        let id = Ustr::from("checked-errors");
        assert_eq!(
            try_get_actor::<TestActor>(&id).unwrap_err(),
            ActorAccessError::Missing
        );
        register_actor(TestActor { id, value: 11 });
        assert_eq!(
            try_get_actor::<OtherActor>(&id).unwrap_err(),
            ActorAccessError::WrongType {
                expected_type: TypeId::of::<OtherActor>(),
                actual_type: TypeId::of::<TestActor>(),
            }
        );
        let mut actor = try_get_actor::<TestActor>(&id).unwrap();
        reject_nested_access(&mut actor);
        assert_eq!(actor.value, 29);
    }

    fn reject_nested_access(actor: &mut TestActor) {
        actor.value = 17;
        assert_eq!(
            try_get_actor::<TestActor>(&actor.id).unwrap_err(),
            ActorAccessError::Busy
        );
        assert_eq!(
            try_get_actor::<OtherActor>(&actor.id).unwrap_err(),
            ActorAccessError::Busy
        );
        actor.value = 29;
    }

    #[rstest]
    fn test_actor_and_component_views_share_access() {
        let id = Ustr::from("checked-component");
        let allocation = register_component_actor(TestActor { id, value: 31 });
        let component: Rc<UnsafeCell<dyn Component>> = allocation;
        let actor = try_get_actor::<TestActor>(&id).unwrap();
        assert!(AllocationGuard::acquire(component.clone()).is_none());
        drop(actor);
        let mut component_guard = AllocationGuard::acquire(component).unwrap();
        reject_nested_component_access(&mut *component_guard, id);
        assert_eq!(component_guard.state(), ComponentState::Ready);
        drop(component_guard);
        assert_eq!(try_get_actor::<TestActor>(&id).unwrap().value, 31);
    }

    #[allow(
        clippy::needless_pass_by_ref_mut,
        reason = "the protected mutable reference must remain live across the rejected lookup"
    )]
    fn reject_nested_component_access(component: &mut dyn Component, id: Ustr) {
        assert_eq!(component.component_id().inner(), id);
        assert_eq!(
            try_get_actor::<TestActor>(&id).unwrap_err(),
            ActorAccessError::Busy
        );
        assert_eq!(component.component_id().inner(), id);
    }

    #[rstest]
    fn test_removal_replacement_and_reregistration_preserve_access() {
        let id = Ustr::from("checked-registration");
        let alias = Ustr::from("checked-alias");
        let allocation = register_actor(TestActor { id, value: 37 });
        let mut actor = try_get_actor::<TestActor>(&id).unwrap();
        deregister_actor(&id);
        assert_eq!(
            try_get_actor::<TestActor>(&id).unwrap_err(),
            ActorAccessError::Missing
        );
        register_actor(TestActor { id, value: 41 });
        assert_eq!(try_get_actor::<TestActor>(&id).unwrap().value, 41);
        with_actor_registry(|registry| {
            registry.insert(id, allocation.clone());
            registry.insert(alias, allocation.clone());
        });

        assert_eq!(
            try_get_actor::<TestActor>(&id).unwrap_err(),
            ActorAccessError::Busy
        );
        assert_eq!(
            try_get_actor::<TestActor>(&alias).unwrap_err(),
            ActorAccessError::Busy
        );
        actor.value = 43;
        drop(actor);
        assert_eq!(try_get_actor::<TestActor>(&id).unwrap().value, 43);
    }

    #[rstest]
    fn test_guard_retains_allocation_after_registry_clear() {
        let id = Ustr::from("checked-lifetime");
        let allocation = register_actor(TestActor { id, value: 47 });
        let weak = Rc::downgrade(&allocation);
        let actor = try_get_actor::<TestActor>(&id).unwrap();
        drop(allocation);
        clear_actor_registry();
        assert_eq!(weak.strong_count(), 1);
        assert_eq!(actor.value, 47);
        drop(actor);
        assert_eq!(weak.strong_count(), 0);
    }

    #[rstest]
    fn test_unwind_releases_access() {
        let id = Ustr::from("checked-unwind");
        register_actor(TestActor { id, value: 53 });

        let result = std::panic::catch_unwind(|| {
            let mut actor = try_get_actor::<TestActor>(&id).unwrap();
            actor.value = 59;
            panic!("unwind checked access");
        });

        assert_eq!(
            result.unwrap_err().downcast_ref::<&str>(),
            Some(&"unwind checked access")
        );
        assert_eq!(try_get_actor::<TestActor>(&id).unwrap().value, 59);
    }

    #[rstest]
    fn test_allocation_guard_mutation_and_unwind() {
        let allocation = Rc::new(UnsafeCell::new(61));

        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let mut guard = AllocationGuard::acquire(allocation.clone()).unwrap();
            *guard = 67;
            assert!(AllocationGuard::acquire(allocation.clone()).is_none());
            panic!("unwind allocation access");
        }));

        assert_eq!(
            result.unwrap_err().downcast_ref::<&str>(),
            Some(&"unwind allocation access")
        );
        assert_eq!(*AllocationGuard::acquire(allocation).unwrap(), 67);
    }

    #[derive(Debug)]
    struct ReentrantDrop {
        allocation: Rc<UnsafeCell<i32>>,
        observed: Rc<Cell<i32>>,
    }

    impl Drop for ReentrantDrop {
        fn drop(&mut self) {
            let guard = AllocationGuard::acquire(self.allocation.clone()).unwrap();
            self.observed.set(*guard);
        }
    }

    #[rstest]
    fn test_allocation_destructor_can_acquire_access() {
        let observed = Rc::new(Cell::new(0));

        let allocation = Rc::new(UnsafeCell::new(ReentrantDrop {
            allocation: Rc::new(UnsafeCell::new(71)),
            observed: observed.clone(),
        }));

        let guard = AllocationGuard::acquire(allocation).unwrap();
        drop(guard);
        assert_eq!(observed.get(), 71);
    }
}
