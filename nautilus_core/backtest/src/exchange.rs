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

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use nautilus_common::{cache::Cache, msgbus::MessageBus};
use nautilus_core::{correctness::check_equal, nanos::UnixNanos, time::AtomicTime};
use nautilus_execution::{client::ExecutionClient, messages::TradingCommand};
use nautilus_model::{
    data::{
        bar::Bar, delta::OrderBookDelta, deltas::OrderBookDeltas, quote::QuoteTick,
        status::InstrumentStatus, trade::TradeTick,
    },
    enums::{AccountType, BookType, OmsType},
    identifiers::{InstrumentId, Venue},
    instruments::any::InstrumentAny,
    types::{currency::Currency, money::Money},
};
use rust_decimal::Decimal;

use crate::{
    matching_engine::{OrderMatchingEngine, OrderMatchingEngineConfig},
    models::{fee::FeeModelAny, fill::FillModel, latency::LatencyModel},
    modules::SimulationModule,
};

pub struct SimulatedExchange {
    id: Venue,
    oms_type: OmsType,
    account_type: AccountType,
    book_type: BookType,
    default_leverage: Decimal,
    exec_client: Option<ExecutionClient>,
    fee_model: FeeModelAny,
    fill_model: FillModel,
    latency_model: LatencyModel,
    instruments: HashMap<InstrumentId, InstrumentAny>,
    matching_engines: HashMap<InstrumentId, OrderMatchingEngine>,
    leverages: HashMap<InstrumentId, Decimal>,
    modules: Vec<Box<dyn SimulationModule>>,
    clock: &'static AtomicTime,
    msgbus: Rc<RefCell<MessageBus>>,
    cache: Rc<RefCell<Cache>>,
    frozen_account: bool,
    bar_execution: bool,
    reject_stop_orders: bool,
    support_gtd_orders: bool,
    support_contingent_orders: bool,
    use_position_ids: bool,
    use_random_ids: bool,
    use_reduce_only: bool,
    use_message_queue: bool,
}

