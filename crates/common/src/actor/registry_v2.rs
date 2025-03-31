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
    any::Any,
    cell::{RefCell, UnsafeCell},
    collections::HashMap,
    rc::Rc,
    sync::OnceLock,
};

use ahash::AHashMap;
use nautilus_core::UUID4;
use nautilus_model::identifiers::ComponentId;
use ustr::Ustr;

use crate::{cache::Cache, messages::data::DataResponse, msgbus::MessageBus};

pub trait Actor: Any {
    fn id(&self) -> ComponentId;
    fn handle(&mut self, msg: &dyn Any);
    fn as_any(&self) -> &dyn Any;
}

pub struct ActorRegistry {
    actors: RefCell<AHashMap<Ustr, Rc<RefCell<dyn Actor>>>>,
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

    pub fn insert(&self, id: Ustr, actor: Rc<RefCell<dyn Actor>>) {
        self.actors.borrow_mut().insert(id, actor);
    }

    pub fn get(&self, id: &Ustr) -> Option<Rc<RefCell<dyn Actor>>> {
        self.actors.borrow().get(id).cloned()
    }
}

// SAFETY: Actor registry is not meant to be passed between threads
unsafe impl Sync for ActorRegistry {}
unsafe impl Send for ActorRegistry {}

static ACTOR_REGISTRY: OnceLock<ActorRegistry> = OnceLock::new();

pub fn get_actor_registry() -> &'static ActorRegistry {
    ACTOR_REGISTRY.get_or_init(ActorRegistry::new)
}

pub fn register_actor(actor_id: Ustr, actor: Rc<RefCell<dyn Actor>>) {
    get_actor_registry().insert(actor_id, actor);
}

pub fn get_actor(actor_id: &Ustr) -> Option<Rc<RefCell<dyn Actor>>> {
    get_actor_registry().get(actor_id)
}

pub fn get_actor_unchecked(actor_id: &Ustr) -> Rc<RefCell<dyn Actor>> {
    get_actor_registry()
        .get(actor_id)
        .expect("Actor not found for ID {actor_id}")
}
