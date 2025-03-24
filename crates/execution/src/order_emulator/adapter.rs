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
    cell::{Ref, RefCell, RefMut},
    rc::Rc,
};

use nautilus_common::{
    cache::Cache,
    clock::Clock,
    msgbus::{handler::ShareableMessageHandler, register},
};
use nautilus_core::UUID4;
use ustr::Ustr;

use crate::{
    messages::{
        cancel::CancelOrderHandlerAny, modify::ModifyOrderHandlerAny, submit::SubmitOrderHandlerAny,
    },
    order_emulator::{
        emulator::OrderEmulator,
        handlers::{OrderEmulatorExecuteHandler, OrderEmulatorOnEventHandler},
    },
};

pub struct OrderEmulatorAdapter {
    emulator: Rc<RefCell<OrderEmulator>>,
}

impl OrderEmulatorAdapter {
    pub fn new(clock: Rc<RefCell<dyn Clock>>, cache: Rc<RefCell<Cache>>) -> Self {
        let emulator = Rc::new(RefCell::new(OrderEmulator::new(clock, cache)));

        Self::initialize_execute_handler(emulator.clone());
        Self::initialize_on_event_handler(emulator.clone());
        Self::initialize_submit_order_handler(emulator.clone());
        Self::initialize_cancel_order_handler(emulator.clone());
        Self::initialize_modify_order_handler(emulator.clone());

        Self { emulator }
    }

    fn initialize_submit_order_handler(emulator: Rc<RefCell<OrderEmulator>>) {
        let handler = SubmitOrderHandlerAny::OrderEmulator(emulator.clone());
        emulator.borrow_mut().set_submit_order_handler(handler);
    }

    fn initialize_cancel_order_handler(emulator: Rc<RefCell<OrderEmulator>>) {
        let handler = CancelOrderHandlerAny::OrderEmulator(emulator.clone());
        emulator.borrow_mut().set_cancel_order_handler(handler);
    }

    fn initialize_modify_order_handler(emulator: Rc<RefCell<OrderEmulator>>) {
        let handler = ModifyOrderHandlerAny::OrderEmulator(emulator.clone());
        emulator.borrow_mut().set_modify_order_handler(handler);
    }

    fn initialize_execute_handler(emulator: Rc<RefCell<OrderEmulator>>) {
        let handler = ShareableMessageHandler(Rc::new(OrderEmulatorExecuteHandler {
            id: Ustr::from(&UUID4::new().to_string()),
            emulator,
        }));

        register("OrderEmulator.execute", handler);
    }

    fn initialize_on_event_handler(emulator: Rc<RefCell<OrderEmulator>>) {
        let handler = ShareableMessageHandler(Rc::new(OrderEmulatorOnEventHandler {
            id: Ustr::from(&UUID4::new().to_string()),
            emulator,
        }));

        register("OrderEmulator.on_event", handler);
    }

    #[must_use]
    pub fn get_emulator(&self) -> Ref<OrderEmulator> {
        self.emulator.borrow()
    }

    #[must_use]
    pub fn get_emulator_mut(&self) -> RefMut<OrderEmulator> {
        self.emulator.borrow_mut()
    }
}
