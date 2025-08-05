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
    cell::{RefCell, UnsafeCell},
    fmt::Debug,
    rc::Rc,
};

use ahash::{HashMap, HashMapExt};
use ustr::Ustr;

use super::Actor;

thread_local! {
    static ACTOR_REGISTRY: ActorRegistry = ActorRegistry::new();
}

/// Registry for storing actors.
pub struct ActorRegistry {
    actors: RefCell<HashMap<Ustr, Rc<UnsafeCell<dyn Actor>>>>,
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
            actors: RefCell::new(HashMap::new()),
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

    // Register as Actor (message handling and lifecycle management)
    let actor_trait_ref: Rc<UnsafeCell<dyn Actor>> = actor_ref.clone();
    get_actor_registry().insert(actor_id, actor_trait_ref);

    actor_ref
}

/// Registers an actor by reference without consuming it.
///
/// This is useful for actors that are owned by other structures (like Python instances)
/// but still need to be registered in the global actor registry for lifecycle management.
///
/// # Safety
///
/// The caller must ensure that the referenced actor remains valid for the lifetime
/// of the registration. The actor should not be dropped while registered.
/// This creates an aliasing raw pointer which could lead to undefined behavior if
/// the original actor is mutated while the registry holds a reference.
pub unsafe fn register_actor_by_ref<T>(actor: &T)
where
    T: Actor + 'static,
{
    let actor_id = actor.id();

    // SAFETY: We create an Rc<UnsafeCell<T>> pointing to the same memory as the actor.
    // This is extremely unsafe as it creates aliasing mutable references.
    // The caller must ensure no mutation happens while registered.
    let actor_ptr = actor as *const T as *mut T;
    let actor_ref = Rc::new(UnsafeCell::new(unsafe {
        std::ptr::read_unaligned(actor_ptr)
    }));

    // Register in actor registry
    let actor_trait_ref: Rc<UnsafeCell<dyn Actor>> = actor_ref.clone();
    get_actor_registry().insert(actor_id, actor_trait_ref);
}

pub fn get_actor(id: &Ustr) -> Option<Rc<UnsafeCell<dyn Actor>>> {
    get_actor_registry().get(id)
}

/// Returns a mutable reference to the registered actor of type `T` for the `id`.
///
/// # Safety
///
/// This function bypasses Rust's borrow checker and type safety.
/// Caller must ensure:
/// - Actor with `id` exists in registry.
/// - No other mutable references to the same actor exist.
/// - Type `T` matches the actual actor type.
///
/// # Panics
///
/// Panics if no actor with the specified `id` is found in the registry.
#[allow(clippy::mut_from_ref)]
pub fn get_actor_unchecked<T: Actor>(id: &Ustr) -> &mut T {
    let actor = get_actor(id).unwrap_or_else(|| panic!("Actor for {id} not found"));
    // SAFETY: Caller must ensure no aliasing and correct type
    unsafe { &mut *(actor.get() as *mut _ as *mut T) }
}

/// Safely attempts to get a mutable reference to the registered actor.
///
/// Returns `None` if the actor is not found, avoiding panics.
#[allow(clippy::mut_from_ref)]
pub fn try_get_actor_unchecked<T: Actor>(id: &Ustr) -> Option<&mut T> {
    let actor = get_actor(id)?;
    // SAFETY: Registry guarantees valid actor pointers
    Some(unsafe { &mut *(actor.get() as *mut _ as *mut T) })
}

/// Checks if an actor with the `id` exists in the registry.
pub fn actor_exists(id: &Ustr) -> bool {
    get_actor_registry().contains(id)
}

/// Returns the number of registered actors.
pub fn actor_count() -> usize {
    get_actor_registry().len()
}

// Lifecycle management functions for actors

/// Safely calls start() on an actor in the global registry.
///
/// # Errors
///
/// Returns an error if the actor is not found or if start() fails.
pub fn start_actor(id: &Ustr) -> anyhow::Result<()> {
    if let Some(actor_ref) = get_actor_registry().get(id) {
        // SAFETY: We have exclusive access to the actor and are calling start() which takes &mut self
        unsafe {
            let actor = &mut *actor_ref.get();
            actor.start()
        }
    } else {
        anyhow::bail!("Actor '{id}' not found in global registry");
    }
}

/// Safely calls stop() on an actor in the global registry.
///
/// # Errors
///
/// Returns an error if the actor is not found or if stop() fails.
pub fn stop_actor(id: &Ustr) -> anyhow::Result<()> {
    if let Some(actor_ref) = get_actor_registry().get(id) {
        unsafe {
            let actor = &mut *actor_ref.get();
            actor.stop()
        }
    } else {
        anyhow::bail!("Actor '{id}' not found in global registry");
    }
}

/// Safely calls reset() on an actor in the global registry.
///
/// # Errors
///
/// Returns an error if the actor is not found or if reset() fails.
pub fn reset_actor(id: &Ustr) -> anyhow::Result<()> {
    if let Some(actor_ref) = get_actor_registry().get(id) {
        unsafe {
            let actor = &mut *actor_ref.get();
            actor.reset()
        }
    } else {
        anyhow::bail!("Actor '{id}' not found in global registry");
    }
}

/// Safely calls dispose() on an actor in the global registry.
///
/// # Errors
///
/// Returns an error if the actor is not found or if dispose() fails.
pub fn dispose_actor(id: &Ustr) -> anyhow::Result<()> {
    if let Some(actor_ref) = get_actor_registry().get(id) {
        unsafe {
            let actor = &mut *actor_ref.get();
            actor.dispose()
        }
    } else {
        anyhow::bail!("Actor '{id}' not found in global registry");
    }
}

#[cfg(test)]
/// Clears the actor registry (for test isolation).
pub fn clear_actor_registry() {
    // SAFETY: Clearing registry actors; tests should run single-threaded for actor registry
    get_actor_registry().actors.borrow_mut().clear();
}
