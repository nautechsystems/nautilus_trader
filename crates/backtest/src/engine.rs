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

//! The core `BacktestEngine` for backtesting on historical data.

use std::{any::Any, cell::RefCell, fmt::Debug, rc::Rc, sync::Arc};

use ahash::{AHashMap, AHashSet};
use nautilus_analysis::analyzer::PortfolioAnalyzer;
use nautilus_common::{
    actor::DataActor,
    cache::Cache,
    clock::{Clock, TestClock},
    component::Component,
    logging::{
        logging_clock_set_realtime_mode, logging_clock_set_static_mode,
        logging_clock_set_static_time,
    },
    runner::{
        SyncDataCommandSender, SyncTradingCommandSender, data_cmd_queue_is_empty,
        drain_data_cmd_queue, drain_trading_cmd_queue, init_data_cmd_sender, init_exec_cmd_sender,
        trading_cmd_queue_is_empty,
    },
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_data::client::DataClientAdapter;
use nautilus_execution::models::{fee::FeeModelAny, fill::FillModel, latency::LatencyModel};
use nautilus_model::{
    accounts::{Account, AccountAny},
    data::{Data, HasTsInit},
    enums::{AccountType, BookType, OmsType},
    identifiers::{AccountId, ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
    position::Position,
    types::{Currency, Money},
};
use nautilus_system::{config::NautilusKernelConfig, kernel::NautilusKernel};
use nautilus_trading::strategy::Strategy;
use rust_decimal::Decimal;

use crate::{
    accumulator::TimeEventAccumulator, config::BacktestEngineConfig,
    data_client::BacktestDataClient, data_iterator::BacktestDataIterator,
    exchange::SimulatedExchange, execution_client::BacktestExecutionClient,
    modules::SimulationModule,
};

/// Results from a completed backtest run.
#[derive(Debug)]
pub struct BacktestResult {
    pub trader_id: String,
    pub machine_id: String,
    pub instance_id: UUID4,
    pub run_config_id: Option<String>,
    pub run_id: Option<UUID4>,
    pub run_started: Option<UnixNanos>,
    pub run_finished: Option<UnixNanos>,
    pub backtest_start: Option<UnixNanos>,
    pub backtest_end: Option<UnixNanos>,
    pub elapsed_time_secs: f64,
    pub iterations: usize,
    pub total_events: usize,
    pub total_orders: usize,
    pub total_positions: usize,
    pub stats_pnls: AHashMap<String, AHashMap<String, f64>>,
    pub stats_returns: AHashMap<String, f64>,
    pub stats_general: AHashMap<String, f64>,
}

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
    run_config_id: Option<String>,
    run_id: Option<UUID4>,
    venues: AHashMap<Venue, Rc<RefCell<SimulatedExchange>>>,
    exec_clients: Vec<BacktestExecutionClient>,
    has_data: AHashSet<InstrumentId>,
    has_book_data: AHashSet<InstrumentId>,
    data_iterator: BacktestDataIterator,
    data_len: usize,
    data_stream_counter: usize,
    ts_first: Option<UnixNanos>,
    ts_last_data: Option<UnixNanos>,
    iteration: usize,
    force_stop: bool,
    last_ns: UnixNanos,
    end_ns: UnixNanos,
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
            venues: AHashMap::new(),
            exec_clients: Vec::new(),
            has_data: AHashSet::new(),
            has_book_data: AHashSet::new(),
            data_iterator: BacktestDataIterator::new(),
            data_len: 0,
            data_stream_counter: 0,
            ts_first: None,
            ts_last_data: None,
            iteration: 0,
            force_stop: false,
            last_ns: UnixNanos::default(),
            end_ns: UnixNanos::default(),
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
        leverages: AHashMap<InstrumentId, Decimal>,
        modules: Vec<Box<dyn SimulationModule>>,
        fill_model: FillModel,
        fee_model: FeeModelAny,
        latency_model: Option<Box<dyn LatencyModel>>,
        routing: Option<bool>,
        reject_stop_orders: Option<bool>,
        support_gtd_orders: Option<bool>,
        support_contingent_orders: Option<bool>,
        use_position_ids: Option<bool>,
        use_random_ids: Option<bool>,
        use_reduce_only: Option<bool>,
        use_message_queue: Option<bool>,
        use_market_order_acks: Option<bool>,
        bar_execution: Option<bool>,
        bar_adaptive_high_low_ordering: Option<bool>,
        trade_execution: Option<bool>,
        allow_cash_borrowing: Option<bool>,
        frozen_account: Option<bool>,
        price_protection_points: Option<u32>,
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
            bar_adaptive_high_low_ordering,
            trade_execution,
            None, // liquidity_consumption - use default (true)
            reject_stop_orders,
            support_gtd_orders,
            support_contingent_orders,
            use_position_ids,
            use_random_ids,
            use_reduce_only,
            use_message_queue,
            use_market_order_acks,
            allow_cash_borrowing,
            frozen_account,
            price_protection_points,
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

        exchange
            .borrow_mut()
            .register_client(Rc::new(exec_client.clone()));

        self.exec_clients.push(exec_client.clone());

        self.kernel
            .exec_engine
            .borrow_mut()
            .register_client(Box::new(exec_client))?;

        log::info!("Adding exchange {venue} to engine");

        Ok(())
    }

    pub fn change_fill_model(&mut self, venue: Venue, fill_model: FillModel) {
        if let Some(exchange) = self.venues.get_mut(&venue) {
            exchange.borrow_mut().set_fill_model(fill_model);
        } else {
            log::warn!(
                "BacktestEngine::change_fill_model called for unknown venue {venue}, ignoring"
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
            if matches!(instrument, InstrumentAny::CurrencyPair(_))
                && exchange.borrow().account_type != AccountType::Margin
                && exchange.borrow().base_currency.is_some()
            {
                anyhow::bail!(
                    "Cannot add a `CurrencyPair` instrument {instrument_id} for a venue with a single-currency CASH account"
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
        _client_id: Option<ClientId>,
        validate: bool,
        sort: bool,
    ) {
        if data.is_empty() {
            log::warn!("add_data called with empty data slice – ignoring");
            return;
        }

        let count = data.len();

        let mut to_add = data;
        if sort {
            to_add.sort_by_key(HasTsInit::ts_init);
        }

        if validate {
            for item in &to_add {
                let instr_id = item.instrument_id();
                self.has_data.insert(instr_id);

                if item.is_order_book_data() {
                    self.has_book_data.insert(instr_id);
                }

                self.add_market_data_client_if_not_exists(instr_id.venue);
            }
        }

        // Track time bounds for start/end defaults
        if let Some(first) = to_add.first() {
            let ts = first.ts_init();
            if self.ts_first.is_none_or(|t| ts < t) {
                self.ts_first = Some(ts);
            }
        }
        if let Some(last) = to_add.last() {
            let ts = last.ts_init();
            if self.ts_last_data.is_none_or(|t| ts > t) {
                self.ts_last_data = Some(ts);
            }
        }

        self.data_len += count;
        let stream_name = format!("backtest_data_{}", self.data_stream_counter);
        self.data_stream_counter += 1;
        self.data_iterator.add_data(&stream_name, to_add, true);

        log::info!(
            "Added {count} data element{} to BacktestEngine ({} total)",
            if count == 1 { "" } else { "s" },
            self.data_len,
        );
    }

    /// Adds a strategy to the backtest engine.
    ///
    /// # Errors
    ///
    /// Returns an error if the strategy is already registered or the trader is running.
    pub fn add_strategy<T>(&mut self, strategy: T) -> anyhow::Result<()>
    where
        T: Strategy + Component + Debug + 'static,
    {
        self.kernel.trader.add_strategy(strategy)
    }

    /// Adds an actor to the backtest engine.
    ///
    /// # Errors
    ///
    /// Returns an error if the actor is already registered or the trader is running.
    pub fn add_actor<T>(&mut self, actor: T) -> anyhow::Result<()>
    where
        T: DataActor + Component + Debug + 'static,
    {
        self.kernel.trader.add_actor(actor)
    }

    /// Adds an execution algorithm to the backtest engine.
    ///
    /// # Errors
    ///
    /// Returns an error if the algorithm is already registered or the trader is running.
    pub fn add_exec_algorithm<T>(&mut self, exec_algorithm: T) -> anyhow::Result<()>
    where
        T: DataActor + Component + Debug + 'static,
    {
        self.kernel.trader.add_exec_algorithm(exec_algorithm)
    }

    /// Run a backtest.
    ///
    /// Processes all data chronologically. When `streaming` is false (default),
    /// finalizes the run via [`end`](Self::end). When `streaming` is true, the
    /// run pauses without finalizing, allowing additional data to be loaded:
    ///
    /// 1. Add initial data and strategies
    /// 2. Call `run(streaming=true)`
    /// 3. Call `clear_data()`
    /// 4. Add next batch of data
    /// 5. Repeat steps 2-4, then call `run(streaming=false)` or `end()` for the final batch
    ///
    /// # Errors
    ///
    /// Returns an error if the backtest encounters an unrecoverable state.
    pub fn run(
        &mut self,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        run_config_id: Option<String>,
        streaming: bool,
    ) -> anyhow::Result<()> {
        self.run_impl(start, end, run_config_id)?;

        if !streaming {
            self.end();
        }

        Ok(())
    }

    fn run_impl(
        &mut self,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        run_config_id: Option<String>,
    ) -> anyhow::Result<()> {
        // Determine time boundaries
        let start_ns = start.unwrap_or_else(|| self.ts_first.unwrap_or_default());
        let end_ns = end.unwrap_or_else(|| {
            self.ts_last_data
                .unwrap_or(UnixNanos::from(4_102_444_800_000_000_000u64))
        });
        anyhow::ensure!(start_ns <= end_ns, "start was > end");
        self.end_ns = end_ns;
        self.last_ns = start_ns;

        // Set all component clocks to start
        let clocks = self.collect_all_clocks();
        Self::set_all_clocks_time(&clocks, start_ns);

        // First-iteration initialization
        if self.iteration == 0 {
            self.run_config_id = run_config_id;
            self.run_id = Some(UUID4::new());
            self.run_started = Some(UnixNanos::from(std::time::SystemTime::now()));
            self.backtest_start = Some(start_ns);

            // Initialize exchange accounts
            for exchange in self.venues.values() {
                exchange.borrow_mut().initialize_account();
            }

            // Re-set clocks after account init
            Self::set_all_clocks_time(&clocks, start_ns);

            // Reset force stop flag
            self.force_stop = false;

            // Initialize sync command senders (once per thread)
            Self::init_command_senders();

            // Set logging to static clock mode for deterministic timestamps
            logging_clock_set_static_mode();
            logging_clock_set_static_time(start_ns.as_u64());

            // Start kernel (engines + trader init + clients)
            self.kernel.start();
            self.kernel.start_trader();

            self.log_pre_run();
        }

        self.log_run();

        // Skip data before start_ns
        let mut data = self.data_iterator.next();
        while let Some(ref d) = data {
            if d.ts_init() >= start_ns {
                break;
            }
            data = self.data_iterator.next();
        }

        // Initialize last_ns before first data point
        if let Some(ref d) = data {
            let ts = d.ts_init();
            self.last_ns = if ts.as_u64() > 0 {
                UnixNanos::from(ts.as_u64() - 1)
            } else {
                UnixNanos::default()
            };
        } else {
            self.last_ns = start_ns;
        }

        loop {
            if self.force_stop {
                log::error!("Force stop triggered, ending backtest");
                break;
            }

            if data.is_none() {
                let done = self.process_next_timer(&clocks);
                data = self.data_iterator.next();
                if data.is_none() && done {
                    break;
                }
                continue;
            }

            let d = data.as_ref().unwrap();
            let ts_init = d.ts_init();

            if ts_init > end_ns {
                break;
            }

            if ts_init > self.last_ns {
                self.last_ns = ts_init;
                self.advance_time_impl(ts_init, &clocks);
            }

            // Route data to exchange
            self.route_data_to_exchange(d);

            // Process through data engine (may trigger strategy callbacks
            // which queue trading commands via the sync senders)
            self.kernel.data_engine.borrow_mut().process_data(d.clone());

            // Drain deferred commands, then process exchange queues
            self.drain_command_queues();
            self.process_and_settle_venues(ts_init);

            let prev_last_ns = self.last_ns;
            data = self.data_iterator.next();

            // If timestamp changed, flush accumulated timer events
            if data.is_none() || data.as_ref().unwrap().ts_init() > prev_last_ns {
                self.flush_accumulator_events(&clocks, prev_last_ns);
            }

            self.iteration += 1;
        }

        // Process remaining exchange messages
        let ts_now = self.kernel.clock.borrow().timestamp_ns();
        for exchange in self.venues.values() {
            exchange.borrow_mut().process(ts_now);
        }

        // Flush remaining timer events up to end time
        self.flush_accumulator_events(&clocks, end_ns);

        Ok(())
    }

    /// Manually end the backtest.
    pub fn end(&mut self) {
        // Stop trader
        self.kernel.stop_trader();

        // Stop engines
        self.kernel.data_engine.borrow_mut().stop();
        self.kernel.risk_engine.borrow_mut().stop();
        self.kernel.exec_engine.borrow_mut().stop();

        // Process remaining exchange messages
        let ts_now = self.kernel.clock.borrow().timestamp_ns();
        for exchange in self.venues.values() {
            exchange.borrow_mut().process(ts_now);
        }

        self.run_finished = Some(UnixNanos::from(std::time::SystemTime::now()));
        self.backtest_end = Some(self.kernel.clock.borrow().timestamp_ns());

        // Switch logging back to realtime mode
        logging_clock_set_realtime_mode();

        self.log_post_run();
    }

    /// Reset the backtest engine.
    ///
    /// All stateful fields are reset to their initial value. Data and instruments
    /// persist across resets to enable repeated runs with different strategies.
    pub fn reset(&mut self) {
        log::debug!("Resetting");

        if self.kernel.trader.is_running() {
            self.end();
        }

        // Stop and reset engines
        self.kernel.data_engine.borrow_mut().stop();
        self.kernel.data_engine.borrow_mut().reset();

        self.kernel.exec_engine.borrow_mut().stop();
        self.kernel.exec_engine.borrow_mut().reset();

        self.kernel.risk_engine.borrow_mut().stop();
        self.kernel.risk_engine.borrow_mut().reset();

        // Reset trader
        if let Err(e) = self.kernel.trader.reset() {
            log::error!("Error resetting trader: {e:?}");
        }

        // Reset all exchanges
        for exchange in self.venues.values() {
            exchange.borrow_mut().reset();
        }

        // Clear run state
        self.run_config_id = None;
        self.run_id = None;
        self.run_started = None;
        self.run_finished = None;
        self.backtest_start = None;
        self.backtest_end = None;
        self.iteration = 0;
        self.force_stop = false;
        self.last_ns = UnixNanos::default();
        self.end_ns = UnixNanos::default();

        self.accumulator.clear();

        // Reset all iterator cursors to beginning (data persists)
        self.data_iterator.reset_all_cursors();

        log::info!("Reset");
    }

    /// Sort the engine's internal data stream by timestamp.
    ///
    /// Useful when data has been added with `sort=false` for batch performance,
    /// then sorted once before running.
    pub fn sort_data(&mut self) {
        // The iterator sorts internally on add_data, but if multiple streams
        // were added unsorted we need to re-add them. Since we use a single
        // "backtest_data" stream, the iterator already maintains sort order.
        // This is a no-op when using the iterator (data is sorted on insert).
        log::info!("Data sort requested (iterator maintains sort order)");
    }

    /// Clear the engine's internal data stream. Does not clear instruments.
    pub fn clear_data(&mut self) {
        self.has_data.clear();
        self.has_book_data.clear();
        self.data_iterator = BacktestDataIterator::new();
        self.data_len = 0;
        self.data_stream_counter = 0;
        self.ts_first = None;
        self.ts_last_data = None;
    }

    /// Clear all trading strategies from the engine's internal trader.
    ///
    /// # Errors
    ///
    /// Returns an error if any strategy fails to dispose.
    pub fn clear_strategies(&mut self) -> anyhow::Result<()> {
        self.kernel.trader.clear_strategies()
    }

    /// Clear all execution algorithms from the engine's internal trader.
    ///
    /// # Errors
    ///
    /// Returns an error if any execution algorithm fails to dispose.
    pub fn clear_exec_algorithms(&mut self) -> anyhow::Result<()> {
        self.kernel.trader.clear_exec_algorithms()
    }

    /// Dispose of the backtest engine, releasing all resources.
    pub fn dispose(&mut self) {
        self.clear_data();
        self.kernel.dispose();
    }

    /// Return the backtest result from the last run.
    #[must_use]
    pub fn get_result(&self) -> BacktestResult {
        let elapsed_time_secs = match (self.backtest_start, self.backtest_end) {
            (Some(start), Some(end)) => {
                (end.as_u64() as f64 - start.as_u64() as f64) / 1_000_000_000.0
            }
            _ => 0.0,
        };

        let cache = self.kernel.cache.borrow();
        let total_orders = cache.orders_total_count(None, None, None, None, None);
        let positions = cache.positions(None, None, None, None, None);
        let total_positions = positions.len();

        let analyzer = self.build_analyzer(&cache, &positions);
        let mut stats_pnls = AHashMap::new();
        for currency in analyzer.currencies() {
            if let Ok(pnls) = analyzer.get_performance_stats_pnls(Some(currency), None) {
                stats_pnls.insert(currency.code.to_string(), pnls);
            }
        }

        let stats_returns = analyzer.get_performance_stats_returns();
        let stats_general = analyzer.get_performance_stats_general();

        BacktestResult {
            trader_id: self.config.trader_id().to_string(),
            machine_id: self.kernel.machine_id.clone(),
            instance_id: self.instance_id,
            run_config_id: self.run_config_id.clone(),
            run_id: self.run_id,
            run_started: self.run_started,
            run_finished: self.run_finished,
            backtest_start: self.backtest_start,
            backtest_end: self.backtest_end,
            elapsed_time_secs,
            iterations: self.iteration,
            total_events: self.iteration,
            total_orders,
            total_positions,
            stats_pnls,
            stats_returns,
            stats_general,
        }
    }

    fn build_analyzer(&self, cache: &Cache, positions: &[&Position]) -> PortfolioAnalyzer {
        let mut analyzer = PortfolioAnalyzer::default();
        let positions_owned: Vec<_> = positions.iter().map(|p| (*p).clone()).collect();

        // Aggregate starting and current balances across all venue accounts
        for venue in self.venues.keys() {
            if let Some(account) = cache.account_for_venue(venue) {
                let account_ref: &dyn Account = match account {
                    AccountAny::Cash(cash) => cash,
                    AccountAny::Margin(margin) => margin,
                };
                for (currency, money) in account_ref.starting_balances() {
                    analyzer
                        .account_balances_starting
                        .entry(currency)
                        .and_modify(|existing| *existing = *existing + money)
                        .or_insert(money);
                }
                for (currency, money) in account_ref.balances_total() {
                    analyzer
                        .account_balances
                        .entry(currency)
                        .and_modify(|existing| *existing = *existing + money)
                        .or_insert(money);
                }
            }
        }

        analyzer.add_positions(&positions_owned);
        analyzer
    }

    fn route_data_to_exchange(&self, data: &Data) {
        let venue = data.instrument_id().venue;
        if let Some(exchange) = self.venues.get(&venue) {
            let mut ex = exchange.borrow_mut();
            match data {
                Data::Delta(delta) => ex.process_order_book_delta(*delta),
                Data::Deltas(deltas) => ex.process_order_book_deltas((**deltas).clone()),
                Data::Quote(quote) => ex.process_quote_tick(quote),
                Data::Trade(trade) => ex.process_trade_tick(trade),
                Data::Bar(bar) => ex.process_bar(*bar),
                Data::InstrumentClose(_) => {
                    // TODO: Add process_instrument_close to SimulatedExchange
                }
                Data::Depth10(depth) => ex.process_order_book_depth10(depth),
                Data::MarkPriceUpdate(_) | Data::IndexPriceUpdate(_) => {
                    // Not routed to exchange — processed by data engine only
                }
            }
        } else {
            log::warn!("No exchange found for venue {venue}, data not routed");
        }
    }

    fn advance_time_impl(&mut self, ts_now: UnixNanos, clocks: &[Rc<RefCell<dyn Clock>>]) {
        // Advance all clocks to ts_now via accumulator
        for clock in clocks {
            Self::advance_clock_on_accumulator(&mut self.accumulator, clock, ts_now, false);
        }

        // Process events with ts_event < ts_now
        let ts_before = if ts_now.as_u64() > 0 {
            UnixNanos::from(ts_now.as_u64() - 1)
        } else {
            UnixNanos::default()
        };

        let mut ts_last: Option<UnixNanos> = None;

        while let Some(handler) = self.accumulator.pop_next_at_or_before(ts_before) {
            let ts_event = handler.event.ts_event;

            Self::set_all_clocks_time(clocks, ts_event);
            logging_clock_set_static_time(ts_event.as_u64());

            handler.run();
            self.drain_command_queues();

            if ts_last != Some(ts_event) {
                ts_last = Some(ts_event);
                self.process_and_settle_venues(ts_event);
            }

            // Re-advance clocks to capture chained timers
            for clock in clocks {
                Self::advance_clock_on_accumulator(&mut self.accumulator, clock, ts_now, false);
            }
        }

        Self::set_all_clocks_time(clocks, ts_now);
        logging_clock_set_static_time(ts_now.as_u64());
    }

    fn flush_accumulator_events(&mut self, clocks: &[Rc<RefCell<dyn Clock>>], ts_now: UnixNanos) {
        for clock in clocks {
            Self::advance_clock_on_accumulator(&mut self.accumulator, clock, ts_now, false);
        }

        let mut ts_last: Option<UnixNanos> = None;

        while let Some(handler) = self.accumulator.pop_next_at_or_before(ts_now) {
            let ts_event = handler.event.ts_event;

            Self::set_all_clocks_time(clocks, ts_event);
            logging_clock_set_static_time(ts_event.as_u64());

            handler.run();
            self.drain_command_queues();

            if ts_last != Some(ts_event) {
                ts_last = Some(ts_event);
                self.process_and_settle_venues(ts_event);
            }

            // Re-advance clocks to capture chained timers
            for clock in clocks {
                Self::advance_clock_on_accumulator(&mut self.accumulator, clock, ts_now, false);
            }
        }
    }

    fn process_next_timer(&mut self, clocks: &[Rc<RefCell<dyn Clock>>]) -> bool {
        self.flush_accumulator_events(clocks, self.last_ns);

        // Find minimum next timer time across all component clocks
        let mut min_next_time: Option<UnixNanos> = None;

        for clock in clocks {
            let clock_ref = clock.borrow();
            for name in clock_ref.timer_names() {
                if let Some(next_time) = clock_ref.next_time_ns(name)
                    && next_time > self.last_ns
                {
                    min_next_time = Some(match min_next_time {
                        Some(current_min) => next_time.min(current_min),
                        None => next_time,
                    });
                }
            }
        }

        match min_next_time {
            None => true,
            Some(t) if t > self.end_ns => true,
            Some(t) => {
                self.last_ns = t;
                self.flush_accumulator_events(clocks, t);
                false
            }
        }
    }

    fn collect_all_clocks(&self) -> Vec<Rc<RefCell<dyn Clock>>> {
        let mut clocks = vec![self.kernel.clock.clone()];
        clocks.extend(self.kernel.trader.get_component_clocks());
        clocks
    }

    fn process_and_settle_venues(&self, ts_now: UnixNanos) {
        loop {
            for exchange in self.venues.values() {
                exchange.borrow_mut().process(ts_now);
            }
            self.drain_command_queues();

            let has_pending = self
                .venues
                .values()
                .any(|exchange| exchange.borrow().has_pending_commands(ts_now));
            if !has_pending {
                break;
            }
        }
    }

    fn drain_exec_client_events(&self) {
        for client in &self.exec_clients {
            client.drain_queued_events();
        }
    }

    fn drain_command_queues(&self) {
        // Drain trading commands, exec client events, and data commands
        // in a loop until all queues settle. Handles cascading re-entrancy
        // (e.g. strategy submits order from on_order_filled).
        loop {
            drain_trading_cmd_queue();
            drain_data_cmd_queue();
            self.drain_exec_client_events();

            if trading_cmd_queue_is_empty() && data_cmd_queue_is_empty() {
                break;
            }
        }
    }

    fn init_command_senders() {
        init_data_cmd_sender(Arc::new(SyncDataCommandSender));
        init_exec_cmd_sender(Arc::new(SyncTradingCommandSender));
    }

    fn advance_clock_on_accumulator(
        accumulator: &mut TimeEventAccumulator,
        clock: &Rc<RefCell<dyn Clock>>,
        to_time_ns: UnixNanos,
        set_time: bool,
    ) {
        let mut clock_ref = clock.borrow_mut();
        let test_clock = clock_ref
            .as_any_mut()
            .downcast_mut::<TestClock>()
            .expect("BacktestEngine requires TestClock");
        accumulator.advance_clock(test_clock, to_time_ns, set_time);
    }

    fn set_all_clocks_time(clocks: &[Rc<RefCell<dyn Clock>>], ts: UnixNanos) {
        for clock in clocks {
            let mut clock_ref = clock.borrow_mut();
            let test_clock = clock_ref
                .as_any_mut()
                .downcast_mut::<TestClock>()
                .expect("BacktestEngine requires TestClock");
            test_clock.set_time(ts);
        }
    }

    fn log_pre_run(&self) {
        log::info!("=================================================================");
        log::info!(" BACKTEST PRE-RUN");
        log::info!("=================================================================");

        for exchange in self.venues.values() {
            let ex = exchange.borrow();
            log::info!(" SimulatedVenue {} ({})", ex.id, ex.account_type);
        }

        log::info!("-----------------------------------------------------------------");
    }

    fn log_run(&self) {
        log::info!("=================================================================");
        log::info!(" BACKTEST RUN");
        log::info!("=================================================================");
        log::info!("Run config ID:  {:?}", self.run_config_id);
        log::info!("Run ID:         {:?}", self.run_id);
        log::info!("Backtest start: {:?}", self.backtest_start);
        log::info!("Data elements:  {}", self.data_len);
        log::info!("-----------------------------------------------------------------");
    }

    fn log_post_run(&self) {
        let cache = self.kernel.cache.borrow();
        let total_orders = cache.orders_total_count(None, None, None, None, None);
        let positions = cache.positions(None, None, None, None, None);
        let total_positions = positions.len();

        log::info!("=================================================================");
        log::info!(" BACKTEST POST-RUN");
        log::info!("=================================================================");
        log::info!("Run config ID:  {:?}", self.run_config_id);
        log::info!("Run ID:         {:?}", self.run_id);
        log::info!("Run started:    {:?}", self.run_started);
        log::info!("Run finished:   {:?}", self.run_finished);
        log::info!("Backtest start: {:?}", self.backtest_start);
        log::info!("Backtest end:   {:?}", self.backtest_end);
        log::info!("Iterations:     {}", self.iteration);
        log::info!("Total orders:   {total_orders}");
        log::info!("Total positions:{total_positions}");

        if self.config.run_analysis {
            let analyzer = self.build_analyzer(&cache, &positions);

            for currency in analyzer.currencies() {
                log::info!("-----------------------------------------------------------------");
                log::info!(" PnL Statistics ({})", currency.code);

                if let Ok(pnl_lines) = analyzer.get_stats_pnls_formatted(Some(currency), None) {
                    for line in &pnl_lines {
                        log::info!(" {line}");
                    }
                }
            }

            log::info!("-----------------------------------------------------------------");
            log::info!(" Returns Statistics");
            for line in &analyzer.get_stats_returns_formatted() {
                log::info!(" {line}");
            }

            log::info!("-----------------------------------------------------------------");
            log::info!(" General Statistics");
            for line in &analyzer.get_stats_general_formatted() {
                log::info!(" {line}");
            }
        }

        log::info!("=================================================================");
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

        let venue = Venue::from(client_id.as_str());
        let backtest_client = BacktestDataClient::new(client_id, venue, self.kernel.cache.clone());
        let data_client_adapter = DataClientAdapter::new(
            backtest_client.client_id,
            None,
            false,
            false,
            Box::new(backtest_client),
        );

        self.kernel
            .data_engine
            .borrow_mut()
            .register_client(data_client_adapter, None);
    }

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
                Some(venue),
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
    use ahash::AHashMap;
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

    #[allow(clippy::missing_panics_doc)]
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
                AHashMap::new(),
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

        // Check the venue has been added
        assert_eq!(engine.venues.len(), 1);
        assert!(engine.venues.contains_key(&venue));

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
