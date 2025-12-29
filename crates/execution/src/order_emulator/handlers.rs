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

use std::any::Any;

use nautilus_common::{messages::execution::TradingCommand, msgbus::handler::MessageHandler};
use nautilus_core::WeakCell;
use nautilus_model::events::OrderEventAny;
use ustr::Ustr;

use super::emulator::OrderEmulator;

#[derive(Debug)]
pub struct OrderEmulatorExecuteHandler {
    id: Ustr,
    emulator: WeakCell<OrderEmulator>,
}

impl OrderEmulatorExecuteHandler {
    #[inline]
    #[must_use]
    pub const fn new(id: Ustr, emulator: WeakCell<OrderEmulator>) -> Self {
        Self { id, emulator }
    }
}

impl MessageHandler for OrderEmulatorExecuteHandler {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, msg: &dyn Any) {
        if let Some(emulator) = self.emulator.upgrade() {
            if let Some(command) = msg.downcast_ref::<TradingCommand>() {
                emulator.borrow_mut().execute(command.clone());
            } else {
                log::error!("OrderEmulator received unexpected message type");
            }
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Debug)]
pub struct OrderEmulatorOnEventHandler {
    id: Ustr,
    emulator: WeakCell<OrderEmulator>,
}

impl OrderEmulatorOnEventHandler {
    #[inline]
    #[must_use]
    pub const fn new(id: Ustr, emulator: WeakCell<OrderEmulator>) -> Self {
        Self { id, emulator }
    }
}

impl MessageHandler for OrderEmulatorOnEventHandler {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, msg: &dyn Any) {
        if let Some(emulator) = self.emulator.upgrade() {
            if let Some(event) = msg.downcast_ref::<OrderEventAny>() {
                emulator.borrow_mut().on_event(event.clone());
            } else {
                log::error!("OrderEmulator on_event received unexpected message type");
            }
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
