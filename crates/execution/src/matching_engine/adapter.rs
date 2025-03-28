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
    matching_core::handlers::{
        FillLimitOrderHandlerAny, FillMarketOrderHandlerAny, ShareableFillLimitOrderHandler,
        ShareableFillMarketOrderHandler, ShareableTriggerStopOrderHandler,
        TriggerStopOrderHandlerAny,
    },
    matching_engine::{config::OrderMatchingEngineConfig, engine::OrderMatchingEngine},
    models::{fee::FeeModelAny, fill::FillModel},
};

pub struct OrderEngineAdapter {
    engine: Rc<RefCell<OrderMatchingEngine>>,
}

impl OrderEngineAdapter {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument: InstrumentAny,
        raw_id: u32,
        fill_model: FillModel,
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

        Self::initialize_fill_order_handler(engine.clone());
        Self::initialize_fill_market_order_handler(engine.clone());
        Self::initialize_trigger_stop_order_handler(engine.clone());

        Self { engine }
    }

    fn initialize_fill_order_handler(engine: Rc<RefCell<OrderMatchingEngine>>) {
        let handler = ShareableFillLimitOrderHandler(
            FillLimitOrderHandlerAny::OrderMatchingEngine(engine.clone()),
        );
        engine
            .borrow_mut()
            .core
            .set_fill_limit_order_handler(handler);
    }

    fn initialize_fill_market_order_handler(engine: Rc<RefCell<OrderMatchingEngine>>) {
        let handler = ShareableFillMarketOrderHandler(
            FillMarketOrderHandlerAny::OrderMatchingEngine(engine.clone()),
        );
        engine
            .borrow_mut()
            .core
            .set_fill_market_order_handler(handler);
    }

    fn initialize_trigger_stop_order_handler(engine: Rc<RefCell<OrderMatchingEngine>>) {
        let handler = ShareableTriggerStopOrderHandler(
            TriggerStopOrderHandlerAny::OrderMatchingEngine(engine.clone()),
        );
        engine
            .borrow_mut()
            .core
            .set_trigger_stop_order_handler(handler);
    }

    #[must_use]
    pub fn get_engine(&self) -> Ref<OrderMatchingEngine> {
        self.engine.borrow()
    }

    #[must_use]
    pub fn get_engine_mut(&self) -> RefMut<OrderMatchingEngine> {
        self.engine.borrow_mut()
    }
}
