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
use crate::component::Component;

// TODO: Currently using two different registeries to satsify the two traits
// this could potentially be unified but this works for now

thread_local! {
    static ACTOR_REGISTRY: ActorRegistry = ActorRegistry::new();
    static COMPONENT_REGISTRY: ComponentRegistry = ComponentRegistry::new();
}

/// Registry for storing components.
pub struct ComponentRegistry {
    components: RefCell<HashMap<Ustr, Rc<UnsafeCell<dyn Component>>>>,
}

impl Debug for ComponentRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let components_ref = self.components.borrow();
        let keys: Vec<&Ustr> = components_ref.keys().collect();
        f.debug_struct(stringify!(ComponentRegistry))
            .field("components", &keys)
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
            components: RefCell::new(HashMap::new()),
        }
    }

    pub fn insert(&self, id: Ustr, component: Rc<UnsafeCell<dyn Component>>) {
        self.components.borrow_mut().insert(id, component);
    }

    pub fn get(&self, id: &Ustr) -> Option<Rc<UnsafeCell<dyn Component>>> {
        self.components.borrow().get(id).cloned()
    }
}

/// Registry for storing actors that don't implement Component.
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

pub fn get_actor_registry() -> &'static ActorRegistry {
    ACTOR_REGISTRY.with(|registry| unsafe {
        // SAFETY: We return a static reference that lives for the lifetime of the thread.
        // Since this is thread_local storage, each thread has its own instance.
        std::mem::transmute::<&ActorRegistry, &'static ActorRegistry>(registry)
    })
}

pub fn get_component_registry() -> &'static ComponentRegistry {
    COMPONENT_REGISTRY.with(|registry| unsafe {
        // SAFETY: We return a static reference that lives for the lifetime of the thread.
        // Since this is thread_local storage, each thread has its own instance.
        std::mem::transmute::<&ComponentRegistry, &'static ComponentRegistry>(registry)
    })
}

/// Registers a component.
///
/// Since Component extends Actor, this handles both message handling and lifecycle management.
pub fn register_component<T>(component: T) -> Rc<UnsafeCell<T>>
where
    T: Component + 'static,
{
    let actor_id = component.id();
    let component_ref = Rc::new(UnsafeCell::new(component));

    // Register as Component (provides both Actor and Component interfaces)
    let component_trait_ref: Rc<UnsafeCell<dyn Component>> = component_ref.clone();
    get_component_registry().insert(actor_id, component_trait_ref);

    component_ref
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

/// Safely calls start() on a component in the global registry.
///
/// # Errors
///
/// Returns an error if the component is not found or if start() fails.
pub fn start_component(id: &Ustr) -> anyhow::Result<()> {
    if let Some(component_ref) = get_component_registry().get(id) {
        // SAFETY: We have exclusive access to the component and are calling start() which takes &mut self
        unsafe {
            let component = &mut *component_ref.get();
            component.start()
        }
    } else {
        anyhow::bail!("Component '{id}' not found in global registry");
    }
}

/// Safely calls stop() on a component in the global registry.
///
/// # Errors
///
/// Returns an error if the component is not found or if stop() fails.
pub fn stop_component(id: &Ustr) -> anyhow::Result<()> {
    if let Some(component_ref) = get_component_registry().get(id) {
        unsafe {
            let component = &mut *component_ref.get();
            component.stop()
        }
    } else {
        anyhow::bail!("Component '{id}' not found in global registry");
    }
}

/// Safely calls reset() on a component in the global registry.
///
/// # Errors
///
/// Returns an error if the component is not found or if reset() fails.
pub fn reset_component(id: &Ustr) -> anyhow::Result<()> {
    if let Some(component_ref) = get_component_registry().get(id) {
        unsafe {
            let component = &mut *component_ref.get();
            component.reset()
        }
    } else {
        anyhow::bail!("Component '{id}' not found in global registry");
    }
}

/// Safely calls dispose() on a component in the global registry.
///
/// # Errors
///
/// Returns an error if the component is not found or if dispose() fails.
pub fn dispose_component(id: &Ustr) -> anyhow::Result<()> {
    if let Some(component_ref) = get_component_registry().get(id) {
        unsafe {
            let component = &mut *component_ref.get();
            component.dispose()
        }
    } else {
        anyhow::bail!("Component '{id}' not found in global registry");
    }
}

pub fn get_actor(id: &Ustr) -> Option<Rc<UnsafeCell<dyn Actor>>> {
    // First check ComponentRegistry (since Component extends Actor)
    if let Some(component) = get_component_registry().get(id) {
        // Cast Component to Actor
        let actor: Rc<UnsafeCell<dyn Actor>> = component;
        Some(actor)
    } else {
        // Fallback to ActorRegistry for pure actors
        get_actor_registry().get(id)
    }
}

pub fn get_component(id: &Ustr) -> Option<Rc<UnsafeCell<dyn Component>>> {
    get_component_registry().get(id)
}

/// Returns a mutable reference to the registered actor of type `T` for the given `id`.
///
/// This searches both ComponentRegistry (for components) and ActorRegistry (for pure actors).
///
/// # Panics
///
/// Panics if no actor with the specified `id` is found in either registry.
#[allow(clippy::mut_from_ref)]
pub fn get_actor_unchecked<T: Actor>(id: &Ustr) -> &mut T {
    let actor = get_actor(id).unwrap_or_else(|| panic!("Actor for {id} not found"));
    unsafe { &mut *(actor.get() as *mut _ as *mut T) }
}

// Clears both global registries (for test isolation).
#[cfg(test)]
pub fn clear_actor_registry() {
    // SAFETY: Clearing registry actors; tests should run single-threaded for actor registry
    get_actor_registry().actors.borrow_mut().clear();
    get_component_registry().components.borrow_mut().clear();
}
