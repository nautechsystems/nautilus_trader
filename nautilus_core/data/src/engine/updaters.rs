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

use std::{any::Any, cell::RefCell, rc::Rc};

use nautilus_common::{
    cache::Cache, messages::data::DataResponse, msgbus::handler::MessageHandler,
};
use nautilus_model::{data::Data, identifiers::InstrumentId};
use ustr::Ustr;

pub struct BookUpdater {
    pub id: Ustr,
    pub instrument_id: InstrumentId,
    pub cache: Rc<RefCell<Cache>>,
}

impl BookUpdater {
    /// Creates a new [`BookUpdater`] instance.
    pub fn new(instrument_id: &InstrumentId, cache: Rc<RefCell<Cache>>) -> Self {
        Self {
            id: Ustr::from(&format!("{}-{}", stringify!(BookUpdater), instrument_id)),
            instrument_id: *instrument_id,
            cache,
        }
    }
}

impl MessageHandler for BookUpdater {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, message: &dyn Any) {}
    fn handle_response(&self, _resp: DataResponse) {}
    fn handle_data(&self, data: Data) {
        if let Some(book) = self.cache.borrow_mut().order_book(&data.instrument_id()) {
            match data {
                Data::Delta(delta) => book.apply_delta(&delta),
                Data::Deltas(deltas) => book.apply_deltas(&deltas),
                Data::Depth10(depth) => book.apply_depth(&depth),
                _ => log::error!("Invalid data type for book update, was {data:?}"),
            }
        }
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
