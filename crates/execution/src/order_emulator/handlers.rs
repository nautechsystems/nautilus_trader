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

use std::{any::Any, cell::RefCell, rc::Rc};

use nautilus_common::msgbus::handler::MessageHandler;
use nautilus_model::events::OrderEventAny;
use ustr::Ustr;

use crate::{messages::TradingCommand, order_emulator::emulator::OrderEmulator};

pub struct OrderEmulatorExecuteHandler {
    pub id: Ustr,
    pub emulator: Rc<RefCell<OrderEmulator>>,
}

impl MessageHandler for OrderEmulatorExecuteHandler {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, msg: &dyn Any) {
        self.emulator.borrow_mut().execute(
            msg.downcast_ref::<&TradingCommand>()
                .unwrap()
                .to_owned()
                .clone(),
        );
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub struct OrderEmulatorOnEventHandler {
    pub id: Ustr,
    pub emulator: Rc<RefCell<OrderEmulator>>,
}

impl MessageHandler for OrderEmulatorOnEventHandler {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, msg: &dyn Any) {
        self.emulator.borrow_mut().on_event(
            msg.downcast_ref::<&OrderEventAny>()
                .unwrap()
                .to_owned()
                .clone(),
        );
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
