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

//! Thread-local actor registry with access guards.
//!
//! # Design
//!
//! The actor registry stores actors in thread-local storage and provides access via
//! [`ActorRef<T>`] guards. This design addresses several constraints:
//!
//! - **Use-after-free prevention**: `ActorRef` holds an `Rc` clone, keeping the actor
//!   alive even if removed from the registry while the guard exists.
//! - **Scoped registry access**: Registry access stays tied to the thread-local storage
//!   access callback.
//! - **Thread-local only**: Guards must not be sent across threads.
//!
//! # Limitations
//!
//! - **Aliasing not prevented**: Two guards can exist for the same actor simultaneously,
//!   allowing aliased mutable access. This is undefined behavior if both guards create
//!   overlapping references to the same actor. The current actor dispatch model relies
//!   on same-actor re-entrant lookups, so fixing this requires a broader dispatch and
//!   ownership redesign.
//!
//! # Invariants
//!
//! These contracts must hold regardless of how the registry is implemented
//! internally. The first three are verified by tests in this module. The
//! fourth is a usage discipline enforced by convention.
//!
//! - **Thread isolation**: Each thread has its own registry instance. An actor
//!   registered on one thread is never visible from another.
//! - **Guard survival**: An [`ActorRef`] keeps its actor alive via reference
//!   counting. Removing or replacing an actor in the registry does not invalidate
//!   existing guards.
//! - **Type safety**: [`get_actor_unchecked`] and [`try_get_actor_unchecked`]
//!   verify the concrete type at runtime before casting. A type mismatch panics
//!   or returns `None`, respectively.
//! - **Short-lived guards**: Guards must be obtained, used, and dropped within a
//!   single synchronous scope. Never store an [`ActorRef`] in a struct or hold
//!   one across an `.await` point.

use std::{
    any::TypeId,
    cell::{RefCell, UnsafeCell},
    fmt::Debug,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    rc::Rc,
};

use ahash::AHashMap;
use ustr::Ustr;

use super::Actor;

/// A guard providing mutable access to an actor.
///
/// This guard holds an `Rc` reference to keep the actor alive.
pub struct ActorRef<T: Actor> {
    actor_rc: Rc<UnsafeCell<dyn Actor>>,
    _marker: PhantomData<T>,
}

impl<T: Actor> Debug for ActorRef<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(ActorRef))
            .field("actor_id", &self.deref().id())
            .finish()
    }
}

impl<T: Actor> Deref for ActorRef<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: Type was verified at construction time.
        unsafe { &*(self.actor_rc.get() as *const T) }
    }
}

impl<T: Actor> DerefMut for ActorRef<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: Type was verified at construction time.
        unsafe { &mut *self.actor_rc.get().cast::<T>() }
    }
}

thread_local! {
    static ACTOR_REGISTRY: ActorRegistry = ActorRegistry::new();
}

/// Registry for storing actors.
pub struct ActorRegistry {
    actors: RefCell<AHashMap<Ustr, Rc<UnsafeCell<dyn Actor>>>>,
}

impl Debug for ActorRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let actors_ref = self.actors.borrow();
        let keys: Vec<&Ustr> = actors_ref.keys().collect();
        f.debug_struct(stringify!(ActorRegistry))
            .field("actors", &keys)
            .finish()
    }
}

impl Default for ActorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ActorRegistry {
    pub fn new() -> Self {
        Self {
            actors: RefCell::new(AHashMap::new()),
        }
    }

    pub fn insert(&self, id: Ustr, actor: Rc<UnsafeCell<dyn Actor>>) {
        let mut actors = self.actors.borrow_mut();
        if actors.contains_key(&id) {
            log::warn!("Replacing existing actor with id: {id}");
        }
        actors.insert(id, actor);
    }

    pub fn get(&self, id: &Ustr) -> Option<Rc<UnsafeCell<dyn Actor>>> {
        self.actors.borrow().get(id).cloned()
    }

    /// Returns the number of registered actors.
    pub fn len(&self) -> usize {
        self.actors.borrow().len()
    }

