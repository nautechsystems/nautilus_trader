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

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

//! The core `BacktestEngine` for backtesting on historical data.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    rc::Rc,
};

use nautilus_core::{UUID4, UnixNanos};
use nautilus_execution::models::{fee::FeeModelAny, fill::FillModel, latency::LatencyModel};
use nautilus_model::{
    data::Data,
    enums::{AccountType, BookType, OmsType},
    identifiers::{AccountId, ClientId, InstrumentId, Venue},
    instruments::InstrumentAny,
    types::{Currency, Money},
};
use nautilus_system::kernel::NautilusKernel;
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::{
    accumulator::TimeEventAccumulator, config::BacktestEngineConfig, exchange::SimulatedExchange,
    execution_client::BacktestExecutionClient, modules::SimulationModule,
};

pub struct BacktestEngine {
    instance_id: UUID4,
    config: BacktestEngineConfig,
    kernel: NautilusKernel,
    accumulator: TimeEventAccumulator,
    run_config_id: Option<UUID4>,
    run_id: Option<UUID4>,
    venues: HashMap<Venue, Rc<RefCell<SimulatedExchange>>>,
    has_data: HashSet<InstrumentId>,
    has_book_data: HashSet<InstrumentId>,
    data: VecDeque<Data>,
    index: usize,
    iteration: usize,
    run_started: Option<UnixNanos>,
    run_finished: Option<UnixNanos>,
    backtest_start: Option<UnixNanos>,
    backtest_end: Option<UnixNanos>,
}

impl BacktestEngine {
    #[must_use]
    pub fn new(config: BacktestEngineConfig) -> Self {
        let kernel = NautilusKernel::new(Ustr::from("BacktestEngine"), config.kernel.clone());
        Self {
            instance_id: kernel.instance_id,
            config,
            accumulator: TimeEventAccumulator::new(),
            kernel,
            run_config_id: None,
            run_id: None,
            venues: HashMap::new(),
            has_data: HashSet::new(),
            has_book_data: HashSet::new(),
            data: VecDeque::new(),
            index: 0,
            iteration: 0,
            run_started: None,
            run_finished: None,
            backtest_start: None,
            backtest_end: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_venue(
        &mut self,
        venue: Venue,
        oms_type: OmsType,
        account_type: AccountType,
        book_type: BookType,
        starting_balances: Vec<Money>,
        base_currency: Option<Currency>,
        default_leverage: Option<Decimal>,
        leverages: HashMap<InstrumentId, Decimal>,
        modules: Vec<Box<dyn SimulationModule>>,
        fill_model: FillModel,
        fee_model: FeeModelAny,
        latency_model: Option<LatencyModel>,
        routing: Option<bool>,
        frozen_account: Option<bool>,
        reject_stop_orders: Option<bool>,
        support_gtd_orders: Option<bool>,
        support_contingent_orders: Option<bool>,
        use_position_ids: Option<bool>,
        use_random_ids: Option<bool>,
        use_reduce_only: Option<bool>,
        use_message_queue: Option<bool>,
        bar_execution: Option<bool>,
        bar_adaptive_high_low_ordering: Option<bool>,
        trade_execution: Option<bool>,
    ) {
        let default_leverage: Decimal = default_leverage.unwrap_or_else(|| {
            if account_type == AccountType::Margin {
                Decimal::from(10)
            } else {
                Decimal::from(0)
            }
        });

        let exchange = SimulatedExchange::new(
            venue,
            oms_type,
            account_type,
            starting_balances,
            base_currency,
            default_leverage,
            leverages,
            modules,
            self.kernel.cache.clone(),
            self.kernel.clock.clone(),
            fill_model,
            fee_model,
            book_type,
            latency_model,
            frozen_account,
            bar_execution,
            reject_stop_orders,
            support_gtd_orders,
            support_contingent_orders,
            use_position_ids,
            use_random_ids,
            use_reduce_only,
            use_message_queue,
        )
        .unwrap();
        let exchange = Rc::new(RefCell::new(exchange));
        self.venues.insert(venue, exchange.clone());

        let account_id = AccountId::from(format!("{}-001", venue).as_str());
        let exec_client = BacktestExecutionClient::new(
            self.kernel.config.trader_id,
            account_id,
            exchange.clone(),
            self.kernel.cache.clone(),
            self.kernel.clock.clone(),
            routing,
            frozen_account,
        );
        let exec_client = Rc::new(exec_client);

        exchange.borrow_mut().register_client(exec_client.clone());
        self.kernel
            .exec_engine
            .register_client(exec_client)
            .unwrap();
        log::info!("Adding exchange {} to engine", venue);
    }

    pub fn change_fill_model(&mut self, venue: Venue, fill_model: FillModel) {
        todo!("implement change_fill_model")
    }

    pub fn add_instrument(&mut self, instrument: InstrumentAny) {
        todo!("implement add_instrument")
    }

    pub fn add_data(
        &mut self,
        data: Vec<Data>,
        client_id: Option<ClientId>,
        validate: bool,
        sort: bool,
    ) {
        todo!("implement add_data")
    }

    pub fn add_actor(&mut self) {
        todo!("implement add_actor")
    }

    pub fn add_actors(&mut self) {
        todo!("implement add_actors")
    }

    pub fn add_strategy(&mut self) {
        todo!("implement add_strategy")
    }

    pub fn add_strategies(&mut self) {
        todo!("implement add_strategies")
    }

    pub fn add_exec_algorithm(&mut self) {
        todo!("implement add_exec_algorithm")
    }

    pub fn add_exec_algorithms(&mut self) {
        todo!("implement add_exec_algorithms")
    }

    pub fn reset(&mut self) {
        todo!("implement reset")
    }

    pub fn clear_data(&mut self) {
        todo!("implement clear_data")
    }

    pub fn clear_strategies(&mut self) {
        todo!("implement clear_strategies")
    }

    pub fn clear_exec_algorithms(&mut self) {
        todo!("implement clear_exec_algorithms")
    }

    pub fn dispose(&mut self) {
        todo!("implement dispose")
    }

    pub fn run(&mut self) {
        todo!("implement run")
    }

    pub fn end(&mut self) {
        todo!("implement end")
    }

    pub fn get_result(&self) {
        todo!("implement get_result")
    }

    pub fn next(&mut self) {
        todo!("implement next")
    }

    pub fn advance_time(&mut self) {
        todo!("implement advance_time")
    }

    pub fn process_raw_time_event_handlers(&mut self) {
        todo!("implement process_raw_time_event_handlers")
    }

    pub fn log_pre_run(&self) {
        todo!("implement log_pre_run_diagnostics")
    }

    pub fn log_run(&self) {
        todo!("implement log_run")
    }

    pub fn log_post_run(&self) {
        todo!("implement log_post_run")
    }

    pub fn add_data_client_if_not_exists(&mut self) {
        todo!("implement add_data_client_if_not_exists")
    }

    pub fn add_market_data_client_if_not_exists(&mut self) {
        todo!("implement add_market_data_client_if_not_exists")
    }
}
