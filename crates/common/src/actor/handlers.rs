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

use nautilus_model::{data::Data, instruments::InstrumentAny};
use ustr::Ustr;
use uuid::Uuid;

use crate::{messages::data::DataResponse, msgbus::handler::MessageHandler};

type CustomDataCallback = Box<dyn Fn(&dyn Any) + 'static>;
type InstrumentCallback = Box<dyn Fn(&InstrumentAny)>;
type InstrumentsCallback = Box<dyn Fn(&Vec<InstrumentAny>)>;

fn generate_unique_handler_id() -> Ustr {
    Ustr::from(&Uuid::new_v4().to_string())
}

// TODO: Revisiting this handler pattern is becoming a priority

pub(crate) struct HandleData {
    pub(crate) id: Ustr,
    pub(crate) callback: CustomDataCallback,
}

impl HandleData {
    /// Creates a new [`HandleData`] instance with an automatically generated ID.
    pub fn new(callback: CustomDataCallback) -> Self {
        Self {
            id: generate_unique_handler_id(),
            callback,
        }
    }
}

impl MessageHandler for HandleData {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, msg: &dyn Any) {
        (self.callback)(msg);
    }
    fn handle_response(&self, _resp: DataResponse) {}
    fn handle_data(&self, _data: Data) {}
    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub(crate) struct HandleInstrument {
    pub(crate) id: Ustr,
    pub(crate) callback: InstrumentCallback,
}

impl HandleInstrument {
    /// Creates a new [`HandleInstruments`] instance with an automatically generated ID.
    pub fn new(callback: InstrumentCallback) -> Self {
        Self {
            id: generate_unique_handler_id(),
            callback,
        }
    }
}

impl MessageHandler for HandleInstrument {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, msg: &dyn Any) {
        (self.callback)(msg.downcast_ref::<&InstrumentAny>().unwrap());
    }
    fn handle_response(&self, _resp: DataResponse) {}
    fn handle_data(&self, _data: Data) {}
    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub(crate) struct HandleInstruments {
    pub(crate) id: Ustr,
    pub(crate) callback: InstrumentsCallback,
}

impl HandleInstruments {
    /// Creates a new [`HandleInstruments`] instance with an automatically generated ID.
    pub fn new(callback: InstrumentsCallback) -> Self {
        Self {
            id: generate_unique_handler_id(),
            callback,
        }
    }
}

impl MessageHandler for HandleInstruments {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, msg: &dyn Any) {
        (self.callback)(msg.downcast_ref::<Vec<InstrumentAny>>().unwrap());
    }
    fn handle_response(&self, _resp: DataResponse) {}
    fn handle_data(&self, _data: Data) {}
    fn as_any(&self) -> &dyn Any {
        self
    }
}
