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

use std::{
    cell::{Ref, RefCell, RefMut},
    rc::Rc,
};

use nautilus_common::{cache::Cache, clock::Clock};

use crate::order_emulator::emulator::OrderEmulator;

#[derive(Debug)]
pub struct OrderEmulatorAdapter {
    emulator: Rc<RefCell<OrderEmulator>>,
}

impl OrderEmulatorAdapter {
    /// Creates a new [`OrderEmulatorAdapter`] instance.
    pub fn new(clock: Rc<RefCell<dyn Clock>>, cache: Rc<RefCell<Cache>>) -> Self {
        let emulator = Rc::new(RefCell::new(OrderEmulator::new(clock, cache)));

        Self { emulator }
    }

    #[must_use]
    pub fn get_emulator(&self) -> Ref<'_, OrderEmulator> {
        self.emulator.borrow()
    }

    #[must_use]
    pub fn get_emulator_mut(&self) -> RefMut<'_, OrderEmulator> {
        self.emulator.borrow_mut()
    }

    #[must_use]
    pub fn emulator(&self) -> Rc<RefCell<OrderEmulator>> {
        self.emulator.clone()
    }
}
