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

use nautilus_common::{cache::Cache, clock::Clock};
use nautilus_model::{
    enums::{AccountType, BookType, OmsType},
    instruments::InstrumentAny,
};

use crate::{
    matching_engine::{config::OrderMatchingEngineConfig, engine::OrderMatchingEngine},
    models::{fee::FeeModelAny, fill::FillModelAny},
};

#[derive(Debug)]
pub struct OrderEngineAdapter {
    engine: Rc<RefCell<OrderMatchingEngine>>,
}

impl OrderEngineAdapter {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument: InstrumentAny,
        raw_id: u32,
        fill_model: FillModelAny,
        fee_model: FeeModelAny,
        book_type: BookType,
        oms_type: OmsType,
        account_type: AccountType,
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
        config: OrderMatchingEngineConfig,
    ) -> Self {
        let engine = Rc::new(RefCell::new(OrderMatchingEngine::new(
            instrument,
            raw_id,
            fill_model,
            fee_model,
            book_type,
            oms_type,
            account_type,
            clock,
            cache,
            config,
        )));

        Self { engine }
    }

    #[must_use]
    pub fn get_engine(&self) -> Ref<'_, OrderMatchingEngine> {
        self.engine.borrow()
    }

    #[must_use]
    pub fn get_engine_mut(&self) -> RefMut<'_, OrderMatchingEngine> {
        self.engine.borrow_mut()
    }
}
