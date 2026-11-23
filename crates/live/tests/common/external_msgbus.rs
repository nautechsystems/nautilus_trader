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
    cell::{Cell, RefCell},
    rc::Rc,
};

use nautilus_common::msgbus::{BusMessage, MessageBusExternalEgress};

pub(super) type CapturedEgressMessages = Rc<RefCell<Vec<BusMessage>>>;
pub(super) type SharedClosed = Rc<Cell<bool>>;

pub(super) struct CapturingExternalEgress {
    publications: CapturedEgressMessages,
    closed: SharedClosed,
}

impl CapturingExternalEgress {
    pub(super) fn new() -> (Self, CapturedEgressMessages, SharedClosed) {
        let publications = Rc::new(RefCell::new(Vec::new()));
        let closed = Rc::new(Cell::new(false));

        (
            Self {
                publications: publications.clone(),
                closed: closed.clone(),
            },
            publications,
            closed,
        )
    }
}

impl MessageBusExternalEgress for CapturingExternalEgress {
    fn is_closed(&self) -> bool {
        self.closed.get()
    }

    fn publish(&self, message: BusMessage) {
        self.publications.borrow_mut().push(message);
    }

    fn close(&mut self) {
        self.closed.set(true);
    }
}
