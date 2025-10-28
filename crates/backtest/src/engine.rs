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
    any::Any,
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    fmt::Debug,
    rc::Rc,
};

use nautilus_common::timer::TimeEventHandlerV2;
use nautilus_core::{UUID4, UnixNanos};
use nautilus_data::client::DataClientAdapter;
use nautilus_execution::models::{fee::FeeModelAny, fill::FillModel, latency::LatencyModel};
use nautilus_model::{
    data::Data,
    enums::{AccountType, BookType, OmsType},
    identifiers::{AccountId, ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
    types::{Currency, Money},
};
use nautilus_system::{config::NautilusKernelConfig, kernel::NautilusKernel};
use rust_decimal::Decimal;

use crate::{
    accumulator::TimeEventAccumulator, config::BacktestEngineConfig,
    data_client::BacktestDataClient, exchange::SimulatedExchange,
    execution_client::BacktestExecutionClient, modules::SimulationModule,
};

/// Core backtesting engine for running event-driven strategy backtests on historical data.
///
/// The `BacktestEngine` provides a high-fidelity simulation environment that processes
/// historical market data chronologically through an event-driven architecture. It maintains
/// simulated exchanges with realistic order matching and execution, allowing strategies
/// to be tested exactly as they would run in live trading:
///
/// - Event-driven data replay with configurable latency models.
/// - Multi-venue and multi-asset support.
/// - Realistic order matching and execution simulation.
/// - Strategy and portfolio performance analysis.
/// - Seamless transition from backtesting to live trading.
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

impl Debug for BacktestEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(BacktestEngine))
            .field("instance_id", &self.instance_id)
            .field("run_config_id", &self.run_config_id)
            .field("run_id", &self.run_id)
            .finish()
    }
}

impl BacktestEngine {
    /// Create a new [`BacktestEngine`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the core `NautilusKernel` fails to initialize.
    pub fn new(config: BacktestEngineConfig) -> anyhow::Result<Self> {
        let kernel = NautilusKernel::new("BacktestEngine".to_string(), config.clone())?;

        Ok(Self {
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
        })
    }

