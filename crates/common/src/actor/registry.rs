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

//! Thread-local actor registry with lifetime-safe access guards.
//!
//! # Design
//!
//! The actor registry stores actors in thread-local storage and provides access via
//! [`ActorRef<T>`] guards. This design addresses several constraints:
//!
//! - **Use-after-free prevention**: `ActorRef` holds an `Rc` clone, keeping the actor
//!   alive even if removed from the registry while the guard exists.
//! - **Re-entrant callbacks**: Message handlers frequently call back into the registry
//!   to access other actors. Unlike `RefCell`-style borrow tracking, multiple `ActorRef`
//!   guards can exist simultaneously without panicking.
//! - **No `'static` lifetime lie**: Previous designs returned `&'static mut T`, which
//!   didn't reflect actual validity. The guard-based approach ties the borrow to the
//!   guard's lifetime.
//!
//! # Limitations
//!
//! - **Aliasing not prevented**: Two guards can exist for the same actor simultaneously,
//!   allowing aliased mutable access. This is technically undefined behavior but is
//!   required by the re-entrant callback pattern. Higher-level discipline is required.
//! - **Thread-local only**: Guards must not be sent across threads.

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
/// This guard holds an `Rc` reference to keep the actor alive, preventing
/// use-after-free if the actor is removed from the registry while the guard
/// exists. The guard implements `Deref` and `DerefMut` for ergonomic access.
///
/// # Safety
///
/// While this guard prevents use-after-free from registry removal, it does not
/// prevent aliasing. Multiple `ActorRef` instances can exist for the same actor
/// simultaneously, which is technically undefined behavior but is required by
/// the re-entrant callback pattern in this codebase.
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
        // SAFETY: Type was verified at construction time
        unsafe { &*(self.actor_rc.get() as *const T) }
    }
}

impl<T: Actor> DerefMut for ActorRef<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: Type was verified at construction time
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

pub fn get_actor_registry() -> &'static ActorRegistry {
    ACTOR_REGISTRY.with(|registry| unsafe {
        // SAFETY: We return a static reference that lives for the lifetime of the thread.
        // Since this is thread_local storage, each thread has its own instance.
        // The transmute extends the lifetime to 'static which is safe because
        // thread_local ensures the registry lives for the thread's entire lifetime.
        std::mem::transmute::<&ActorRegistry, &'static ActorRegistry>(registry)
    })
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
    get_actor_registry().insert(actor_id, actor_trait_ref);

    actor_ref
}

pub fn get_actor(id: &Ustr) -> Option<Rc<UnsafeCell<dyn Actor>>> {
    get_actor_registry().get(id)
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
///
/// # Safety
///
/// While this function is not marked `unsafe`, aliasing constraints apply:
///
/// - **Aliasing**: The caller should ensure no other mutable references to the same
///   actor exist simultaneously. The callback-based message handling pattern in this
///   codebase requires re-entrant access, which technically violates this invariant.
/// - **Thread safety**: The registry is thread-local; do not send guards across
///   threads.
#[must_use]
pub fn get_actor_unchecked<T: Actor>(id: &Ustr) -> ActorRef<T> {
    let registry = get_actor_registry();
    let actor_rc = registry
        .get(id)
        .unwrap_or_else(|| panic!("Actor for {id} not found"));

    // SAFETY: Get a reference to check the type before casting
    let actor_ref = unsafe { &*actor_rc.get() };
    let actual_type = actor_ref.as_any().type_id();
    let expected_type = TypeId::of::<T>();

    assert!(
        actual_type == expected_type,
        "Actor type mismatch for '{id}': expected {expected_type:?}, found {actual_type:?}"
    );

    ActorRef {
        actor_rc,
        _marker: PhantomData,
    }
}

/// Attempts to get a guard providing mutable access to the registered actor.
///
/// Returns `None` if the actor is not found or the type doesn't match.
///
/// # Safety
///
/// See [`get_actor_unchecked`] for safety requirements. The same aliasing
/// and thread-safety constraints apply.
#[must_use]
pub fn try_get_actor_unchecked<T: Actor>(id: &Ustr) -> Option<ActorRef<T>> {
    let registry = get_actor_registry();
    let actor_rc = registry.get(id)?;

    // SAFETY: Get a reference to check the type before casting
    let actor_ref = unsafe { &*actor_rc.get() };
    let actual_type = actor_ref.as_any().type_id();
    let expected_type = TypeId::of::<T>();

    if actual_type != expected_type {
        return None;
    }

    Some(ActorRef {
        actor_rc,
        _marker: PhantomData,
    })
}

/// Checks if an actor with the `id` exists in the registry.
pub fn actor_exists(id: &Ustr) -> bool {
    get_actor_registry().contains(id)
}

/// Returns the number of registered actors.
pub fn actor_count() -> usize {
    get_actor_registry().len()
}

#[cfg(test)]
/// Clears the actor registry (for test isolation).
pub fn clear_actor_registry() {
    let registry = get_actor_registry();
    registry.actors.borrow_mut().clear();
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
}
