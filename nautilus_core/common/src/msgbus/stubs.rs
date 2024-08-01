// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use nautilus_core::message::Message;
use nautilus_model::data::Data;
use ustr::Ustr;

use crate::{
    messages::data::DataResponse,
    msgbus::{MessageHandler, ShareableMessageHandler},
};

// Stub message handler which logs the data it receives
pub struct StubMessageHandler {
    id: Ustr,
    callback: Arc<dyn Fn(Message) + Send>,
}

impl MessageHandler for StubMessageHandler {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, message: &dyn Any) {
        (self.callback)(message.downcast_ref::<Message>().unwrap().clone());
    }

    fn handle_response(&self, _resp: DataResponse) {}

    fn handle_data(&self, _resp: &Data) {}

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[must_use]
pub fn get_stub_shareable_handler(id: Ustr) -> ShareableMessageHandler {
    ShareableMessageHandler(Rc::new(StubMessageHandler {
        id,
        callback: Arc::new(|m: Message| {
            format!("{m:?}");
        }),
    }))
}

// Stub message handler which checks if handle was called
pub struct CallCheckMessageHandler {
    id: Ustr,
    called: Arc<AtomicBool>,
}

impl CallCheckMessageHandler {
    #[must_use]
    pub fn was_called(&self) -> bool {
        self.called.load(Ordering::SeqCst)
    }
}

impl MessageHandler for CallCheckMessageHandler {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, _message: &dyn Any) {
        self.called.store(true, Ordering::SeqCst);
    }

    fn handle_response(&self, _resp: DataResponse) {}

    fn handle_data(&self, _resp: &Data) {}

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[must_use]
pub fn get_call_check_shareable_handler(id: Ustr) -> ShareableMessageHandler {
    ShareableMessageHandler(Rc::new(CallCheckMessageHandler {
        id,
        called: Arc::new(AtomicBool::new(false)),
    }))
}