    /// Checks if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.actors.borrow().is_empty()
    }

    /// Removes an actor from the registry.
    pub fn remove(&self, id: &Ustr) -> Option<Rc<UnsafeCell<dyn Actor>>> {
        self.actors.borrow_mut().remove(id)
    }

    /// Checks if an actor with the `id` exists.
    pub fn contains(&self, id: &Ustr) -> bool {
        self.actors.borrow().contains_key(id)
    }
}

pub fn with_actor_registry<R>(f: impl FnOnce(&ActorRegistry) -> R) -> R {
    ACTOR_REGISTRY.with(f)
}

/// Registers an actor.
pub fn register_actor<T>(actor: T) -> Rc<UnsafeCell<T>>
where
    T: Actor + 'static,
{
    let actor_id = actor.id();
    let actor_ref = Rc::new(UnsafeCell::new(actor));

    // Register as Actor (message handling only)
    let actor_trait_ref: Rc<UnsafeCell<dyn Actor>> = actor_ref.clone();
    with_actor_registry(|registry| registry.insert(actor_id, actor_trait_ref));

    actor_ref
}

pub fn get_actor(id: &Ustr) -> Option<Rc<UnsafeCell<dyn Actor>>> {
    with_actor_registry(|registry| registry.get(id))
}

/// Returns a guard providing mutable access to the registered actor of type `T`.
///
/// The returned [`ActorRef`] holds an `Rc` to keep the actor alive, preventing
/// use-after-free if the actor is removed from the registry.
///
/// # Panics
///
/// - Panics if no actor with the specified `id` is found in the registry.
/// - Panics if the stored actor is not of type `T`.
#[must_use]
pub fn get_actor_unchecked<T: Actor>(id: &Ustr) -> ActorRef<T> {
    let actor_rc = with_actor_registry(|registry| registry.get(id))
        .unwrap_or_else(|| panic!("Actor for {id} not found"));

    match actor_ref_from_rc(actor_rc) {
        Ok(actor_ref) => actor_ref,
        Err(ActorRefError {
            expected_type,
            actual_type,
        }) => {
            panic!(
                "Actor type mismatch for '{id}': expected {expected_type:?}, found {actual_type:?}"
            )
        }
    }
}

/// Attempts to get a guard providing mutable access to the registered actor.
///
/// Returns `None` if the actor is not found or the type doesn't match.
#[must_use]
pub fn try_get_actor_unchecked<T: Actor>(id: &Ustr) -> Option<ActorRef<T>> {
    let actor_rc = with_actor_registry(|registry| registry.get(id))?;
    actor_ref_from_rc(actor_rc).ok()
}

#[derive(Debug)]
struct ActorRefError {
    expected_type: TypeId,
    actual_type: TypeId,
}

fn actor_ref_from_rc<T: Actor>(
    actor_rc: Rc<UnsafeCell<dyn Actor>>,
) -> Result<ActorRef<T>, ActorRefError> {
    // SAFETY: Get a reference to check the type before casting.
    let actor_ref = unsafe { &*actor_rc.get() };
    let actual_type = actor_ref.as_any().type_id();
    let expected_type = TypeId::of::<T>();

    if actual_type != expected_type {
        return Err(ActorRefError {
            expected_type,
            actual_type,
        });
    }

    Ok(ActorRef {
        actor_rc,
        _marker: PhantomData,
    })
}

/// Checks if an actor with the `id` exists in the registry.
pub fn actor_exists(id: &Ustr) -> bool {
    with_actor_registry(|registry| registry.contains(id))
}

/// Returns the number of registered actors.
pub fn actor_count() -> usize {
    with_actor_registry(ActorRegistry::len)
}

#[cfg(test)]
/// Clears the actor registry (for test isolation).
pub fn clear_actor_registry() {
    with_actor_registry(|registry| registry.actors.borrow_mut().clear());
}

#[cfg(test)]
mod tests {
    use std::any::Any;

    use rstest::rstest;

    use super::*;

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

    #[rstest]
    fn test_register_and_get_actor() {
        clear_actor_registry();

        let id = Ustr::from("test-actor");
        let actor = TestActor { id, value: 42 };
        register_actor(actor);

        let actor_ref = get_actor_unchecked::<TestActor>(&id);
        assert_eq!(actor_ref.value, 42);
    }