    /// # Errors
    ///
    /// Returns an error if initializing the simulated exchange for the venue fails.
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
        allow_cash_borrowing: Option<bool>,
        frozen_account: Option<bool>,
    ) -> anyhow::Result<()> {
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
            bar_execution,
            reject_stop_orders,
            support_gtd_orders,
            support_contingent_orders,
            use_position_ids,
            use_random_ids,
            use_reduce_only,
            use_message_queue,
            allow_cash_borrowing,
            frozen_account,
        )?;
        let exchange = Rc::new(RefCell::new(exchange));
        self.venues.insert(venue, exchange.clone());

        let account_id = AccountId::from(format!("{venue}-001").as_str());
        let exec_client = BacktestExecutionClient::new(
            self.config.trader_id(),
            account_id,
            exchange.clone(),
            self.kernel.cache.clone(),
            self.kernel.clock.clone(),
            routing,
            frozen_account,
        );
        let exec_client = Rc::new(exec_client);

        exchange.borrow_mut().register_client(exec_client.clone());
        self.kernel.exec_engine.register_client(exec_client)?;

        log::info!("Adding exchange {venue} to engine");

        Ok(())
    }

    pub fn change_fill_model(&mut self, venue: Venue, fill_model: FillModel) {
        if let Some(exchange) = self.venues.get_mut(&venue) {
            exchange.borrow_mut().set_fill_model(fill_model);
        } else {
            log::warn!(
                "BacktestEngine::change_fill_model called for unknown venue {venue}. Ignoring."
            );
        }
    }

    /// Adds an instrument to the backtest engine for the specified venue.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The instrument's associated venue has not been added via `add_venue`.
    /// - Attempting to add a `CurrencyPair` instrument for a single-currency CASH account.
    ///
    /// # Panics
    ///
    /// Panics if adding the instrument to the simulated exchange fails.
    pub fn add_instrument(&mut self, instrument: InstrumentAny) -> anyhow::Result<()> {
        let instrument_id = instrument.id();
        if let Some(exchange) = self.venues.get_mut(&instrument.id().venue) {
            // check if instrument is of variant CurrencyPair
            if matches!(instrument, InstrumentAny::CurrencyPair(_))
                && exchange.borrow().account_type != AccountType::Margin
                && exchange.borrow().base_currency.is_some()
            {
                anyhow::bail!(
                    "Cannot add a `CurrencyPair` instrument {} for a venue with a single-currency CASH account",
                    instrument_id
                )
            }
            exchange
                .borrow_mut()
                .add_instrument(instrument.clone())
                .unwrap();
        } else {
            anyhow::bail!(
                "Cannot add an `Instrument` object without first adding its associated venue {}",
                instrument.id().venue
            )
        }

        // Check client has been registered
        self.add_market_data_client_if_not_exists(instrument.id().venue);

        self.kernel
            .data_engine
            .borrow_mut()
            .process(&instrument as &dyn Any);
        log::info!(
            "Added instrument {} to exchange {}",
            instrument_id,
            instrument_id.venue
        );
        Ok(())
    }

    pub fn add_data(
        &mut self,
        data: Vec<Data>,
        client_id: Option<ClientId>,
        validate: bool,
        sort: bool,
    ) {
        if data.is_empty() {
            log::warn!("add_data called with empty data slice – ignoring");
            return;
        }

        // If requested, sort by ts_init so internal stream is monotonic.
        let mut to_add = data;
        if sort {
            to_add.sort_by_key(nautilus_model::data::HasTsInit::ts_init);
        }

        // Instrument & book tracking using Data helpers
        if validate {
            for item in &to_add {
                let instr_id = item.instrument_id();
                self.has_data.insert(instr_id);

                if item.is_order_book_data() {
                    self.has_book_data.insert(instr_id);
                }

                // Ensure appropriate market data client exists
                self.add_market_data_client_if_not_exists(instr_id.venue);
            }
        }

        // Extend master data vector and ensure internal iterator (index) remains valid.
        for item in to_add {
            self.data.push_back(item);
        }

        if sort {
            // VecDeque cannot be sorted directly; convert to Vec for sorting, then back.
            let mut vec: Vec<Data> = self.data.drain(..).collect();
            vec.sort_by_key(nautilus_model::data::HasTsInit::ts_init);
            self.data = vec.into();
        }

        log::info!(
            "Added {} data element{} to BacktestEngine",
            self.data.len(),
            if self.data.len() == 1 { "" } else { "s" }
        );
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
        // TODO: implement full BacktestResult aggregation once portfolio analysis
        // components are available in Rust. For now we simply log and return.
        log::info!("BacktestEngine::get_result called – not yet implemented");
    }

    pub fn next(&mut self) {
        self.data.pop_front();
    }

    pub fn advance_time(&mut self, _ts_now: UnixNanos) -> Vec<TimeEventHandlerV2> {
        // TODO: integrate TestClock advancement when kernel clocks are exposed.
        self.accumulator.drain()
    }

    pub fn process_raw_time_event_handlers(
        &mut self,
        handlers: Vec<TimeEventHandlerV2>,
        ts_now: UnixNanos,
        only_now: bool,
        as_of_now: bool,
    ) {
        let mut last_ts_init: Option<UnixNanos> = None;

        for handler in handlers {
            let ts_event_init = handler.event.ts_event; // event time

            if Self::should_skip_time_event(ts_event_init, ts_now, only_now, as_of_now) {
                continue;
            }

            if last_ts_init != Some(ts_event_init) {
                // First handler for this timestamp – process exchange queues beforehand.
                for exchange in self.venues.values() {
                    exchange.borrow_mut().process(ts_event_init);
                }
                last_ts_init = Some(ts_event_init);
            }

            handler.run();
        }
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

    pub fn add_data_client_if_not_exists(&mut self, client_id: ClientId) {
        if self
            .kernel
            .data_engine
            .borrow()
            .registered_clients()
            .contains(&client_id)
        {
            return;
        }

        // Create a generic, venue-agnostic backtest data client. We use a dummy
        // venue derived from the client id for uniqueness.
        let venue = Venue::from(client_id.as_str());
        let backtest_client = BacktestDataClient::new(client_id, venue, self.kernel.cache.clone());
        let data_client_adapter = DataClientAdapter::new(
            backtest_client.client_id,
            None, // no specific venue association
            false,
            false,
            Box::new(backtest_client),
        );

        self.kernel
            .data_engine
            .borrow_mut()
            .register_client(data_client_adapter, None);
    }

    // Helper matching Cython semantics for determining whether to skip
    // processing a time event.
    fn should_skip_time_event(
        ts_event_init: UnixNanos,
        ts_now: UnixNanos,
        only_now: bool,
        as_of_now: bool,
    ) -> bool {
        if only_now {
            ts_event_init != ts_now
        } else if as_of_now {
            ts_event_init > ts_now
        } else {
            ts_event_init >= ts_now
        }
    }

    // TODO: We might want venue to be optional for multi-venue clients
    pub fn add_market_data_client_if_not_exists(&mut self, venue: Venue) {
        let client_id = ClientId::from(venue.as_str());
        if !self
            .kernel
            .data_engine
            .borrow()
            .registered_clients()
            .contains(&client_id)
        {
            let backtest_client =
                BacktestDataClient::new(client_id, venue, self.kernel.cache.clone());
            let data_client_adapter = DataClientAdapter::new(
                client_id,
                Some(venue), // TBD
                false,
                false,
                Box::new(backtest_client),
            );
            self.kernel
                .data_engine
                .borrow_mut()
                .register_client(data_client_adapter, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use nautilus_execution::models::{fee::FeeModelAny, fill::FillModel};
    use nautilus_model::{
        enums::{AccountType, BookType, OmsType},
        identifiers::{ClientId, Venue},
        instruments::{
            CryptoPerpetual, Instrument, InstrumentAny, stubs::crypto_perpetual_ethusdt,
        },
        types::Money,
    };
    use rstest::rstest;

    use crate::{config::BacktestEngineConfig, engine::BacktestEngine};

    #[allow(clippy::missing_panics_doc, reason = "OK for testing")]
    fn get_backtest_engine(config: Option<BacktestEngineConfig>) -> BacktestEngine {
        let config = config.unwrap_or_default();
        let mut engine = BacktestEngine::new(config).unwrap();
        engine
            .add_venue(
                Venue::from("BINANCE"),
                OmsType::Netting,
                AccountType::Margin,
                BookType::L2_MBP,
                vec![Money::from("1_000_000 USD")],
                None,
                None,
                HashMap::new(),
                vec![],
                FillModel::default(),
                FeeModelAny::default(),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
        engine
    }

    #[rstest]
    fn test_engine_venue_and_instrument_initialization(crypto_perpetual_ethusdt: CryptoPerpetual) {
        let venue = Venue::from("BINANCE");
        let client_id = ClientId::from(venue.as_str());
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
        let instrument_id = instrument.id();
        let mut engine = get_backtest_engine(None);
        engine.add_instrument(instrument).unwrap();

        // Check the venue and exec client has been added
        assert_eq!(engine.venues.len(), 1);
        assert!(engine.venues.contains_key(&venue));
        assert!(engine.kernel.exec_engine.get_client(&client_id).is_some());

        // Check the instrument has been added
        assert!(
            engine
                .venues
                .get(&venue)
                .is_some_and(|venue| venue.borrow().get_matching_engine(&instrument_id).is_some())
        );
        assert_eq!(
            engine
                .kernel
                .data_engine
                .borrow()
                .registered_clients()
                .len(),
            1
        );
        assert!(
            engine
                .kernel
                .data_engine
                .borrow()
                .registered_clients()
                .contains(&client_id)
        );
    }
}
