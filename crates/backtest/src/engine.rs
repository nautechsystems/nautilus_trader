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

use std::collections::{HashMap, HashSet, VecDeque};

use nautilus_core::{UUID4, UnixNanos};
use nautilus_execution::models::{fee::FeeModelAny, fill::FillModel, latency::LatencyModel};
use nautilus_model::{
    data::Data,
    enums::{AccountType, BookType, OmsType},
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::InstrumentAny,
    types::{Currency, Money},
};
use nautilus_system::kernel::NautilusKernel;
use ustr::Ustr;

use crate::{
    accumulator::TimeEventAccumulator, config::BacktestEngineConfig, exchange::SimulatedExchange,
    modules::SimulationModule,
};

pub struct BacktestEngine {
    instance_id: UUID4,
    config: BacktestEngineConfig,
    kernel: NautilusKernel,
    accumulator: TimeEventAccumulator,
    run_config_id: Option<UUID4>,
    run_id: Option<UUID4>,
    venues: HashMap<Venue, SimulatedExchange>,
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
        &self,
        _venue: Venue,
        _oms_type: OmsType,
        _account_type: AccountType,
        _book_type: BookType,
        _starting_balances: Vec<Money>,
        _base_currency: Option<Currency>,
        _default_leverage: Option<f64>,
        _leverages: Option<HashMap<Currency, f64>>,
        _modules: Vec<Box<dyn SimulationModule>>,
        _fill_model: Option<FillModel>,
        _fee_model: Option<FeeModelAny>,
        _latency_model: Option<LatencyModel>,
        _routing: Option<bool>,
        _frozen_account: Option<bool>,
        _reject_stop_orders: Option<bool>,
        _support_gtd_orders: Option<bool>,
        _support_contingent_orders: Option<bool>,
        _use_position_ids: Option<bool>,
        _use_random_ids: Option<bool>,
        _use_reduce_only: Option<bool>,
        _use_message_queue: Option<bool>,
        _bar_execution: Option<bool>,
        _bar_adaptive_high_low_ordering: Option<bool>,
        _trade_execution: Option<bool>,
    ) {
        todo!("implement add_venue")
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