    #[rstest]
    fn test_mutation_through_reference() {
        clear_actor_registry();

        let id = Ustr::from("test-actor-mut");
        let actor = TestActor { id, value: 0 };
        register_actor(actor);

        let mut actor_ref = get_actor_unchecked::<TestActor>(&id);
        actor_ref.value = 999;
        drop(actor_ref);

        let actor_ref2 = get_actor_unchecked::<TestActor>(&id);
        assert_eq!(actor_ref2.value, 999);
    }

    #[rstest]
    fn test_try_get_returns_none_for_missing() {
        clear_actor_registry();

        let id = Ustr::from("nonexistent");
        let result = try_get_actor_unchecked::<TestActor>(&id);
        assert!(result.is_none());
    }

    #[rstest]
    fn test_try_get_returns_none_for_wrong_type() {
        #[derive(Debug)]
        struct OtherActor {
            id: Ustr,
        }

        impl Actor for OtherActor {
            fn id(&self) -> Ustr {
                self.id
            }
            fn handle(&mut self, _msg: &dyn Any) {}
            fn as_any(&self) -> &dyn Any {
                self
            }
        }

        clear_actor_registry();

        let id = Ustr::from("other-actor");
        let actor = OtherActor { id };
        register_actor(actor);

        let result = try_get_actor_unchecked::<TestActor>(&id);
        assert!(result.is_none());
    }

    #[rstest]
    fn test_registry_is_thread_local() {
        clear_actor_registry();

        let id = Ustr::from("thread-local-actor");
        let actor = TestActor { id, value: 42 };
        register_actor(actor);

        assert!(actor_exists(&id));
        assert_eq!(actor_count(), 1);

        let visible_on_other_thread = std::thread::spawn(move || {
            // Each thread gets its own empty registry
            (actor_exists(&id), actor_count())
        })
        .join()
        .unwrap();

        assert!(!visible_on_other_thread.0);
        assert_eq!(visible_on_other_thread.1, 0);
    }

    #[rstest]
    fn test_actor_ref_survives_registry_removal() {
        clear_actor_registry();

        let id = Ustr::from("removable-actor");
        let actor = TestActor { id, value: 7 };
        register_actor(actor);
        assert_eq!(actor_count(), 1);

        let mut guard = get_actor_unchecked::<TestActor>(&id);

        with_actor_registry(|registry| {
            registry.remove(&id);
        });
        assert!(!actor_exists(&id));
        assert_eq!(actor_count(), 0);

        assert_eq!(guard.value, 7);
        guard.value = 99;
        assert_eq!(guard.value, 99);
    }

    #[rstest]
    fn test_actor_ref_survives_same_id_replacement() {
        clear_actor_registry();

        let id = Ustr::from("replaceable-actor");
        let actor_a = TestActor { id, value: 1 };
        register_actor(actor_a);

        let guard_a = get_actor_unchecked::<TestActor>(&id);
        assert_eq!(guard_a.value, 1);

        let actor_b = TestActor { id, value: 2 };
        register_actor(actor_b);

        // Old guard still sees actor A
        assert_eq!(guard_a.value, 1);

        // Fresh lookup sees actor B
        let guard_b = get_actor_unchecked::<TestActor>(&id);
        assert_eq!(guard_b.value, 2);
        assert_eq!(actor_count(), 1);
    }

    #[should_panic(expected = "Actor type mismatch")]
    #[rstest]
    fn test_get_actor_unchecked_panics_on_type_mismatch() {
        #[derive(Debug)]
        struct OtherActor {
            id: Ustr,
        }

        impl Actor for OtherActor {
            fn id(&self) -> Ustr {
                self.id
            }
            fn handle(&mut self, _msg: &dyn Any) {}
            fn as_any(&self) -> &dyn Any {
                self
            }
        }

        clear_actor_registry();

        let id = Ustr::from("typed-actor");
        let actor = OtherActor { id };
        register_actor(actor);

        let _guard = get_actor_unchecked::<TestActor>(&id);
    }
}