impl SimulatedExchange {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        venue: Venue,
        oms_type: OmsType,
        account_type: AccountType,
        starting_balances: Vec<Money>,
        base_currency: Option<Currency>,
        default_leverage: Decimal,
        leverages: HashMap<InstrumentId, Decimal>,
        modules: Vec<Box<dyn SimulationModule>>,
        msgbus: Rc<RefCell<MessageBus>>, // TODO add portfolio
        cache: Rc<RefCell<Cache>>,
        clock: &'static AtomicTime,
        fill_model: FillModel,
        fee_model: FeeModelAny,
        latency_model: LatencyModel,
        book_type: BookType,
        frozen_account: Option<bool>,
        bar_execution: Option<bool>,
        reject_stop_orders: Option<bool>,
        support_gtd_orders: Option<bool>,
        support_contingent_orders: Option<bool>,
        use_position_ids: Option<bool>,
        use_random_ids: Option<bool>,
        use_reduce_only: Option<bool>,
        use_message_queue: Option<bool>,
    ) -> anyhow::Result<Self> {
        if starting_balances.is_empty() {
            anyhow::bail!("Starting balances must be provided")
        }
        if base_currency.is_some() && starting_balances.len() > 1 {
            anyhow::bail!("single-currency account has multiple starting currencies")
        }
        // TODO register and load modules
        Ok(Self {
            id: venue,
            oms_type,
            account_type,
            book_type,
            default_leverage,
            exec_client: None,
            fee_model,
            fill_model,
            latency_model,
            instruments: HashMap::new(),
            matching_engines: HashMap::new(),
            leverages,
            modules,
            clock,
            msgbus,
            cache,
            frozen_account: frozen_account.unwrap_or(false),
            bar_execution: bar_execution.unwrap_or(true),
            reject_stop_orders: reject_stop_orders.unwrap_or(true),
            support_gtd_orders: support_gtd_orders.unwrap_or(true),
            support_contingent_orders: support_contingent_orders.unwrap_or(true),
            use_position_ids: use_position_ids.unwrap_or(true),
            use_random_ids: use_random_ids.unwrap_or(false),
            use_reduce_only: use_reduce_only.unwrap_or(true),
            use_message_queue: use_message_queue.unwrap_or(true),
        })
    }

    pub fn register_client(&mut self, client: ExecutionClient) {
        let client_id = client.client_id;
        self.exec_client = Some(client);
        log::info!("Registered ExecutionClient: {client_id}");
    }

    pub fn set_fill_model(&mut self, fill_model: FillModel) {
        for matching_engine in self.matching_engines.values_mut() {
            matching_engine.set_fill_model(fill_model.clone());
            log::info!(
                "Changed fill model for {} to {}",
                matching_engine.venue,
                self.fill_model
            );
        }
        self.fill_model = fill_model;
    }

    pub fn set_latency_model(&mut self, _latency_model: LatencyModel) {
        todo!("set latency model")
    }

    pub fn initialize_account(&mut self, _account_id: u64) {
        todo!("initialize account")
    }

    pub fn add_instrument(&mut self, instrument: InstrumentAny) -> anyhow::Result<()> {
        check_equal(
            instrument.id().venue,
            self.id,
            "Venue of instrument id",
            "Venue of simulated exchange",
        )
        .unwrap();

        if self.account_type == AccountType::Cash
            && (matches!(instrument, InstrumentAny::CryptoPerpetual(_))
                || matches!(instrument, InstrumentAny::CryptoFuture(_)))
        {
            anyhow::bail!("Cash account cannot trade futures or perpetuals")
        }

        self.instruments.insert(instrument.id(), instrument.clone());

        let matching_engine_config = OrderMatchingEngineConfig::new(
            self.bar_execution,
            self.reject_stop_orders,
            self.support_gtd_orders,
            self.support_contingent_orders,
            self.use_position_ids,
            self.use_random_ids,
            self.use_reduce_only,
        );
        let instrument_id = instrument.id();
        let matching_engine = OrderMatchingEngine::new(
            instrument,
            self.instruments.len() as u32,
            self.fill_model.clone(),
            self.book_type,
            self.oms_type,
            self.account_type,
            self.clock,
            Rc::clone(&self.msgbus),
            Rc::clone(&self.cache),
            matching_engine_config,
        );
        self.matching_engines.insert(instrument_id, matching_engine);

        log::info!(
            "Added instrument {} and created matching engine",
            instrument_id
        );
        Ok(())
    }

    pub fn best_bid_price(&self, _instrument_id: InstrumentId) {
        todo!("best bid price")
    }

    pub fn best_ask_price(&self, _instrument_id: InstrumentId) {
        todo!("best ask price")
    }

    pub fn get_book(&self, _instrument_id: InstrumentId) {
        todo!("best bid qty")
    }

    pub fn get_matching_engine(&self, _instrument_id: InstrumentId) {
        todo!("get matching engine")
    }

    pub fn get_matching_engines(&self) {
        todo!("get matching engines")
    }

    pub fn get_books(&self) {
        todo!("get books")
    }

    pub fn get_open_orders(&self, _instrument_id: Option<InstrumentId>) {
        todo!("get open orders")
    }

    pub fn get_open_bid_orders(&self, _instrument_id: Option<InstrumentId>) {
        todo!("get open bid orders")
    }

    pub fn get_open_ask_orders(&self, _instrument_id: Option<InstrumentId>) {
        todo!("get open ask orders")
    }

    pub fn get_account(&self) {
        todo!("get account")
    }

    pub fn adjust_account(&mut self, _adjustment: Money) {
        todo!("adjust account")
    }

    pub fn send(&self, _command: TradingCommand) {
        todo!("send")
    }

    pub fn generate_inflight_command(&self, _command: TradingCommand) {
        todo!("generate inflight command")
    }

    pub fn process_order_book_delta(&mut self, _delta: OrderBookDelta) {
        todo!("process order book delta")
    }

    pub fn process_order_book_deltas(&mut self, _deltas: OrderBookDeltas) {
        todo!("process order book deltas")
    }

    pub fn process_quote_tick(&mut self, _tick: QuoteTick) {
        todo!("process quote tick")
    }

    pub fn process_trade_tick(&mut self, _tick: TradeTick) {
        todo!("process trade tick")
    }

    pub fn process_bar(&mut self, _bar: Bar) {
        todo!("process bar")
    }

    pub fn process_instrument_status(&mut self, _status: InstrumentStatus) {
        todo!("process instrument status")
    }

    pub fn process(&mut self, _ts_now: UnixNanos) {
        todo!("process")
    }

    pub fn reset(&mut self) {
        todo!("reset")
    }

    pub fn process_trading_command(&mut self, _command: TradingCommand) {
        todo!("process trading command")
    }

    pub fn generate_fresh_account_state(&self) {
        todo!("generate fresh account state")
    }
}
