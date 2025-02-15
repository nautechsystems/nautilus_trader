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

//! Message handlers for the Portfolio.

use std::any::Any;

use nautilus_common::{messages::data::DataResponse, msgbus::handler::MessageHandler};
use nautilus_model::{
    data::{Bar, Data, QuoteTick},
    events::{position::PositionEvent, AccountState, OrderEventAny},
};
use ustr::Ustr;

pub(crate) struct UpdateQuoteTickHandler {
    pub(crate) id: Ustr,
    pub(crate) callback: Box<dyn Fn(&QuoteTick)>,
}

impl MessageHandler for UpdateQuoteTickHandler {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, msg: &dyn Any) {
        (self.callback)(msg.downcast_ref::<&QuoteTick>().unwrap());
    }
    fn handle_response(&self, _resp: DataResponse) {}
    fn handle_data(&self, _data: Data) {}
    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub(crate) struct UpdateBarHandler {
    pub(crate) id: Ustr,
    pub(crate) callback: Box<dyn Fn(&Bar)>,
}

impl MessageHandler for UpdateBarHandler {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, msg: &dyn Any) {
        (self.callback)(msg.downcast_ref::<&Bar>().unwrap());
    }
    fn handle_response(&self, _resp: DataResponse) {}
    fn handle_data(&self, _data: Data) {}
    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub(crate) struct UpdateOrderHandler {
    pub(crate) id: Ustr,
    pub(crate) callback: Box<dyn Fn(&OrderEventAny)>,
}

impl MessageHandler for UpdateOrderHandler {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, msg: &dyn Any) {
        (self.callback)(msg.downcast_ref::<&OrderEventAny>().unwrap());
    }
    fn handle_response(&self, _resp: DataResponse) {}
    fn handle_data(&self, _data: Data) {}
    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub(crate) struct UpdatePositionHandler {
    pub(crate) id: Ustr,
    pub(crate) callback: Box<dyn Fn(&PositionEvent)>,
}

impl MessageHandler for UpdatePositionHandler {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, msg: &dyn Any) {
        (self.callback)(msg.downcast_ref::<&PositionEvent>().unwrap());
    }
    fn handle_response(&self, _resp: DataResponse) {}
    fn handle_data(&self, _data: Data) {}
    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub(crate) struct UpdateAccountHandler {
    pub(crate) id: Ustr,
    pub(crate) callback: Box<dyn Fn(&AccountState)>,
}

impl MessageHandler for UpdateAccountHandler {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, msg: &dyn Any) {
        (self.callback)(msg.downcast_ref::<&AccountState>().unwrap());
    }
    fn handle_response(&self, _resp: DataResponse) {}
    fn handle_data(&self, _data: Data) {}
    fn as_any(&self) -> &dyn Any {
        self
    }
}
