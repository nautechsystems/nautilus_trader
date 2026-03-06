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
    enums::LogColor,
    log_info,
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
use nautilus_core::{UUID4, UnixNanos, datetime::unix_nanos_to_iso8601, formatting::Separable};
use nautilus_data::client::DataClientAdapter;
use nautilus_execution::models::{fee::FeeModelAny, fill::FillModelAny, latency::LatencyModel};
use nautilus_model::{
    accounts::{Account, AccountAny, margin_model::MarginModelAny},
    data::{Data, HasTsInit},
    enums::{AccountType, BookType, OmsType},
    identifiers::{AccountId, ClientId, InstrumentId, TraderId, Venue},
    instruments::{Instrument, InstrumentAny},
    orders::Order,
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
    modules::SimulationModule, result::BacktestResult,
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
/// - Transition from backtesting to live trading.
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
    last_module_ns: Option<UnixNanos>,
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
            last_module_ns: None,
            end_ns: UnixNanos::default(),
            run_started: None,
            run_finished: None,
            backtest_start: None,
            backtest_end: None,
        })
    }

    /// Returns a reference to the underlying kernel.
    #[must_use]
    pub const fn kernel(&self) -> &NautilusKernel {
        &self.kernel
    }

    /// Returns a mutable reference to the underlying kernel.
    pub fn kernel_mut(&mut self) -> &mut NautilusKernel {
        &mut self.kernel
    }

    /// Returns the trader ID for this engine.
    #[must_use]
    pub fn trader_id(&self) -> TraderId {
        self.kernel.trader_id()
    }

    /// Returns the unique instance ID for this engine.
    #[must_use]
    pub fn instance_id(&self) -> UUID4 {
        self.instance_id
    }

    /// Returns the current iteration count.
    #[must_use]
    pub fn iteration(&self) -> usize {
        self.iteration
    }

    /// Returns the list of registered venue identifiers.
    #[must_use]
    pub fn list_venues(&self) -> Vec<Venue> {
        self.venues.keys().copied().collect()
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
        margin_model: Option<MarginModelAny>,
        modules: Vec<Box<dyn SimulationModule>>,
        fill_model: FillModelAny,
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
        liquidity_consumption: Option<bool>,
        allow_cash_borrowing: Option<bool>,
        frozen_account: Option<bool>,
        queue_position: Option<bool>,
        oto_full_trigger: Option<bool>,
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
            margin_model,
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
            liquidity_consumption,
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
            queue_position,
            oto_full_trigger,
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

    /// Changes the fill model for the specified venue.
    pub fn change_fill_model(&mut self, venue: Venue, fill_model: FillModelAny) {
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
            exchange.borrow_mut().add_instrument(instrument.clone())?;
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

    /// Adds market data to the engine for replay during the backtest run.
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
    /// run pauses without finalizing so additional data batches can be loaded.
    /// Timer advancement stops at data exhaustion to avoid producing synthetic
    /// events (e.g. zero-volume bars) past the current batch.
    ///
    /// Streaming workflow:
    /// 1. Add initial data and strategies
    /// 2. Loop: call `run(streaming=true)`, `clear_data()`, `add_data(next_batch)`
    /// 3. After all batches: call `end()` to finalize
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
        self.run_impl(start, end, run_config_id, streaming)?;

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
        streaming: bool,
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
        self.last_module_ns = None;

        // Set all component clocks to start
        let clocks = self.collect_all_clocks();
        Self::set_all_clocks_time(&clocks, start_ns);

        // First-iteration initialization
        if self.iteration == 0 {
            self.run_config_id = run_config_id;
            self.run_id = Some(UUID4::new());
            self.run_started = Some(UnixNanos::from(std::time::SystemTime::now()));
            self.backtest_start = Some(start_ns);

            for exchange in self.venues.values() {
                let mut ex = exchange.borrow_mut();
                ex.initialize_account();
                ex.load_open_orders();
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
                if streaming {
                    // In streaming mode, don't advance timers past the
                    // current batch. The next batch will provide more data
                    // and timers will fire naturally as time advances.
                    break;
                }
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
            self.settle_venues(ts_init);

            let prev_last_ns = self.last_ns;
            data = self.data_iterator.next();

            // If timestamp changed (or exhausted), flush timers then run modules
            if data.is_none() || data.as_ref().unwrap().ts_init() > prev_last_ns {
                self.flush_accumulator_events(&clocks, prev_last_ns);
                self.run_venue_modules(prev_last_ns);
            }

            self.iteration += 1;
        }

        // Process remaining exchange messages
        let ts_now = self.kernel.clock.borrow().timestamp_ns();
        self.settle_venues(ts_now);
        self.run_venue_modules(ts_now);

        // Flush remaining timer events. In streaming mode only flush to the
        // last data timestamp to avoid advancing timers past the current batch.
        // The final flush to end_ns happens in end() or a non-streaming run.
        if streaming {
            self.flush_accumulator_events(&clocks, self.last_ns);
        } else {
            self.flush_accumulator_events(&clocks, end_ns);
        }

        Ok(())
    }

    /// Manually end the backtest.
    pub fn end(&mut self) {
        // Flush remaining timer events to the backtest end boundary so that
        // tail alerts/expiries scheduled after the last data point still fire.
        // Must run before stopping engines since DataEngine::stop() cancels
        // bar aggregator timers.
        if self.end_ns.as_u64() > 0 {
            let clocks = self.collect_all_clocks();
            self.flush_accumulator_events(&clocks, self.end_ns);
        }

        // Stop trader
        self.kernel.stop_trader();

        // Stop engines
        self.kernel.data_engine.borrow_mut().stop();
        self.kernel.risk_engine.borrow_mut().stop();
        self.kernel.exec_engine.borrow_mut().stop();

        // Process remaining exchange messages
        let ts_now = self.kernel.clock.borrow().timestamp_ns();
        self.settle_venues(ts_now);
        self.run_venue_modules(ts_now);

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
        self.last_module_ns = None;
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
        let orders = cache.orders(None, None, None, None, None);
        let total_events: usize = orders.iter().map(|o| o.event_count()).sum();
        let total_orders = orders.len();
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
            total_events,
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
        if matches!(
            data,
            Data::MarkPriceUpdate(_) | Data::IndexPriceUpdate(_) | Data::Custom(_)
        ) {
            return;
        }

        let venue = data.instrument_id().venue;
        if let Some(exchange) = self.venues.get(&venue) {
            let mut exchange = exchange.borrow_mut();

            match data {
                Data::Delta(delta) => exchange.process_order_book_delta(*delta),
                Data::Deltas(deltas) => exchange.process_order_book_deltas((**deltas).clone()),
                Data::Quote(quote) => exchange.process_quote_tick(quote),
                Data::Trade(trade) => exchange.process_trade_tick(trade),
                Data::Bar(bar) => exchange.process_bar(*bar),
                Data::InstrumentClose(close) => exchange.process_instrument_close(*close),
                Data::Depth10(depth) => exchange.process_order_book_depth10(depth),
                Data::MarkPriceUpdate(_) | Data::IndexPriceUpdate(_) | Data::Custom(_) => {
                    unreachable!("filtered by early return above")
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

            // Settle previous timestamp batch before advancing
            if let Some(ts) = ts_last
                && ts != ts_event
            {
                self.settle_venues(ts);
                self.run_venue_modules(ts);
            }

            ts_last = Some(ts_event);
            Self::set_all_clocks_time(clocks, ts_event);
            logging_clock_set_static_time(ts_event.as_u64());

            handler.run();
            self.drain_command_queues();

            // Re-advance clocks to capture chained timers
            for clock in clocks {
                Self::advance_clock_on_accumulator(&mut self.accumulator, clock, ts_now, false);
            }
        }

        // Settle the last timestamp batch
        if let Some(ts) = ts_last {
            self.settle_venues(ts);
            self.run_venue_modules(ts);
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

            // Settle previous timestamp batch before advancing
            if let Some(ts) = ts_last
                && ts != ts_event
            {
                self.settle_venues(ts);
                self.run_venue_modules(ts);
            }

            ts_last = Some(ts_event);
            Self::set_all_clocks_time(clocks, ts_event);
            logging_clock_set_static_time(ts_event.as_u64());

            handler.run();
            self.drain_command_queues();

            // Re-advance clocks to capture chained timers
            for clock in clocks {
                Self::advance_clock_on_accumulator(&mut self.accumulator, clock, ts_now, false);
            }
        }

        // Settle the last timestamp batch
        if let Some(ts) = ts_last {
            self.settle_venues(ts);
            self.run_venue_modules(ts);
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

    fn settle_venues(&self, ts_now: UnixNanos) {
        // Advance venue clocks so modules and event generators see the
        // correct timestamp even when no commands are pending
        for exchange in self.venues.values() {
            exchange.borrow().set_clock_time(ts_now);
        }

        // Drain commands then iterate matching engines to fill newly added
        // orders. Fills may enqueue further commands (e.g. hedge orders
        // submitted from on_order_filled), so loop until quiescent.
        // Only process and iterate venues that had pending commands each
        // pass, to avoid extra fill-model rolls on untouched venues.
        loop {
            let active_venues: Vec<Venue> = self
                .venues
                .iter()
                .filter(|(_, ex)| ex.borrow().has_pending_commands(ts_now))
                .map(|(id, _)| *id)
                .collect();

            if active_venues.is_empty() {
                break;
            }

            for venue_id in &active_venues {
                self.venues[venue_id].borrow_mut().process(ts_now);
            }
            self.drain_command_queues();

            for venue_id in &active_venues {
                self.venues[venue_id]
                    .borrow_mut()
                    .iterate_matching_engines(ts_now);
            }

            // Drain again so fill-triggered commands (e.g. hedge orders
            // from on_order_filled) are visible to has_pending_commands
            self.drain_command_queues();
        }
    }

    fn run_venue_modules(&mut self, ts_now: UnixNanos) {
        if self.last_module_ns == Some(ts_now) {
            return;
        }
        self.last_module_ns = Some(ts_now);

        // Pre-settle handler-generated work so modules see final state
        self.drain_command_queues();
        self.settle_venues(ts_now);

        for exchange in self.venues.values() {
            exchange.borrow_mut().process_modules(ts_now);
        }

        // Post-settle any commands emitted by modules
        self.drain_command_queues();
        self.settle_venues(ts_now);
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

    #[rustfmt::skip]
    fn log_pre_run(&self) {
        log_info!("=================================================================", color = LogColor::Cyan);
        log_info!(" BACKTEST PRE-RUN", color = LogColor::Cyan);
        log_info!("=================================================================", color = LogColor::Cyan);

        let cache = self.kernel.cache.borrow();
        for exchange in self.venues.values() {
            let ex = exchange.borrow();
            log_info!("=================================================================", color = LogColor::Cyan);
            log::info!(" SimulatedVenue {} ({})", ex.id, ex.account_type);
            log_info!("-----------------------------------------------------------------", color = LogColor::Cyan);

            if let Some(account) = cache.account_for_venue(&ex.id) {
                log::info!("Balances starting:");
                let account_ref: &dyn Account = match account {
                    AccountAny::Cash(cash) => cash,
                    AccountAny::Margin(margin) => margin,
                };
                for balance in account_ref.starting_balances().values() {
                    log::info!("  {balance}");
                }
            }
        }

        log_info!("-----------------------------------------------------------------", color = LogColor::Cyan);
    }

    #[rustfmt::skip]
    fn log_run(&self) {
        let config_id = self.run_config_id.as_deref().unwrap_or("None");
        let id = format_optional_uuid(self.run_id.as_ref());
        let start = format_optional_nanos(self.backtest_start);

        log_info!("=================================================================", color = LogColor::Cyan);
        log_info!(" BACKTEST RUN", color = LogColor::Cyan);
        log_info!("=================================================================", color = LogColor::Cyan);
        log::info!("Run config ID:  {config_id}");
        log::info!("Run ID:         {id}");
        log::info!("Backtest start: {start}");
        log::info!("Data elements:  {}", self.data_len);
        log_info!("-----------------------------------------------------------------", color = LogColor::Cyan);
    }

    #[rustfmt::skip]
    fn log_post_run(&self) {
        let cache = self.kernel.cache.borrow();
        let orders = cache.orders(None, None, None, None, None);
        let total_events: usize = orders.iter().map(|o| o.event_count()).sum();
        let total_orders = orders.len();
        let positions = cache.positions(None, None, None, None, None);
        let total_positions = positions.len();

        let config_id = self.run_config_id.as_deref().unwrap_or("None");
        let id = format_optional_uuid(self.run_id.as_ref());
        let started = format_optional_nanos(self.run_started);
        let finished = format_optional_nanos(self.run_finished);
        let elapsed = format_optional_duration(self.run_started, self.run_finished);
        let bt_start = format_optional_nanos(self.backtest_start);
        let bt_end = format_optional_nanos(self.backtest_end);
        let bt_range = format_optional_duration(self.backtest_start, self.backtest_end);
        let iterations = self.iteration.separate_with_underscores();
        let events = total_events.separate_with_underscores();
        let num_orders = total_orders.separate_with_underscores();
        let num_positions = total_positions.separate_with_underscores();

        log_info!("=================================================================", color = LogColor::Cyan);
        log_info!(" BACKTEST POST-RUN", color = LogColor::Cyan);
        log_info!("=================================================================", color = LogColor::Cyan);
        log::info!("Run config ID:  {config_id}");
        log::info!("Run ID:         {id}");
        log::info!("Run started:    {started}");
        log::info!("Run finished:   {finished}");
        log::info!("Elapsed time:   {elapsed}");
        log::info!("Backtest start: {bt_start}");
        log::info!("Backtest end:   {bt_end}");
        log::info!("Backtest range: {bt_range}");
        log::info!("Iterations: {iterations}");
        log::info!("Total events: {events}");
        log::info!("Total orders: {num_orders}");
        log::info!("Total positions: {num_positions}");

        if !self.config.run_analysis {
            return;
        }

        let analyzer = self.build_analyzer(&cache, &positions);
        log_portfolio_performance(&analyzer);
    }

    /// Registers a data client for the given `client_id` if one does not already exist.
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

    /// Registers a market data client for the given `venue` if one does not already exist.
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
                .register_client(data_client_adapter, Some(venue));
        }
    }
}

fn format_optional_nanos(nanos: Option<UnixNanos>) -> String {
    nanos.map_or("None".to_string(), unix_nanos_to_iso8601)
}

fn format_optional_uuid(uuid: Option<&UUID4>) -> String {
    uuid.map_or("None".to_string(), |id| id.to_string())
}

fn format_optional_duration(start: Option<UnixNanos>, end: Option<UnixNanos>) -> String {
    match (start, end) {
        (Some(s), Some(e)) => {
            let delta = e.to_datetime_utc() - s.to_datetime_utc();
            let days = delta.num_days().abs();
            let hours = delta.num_hours().abs() % 24;
            let minutes = delta.num_minutes().abs() % 60;
            let seconds = delta.num_seconds().abs() % 60;
            let micros = delta.subsec_nanos().unsigned_abs() / 1_000;
            format!("{days} days {hours:02}:{minutes:02}:{seconds:02}.{micros:06}")
        }
        _ => "None".to_string(),
    }
}

#[rustfmt::skip]
fn log_portfolio_performance(analyzer: &PortfolioAnalyzer) {
    log_info!("=================================================================", color = LogColor::Cyan);
    log_info!(" PORTFOLIO PERFORMANCE", color = LogColor::Cyan);
    log_info!("=================================================================", color = LogColor::Cyan);

    for currency in analyzer.currencies() {
        log::info!(" PnL Statistics ({})", currency.code);
        log_info!("-----------------------------------------------------------------", color = LogColor::Cyan);

        if let Ok(pnl_lines) = analyzer.get_stats_pnls_formatted(Some(currency), None) {
            for line in &pnl_lines {
                log::info!("{line}");
            }
        }

        log_info!("-----------------------------------------------------------------", color = LogColor::Cyan);
    }

    log::info!(" Returns Statistics");
    log_info!("-----------------------------------------------------------------", color = LogColor::Cyan);
    for line in &analyzer.get_stats_returns_formatted() {
        log::info!("{line}");
    }
    log_info!("-----------------------------------------------------------------", color = LogColor::Cyan);

    log::info!(" General Statistics");
    log_info!("-----------------------------------------------------------------", color = LogColor::Cyan);
    for line in &analyzer.get_stats_general_formatted() {
        log::info!("{line}");
    }
    log_info!("-----------------------------------------------------------------", color = LogColor::Cyan);
}
