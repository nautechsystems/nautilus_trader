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
    sync::OnceLock,
};

use ahash::{HashMap, HashMapExt};
use ustr::Ustr;

use super::Actor;

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
        self.actors.borrow_mut().insert(id, actor);
    }

    pub fn get(&self, id: &Ustr) -> Option<Rc<UnsafeCell<dyn Actor>>> {
        self.actors.borrow().get(id).cloned()
    }
}

// SAFETY: ActorRegistry uses non-thread-safe internals (Rc, RefCell, UnsafeCell).
// We mark it Sync + Send to satisfy `OnceLock<T>: Sync` for static initialization,
// but all registry operations must still occur on a single thread. Moving or accessing
// from multiple threads is undefined behavior.
unsafe impl Sync for ActorRegistry {}
unsafe impl Send for ActorRegistry {}

static ACTOR_REGISTRY: OnceLock<ActorRegistry> = OnceLock::new();

pub fn get_actor_registry() -> &'static ActorRegistry {
    ACTOR_REGISTRY.get_or_init(ActorRegistry::new)
}

pub fn register_actor(actor: Rc<UnsafeCell<dyn Actor>>) {
    // SAFETY: We only immutably borrow the actor to call `id()`,
    // which takes &self. This does not violate aliasing or mutable borrow rules.
    let actor_id = unsafe { &*actor.get() }.id();
    get_actor_registry().insert(actor_id, actor);
}

pub fn get_actor(id: &Ustr) -> Option<Rc<UnsafeCell<dyn Actor>>> {
    get_actor_registry().get(id)
}

/// Returns a mutable reference to the registered actor of type `T` for the given `id`.
///
/// # Panics
///
/// Panics if no actor with the specified `id` is found in the registry.
#[allow(clippy::mut_from_ref)]
pub fn get_actor_unchecked<T: Actor>(id: &Ustr) -> &mut T {
    let actor = get_actor(id).unwrap_or_else(|| panic!("Actor for {id} not found"));
    unsafe { &mut *(actor.get() as *mut _ as *mut T) }
}

// Clears the global actor registry (for test isolation).
#[cfg(test)]
pub fn clear_actor_registry() {
    // SAFETY: Clearing registry actors; tests should run single-threaded for actor registry
    get_actor_registry().actors.borrow_mut().clear();
}
