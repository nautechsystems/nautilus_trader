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

use std::{any::Any, collections::VecDeque};

use nautilus_common::{messages::execution::TradingCommand, msgbus::Handler};
use nautilus_core::WeakCell;
use nautilus_model::events::OrderEventAny;
use ustr::Ustr;

use super::{PendingMessage, emulator::OrderEmulator};

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

impl Handler<dyn Any> for OrderEmulatorExecuteHandler {
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
}

#[derive(Debug)]
pub struct OrderEmulatorOnEventHandler {
    id: Ustr,
    emulator: WeakCell<OrderEmulator>,
    pending_messages: WeakCell<VecDeque<PendingMessage>>,
}

impl OrderEmulatorOnEventHandler {
    #[inline]
    #[must_use]
    pub(crate) const fn new(
        id: Ustr,
        emulator: WeakCell<OrderEmulator>,
        pending_messages: WeakCell<VecDeque<PendingMessage>>,
    ) -> Self {
        Self {
            id,
            emulator,
            pending_messages,
        }
    }
}

impl Handler<OrderEventAny> for OrderEmulatorOnEventHandler {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, event: &OrderEventAny) {
        if let Some(emulator) = self.emulator.upgrade() {
            match emulator.try_borrow_mut() {
                Ok(mut emulator) => emulator.on_event(event),
                Err(_) => {
                    // The emulator published this event while handling another
                    // call; defer it so contingency handling is not dropped.
                    if let Some(pending) = self.pending_messages.upgrade() {
                        pending
                            .borrow_mut()
                            .push_back(PendingMessage::Event(Box::new(event.clone())));
                    }
                }
            }
        }
    }
}
