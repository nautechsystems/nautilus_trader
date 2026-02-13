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

use nautilus_common::{
    cache::Cache,
    clock::Clock,
    msgbus::{
        MessagingSwitchboard, TypedHandler, TypedIntoHandler, register_trading_command_endpoint,
    },
};
use nautilus_core::{UUID4, WeakCell};
use nautilus_model::identifiers::TraderId;
use ustr::Ustr;

use crate::{
    order_emulator::{emulator::OrderEmulator, handlers::OrderEmulatorOnEventHandler},
    order_manager::handlers::{
        CancelOrderHandlerAny, ModifyOrderHandlerAny, SubmitOrderHandlerAny,
    },
};

#[derive(Debug)]
pub struct OrderEmulatorAdapter {
    emulator: Rc<RefCell<OrderEmulator>>,
}

impl OrderEmulatorAdapter {
    /// Creates a new [`OrderEmulatorAdapter`] instance.
    ///
    /// # Panics
    ///
    /// Panics if registration with the actor system fails.
    pub fn new(
        trader_id: TraderId,
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
    ) -> Self {
        let emulator = Rc::new(RefCell::new(OrderEmulator::new(
            clock.clone(),
            cache.clone(),
        )));

        emulator
            .borrow_mut()
            .register(trader_id, clock, cache)
            .expect("Failed to register OrderEmulator");

        // Set self-reference for subscription handlers
        Self::initialize_self_ref(emulator.clone());

        Self::initialize_execute_handler(emulator.clone());
        Self::initialize_on_event_handler(emulator.clone());
        Self::initialize_submit_order_handler(emulator.clone());
        Self::initialize_cancel_order_handler(emulator.clone());
        Self::initialize_modify_order_handler(emulator.clone());

        Self { emulator }
    }

    fn initialize_self_ref(emulator: Rc<RefCell<OrderEmulator>>) {
        let self_ref = WeakCell::from(Rc::downgrade(&emulator));
        emulator.borrow_mut().set_self_ref(self_ref);
    }

    fn initialize_submit_order_handler(emulator: Rc<RefCell<OrderEmulator>>) {
        let handler =
            SubmitOrderHandlerAny::OrderEmulator(WeakCell::from(Rc::downgrade(&emulator)));
        emulator.borrow_mut().set_submit_order_handler(handler);
    }

    fn initialize_cancel_order_handler(emulator: Rc<RefCell<OrderEmulator>>) {
        let handler =
            CancelOrderHandlerAny::OrderEmulator(WeakCell::from(Rc::downgrade(&emulator)));
        emulator.borrow_mut().set_cancel_order_handler(handler);
    }

    fn initialize_modify_order_handler(emulator: Rc<RefCell<OrderEmulator>>) {
        let handler =
            ModifyOrderHandlerAny::OrderEmulator(WeakCell::from(Rc::downgrade(&emulator)));
        emulator.borrow_mut().set_modify_order_handler(handler);
    }

    fn initialize_execute_handler(emulator: Rc<RefCell<OrderEmulator>>) {
        let emulator_weak = WeakCell::from(Rc::downgrade(&emulator));
        let handler = TypedIntoHandler::from(move |cmd| {
            if let Some(emulator_rc) = emulator_weak.upgrade() {
                emulator_rc.borrow_mut().execute(cmd);
            }
        });

        let endpoint = MessagingSwitchboard::order_emulator_execute();
        register_trading_command_endpoint(endpoint, handler);
    }

    fn initialize_on_event_handler(emulator: Rc<RefCell<OrderEmulator>>) {
        let handler = TypedHandler::new(OrderEmulatorOnEventHandler::new(
            Ustr::from(UUID4::new().as_str()),
            WeakCell::from(Rc::downgrade(&emulator)),
        ));

        emulator.borrow_mut().set_on_event_handler(handler);
    }

    #[must_use]
    pub fn get_emulator(&self) -> Ref<'_, OrderEmulator> {
        self.emulator.borrow()
    }

    #[must_use]
    pub fn get_emulator_mut(&self) -> RefMut<'_, OrderEmulator> {
        self.emulator.borrow_mut()
    }
}
