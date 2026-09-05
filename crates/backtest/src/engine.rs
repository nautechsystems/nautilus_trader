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

use std::{
    any::Any,
    cell::RefCell,
    fmt::Debug,
    rc::{Rc, Weak},
    sync::Arc,
};

use ahash::{AHashMap, AHashSet};
use indexmap::IndexMap;
use nautilus_analysis::analyzer::PortfolioAnalyzer;
use nautilus_common::{
    actor::{DataActor, DataActorNative},
    cache::Cache,
    clock::{Clock, TestClock},
    component::{Component, component_state},
    enums::{ComponentState, LogColor},
    log_info,
    logging::{
        logging_clock_set_realtime_mode, logging_clock_set_static_mode,
        logging_clock_set_static_time,
    },
    runner::{
        SyncDataCommandSender, SyncTradingCommandSender, data_cmd_queue_is_empty,
        drain_data_cmd_queue, drain_trading_cmd_queue, replace_data_cmd_sender,
        replace_exec_cmd_sender, trading_cmd_queue_is_empty,
    },
    timer::{TimeEvent, TimeEventCallback},
};
use nautilus_core::{
    UUID4, UnixNanos, datetime::unix_nanos_to_iso8601, string::formatting::Separable,
    time::nanos_since_unix_epoch,
};
use nautilus_data::client::DataClientAdapter;
use nautilus_execution::models::fill::FillModelHandle;
use nautilus_model::{
    accounts::{Account, AccountAny},
    data::{Data, DataBatch, DataRef, HasTsInit},
    enums::{AccountType, AggregationSource, BookType},
    identifiers::{AccountId, ClientId, InstrumentId, StrategyId, TraderId, Venue},
    instruments::{Instrument, InstrumentAny},
    position::Position,
};
#[cfg(feature = "python")]
use nautilus_system::trader::Trader;
use nautilus_system::{config::NautilusKernelConfig, kernel::NautilusKernel};
use nautilus_trading::{
    ExecutionAlgorithm, ExecutionAlgorithmNative,
    strategy::{Strategy, StrategyNative},
};

use crate::{
    accumulator::TimeEventAccumulator,
    config::{BacktestEngineConfig, SimulatedVenueConfig},
    data_client::BacktestDataClient,
    data_iterator::BacktestDataIterator,
    exchange::{SettlementScope, SimulatedExchange},
    execution_client::BacktestExecutionClient,
    result::{
        BacktestResult, CanonicalBacktestResult, CanonicalBacktestState, CanonicalDiagnostic,
        CanonicalDiagnosticCode, CanonicalRunOutcome,
    },
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
    kernel: NautilusKernel,
    instance_id: UUID4,
    config: BacktestEngineConfig,
    accumulator: TimeEventAccumulator,
    run_config_id: Option<String>,
    run_id: Option<UUID4>,
    venues: IndexMap<Venue, Rc<RefCell<SimulatedExchange>>>,
    exec_clients: Vec<BacktestExecutionClient>,
    has_data: AHashSet<InstrumentId>,
    has_book_data: AHashSet<InstrumentId>,
    has_book_processed: AHashSet<InstrumentId>,
    data_iterator: BacktestDataIterator,
    data_len: usize,
    data_stream_counter: usize,
    ts_first: Option<UnixNanos>,
    ts_last_data: Option<UnixNanos>,
    sorted: bool,
    iteration: usize,
    force_stop: bool,
    last_ns: UnixNanos,
    last_module_ns: Option<UnixNanos>,
    last_liquidation_ns: Option<UnixNanos>,
    end_ns: UnixNanos,
    run_started: Option<UnixNanos>,
    run_finished: Option<UnixNanos>,
    backtest_start: Option<UnixNanos>,
    backtest_end: Option<UnixNanos>,
    funding_error: Option<String>,
}

impl Debug for BacktestEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(BacktestEngine))
            .field("instance_id", &self.instance_id)
            .field("run_config_id", &self.run_config_id)
            .field("run_id", &self.run_id)
            .finish_non_exhaustive()
    }
}

impl BacktestEngine {
    /// Create a new [`BacktestEngine`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the core `NautilusKernel` fails to initialize.
    pub fn new(mut config: BacktestEngineConfig) -> anyhow::Result<Self> {
        // The engine does not replay `add_instrument` on reset, so reruns rely
        // on the cache retaining instruments regardless of the caller's config.
        let mut cache_config = config.cache.unwrap_or_default();
        cache_config.drop_instruments_on_reset = false;
        config.cache = Some(cache_config);
        let kernel = NautilusKernel::new("BacktestEngine".to_string(), config.clone())?;
        let instance_id = kernel.instance_id;
        #[cfg(feature = "python")]
        if let Some(controller) = config.controller.as_ref() {
            Trader::add_controller_from_importable_config(&kernel.trader, controller)?;
        }
        #[cfg(not(feature = "python"))]
        if let Some(controller) = config.controller.as_ref() {
            anyhow::bail!(
                "BacktestEngineConfig.controller for importable controller '{}' requires the python feature",
                controller.controller_path
            );
        }

        Ok(Self {
            kernel,
            instance_id,
            config,
            accumulator: TimeEventAccumulator::new(),
            run_config_id: None,
            run_id: None,
            venues: IndexMap::new(),
            exec_clients: Vec::new(),
            has_data: AHashSet::new(),
            has_book_data: AHashSet::new(),
            has_book_processed: AHashSet::new(),
            data_iterator: BacktestDataIterator::new(),
            data_len: 0,
            data_stream_counter: 0,
            ts_first: None,
            ts_last_data: None,
            sorted: true,
            iteration: 0,
            force_stop: false,
            last_ns: UnixNanos::default(),
            last_module_ns: None,
            last_liquidation_ns: None,
            end_ns: UnixNanos::default(),
            run_started: None,
            run_finished: None,
            backtest_start: None,
            backtest_end: None,
            funding_error: None,
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

    /// Returns the machine ID for this engine.
    #[must_use]
    pub fn machine_id(&self) -> &str {
        self.kernel.machine_id()
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

    /// Returns the last run config ID, if any.
    #[must_use]
    pub fn run_config_id(&self) -> Option<&str> {
        self.run_config_id.as_deref()
    }

    /// Returns the last run ID, if any.
    #[must_use]
    pub const fn run_id(&self) -> Option<UUID4> {
        self.run_id
    }

    /// Returns when the last run started, if any.
    #[must_use]
    pub const fn run_started(&self) -> Option<UnixNanos> {
        self.run_started
    }

    /// Returns when the last run finished, if any.
    #[must_use]
    pub const fn run_finished(&self) -> Option<UnixNanos> {
        self.run_finished
    }

    /// Returns the last backtest range start, if any.
    #[must_use]
    pub const fn backtest_start(&self) -> Option<UnixNanos> {
        self.backtest_start
    }

    /// Returns the last backtest range end, if any.
    #[must_use]
    pub const fn backtest_end(&self) -> Option<UnixNanos> {
        self.backtest_end
    }

    /// Returns the list of registered venue identifiers.
    #[must_use]
    pub fn list_venues(&self) -> Vec<Venue> {
        self.venues.keys().copied().collect()
    }

    /// # Errors
    ///
    /// Returns an error if the venue is already registered, initializing the simulated exchange
    /// fails, or registering its execution client fails.
    pub fn add_venue(&mut self, config: SimulatedVenueConfig) -> anyhow::Result<()> {
        // `routing` and `frozen_account` flow to the exec client, so capture
        // them before the config is consumed by the exchange constructor.
        let venue = config.venue;
        if self.venues.contains_key(&venue) {
            anyhow::bail!("Venue {venue} is already registered");
        }

        let routing = Some(config.routing);
        let frozen_account = Some(config.frozen_account);
        let use_message_queue = config.use_message_queue;

        let exchange =
            SimulatedExchange::new(config, self.kernel.cache.clone(), self.kernel.clock.clone())?;
        let exchange = Rc::new(RefCell::new(exchange));

        let account_id = AccountId::from(format!("{venue}-001").as_str());

        let exec_client = BacktestExecutionClient::new(
            self.config.trader_id(),
            account_id,
            &exchange,
            self.kernel.cache.clone(),
            self.kernel.clock.clone(),
            routing,
            frozen_account,
        );

        if !use_message_queue {
            exchange
                .borrow_mut()
                .set_event_handler(exec_client.order_event_handler());
        }

        exchange
            .borrow_mut()
            .register_client(Rc::new(exec_client.clone()));

        self.kernel
            .exec_engine
            .borrow_mut()
            .register_client(Box::new(exec_client.clone()))?;

        SimulatedExchange::register_spread_quote_endpoint(&exchange);
        self.venues.insert(venue, exchange);
        self.exec_clients.push(exec_client);

        log::info!("Adding exchange {venue} to engine");

        Ok(())
    }

    /// Changes the fill model for the specified venue.
    pub fn change_fill_model(&mut self, venue: Venue, fill_model: FillModelHandle) {
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
    pub fn add_instrument(&mut self, instrument: &InstrumentAny) -> anyhow::Result<()> {
        let instrument_id = instrument.id();
        if let Some(exchange) = self.venues.get(&instrument.id().venue) {
            let previous_expiration_ns = exchange.borrow().instrument_expiration(instrument_id);

            if matches!(
                instrument,
                InstrumentAny::CurrencyPair(_) | InstrumentAny::TokenizedAsset(_)
            ) && exchange.borrow().account_type != AccountType::Margin
                && exchange.borrow().base_currency.is_some()
            {
                anyhow::bail!(
                    "Cannot add a multi-currency spot instrument {instrument_id} for a venue with a single-currency CASH account"
                )
            }
            exchange.borrow_mut().add_instrument(instrument.clone())?;
            if let Some(expiration_ns) = instrument.expiration_ns() {
                self.set_instrument_expiration_timer(exchange, instrument_id, expiration_ns)?;
            }

            if let Some(previous_expiration_ns) = previous_expiration_ns
                && instrument.expiration_ns() != Some(previous_expiration_ns)
                && !exchange
                    .borrow()
                    .has_unprocessed_instrument_expiration(previous_expiration_ns)
            {
                let timer_name = Self::instrument_expiration_timer_name(
                    instrument_id.venue,
                    previous_expiration_ns,
                );
                self.kernel.clock.borrow_mut().cancel_timer(&timer_name);
            }
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
            .process(instrument as &dyn Any);
        log::info!(
            "Added instrument {} to exchange {}",
            instrument_id,
            instrument_id.venue
        );
        Ok(())
    }

    /// Adds data to the engine for replay during the backtest run.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `data` is empty.
    /// - `validate` is `true`, the first element is built-in market data (excluding
    ///   custom and DeFi data), and its instrument has not been added to the cache via
    ///   [`add_instrument`](Self::add_instrument).
    /// - `validate` is `true` and the first element is a [`Data::Bar`] whose
    ///   `aggregation_source` is not [`AggregationSource::External`].
    pub fn add_data(
        &mut self,
        mut data: Vec<Data>,
        client_id: Option<ClientId>,
        validate: bool,
        sort: bool,
    ) -> anyhow::Result<()> {
        if sort {
            data.sort_by_key(HasTsInit::ts_init);
        }

        let stream_name =
            self.register_added_data(data.iter().map(DataRef::from), client_id, validate)?;
        self.data_iterator.add_data(&stream_name, data, true);
        self.sorted = sort;

        Ok(())
    }

    /// Adds a typed data batch to the engine for replay during the backtest run.
    ///
    /// The batch keeps its typed storage through replay, so no per-item [`Data`] value is
    /// constructed. Items are ordered by replay key as the batch is added; `sort` records whether
    /// the engine may run, matching [`add_data`](Self::add_data).
    ///
    /// # Errors
    ///
    /// Returns an error under the same conditions as [`add_data`](Self::add_data).
    pub fn add_data_batch(
        &mut self,
        data: DataBatch,
        client_id: Option<ClientId>,
        validate: bool,
        sort: bool,
    ) -> anyhow::Result<()> {
        let stream_name = self.register_added_data(
            (0..data.len()).filter_map(|index| data.get(index)),
            client_id,
            validate,
        )?;
        self.data_iterator.add_data_batch(&stream_name, data, true);
        self.sorted = sort;

        Ok(())
    }

    fn register_added_data<'a>(
        &mut self,
        items: impl Iterator<Item = DataRef<'a>> + Clone,
        client_id: Option<ClientId>,
        validate: bool,
    ) -> anyhow::Result<String> {
        #[cfg(not(feature = "defi"))]
        let _ = client_id;

        let Some(first) = items.clone().next() else {
            anyhow::bail!("data was empty");
        };

        if validate {
            // Validate against the first element only and assume the batch is
            // homogeneous (documented contract on add_data).
            #[cfg(feature = "defi")]
            let first_is_defi = matches!(first, DataRef::Defi(_));
            #[cfg(not(feature = "defi"))]
            let first_is_defi = false;

            if !first_is_defi && !matches!(first, DataRef::Custom(_)) {
                let first_instrument_id = first.instrument_id();
                anyhow::ensure!(
                    self.kernel
                        .cache
                        .borrow()
                        .instrument(&first_instrument_id)
                        .is_some(),
                    "Instrument {first_instrument_id} for the given data not found in the cache. \
                     Add the instrument through `add_instrument()` prior to adding related data."
                );

                if let DataRef::Bar(bar) = first {
                    anyhow::ensure!(
                        bar.bar_type.aggregation_source() == AggregationSource::External,
                        "bar_type.aggregation_source must be External, was {:?}",
                        bar.bar_type.aggregation_source(),
                    );
                }
            }
        }

        // Track has_data / has_book_data unconditionally so the depth-vs-data
        // run-time check still fires for callers that pass validate=false
        // (e.g. node.rs run_oneshot loading from a catalog). Time bounds are
        // also tracked here so start/end defaults are correct even when the
        // batch was added with sort=false.
        let mut count = 0;
        let mut batch_min_ts: Option<UnixNanos> = None;
        let mut batch_max_ts: Option<UnixNanos> = None;

        #[cfg(feature = "defi")]
        if items.clone().any(|item| matches!(item, DataRef::Defi(_))) {
            self.add_defi_data_client_if_not_exists(client_id);
        }

        for item in items {
            count += 1;
            let ts = item.ts_init();
            batch_min_ts = Some(batch_min_ts.map_or(ts, |cur| cur.min(ts)));
            batch_max_ts = Some(batch_max_ts.map_or(ts, |cur| cur.max(ts)));

            #[cfg(feature = "defi")]
            if matches!(item, DataRef::Defi(_)) {
                continue;
            }

            if matches!(item, DataRef::Custom(_)) {
                // Custom data routes by DataType and is independent of market venue bookkeeping.
                continue;
            }

            let instr_id = item.instrument_id();
            self.has_data.insert(instr_id);

            if item.is_order_book_data() {
                self.has_book_data.insert(instr_id);
            }

            self.add_market_data_client_if_not_exists(instr_id.venue);
        }

        if let Some(ts) = batch_min_ts
            && self.ts_first.is_none_or(|t| ts < t)
        {
            self.ts_first = Some(ts);
        }

        if let Some(ts) = batch_max_ts
            && self.ts_last_data.is_none_or(|t| ts > t)
        {
            self.ts_last_data = Some(ts);
        }

        self.data_len += count;
        let stream_name = format!("backtest_data_{}", self.data_stream_counter);
        self.data_stream_counter += 1;

        log::info!(
            "Added {count} data element{} to BacktestEngine ({} total)",
            if count == 1 { "" } else { "s" },
            self.data_len,
        );

        Ok(stream_name)
    }

    /// Adds an actor to the backtest engine.
    ///
    /// # Errors
    ///
    /// Returns an error if the actor is already registered or the trader is in an invalid
    /// state for actor registration.
    pub fn add_actor<T>(&mut self, actor: T) -> anyhow::Result<()>
    where
        T: DataActor + DataActorNative + Component + Debug + 'static,
    {
        self.kernel.trader.borrow_mut().add_actor(actor)
    }

    /// Adds the given actors to the backtest engine. Stops at the first error.
    ///
    /// # Errors
    ///
    /// Returns an error if any actor fails to register; preceding actors remain registered.
    pub fn add_actors<T>(&mut self, actors: Vec<T>) -> anyhow::Result<()>
    where
        T: DataActor + DataActorNative + Component + Debug + 'static,
    {
        for actor in actors {
            self.add_actor(actor)?;
        }
        Ok(())
    }

    /// Adds a strategy to the backtest engine.
    ///
    /// # Errors
    ///
    /// Returns an error if the strategy is already registered or the trader is in an invalid
    /// state for strategy registration.
    pub fn add_strategy<T>(&mut self, mut strategy: T) -> anyhow::Result<()>
    where
        T: Strategy + StrategyNative + DataActorNative + Component + Debug + 'static,
    {
        let strategy_id = self
            .kernel
            .trader
            .borrow()
            .prepare_strategy_for_registration(&mut strategy)?;
        let oms_type = StrategyNative::strategy_core(&strategy).config.oms_type;

        self.kernel.trader.borrow_mut().add_strategy(strategy)?;

        if let Some(oms_type) = oms_type {
            self.kernel
                .exec_engine
                .borrow_mut()
                .register_oms_type(strategy_id, oms_type);
        }

        Ok(())
    }

    /// Adds the given strategies to the backtest engine. Stops at the first error.
    ///
    /// # Errors
    ///
    /// Returns an error if any strategy fails to register; preceding strategies remain registered.
    pub fn add_strategies<T>(&mut self, strategies: Vec<T>) -> anyhow::Result<()>
    where
        T: Strategy + StrategyNative + DataActorNative + Component + Debug + 'static,
    {
        for strategy in strategies {
            self.add_strategy(strategy)?;
        }
        Ok(())
    }

    /// Adds an execution algorithm to the backtest engine.
    ///
    /// # Errors
    ///
    /// Returns an error if the algorithm is already registered or the trader is running.
    pub fn add_exec_algorithm<T>(&mut self, exec_algorithm: T) -> anyhow::Result<()>
    where
        T: ExecutionAlgorithm + ExecutionAlgorithmNative + Component + Debug + 'static,
    {
        self.kernel
            .trader
            .borrow_mut()
            .add_exec_algorithm(exec_algorithm)
    }

    /// Adds the given execution algorithms to the backtest engine. Stops at the first error.
    ///
    /// # Errors
    ///
    /// Returns an error if any execution algorithm fails to register; preceding algorithms remain
    /// registered.
    pub fn add_exec_algorithms<T>(&mut self, exec_algorithms: Vec<T>) -> anyhow::Result<()>
    where
        T: ExecutionAlgorithm + ExecutionAlgorithmNative + Component + Debug + 'static,
    {
        for exec_algorithm in exec_algorithms {
            self.add_exec_algorithm(exec_algorithm)?;
        }
        Ok(())
    }

    /// Run a backtest.
    ///
    /// Processes all data chronologically. When `streaming` is false (default),
    /// finalizes the run via [`end`](Self::end). When `streaming` is true, the
    /// run pauses without finalizing so additional data batches can be loaded.
    /// Timer advancement stops at data exhaustion to avoid producing synthetic
    /// events (e.g. zero-volume bars) past the current batch.
    ///
    /// Each streaming batch must include every data item with its final `ts_init`;
    /// splitting one replay timestamp across calls can finalize timers and venue
    /// modules before later items at that timestamp. [`BacktestNode`](crate::node::BacktestNode)
    /// aligns its chunks to this boundary.
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
        if let Some(error) = &self.funding_error {
            anyhow::bail!("{error}");
        }
        self.check_module_errors()?;

        if let Err(e) = self.run_impl(start, end, run_config_id, streaming) {
            if self.funding_error.is_some()
                || self
                    .venues
                    .values()
                    .any(|exchange| exchange.borrow().has_module_error())
            {
                self.abort_run();
            }
            return Err(e);
        }

        // Finalize on non-streaming runs, or when a shutdown was triggered
        // at any point during the run (including the trailing settle, module,
        // and flush callbacks that execute after the main data loop) so the
        // trader and engines actually stop.
        // Streaming batches retain commands deferred by other instruments,
        // and end() performs the unrestricted drain after all batches are loaded.
        if !streaming || self.force_stop || self.kernel.is_shutdown_requested() {
            self.end()?;
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
        anyhow::ensure!(
            self.sorted,
            "Data has been added but not sorted, call `engine.sort_data()` or use \
             `engine.add_data(..., sort=true)` before running"
        );

        for exchange in self.venues.values() {
            let exchange = exchange.borrow();
            let book_type_has_depth = exchange.book_type() as u8 > BookType::L1_MBP as u8;
            if !book_type_has_depth {
                continue;
            }

            for instrument_id in exchange.instrument_ids() {
                let has_data = self.has_data.contains(instrument_id);
                let missing_book_data = !self.has_book_data.contains(instrument_id)
                    && !self.has_book_processed.contains(instrument_id);

                if has_data && missing_book_data {
                    anyhow::bail!(
                        "No order book data found for instrument '{instrument_id}' when `book_type` \
                         is '{:?}'. Set the venue `book_type` to 'L1_MBP' (for top-of-book data \
                         like quotes, trades, and bars) or provide order book data for this \
                         instrument.",
                        exchange.book_type()
                    );
                }
            }
        }

        // Determine time boundaries
        let start_ns = start.unwrap_or_else(|| self.ts_first.unwrap_or_default());
        let end_ns = end.unwrap_or_else(|| self.ts_last_data.unwrap_or(start_ns));
        anyhow::ensure!(start_ns <= end_ns, "start was > end");
        self.end_ns = end_ns;
        self.last_ns = start_ns;
        self.last_module_ns = None;

        // Set all component clocks to start
        let clocks = self.collect_all_clocks();
        Self::set_all_clocks_time(&clocks, start_ns);

        // First-iteration initialization
        if self.iteration == 0 {
            self.set_instrument_expiration_timers()?;

            self.run_config_id = run_config_id;
            self.run_id = Some(UUID4::new());
            self.run_started = Some(UnixNanos::from(nanos_since_unix_epoch()));
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
            self.kernel.reset_shutdown_flag();

            // Initialize sync command senders (once per thread)
            Self::init_command_senders();

            // Set logging to static clock mode for deterministic timestamps
            logging_clock_set_static_mode();
            logging_clock_set_static_time(start_ns.as_u64());

            // Start kernel, then stop before trader startup for event-store replay
            self.kernel.start();
            if self.kernel.is_event_store_replay() {
                self.log_pre_run();
                return Ok(());
            }

            if self.kernel.is_event_store_replay_configured() {
                anyhow::bail!("event-store replay did not start");
            }
            self.kernel.start_trader()?;

            // Drain on_start data subscriptions so aggregators subscribe before the first data
            // point, else internal aggregation drops the first tick. Trading/exec stay queued
            while !data_cmd_queue_is_empty() {
                drain_data_cmd_queue();
            }

            self.log_pre_run();
        }

        self.log_run();

        // Skip data before start_ns
        while let Some(d) = self.data_iterator.peek() {
            if d.ts_init() >= start_ns {
                break;
            }
            self.data_iterator.advance();
        }

        // Initialize last_ns before first data point
        if let Some(d) = self.data_iterator.peek() {
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
            if self.kernel.is_shutdown_requested() {
                log::info!("Shutdown requested via ShutdownSystem, ending backtest");
                self.force_stop = true;
            }

            if self.force_stop {
                log::info!("Force stop triggered, ending backtest");
                break;
            }

            let Some(data) = self.data_iterator.peek() else {
                if streaming {
                    // In streaming mode, don't advance timers past the
                    // current batch. The next batch will provide more data
                    // and timers will fire naturally as time advances.
                    break;
                }
                let done = self.process_next_timer(&clocks)?;
                if self.data_iterator.peek().is_none() && done {
                    break;
                }
                continue;
            };

            let ts_init = data.ts_init();

            if ts_init > end_ns {
                break;
            }

            if ts_init > self.last_ns {
                self.advance_time_impl(ts_init, &clocks)?;
            }

            // A timer fired during clock advance may have requested shutdown,
            // skip delivering this data point in that case
            if self.kernel.is_shutdown_requested() {
                self.force_stop = true;
                break;
            }

            let settlement_scope = {
                let Some(data) = self.data_iterator.peek() else {
                    continue;
                };
                let settlement_scope = Self::settlement_scope(data);
                Self::route_data_to_exchange(
                    &self.venues,
                    &mut self.has_book_processed,
                    &self.kernel.clock,
                    data,
                )?;
                self.kernel.data_engine.borrow_mut().process_data_ref(data);
                settlement_scope
            };
            self.data_iterator.advance();

            // Drain deferred commands, then process exchange queues
            self.drain_command_queues();
            self.settle_venues(ts_init, settlement_scope);

            let prev_last_ns = self.last_ns;
            // If timestamp changed (or exhausted), flush timers then run modules
            if self
                .data_iterator
                .peek()
                .is_none_or(|next| next.ts_init() > prev_last_ns)
            {
                self.flush_accumulator_events(&clocks, prev_last_ns)?;
                self.finalize_timestamp(&clocks, prev_last_ns, settlement_scope)?;
            }

            self.iteration += 1;
        }

        if !streaming || self.force_stop || self.kernel.is_shutdown_requested() {
            let ts_now = self.kernel.clock.borrow().timestamp_ns();
            self.finalize_timestamp(&clocks, ts_now, SettlementScope::All)?;
        }

        // Cap at last_ns when streaming or after shutdown to avoid firing
        // timers past the current batch or the graceful stop
        let flush_ts = if streaming || self.force_stop || self.kernel.is_shutdown_requested() {
            self.last_ns
        } else {
            end_ns
        };
        self.flush_accumulator_events(&clocks, flush_ts)?;

        Ok(())
    }

    fn settlement_scope(data: DataRef<'_>) -> SettlementScope {
        match data {
            DataRef::BookDelta(_)
            | DataRef::BookDeltas(_)
            | DataRef::BookDepth10(_)
            | DataRef::Quote(_)
            | DataRef::Trade(_)
            | DataRef::Bar(_) => SettlementScope::Data(Some(data.instrument_id())),
            DataRef::MarkPrice(_) | DataRef::IndexPrice(_) => SettlementScope::Data(None),
            DataRef::FundingRate(_) => SettlementScope::Data(Some(data.instrument_id())),
            DataRef::OptionGreeks(_) => SettlementScope::Data(None),
            DataRef::InstrumentStatus(_) | DataRef::InstrumentClose(_) => {
                SettlementScope::Data(Some(data.instrument_id()))
            }
            DataRef::Custom(_) => SettlementScope::Data(None),
            #[cfg(feature = "defi")]
            DataRef::Defi(_) => SettlementScope::Data(None),
        }
    }

    fn abort_run(&mut self) {
        self.force_stop = true;
        self.accumulator.clear();
        self.kernel.stop_trader();
        self.kernel.data_engine.borrow_mut().stop();
        self.kernel.risk_engine.borrow_mut().stop();
        self.kernel.exec_engine.borrow_mut().stop();
        self.run_finished = Some(UnixNanos::from(nanos_since_unix_epoch()));
        self.backtest_end = Some(self.kernel.clock.borrow().timestamp_ns());
        logging_clock_set_realtime_mode();
    }

    /// Manually ends the backtest.
    ///
    /// # Errors
    ///
    /// Returns an error if actor or strategy state cannot be saved or a simulation module cannot
    /// produce its diagnostics.
    pub fn end(&mut self) -> anyhow::Result<()> {
        if let Some(error) = &self.funding_error {
            anyhow::bail!("{error}");
        }

        // Flush remaining timer events to the backtest end boundary so that
        // tail alerts/expiries scheduled after the last data point still fire.
        // Must run before stopping engines since DataEngine::stop() cancels
        // bar aggregator timers. When a shutdown was requested, cap the flush
        // at the last processed timestamp so timers scheduled past the stop
        // point do not fire extra callbacks after the graceful stop request.
        if self.end_ns.as_u64() > 0 {
            let clocks = self.collect_all_clocks();
            let flush_ts = if self.force_stop || self.kernel.is_shutdown_requested() {
                self.last_ns
            } else {
                self.end_ns
            };

            if let Err(e) = self.flush_accumulator_events(&clocks, flush_ts) {
                if self.funding_error.is_some()
                    || self
                        .venues
                        .values()
                        .any(|exchange| exchange.borrow().has_module_error())
                {
                    self.abort_run();
                }
                return Err(e);
            }
        }

        // Settle commands already due at the final data timestamp while strategies
        // are still running, so callbacks and on_stop observe the final state.
        let mut ts_now = self.kernel.clock.borrow().timestamp_ns();
        self.settle_venues(ts_now, SettlementScope::All);

        self.kernel.stop_trader();

        // Settle residual on_stop commands before stopping engines. Venue modules are
        // not re-run; process_modules is once per timestamp.

        // Drain first so latency-deferred commands reach venue inflight queues
        self.drain_command_queues();

        // Advance the clock to the latest inflight arrival; otherwise commands deferred
        // by a LatencyModel sit past ts_now and never settle.
        if let Some(max_inflight_ts) = self.max_inflight_command_ts()
            && max_inflight_ts > ts_now
        {
            ts_now = max_inflight_ts;
            let clocks = self.collect_all_clocks();
            Self::set_all_clocks_time(&clocks, ts_now);
        }

        self.settle_venues(ts_now, SettlementScope::All);

        for strategy_id in self.running_strategy_ids() {
            log::error!(
                "Strategy {strategy_id} is still RUNNING after the backtest end sequence; its stop did not complete",
            );
        }

        let save_result = self.kernel.save_trader_state();
        let diagnostics_result = self
            .venues
            .values()
            .try_for_each(|exchange| exchange.borrow().log_diagnostics());
        self.kernel.portfolio.borrow_mut().finalize_equity_curve();

        // Stop engines
        self.kernel.data_engine.borrow_mut().stop();
        self.kernel.risk_engine.borrow_mut().stop();
        self.kernel.exec_engine.borrow_mut().stop();

        let streaming_result = self.kernel.flush_streaming();

        self.run_finished = Some(UnixNanos::from(nanos_since_unix_epoch()));
        self.backtest_end = Some(self.kernel.clock.borrow().timestamp_ns());

        // Switch logging back to realtime mode
        logging_clock_set_realtime_mode();

        self.log_post_run();
        save_result?;
        diagnostics_result?;
        streaming_result
    }

    /// Returns registered strategies whose state resolves to `Running` after the end sequence.
    ///
    /// Known causes include a stop deferred for a managed market exit that never completed,
    /// and an earlier component stop failure making `Trader::stop_components` return before
    /// reaching the strategy - so callers must report the state observed rather than
    /// attribute a cause.
    fn running_strategy_ids(&self) -> Vec<StrategyId> {
        self.kernel
            .trader
            .borrow()
            .strategy_ids()
            .into_iter()
            .filter(|strategy_id| match component_state(&strategy_id.inner()) {
                Ok(state) => matches!(state, ComponentState::Running),
                Err(e) => {
                    log::warn!("Cannot resolve stop state for strategy {strategy_id}: {e}");
                    false
                }
            })
            .collect()
    }

    /// Reset the backtest engine.
    ///
    /// All stateful fields are reset to their initial value. Data and instruments
    /// persist across resets to enable repeated runs with different strategies.
    ///
    /// # Errors
    ///
    /// Returns an error if ending the current run or resetting a simulation module fails.
    pub fn reset(&mut self) -> anyhow::Result<()> {
        log::debug!("Resetting");

        let mut reset_error = None;

        if self.kernel.trader.borrow().is_running()
            && let Err(e) = self.end()
        {
            reset_error = Some(e);
        }

        // Stop and reset engines
        self.kernel.data_engine.borrow_mut().stop();
        self.kernel.data_engine.borrow_mut().reset();

        self.kernel.exec_engine.borrow_mut().stop();

        // Reset exchanges before the exec engine wipes the cache so
        // exchange.reset() can see the prior run's account.
        for exchange in self.venues.values() {
            if let Err(e) = exchange.borrow_mut().reset()
                && reset_error.is_none()
            {
                reset_error = Some(e);
            }
        }
        self.kernel.exec_engine.borrow_mut().reset();

        self.kernel.risk_engine.borrow_mut().stop();
        self.kernel.risk_engine.borrow_mut().reset();

        self.kernel.order_emulator.reset();

        // Reset trader
        if let Err(e) = self.kernel.trader.borrow_mut().reset() {
            log::error!("Error resetting trader: {e:?}");
        }

        self.kernel.portfolio.borrow_mut().reset();

        // Clear run state
        self.run_config_id = None;
        self.run_id = None;
        self.run_started = None;
        self.run_finished = None;
        self.backtest_start = None;
        self.backtest_end = None;
        self.funding_error = None;
        self.iteration = 0;
        self.force_stop = false;
        self.last_ns = UnixNanos::default();
        self.last_module_ns = None;
        self.last_liquidation_ns = None;
        self.end_ns = UnixNanos::default();
        self.has_book_processed.clear();

        self.accumulator.clear();
        self.cancel_funding_settlement_timers();

        // Reset all iterator cursors to beginning (data persists)
        self.data_iterator.reset_all_cursors();

        log::info!("Reset");

        if let Some(e) = reset_error {
            return Err(e);
        }
        Ok(())
    }

    /// Sort the engine's internal data stream by timestamp.
    ///
    /// Useful when data has been added with `sort=false` for batch performance,
    /// then sorted once before running.
    pub fn sort_data(&mut self) {
        // Each add call creates its own stream; the iterator merges streams by
        // replay timestamp across streams. Mark the engine as sorted so `run`
        // no longer rejects it.
        self.sorted = true;
        log::info!("Data sort requested (iterator merges streams by replay timestamp)");
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
        self.sorted = true;
    }

    /// Clear all actors from the engine's internal trader.
    ///
    /// # Errors
    ///
    /// Returns an error if any actor fails to dispose.
    pub fn clear_actors(&mut self) -> anyhow::Result<()> {
        self.kernel.trader.borrow_mut().clear_actors()
    }

    /// Clear all trading strategies from the engine's internal trader.
    ///
    /// # Errors
    ///
    /// Returns an error if any strategy fails to dispose.
    pub fn clear_strategies(&mut self) -> anyhow::Result<()> {
        self.kernel.trader.borrow_mut().clear_strategies()
    }

    /// Clear all execution algorithms from the engine's internal trader.
    ///
    /// # Errors
    ///
    /// Returns an error if any execution algorithm fails to dispose.
    pub fn clear_exec_algorithms(&mut self) -> anyhow::Result<()> {
        self.kernel.trader.borrow_mut().clear_exec_algorithms()
    }

    /// Dispose of the backtest engine, releasing all resources.
    pub fn dispose(&mut self) {
        self.clear_data();
        self.accumulator.clear();
        self.kernel.dispose();
    }

    /// Return the backtest result from the last run.
    #[must_use]
    pub fn get_result(&self) -> BacktestResult {
        let elapsed_time_secs = match (self.backtest_start, self.backtest_end) {
            (Some(start), Some(end)) => (end.as_f64() - start.as_f64()) / 1_000_000_000.0,
            _ => 0.0,
        };

        let cache = self.kernel.cache.borrow();
        let orders = cache.orders(None, None, None, None, None);
        let total_events = event_count_as_usize(self.kernel.exec_engine.borrow().event_count());
        let total_orders = orders.len();
        let positions: Vec<Position> = cache
            .positions(None, None, None, None, None)
            .into_iter()
            .map(|p| p.cloned())
            .collect();
        let cached_positions_count = positions.len();
        let snapshot_positions = cache.position_snapshots(None, None).len();
        let total_positions = Self::total_positions_with_snapshots(&cache, cached_positions_count);
        let summary = self.build_result_summary(
            &cache,
            total_events,
            total_orders,
            cached_positions_count,
            snapshot_positions,
        );

        let stats = self.kernel.portfolio.borrow().statistics();
        let stats_pnls = stats.pnls;
        let stats_returns = stats.returns;
        let stats_general = stats.general;
        let returns_series = stats.returns_series;

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
            summary,
            stats_pnls,
            stats_returns,
            stats_general,
            returns_series,
        }
    }

    /// Returns the versioned deterministic projection of observable state from the last run.
    ///
    /// This projection excludes host, process, random identity, wall-clock, and elapsed-time noise.
    /// It retains deterministic references between domain events and includes the observable cache,
    /// account, portfolio, component, outcome, and diagnostic state available after the run ends.
    ///
    /// # Errors
    ///
    /// Returns an error if observable state cannot be projected into the canonical schema.
    pub fn get_canonical_result(&self) -> anyhow::Result<CanonicalBacktestResult> {
        let result = self.get_result();
        let cache = self.kernel.cache.borrow();
        let orders = cache
            .orders(None, None, None, None, None)
            .into_iter()
            .map(|order| order.cloned())
            .collect();
        let positions = cache
            .positions(None, None, None, None, None)
            .into_iter()
            .map(|position| position.cloned())
            .collect();
        let position_snapshots = cache.position_snapshots(None, None);
        let accounts = cache.accounts_all_owned();
        drop(cache);

        let portfolio = self.kernel.portfolio.borrow();
        let mut portfolio_snapshots = Vec::new();
        for account in &accounts {
            portfolio_snapshots.extend(portfolio.snapshots(&account.id()));
        }
        drop(portfolio);

        let trader = self.kernel.trader.borrow();
        let trader_state = trader.state().to_string();
        let actor_ids = trader
            .actor_ids()
            .into_iter()
            .map(|id| id.to_string())
            .collect();
        let strategy_ids = trader
            .strategy_ids()
            .into_iter()
            .map(|id| id.to_string())
            .collect();
        let exec_algorithm_ids = trader
            .exec_algorithm_ids()
            .into_iter()
            .map(|id| id.to_string())
            .collect();
        drop(trader);

        let outcome = if self.funding_error.is_some() {
            CanonicalRunOutcome::Failed
        } else if self.run_finished.is_none() {
            CanonicalRunOutcome::Incomplete
        } else if self.force_stop || self.kernel.is_shutdown_requested() {
            CanonicalRunOutcome::Stopped
        } else {
            CanonicalRunOutcome::Completed
        };
        let diagnostics = self
            .funding_error
            .as_ref()
            .map(|_| CanonicalDiagnostic {
                code: CanonicalDiagnosticCode::FundingSettlementFailed,
            })
            .into_iter()
            .collect();
        let statistics = nautilus_analysis::PortfolioStatistics {
            pnls: result.stats_pnls,
            returns: result.stats_returns,
            general: result.stats_general,
            returns_series: result.returns_series,
        };

        CanonicalBacktestResult::from_state(CanonicalBacktestState {
            trader_id: result.trader_id,
            run_config_id: result.run_config_id,
            backtest_start: result.backtest_start,
            backtest_end: result.backtest_end,
            iterations: result.iterations,
            total_events: result.total_events,
            total_orders: result.total_orders,
            total_positions: result.total_positions,
            outcome,
            diagnostics,
            trader_state,
            actor_ids,
            strategy_ids,
            exec_algorithm_ids,
            summary: result.summary.into_iter().collect(),
            orders,
            positions,
            position_snapshots,
            accounts,
            portfolio_snapshots,
            statistics,
        })
    }

    fn build_result_summary(
        &self,
        cache: &Cache,
        total_events: usize,
        total_orders: usize,
        cached_positions_count: usize,
        snapshot_positions: usize,
    ) -> AHashMap<String, String> {
        let mut summary = AHashMap::new();
        summary.insert("iterations".to_string(), self.iteration.to_string());
        summary.insert("total_events".to_string(), total_events.to_string());
        summary.insert("orders.total".to_string(), total_orders.to_string());
        summary.insert(
            "orders.open".to_string(),
            cache
                .orders_open_count(None, None, None, None, None)
                .to_string(),
        );
        summary.insert(
            "orders.closed".to_string(),
            cache
                .orders_closed_count(None, None, None, None, None)
                .to_string(),
        );
        summary.insert(
            "orders.emulated".to_string(),
            cache
                .orders_emulated_count(None, None, None, None, None)
                .to_string(),
        );
        summary.insert(
            "orders.inflight".to_string(),
            cache
                .orders_inflight_count(None, None, None, None, None)
                .to_string(),
        );
        summary.insert(
            "positions.total".to_string(),
            cached_positions_count.to_string(),
        );
        summary.insert(
            "positions.open".to_string(),
            cache
                .positions_open_count(None, None, None, None, None)
                .to_string(),
        );
        summary.insert(
            "positions.closed".to_string(),
            cache
                .positions_closed_count(None, None, None, None, None)
                .to_string(),
        );
        summary.insert(
            "positions.snapshots".to_string(),
            snapshot_positions.to_string(),
        );
        summary.insert(
            "positions.total_with_snapshots".to_string(),
            (cached_positions_count + snapshot_positions).to_string(),
        );

        let mut venues: Vec<Venue> = self.venues.keys().copied().collect();
        venues.sort_by_key(ToString::to_string);
        summary.insert("venues.total".to_string(), venues.len().to_string());

        for venue in venues {
            let Some(account) = cache.account_for_venue(&venue) else {
                continue;
            };

            let venue_key = venue.to_string();
            let account_key = format!("account.{venue_key}");
            summary.insert(format!("{account_key}.id"), account.id().to_string());
            summary.insert(
                format!("{account_key}.type"),
                account.account_type().to_string(),
            );
            summary.insert(
                format!("{account_key}.base_currency"),
                account
                    .base_currency()
                    .map_or_else(|| "None".to_string(), |currency| currency.code.to_string()),
            );
            summary.insert(
                format!("{account_key}.event_count"),
                account.event_count().to_string(),
            );

            let mut balances: Vec<_> = account.balances().into_iter().collect();
            balances.sort_by_key(|(currency, _)| currency.code.to_string());

            for (currency, balance) in balances {
                let balance_key = format!("{account_key}.balance.{}", currency.code);
                summary.insert(format!("{balance_key}.total"), balance.total.to_string());
                summary.insert(format!("{balance_key}.free"), balance.free.to_string());
                summary.insert(format!("{balance_key}.locked"), balance.locked.to_string());
            }
        }

        summary
    }

    fn route_data_to_exchange(
        venues: &IndexMap<Venue, Rc<RefCell<SimulatedExchange>>>,
        has_book_processed: &mut AHashSet<InstrumentId>,
        clock: &Rc<RefCell<dyn Clock>>,
        data: DataRef<'_>,
    ) -> anyhow::Result<()> {
        if matches!(
            data,
            DataRef::MarkPrice(_)
                | DataRef::IndexPrice(_)
                | DataRef::OptionGreeks(_)
                | DataRef::Custom(_)
        ) {
            return Ok(());
        }
        #[cfg(feature = "defi")]
        if matches!(data, DataRef::Defi(_)) {
            return Ok(());
        }

        let venue = data.instrument_id().venue;
        if let Some(exchange) = venues.get(&venue) {
            let mut exchange_ref = exchange.borrow_mut();
            let mut processed_book_data = false;

            match data {
                DataRef::BookDelta(delta) => {
                    exchange_ref.process_order_book_delta(*delta)?;
                    processed_book_data = true;
                }
                DataRef::BookDeltas(deltas) => {
                    exchange_ref.process_order_book_deltas(deltas)?;
                    processed_book_data = true;
                }
                DataRef::BookDepth10(depth) => {
                    exchange_ref.process_order_book_depth10(depth)?;
                    processed_book_data = true;
                }
                DataRef::Quote(quote) => exchange_ref.process_quote_tick(quote)?,
                DataRef::Trade(trade) => exchange_ref.process_trade_tick(trade)?,
                DataRef::Bar(bar) => exchange_ref.process_bar(*bar)?,
                DataRef::MarkPrice(_) | DataRef::IndexPrice(_) => {
                    unreachable!("filtered before exchange routing")
                }
                DataRef::FundingRate(funding) => {
                    let settlement_ns =
                        exchange_ref.process_funding_rate_deferred(*funding, data.ts_init())?;
                    Self::schedule_funding_settlement_if_required(clock, venue, settlement_ns);
                }
                DataRef::OptionGreeks(_) => unreachable!("filtered before exchange routing"),
                DataRef::InstrumentStatus(status) => {
                    exchange_ref.process_instrument_status(*status)?;
                }
                DataRef::InstrumentClose(close) => {
                    exchange_ref.process_instrument_close(*close)?;
                }
                DataRef::Custom(_) => unreachable!("filtered before exchange routing"),
                #[cfg(feature = "defi")]
                DataRef::Defi(_) => unreachable!("filtered before exchange routing"),
            }

            drop(exchange_ref);

            if processed_book_data {
                has_book_processed.insert(data.instrument_id());
            }
        } else {
            log::warn!("No exchange found for venue {venue}, data not routed");
        }
        Ok(())
    }

    fn check_module_errors(&self) -> anyhow::Result<()> {
        for exchange in self.venues.values() {
            exchange.borrow().check_module_error()?;
        }
        Ok(())
    }

    fn advance_time_impl(
        &mut self,
        ts_now: UnixNanos,
        clocks: &[Rc<RefCell<dyn Clock>>],
    ) -> anyhow::Result<()> {
        for clock in clocks {
            Self::advance_clock_on_accumulator(&mut self.accumulator, clock, ts_now, false);
        }

        // Process events with ts_event < ts_now
        let ts_before = if ts_now.as_u64() > 0 {
            UnixNanos::from(ts_now.as_u64() - 1)
        } else {
            UnixNanos::default()
        };

        let mut shutdown_at: Option<UnixNanos> = None;

        while let Some(ts_event) = self
            .accumulator
            .peek_next_time()
            .filter(|ts_event| *ts_event <= ts_before)
        {
            self.run_timer_handlers_at(clocks, ts_event, ts_now);

            if self.kernel.is_shutdown_requested() {
                self.accumulator.clear();
                shutdown_at = Some(ts_event);
                break;
            }
            self.finalize_timestamp(clocks, ts_event, SettlementScope::All)?;

            if self.kernel.is_shutdown_requested() {
                self.accumulator.clear();
                shutdown_at = Some(ts_event);
                break;
            }

            for clock in clocks {
                Self::advance_clock_on_accumulator(&mut self.accumulator, clock, ts_now, false);
            }
        }

        // On a mid-drain shutdown, anchor state at the firing timer's ts so
        // post-run settlement and backtest_end reflect the graceful stop
        if let Some(ts_event) = shutdown_at {
            self.last_ns = ts_event;
        } else {
            self.last_ns = ts_now;
            Self::set_all_clocks_time(clocks, ts_now);
            logging_clock_set_static_time(ts_now.as_u64());
        }

        Ok(())
    }

    fn flush_accumulator_events(
        &mut self,
        clocks: &[Rc<RefCell<dyn Clock>>],
        ts_now: UnixNanos,
    ) -> anyhow::Result<()> {
        // Bail after shutdown so handler-scheduled alerts do not fire post-stop
        if self.kernel.is_shutdown_requested() {
            self.accumulator.clear();
            return Ok(());
        }

        let last_ns = self.last_ns;

        for clock in clocks {
            Self::advance_clock_on_accumulator(&mut self.accumulator, clock, ts_now, false);
        }

        while let Some(ts_event) = self
            .accumulator
            .peek_next_time()
            .filter(|ts_event| *ts_event <= ts_now)
        {
            self.run_timer_handlers_at(clocks, ts_event, ts_now);

            if self.kernel.is_shutdown_requested() {
                self.accumulator.clear();
                break;
            }
            self.finalize_timestamp(clocks, ts_event, SettlementScope::All)?;

            if self.kernel.is_shutdown_requested() {
                self.accumulator.clear();
                break;
            }

            for clock in clocks {
                Self::advance_clock_on_accumulator(&mut self.accumulator, clock, ts_now, false);
            }
        }

        if !self.kernel.is_shutdown_requested() {
            self.last_ns = last_ns;
        }

        Ok(())
    }

    fn process_next_timer(&mut self, clocks: &[Rc<RefCell<dyn Clock>>]) -> anyhow::Result<bool> {
        self.flush_accumulator_events(clocks, self.last_ns)?;

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
            None => Ok(true),
            Some(t) if t > self.end_ns => Ok(true),
            Some(t) => {
                self.last_ns = t;
                self.flush_accumulator_events(clocks, t)?;
                Ok(false)
            }
        }
    }

    fn run_timer_handlers_at(
        &mut self,
        clocks: &[Rc<RefCell<dyn Clock>>],
        ts_event: UnixNanos,
        advance_to: UnixNanos,
    ) {
        self.last_ns = ts_event;
        while self.accumulator.peek_next_time() == Some(ts_event) {
            let handler = self
                .accumulator
                .pop_next_at_or_before(ts_event)
                .expect("timer exists at timestamp");
            Self::set_all_clocks_time(clocks, ts_event);
            logging_clock_set_static_time(ts_event.as_u64());
            handler.run();
            self.drain_command_queues();

            if self.kernel.is_shutdown_requested() {
                return;
            }

            for clock in clocks {
                Self::advance_clock_on_accumulator(&mut self.accumulator, clock, advance_to, false);
            }
        }
    }

    fn finalize_timestamp(
        &mut self,
        clocks: &[Rc<RefCell<dyn Clock>>],
        ts_now: UnixNanos,
        mut settlement_scope: SettlementScope,
    ) -> anyhow::Result<()> {
        loop {
            self.settle_venues(ts_now, settlement_scope);

            if self.kernel.is_shutdown_requested() {
                self.accumulator.clear();
                break;
            }

            for clock in clocks {
                Self::advance_clock_on_accumulator(&mut self.accumulator, clock, ts_now, false);
            }

            if self.accumulator.peek_next_time() == Some(ts_now) {
                self.run_timer_handlers_at(clocks, ts_now, ts_now);
                settlement_scope = SettlementScope::All;
                continue;
            }

            if !self.settle_funding_rates(ts_now)? {
                break;
            }
            settlement_scope = SettlementScope::All;
        }

        self.run_venue_modules(ts_now, settlement_scope)?;
        self.run_venue_liquidations(ts_now, settlement_scope);
        Ok(())
    }

    fn settle_funding_rates(&mut self, ts_now: UnixNanos) -> anyhow::Result<bool> {
        let mut due = self
            .venues
            .iter()
            .flat_map(|(venue, exchange)| {
                exchange
                    .borrow()
                    .funding_boundaries_due(ts_now)
                    .into_iter()
                    .map(|(boundary, instrument_id)| (boundary, *venue, instrument_id))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        due.sort_unstable();

        if let Some((boundary, venue, instrument_id)) = due
            .iter()
            .copied()
            .find(|(boundary, _, _)| *boundary < ts_now)
        {
            return self.fail_funding(format!(
                "Late funding boundary for {instrument_id} on {venue}: {boundary} < replay timestamp {ts_now}"
            ));
        }

        if due.is_empty() {
            return Ok(false);
        }

        for (boundary, venue, instrument_id) in due {
            if !self.venues[&venue]
                .borrow_mut()
                .settle_funding_boundary(boundary, instrument_id)
            {
                return self.fail_funding(format!(
                    "Funding settlement failed for {instrument_id} on {venue} at {boundary}"
                ));
            }
        }

        let next_boundaries = self
            .venues
            .iter()
            .filter_map(|(venue, exchange)| {
                exchange
                    .borrow()
                    .next_funding_boundary()
                    .map(|boundary| (*venue, boundary))
            })
            .collect::<Vec<_>>();

        for (venue, boundary) in next_boundaries {
            Self::schedule_funding_settlement_if_required(
                &self.kernel.clock,
                venue,
                Some(boundary),
            );
        }

        Ok(true)
    }

    fn fail_funding<T>(&mut self, error: String) -> anyhow::Result<T> {
        if self.funding_error.is_none() {
            self.funding_error = Some(error.clone());
        }
        Err(anyhow::anyhow!(error))
    }

    fn set_instrument_expiration_timers(&self) -> anyhow::Result<()> {
        for exchange in self.venues.values() {
            let expirations = exchange.borrow().instrument_expirations();
            for (instrument_id, expiration_ns) in expirations {
                self.set_instrument_expiration_timer(exchange, instrument_id, expiration_ns)?;
            }
        }

        Ok(())
    }

    fn set_instrument_expiration_timer(
        &self,
        exchange: &Rc<RefCell<SimulatedExchange>>,
        instrument_id: InstrumentId,
        expiration_ns: UnixNanos,
    ) -> anyhow::Result<()> {
        if expiration_ns == UnixNanos::default() {
            return Ok(());
        }

        let timer_name = Self::instrument_expiration_timer_name(instrument_id.venue, expiration_ns);
        let timer_key = ustr::Ustr::from(timer_name.as_str());
        if self.kernel.clock.borrow().timer_exists(&timer_key) {
            return Ok(());
        }

        let exchange: Weak<RefCell<SimulatedExchange>> = Rc::downgrade(exchange);
        let callback: Rc<dyn Fn(TimeEvent)> = Rc::new(move |event: TimeEvent| {
            if let Some(exchange) = exchange.upgrade() {
                exchange
                    .borrow_mut()
                    .process_instrument_expirations(event.ts_event);
            }
        });
        let mut clock = self.kernel.clock.borrow_mut();

        clock.set_time_alert_ns(
            &timer_name,
            expiration_ns,
            Some(TimeEventCallback::from(callback)),
            None,
        )?;

        Ok(())
    }

    fn instrument_expiration_timer_name(venue: Venue, expiration_ns: UnixNanos) -> String {
        format!("INSTRUMENT-EXPIRATION:{venue}:{expiration_ns}")
    }

    fn schedule_funding_settlement_if_required(
        clock: &Rc<RefCell<dyn Clock>>,
        venue: Venue,
        settlement_ns: Option<UnixNanos>,
    ) {
        let Some(settlement_ns) = settlement_ns else {
            return;
        };

        if let Err(e) = Self::set_funding_settlement_timer(clock, venue, settlement_ns) {
            log::error!("Cannot schedule funding settlement for {venue}: {e}");
        }
    }

    fn set_funding_settlement_timer(
        clock: &Rc<RefCell<dyn Clock>>,
        venue: Venue,
        settlement_ns: UnixNanos,
    ) -> anyhow::Result<()> {
        let timer_name = Self::funding_settlement_timer_name(venue);
        let callback: Rc<dyn Fn(TimeEvent)> = Rc::new(|_| {});
        let mut clock = clock.borrow_mut();

        clock.set_time_alert_ns(
            &timer_name,
            settlement_ns,
            Some(TimeEventCallback::from(callback)),
            None,
        )?;

        Ok(())
    }

    fn funding_settlement_timer_name(venue: Venue) -> String {
        format!("FUNDING-SETTLEMENT:{venue}")
    }

    fn cancel_funding_settlement_timers(&self) {
        let mut clock = self.kernel.clock.borrow_mut();
        for venue in self.venues.keys() {
            clock.cancel_timer(&Self::funding_settlement_timer_name(*venue));
        }
    }

    fn collect_all_clocks(&self) -> Vec<Rc<RefCell<dyn Clock>>> {
        let mut clocks = vec![self.kernel.clock.clone()];
        clocks.extend(self.kernel.trader.borrow().get_component_clocks());
        clocks
    }

    fn max_inflight_command_ts(&self) -> Option<UnixNanos> {
        self.venues
            .values()
            .filter_map(|v| v.borrow().max_inflight_command_ts())
            .max()
    }

    fn settle_venues(&self, ts_now: UnixNanos, settlement_scope: SettlementScope) {
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
            // Drain first so commands buffered in the trading queue (e.g. from
            // on_stop handlers) reach the venues before we check for activity.
            self.drain_command_queues();

            let active_venues: Vec<Venue> = self
                .venues
                .iter()
                .filter(|(_, ex)| {
                    ex.borrow()
                        .has_pending_commands_for_scope(ts_now, settlement_scope)
                })
                .map(|(id, _)| *id)
                .collect();

            if active_venues.is_empty() {
                break;
            }

            for venue_id in &active_venues {
                let mut exchange = self.venues[venue_id].borrow_mut();
                exchange.process_for_scope(ts_now, settlement_scope);
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

    fn run_venue_modules(
        &mut self,
        ts_now: UnixNanos,
        settlement_scope: SettlementScope,
    ) -> anyhow::Result<()> {
        if self.last_module_ns == Some(ts_now) {
            return Ok(());
        }
        self.last_module_ns = Some(ts_now);

        if self
            .venues
            .values()
            .all(|exchange| !exchange.borrow().has_modules())
        {
            return Ok(());
        }

        // Pre-settle handler-generated work so modules see final state
        self.drain_command_queues();
        self.settle_venues(ts_now, settlement_scope);

        for exchange in self.venues.values() {
            exchange.borrow_mut().process_modules(ts_now)?;
        }

        // Post-settle any commands emitted by modules
        self.drain_command_queues();
        self.settle_venues(ts_now, settlement_scope);
        Ok(())
    }

    fn run_venue_liquidations(&mut self, ts_now: UnixNanos, settlement_scope: SettlementScope) {
        if self.last_liquidation_ns == Some(ts_now) {
            return;
        }
        self.last_liquidation_ns = Some(ts_now);

        if self
            .venues
            .values()
            .all(|exchange| !exchange.borrow().liquidation_enabled())
        {
            return;
        }

        for exchange in self.venues.values() {
            exchange.borrow_mut().process_liquidations(ts_now);
        }

        self.drain_command_queues();
        self.settle_venues(ts_now, settlement_scope);
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
        replace_data_cmd_sender(Arc::new(SyncDataCommandSender));
        replace_exec_cmd_sender(Arc::new(SyncTradingCommandSender));
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
                let account_ref: &dyn Account = match &*account {
                    AccountAny::Margin(margin) => margin,
                    AccountAny::Cash(cash) => cash,
                    AccountAny::Betting(betting) => betting,
                    AccountAny::Wallet(wallet) => wallet,
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
        let total_events = event_count_as_usize(self.kernel.exec_engine.borrow().event_count());
        let total_orders = orders.len();
        let positions: Vec<Position> = cache
            .positions(None, None, None, None, None)
            .into_iter()
            .map(|p| p.cloned())
            .collect();
        let total_positions = Self::total_positions_with_snapshots(&cache, positions.len());

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

        log_portfolio_performance(&self.kernel.portfolio.borrow().analyzer());
    }

    fn total_positions_with_snapshots(cache: &Cache, cached_positions_count: usize) -> usize {
        cached_positions_count + cache.position_snapshots(None, None).len()
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
    uuid.map_or("None".to_string(), ToString::to_string)
}

fn event_count_as_usize(event_count: u64) -> usize {
    usize::try_from(event_count).expect("execution event count fits usize")
}

fn format_optional_duration(start: Option<UnixNanos>, end: Option<UnixNanos>) -> String {
    match (start, end) {
        (Some(s), Some(e)) => {
            let delta = s.to_datetime_utc().duration_until(e.to_datetime_utc());
            let days = delta.as_hours().abs() / 24;
            let hours = delta.as_hours().abs() % 24;
            let minutes = delta.as_mins().abs() % 60;
            let seconds = delta.as_secs().abs() % 60;
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

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use indexmap::IndexMap;
    use nautilus_common::{
        actor::DataActor,
        enums::Environment,
        messages::{
            data::{DataCommand, UnsubscribeCommand},
            execution::{ModifyOrder, SubmitOrder, TradingCommand},
        },
        msgbus::{
            self, MessagingSwitchboard, TypedHandler,
            stubs::{TypedIntoMessageSavingHandler, get_typed_into_message_saving_handler},
        },
    };
    use nautilus_execution::engine::{SnapshotAnchorer, stubs::StubExecutionClient};
    use nautilus_model::{
        data::{Data, InstrumentStatus, QuoteTick},
        enums::{
            AccountType, BookType, MarketStatus, MarketStatusAction, OmsType, OrderSide,
            OrderStatus, OrderType, TriggerType,
        },
        events::OrderEventAny,
        identifiers::{AccountId, ActorId, ClientId, ClientOrderId, PositionId, StrategyId, Venue},
        instruments::{
            CryptoPerpetual, Instrument, InstrumentAny, stubs::crypto_perpetual_ethusdt,
        },
        orders::{
            Order, OrderAny, OrderTestBuilder,
            stubs::{OrderFilledTestBuilder, TestOrderEventStubs},
        },
        types::{Money, Price, Quantity},
    };
    use nautilus_system::{KernelEventStore, RegisteredComponents};
    use nautilus_testkit::{
        cache::TestCacheDatabaseControl,
        components::{StateActor, StateStrategy},
    };
    use nautilus_trading::{
        nautilus_strategy,
        strategy::{config::StrategyConfig, core::StrategyCore},
    };
    use rstest::*;
    use ustr::Ustr;

    use super::*;
    use crate::modules::{
        AccountAdjustmentOutcome, ExchangeContext, SimulationModule, SimulationModuleHandle,
        SimulationModuleResult,
    };

    #[derive(Debug)]
    struct BacktestReplayKernelEventStore {
        fail_restore: bool,
    }

    impl KernelEventStore for BacktestReplayKernelEventStore {
        fn restore_parent_cache(
            &mut self,
            _instance_id: UUID4,
            _cache: &mut Cache,
        ) -> anyhow::Result<()> {
            if self.fail_restore {
                anyhow::bail!("replay restore failed");
            }

            Ok(())
        }

        fn open(
            &mut self,
            _instance_id: UUID4,
            _components: &RegisteredComponents,
            _environment: Environment,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        fn snapshot_anchorer(&self) -> Option<SnapshotAnchorer> {
            None
        }

        fn seal(&mut self, _ts_init: UnixNanos) {}

        fn run_id(&self) -> Option<&str> {
            Some("replay-child")
        }

        fn parent_run_id(&self) -> Option<&str> {
            Some("seed-run")
        }

        fn is_event_store_replay_configured(&self) -> bool {
            true
        }

        fn is_halted(&self) -> bool {
            false
        }
    }

    #[derive(Debug)]
    struct TestStrategy {
        core: StrategyCore,
    }

    impl TestStrategy {
        fn new(config: StrategyConfig) -> Self {
            Self {
                core: StrategyCore::new(config),
            }
        }
    }

    impl DataActor for TestStrategy {}

    nautilus_strategy!(TestStrategy);

    struct TestSimulationModule {
        process_count: Rc<Cell<u32>>,
    }

    impl SimulationModule for TestSimulationModule {
        fn pre_process(&self, _data: &Data) -> anyhow::Result<()> {
            Ok(())
        }

        fn process(
            &self,
            _ts_now: UnixNanos,
            _ctx: &ExchangeContext,
        ) -> anyhow::Result<SimulationModuleResult> {
            self.process_count.set(self.process_count.get() + 1);
            Ok(SimulationModuleResult::NotReady)
        }

        fn acknowledge(&self, _outcomes: &[AccountAdjustmentOutcome]) -> anyhow::Result<()> {
            Ok(())
        }

        fn log_diagnostics(&self) -> anyhow::Result<()> {
            Ok(())
        }

        fn reset(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    fn create_engine() -> BacktestEngine {
        let mut engine = BacktestEngine::new(BacktestEngineConfig::default()).unwrap();
        let venue_config = SimulatedVenueConfig::builder()
            .venue(Venue::from("BINANCE"))
            .oms_type(OmsType::Netting)
            .account_type(AccountType::Margin)
            .book_type(BookType::L1_MBP)
            .starting_balances(vec![Money::from("1_000_000 USDT")])
            .build()
            .unwrap();
        engine.add_venue(venue_config).unwrap();
        engine
    }

    fn create_immediate_engine(instrument: &CryptoPerpetual) -> BacktestEngine {
        let mut engine = BacktestEngine::new(BacktestEngineConfig::default()).unwrap();
        let venue_config = SimulatedVenueConfig::builder()
            .venue(instrument.id().venue)
            .oms_type(OmsType::Netting)
            .account_type(AccountType::Margin)
            .book_type(BookType::L1_MBP)
            .starting_balances(vec![Money::from("1_000_000 USDT")])
            .use_message_queue(false)
            .build()
            .unwrap();
        engine.add_venue(venue_config).unwrap();
        engine
            .add_instrument(&InstrumentAny::CryptoPerpetual(instrument.clone()))
            .unwrap();
        engine
            .venues
            .get(&instrument.id().venue)
            .unwrap()
            .borrow_mut()
            .initialize_account();
        engine
    }

    fn create_engine_with_strategy(manage_stop: bool) -> (BacktestEngine, StrategyId) {
        let mut engine = create_engine();
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        let strategy_id = StrategyId::from(if manage_stop {
            "MANAGED-STOP-001"
        } else {
            "IMMEDIATE-STOP-001"
        });
        engine.add_instrument(&instrument).unwrap();
        engine
            .add_strategy(TestStrategy::new(StrategyConfig {
                strategy_id: Some(strategy_id),
                manage_stop,
                ..Default::default()
            }))
            .unwrap();

        if manage_stop {
            let order = OrderTestBuilder::new(OrderType::Market)
                .trader_id(engine.trader_id())
                .strategy_id(strategy_id)
                .instrument_id(instrument.id())
                .side(OrderSide::Buy)
                .quantity(Quantity::from("1.000"))
                .build();
            let fill = OrderFilledTestBuilder::new(&order, &instrument).build();
            let OrderEventAny::Filled(fill) = fill else {
                unreachable!();
            };
            let position = Position::new(&instrument, fill);
            engine
                .kernel
                .cache
                .borrow_mut()
                .add_position_without_order(&position, OmsType::Netting)
                .unwrap();
        }

        (engine, strategy_id)
    }

    fn send_execution_command(command: TradingCommand) {
        msgbus::send_trading_command(MessagingSwitchboard::exec_engine_execute(), command);
    }

    #[rstest]
    #[case(false, false, 0)]
    #[case(false, true, 1)]
    #[case(true, true, 1)]
    fn test_run_venue_modules_settles_only_when_enabled(
        #[case] first_enabled: bool,
        #[case] second_enabled: bool,
        #[case] expected_ns: u64,
    ) {
        let mut engine = BacktestEngine::new(BacktestEngineConfig::default()).unwrap();
        let process_count = Rc::new(Cell::new(0));

        for (venue, enabled) in [
            (Venue::from("BINANCE"), first_enabled),
            (Venue::from("SIM"), second_enabled),
        ] {
            let modules = enabled
                .then(|| {
                    SimulationModuleHandle::new(TestSimulationModule {
                        process_count: Rc::clone(&process_count),
                    })
                })
                .into_iter()
                .collect();
            let venue_config = SimulatedVenueConfig::builder()
                .venue(venue)
                .oms_type(OmsType::Netting)
                .account_type(AccountType::Margin)
                .book_type(BookType::L1_MBP)
                .starting_balances(vec![Money::from("1_000_000 USDT")])
                .modules(modules)
                .build()
                .unwrap();
            engine.add_venue(venue_config).unwrap();
        }

        engine
            .run_venue_modules(UnixNanos::from(1), SettlementScope::All)
            .unwrap();

        assert_eq!(
            engine.kernel.clock.borrow().timestamp_ns(),
            UnixNanos::from(expected_ns)
        );
        assert_eq!(
            process_count.get(),
            u32::from(first_enabled) + u32::from(second_enabled)
        );
    }

    #[rstest]
    #[case(false, false, 0)]
    #[case(false, true, 1)]
    #[case(true, true, 1)]
    fn test_run_venue_liquidations_settles_only_when_enabled(
        #[case] first_enabled: bool,
        #[case] second_enabled: bool,
        #[case] expected_ns: u64,
    ) {
        let mut engine = BacktestEngine::new(BacktestEngineConfig::default()).unwrap();

        for (venue, enabled) in [
            (Venue::from("BINANCE"), first_enabled),
            (Venue::from("SIM"), second_enabled),
        ] {
            let venue_config = SimulatedVenueConfig::builder()
                .venue(venue)
                .oms_type(OmsType::Netting)
                .account_type(AccountType::Margin)
                .book_type(BookType::L1_MBP)
                .starting_balances(vec![Money::from("1_000_000 USDT")])
                .liquidation_enabled(enabled)
                .build()
                .unwrap();
            engine.add_venue(venue_config).unwrap();
        }

        engine.run_venue_liquidations(UnixNanos::from(1), SettlementScope::All);

        assert_eq!(
            engine.kernel.clock.borrow().timestamp_ns(),
            UnixNanos::from(expected_ns)
        );
    }

    #[rstest]
    fn test_immediate_submit_defers_order_events(crypto_perpetual_ethusdt: CryptoPerpetual) {
        let engine = create_immediate_engine(&crypto_perpetual_ethusdt);
        let order = OrderTestBuilder::new(OrderType::Limit)
            .trader_id(engine.trader_id())
            .instrument_id(crypto_perpetual_ethusdt.id)
            .client_order_id(ClientOrderId::from("O-IMMEDIATE-SUBMIT"))
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.000"))
            .price(Price::from("1000.00"))
            .build();
        engine
            .kernel
            .cache
            .borrow_mut()
            .add_order(order.clone(), None, Some(ClientId::from("BINANCE")), false)
            .unwrap();

        send_execution_command(TradingCommand::SubmitOrder(SubmitOrder::new(
            order.trader_id(),
            Some(ClientId::from("BINANCE")),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            order.init_event().clone(),
            order.exec_algorithm_id(),
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
        )));

        {
            let cache = engine.kernel.cache.borrow();
            let cached_order = cache.order(&order.client_order_id()).unwrap();
            assert_eq!(cached_order.status(), OrderStatus::Initialized);
            assert_eq!(cached_order.event_count(), 1);
        }

        engine.drain_command_queues();

        let cache = engine.kernel.cache.borrow();
        let cached_order = cache.order(&order.client_order_id()).unwrap();
        let events = cached_order.events();
        assert!(matches!(events[1], OrderEventAny::Submitted(_)));
        assert!(matches!(events[2], OrderEventAny::Accepted(_)));
    }

    #[rstest]
    fn test_immediate_modify_submitted_order_defers_updated_event(
        crypto_perpetual_ethusdt: CryptoPerpetual,
    ) {
        let engine = create_immediate_engine(&crypto_perpetual_ethusdt);
        let order = OrderTestBuilder::new(OrderType::Limit)
            .trader_id(engine.trader_id())
            .instrument_id(crypto_perpetual_ethusdt.id)
            .client_order_id(ClientOrderId::from("O-IMMEDIATE-MODIFY"))
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.000"))
            .price(Price::from("1000.00"))
            .build();
        let account_id = AccountId::from("BINANCE-001");
        engine
            .kernel
            .cache
            .borrow_mut()
            .add_order(order.clone(), None, Some(ClientId::from("BINANCE")), false)
            .unwrap();
        engine
            .kernel
            .cache
            .borrow_mut()
            .update_order(&TestOrderEventStubs::submitted(&order, account_id))
            .unwrap();

        send_execution_command(TradingCommand::ModifyOrder(ModifyOrder::new(
            order.trader_id(),
            Some(ClientId::from("BINANCE")),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            None,
            Some(Quantity::from("2.000")),
            None,
            None,
            UUID4::new(),
            UnixNanos::from(1),
            None,
            None,
        )));

        {
            let cache = engine.kernel.cache.borrow();
            let cached_order = cache.order(&order.client_order_id()).unwrap();
            assert_eq!(cached_order.quantity(), Quantity::from("1.000"));
            assert!(matches!(
                cached_order.events().last(),
                Some(OrderEventAny::Submitted(_))
            ));
        }

        engine.drain_command_queues();

        let cache = engine.kernel.cache.borrow();
        let order = cache.order(&order.client_order_id()).unwrap();
        assert_eq!(order.quantity(), Quantity::from("2.000"));
        assert!(matches!(
            order.events().last(),
            Some(OrderEventAny::Updated(_))
        ));
    }

    #[rstest]
    fn test_immediate_market_data_dispatches_fill_synchronously(
        crypto_perpetual_ethusdt: CryptoPerpetual,
    ) {
        let engine = create_immediate_engine(&crypto_perpetual_ethusdt);
        let order = OrderTestBuilder::new(OrderType::Limit)
            .trader_id(engine.trader_id())
            .instrument_id(crypto_perpetual_ethusdt.id)
            .client_order_id(ClientOrderId::from("O-IMMEDIATE-QUOTE-FILL"))
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.000"))
            .price(Price::from("1000.00"))
            .build();
        engine
            .kernel
            .cache
            .borrow_mut()
            .add_order(order.clone(), None, Some(ClientId::from("BINANCE")), false)
            .unwrap();

        send_execution_command(TradingCommand::SubmitOrder(SubmitOrder::new(
            order.trader_id(),
            Some(ClientId::from("BINANCE")),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            order.init_event().clone(),
            order.exec_algorithm_id(),
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
        )));
        engine.drain_command_queues();

        let quote = QuoteTick::new(
            order.instrument_id(),
            Price::from("999.00"),
            Price::from("1000.00"),
            Quantity::from("1.000"),
            Quantity::from("1.000"),
            UnixNanos::from(1),
            UnixNanos::from(1),
        );
        msgbus::send_quote(
            format!(
                "SimulatedExchange.process_new_quote.{}",
                order.instrument_id().venue
            )
            .into(),
            &quote,
        );

        let cache = engine.kernel.cache.borrow();
        let cached_order = cache.order(&order.client_order_id()).unwrap();
        assert_eq!(cached_order.status(), OrderStatus::Filled);
        assert!(matches!(
            cached_order.events().last(),
            Some(OrderEventAny::Filled(_))
        ));
    }

    #[rstest]
    fn test_timer_handler_sets_last_ns_to_fire_time() {
        let mut engine = create_engine();
        engine.last_ns = UnixNanos::from(30);
        let fired = Rc::new(Cell::new(false));
        let fired_clone = Rc::clone(&fired);
        let callback = TimeEventCallback::RustLocal(Rc::new(move |_| {
            fired_clone.set(true);
        }));
        engine
            .kernel
            .clock
            .borrow_mut()
            .set_timer_ns(
                "ROLL",
                1,
                Some(UnixNanos::from(20)),
                None,
                Some(callback),
                Some(true),
                Some(true),
            )
            .unwrap();
        let clocks = engine.collect_all_clocks();

        for clock in &clocks {
            BacktestEngine::advance_clock_on_accumulator(
                &mut engine.accumulator,
                clock,
                UnixNanos::from(30),
                false,
            );
        }
        engine.run_timer_handlers_at(&clocks, UnixNanos::from(20), UnixNanos::from(30));

        assert!(fired.get());
        assert_eq!(engine.last_ns, UnixNanos::from(20));
    }

    #[rstest]
    #[case::complete(false, 25)]
    #[case::shutdown(true, 20)]
    fn test_flush_accumulator_events_sets_last_ns_for_completion(
        #[case] shutdown: bool,
        #[case] expected_last_ns: u64,
    ) {
        let mut engine = create_engine();
        let last_ns = UnixNanos::from(25);
        let ts_now = UnixNanos::from(30);
        engine.last_ns = last_ns;
        let clocks = engine.collect_all_clocks();
        BacktestEngine::set_all_clocks_time(&clocks, last_ns);
        let fired = Rc::new(Cell::new(false));
        let fired_clone = Rc::clone(&fired);
        let observed_ns = Rc::new(Cell::new(UnixNanos::default()));
        let observed_ns_clone = Rc::clone(&observed_ns);
        let clock = Rc::clone(&engine.kernel.clock);
        let shutdown_requested = engine.kernel.shutdown_flag();
        let callback = TimeEventCallback::RustLocal(Rc::new(move |_| {
            fired_clone.set(true);
            observed_ns_clone.set(clock.borrow().timestamp_ns());
            shutdown_requested.set(shutdown);
        }));
        engine
            .kernel
            .clock
            .borrow_mut()
            .set_timer_ns(
                "ROLL",
                100,
                Some(UnixNanos::from(20)),
                None,
                Some(callback),
                Some(true),
                Some(true),
            )
            .unwrap();

        engine.flush_accumulator_events(&clocks, ts_now).unwrap();

        assert!(fired.get());
        assert_eq!(observed_ns.get(), UnixNanos::from(20));
        assert_eq!(engine.kernel.is_shutdown_requested(), shutdown);
        assert_eq!(engine.last_ns, UnixNanos::from(expected_last_ns));
    }

    #[rstest]
    fn test_add_duplicate_venue_preserves_original_exchange(
        crypto_perpetual_ethusdt: CryptoPerpetual,
    ) {
        let mut engine = BacktestEngine::new(BacktestEngineConfig::default()).unwrap();
        let venue = Venue::from("BINANCE");
        let venue_config = SimulatedVenueConfig::builder()
            .venue(venue)
            .oms_type(OmsType::Netting)
            .account_type(AccountType::Margin)
            .book_type(BookType::L1_MBP)
            .starting_balances(vec![Money::from("1_000_000 USDT")])
            .build()
            .unwrap();
        engine.add_venue(venue_config).unwrap();

        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
        let instrument_id = instrument.id();
        engine.add_instrument(&instrument).unwrap();

        let initial_quote = QuoteTick::new(
            instrument_id,
            Price::from("1000.00"),
            Price::from("1001.00"),
            Quantity::from("1.000"),
            Quantity::from("1.000"),
            UnixNanos::from(1),
            UnixNanos::from(1),
        );
        msgbus::send_quote(
            format!("SimulatedExchange.process_new_quote.{venue}").into(),
            &initial_quote,
        );

        let best_bid_before = engine
            .venues
            .get(&venue)
            .unwrap()
            .borrow()
            .best_bid_price(instrument_id);
        let best_ask_before = engine
            .venues
            .get(&venue)
            .unwrap()
            .borrow()
            .best_ask_price(instrument_id);
        let original_exchange = Rc::downgrade(engine.venues.get(&venue).unwrap());
        let venues_before = engine.list_venues();
        let exec_clients_len_before = engine.exec_clients.len();
        let client_ids_before = engine.kernel.exec_engine.borrow().client_ids();
        let duplicate_config = SimulatedVenueConfig::builder()
            .venue(venue)
            .oms_type(OmsType::Netting)
            .account_type(AccountType::Margin)
            .book_type(BookType::L1_MBP)
            .starting_balances(vec![Money::from("1_000_000 USDT")])
            .build()
            .unwrap();
        assert!(engine.add_venue(duplicate_config).is_err());

        let original_exchange = original_exchange
            .upgrade()
            .expect("the original exchange must remain alive");
        assert!(Rc::ptr_eq(
            &original_exchange,
            engine.venues.get(&venue).unwrap()
        ));
        assert_eq!(engine.list_venues(), venues_before);
        assert_eq!(engine.exec_clients.len(), exec_clients_len_before);
        assert_eq!(
            engine.kernel.exec_engine.borrow().client_ids(),
            client_ids_before
        );

        let distinct_quote = QuoteTick::new(
            instrument_id,
            Price::from("2000.00"),
            Price::from("2001.00"),
            Quantity::from("2.000"),
            Quantity::from("2.000"),
            UnixNanos::from(2),
            UnixNanos::from(2),
        );
        msgbus::send_quote(
            format!("SimulatedExchange.process_new_quote.{venue}").into(),
            &distinct_quote,
        );

        let original_exchange = original_exchange.borrow();
        let best_bid_after = original_exchange.best_bid_price(instrument_id);
        let best_ask_after = original_exchange.best_ask_price(instrument_id);
        assert_ne!(best_bid_after, best_bid_before);
        assert_ne!(best_ask_after, best_ask_before);
        assert_eq!(best_bid_after, Some(Price::from("2000.00")));
        assert_eq!(best_ask_after, Some(Price::from("2001.00")));
    }

    #[rstest]
    fn test_add_venue_execution_registration_failure_publishes_nothing() {
        let mut engine = BacktestEngine::new(BacktestEngineConfig::default()).unwrap();
        let venue = Venue::from("SIM");
        engine
            .kernel
            .exec_engine
            .borrow_mut()
            .register_client(Box::new(StubExecutionClient::new(
                ClientId::from(venue.as_str()),
                AccountId::from("SIM-001"),
                venue,
                OmsType::Netting,
                None,
            )))
            .unwrap();
        let client_ids_before = engine.kernel.exec_engine.borrow().client_ids();

        let endpoint = format!("SimulatedExchange.process_new_quote.{venue}");
        let received_quotes = Rc::new(RefCell::new(Vec::new()));
        let received_quotes_handler = Rc::clone(&received_quotes);
        let sentinel = TypedHandler::from_with_id("venue-setup-sentinel", move |quote| {
            received_quotes_handler.borrow_mut().push(*quote);
        });
        msgbus::register_quote_endpoint(endpoint.as_str().into(), sentinel);

        let venue_config = SimulatedVenueConfig::builder()
            .venue(venue)
            .oms_type(OmsType::Netting)
            .account_type(AccountType::Margin)
            .book_type(BookType::L1_MBP)
            .starting_balances(vec![Money::from("1_000_000 USD")])
            .build()
            .unwrap();
        assert!(engine.add_venue(venue_config).is_err());

        assert!(!engine.venues.contains_key(&venue));
        assert!(engine.exec_clients.is_empty());
        assert_eq!(
            engine.kernel.exec_engine.borrow().client_ids(),
            client_ids_before
        );

        let quote = QuoteTick::new(
            InstrumentId::from("TEST.SIM"),
            Price::from("100.00"),
            Price::from("101.00"),
            Quantity::from("1"),
            Quantity::from("1"),
            UnixNanos::from(1),
            UnixNanos::from(1),
        );
        msgbus::send_quote(endpoint.as_str().into(), &quote);
        assert_eq!(received_quotes.borrow().as_slice(), &[quote]);
    }

    #[rstest]
    fn test_add_strategy_registers_configured_hedging_oms_type() {
        let mut engine = create_engine();
        let instrument = crypto_perpetual_ethusdt();
        let strategy_id = StrategyId::from("FUNDING_ARBITRAGE-001");

        engine
            .add_instrument(&InstrumentAny::CryptoPerpetual(instrument.clone()))
            .unwrap();
        engine
            .add_strategy(TestStrategy::new(StrategyConfig {
                strategy_id: Some(strategy_id),
                oms_type: Some(OmsType::Hedging),
                ..Default::default()
            }))
            .unwrap();

        let order = OrderTestBuilder::new(OrderType::Market)
            .trader_id(engine.trader_id())
            .strategy_id(strategy_id)
            .instrument_id(instrument.id())
            .quantity(Quantity::from("1.000"))
            .build();
        let position_id = PositionId::new("CUSTOM-POSITION-001");

        engine
            .kernel
            .exec_engine
            .borrow()
            .cache()
            .borrow_mut()
            .add_order(
                order.clone(),
                Some(position_id),
                Some(ClientId::from("BINANCE")),
                true,
            )
            .unwrap();

        let submit_order = SubmitOrder::new(
            order.trader_id(),
            Some(ClientId::from("BINANCE")),
            strategy_id,
            instrument.id(),
            order.client_order_id(),
            order.init_event().clone(),
            order.exec_algorithm_id(),
            Some(position_id),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
        );

        engine
            .kernel
            .exec_engine
            .borrow()
            .execute(TradingCommand::SubmitOrder(submit_order));

        let exec_engine = engine.kernel.exec_engine.borrow();
        let cache = exec_engine.cache().borrow();
        let cached_order = cache
            .order(&order.client_order_id())
            .expect("Order should be cached");

        assert_eq!(cached_order.status(), OrderStatus::Initialized);
    }

    fn create_engine_with_replay_store(fail_restore: bool) -> BacktestEngine {
        let config = BacktestEngineConfig {
            load_state: true,
            run_analysis: false,
            ..Default::default()
        };
        let mut engine = BacktestEngine::new(config.clone()).unwrap();
        let event_store_factory = move |_instance_id: UUID4, _clock: Rc<RefCell<dyn Clock>>| {
            Ok::<_, anyhow::Error>(Box::new(BacktestReplayKernelEventStore { fail_restore })
                as Box<dyn KernelEventStore>)
        };

        engine.kernel = NautilusKernel::new_with(
            "BacktestEngine".to_string(),
            config,
            None,
            Some(Box::new(event_store_factory)),
        )
        .unwrap();
        engine.instance_id = engine.kernel.instance_id;
        engine
    }

    fn create_stop_market_order(instrument: &CryptoPerpetual) -> OrderAny {
        OrderTestBuilder::new(OrderType::StopMarket)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .trigger_price(Price::from("5100.00"))
            .quantity(Quantity::from(1))
            .emulation_trigger(TriggerType::BidAsk)
            .build()
    }

    fn create_submit_order_command(order: &OrderAny) -> SubmitOrder {
        SubmitOrder::new(
            order.trader_id(),
            None,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            order.init_event().clone(),
            order.exec_algorithm_id(),
            None,
            None,
            UUID4::new(),
            0.into(),
            None, // correlation_id
        )
    }

    fn register_data_command_handler(id: &str) -> TypedIntoMessageSavingHandler<DataCommand> {
        let (handler, saving_handler) =
            get_typed_into_message_saving_handler::<DataCommand>(Some(Ustr::from(id)));
        msgbus::register_data_command_endpoint(
            MessagingSwitchboard::data_engine_queue_execute(),
            handler,
        );
        saving_handler
    }

    #[rstest]
    fn test_run_impl_event_store_replay_skips_trader_start() {
        let mut engine = create_engine_with_replay_store(false);

        engine
            .run_impl(
                Some(UnixNanos::from(0)),
                Some(UnixNanos::from(1)),
                None,
                true,
            )
            .unwrap();

        assert!(engine.kernel.is_event_store_replay_configured());
        assert!(engine.kernel.is_event_store_replay());
        assert!(!engine.kernel.trader.borrow().is_running());
    }

    #[rstest]
    fn test_end_reports_strategy_stranded_by_managed_stop() {
        let (mut engine, strategy_id) = create_engine_with_strategy(true);

        let result = engine.run(
            Some(UnixNanos::from(0)),
            Some(UnixNanos::from(1)),
            None,
            false,
        );

        assert!(result.is_ok());
        assert_eq!(
            component_state(&strategy_id.inner()).unwrap(),
            ComponentState::Running
        );
        assert_eq!(engine.running_strategy_ids(), vec![strategy_id]);
    }

    #[rstest]
    fn test_end_reports_no_cleanly_stopped_strategies() {
        let mut empty_engine = create_engine();
        let empty_result = empty_engine.run(
            Some(UnixNanos::from(0)),
            Some(UnixNanos::from(1)),
            None,
            false,
        );
        assert!(empty_result.is_ok());
        assert!(empty_engine.running_strategy_ids().is_empty());

        let (mut engine, strategy_id) = create_engine_with_strategy(false);
        let result = engine.run(
            Some(UnixNanos::from(0)),
            Some(UnixNanos::from(1)),
            None,
            false,
        );

        assert!(result.is_ok());
        assert_ne!(
            component_state(&strategy_id.inner()).unwrap(),
            ComponentState::Running
        );
        assert!(engine.running_strategy_ids().is_empty());
    }

    #[rstest]
    fn test_run_impl_event_store_replay_config_failure_errors() {
        let mut engine = create_engine_with_replay_store(true);

        let error = engine
            .run_impl(
                Some(UnixNanos::from(0)),
                Some(UnixNanos::from(1)),
                None,
                true,
            )
            .unwrap_err();

        assert_eq!(error.to_string(), "event-store replay did not start");
        assert!(engine.kernel.is_event_store_replay_configured());
        assert!(!engine.kernel.is_event_store_replay());
        assert!(!engine.kernel.trader.borrow().is_running());
    }

    #[rstest]
    fn test_backtest_state_persistence_loads_before_start_and_saves_after_settle() {
        let actor_id = ActorId::from("BACKTEST-STATE-ACTOR");
        let strategy_id = StrategyId::from("BACKTEST-STATE-STRATEGY-001");
        let actor_load = IndexMap::from([("actor-load".to_string(), b"actor-loaded".to_vec())]);
        let strategy_load =
            IndexMap::from([("strategy-load".to_string(), b"strategy-loaded".to_vec())]);
        let actor_save = IndexMap::from([("actor-save".to_string(), b"actor-saved".to_vec())]);
        let strategy_save =
            IndexMap::from([("strategy-save".to_string(), b"strategy-saved".to_vec())]);
        let (database, control) = TestCacheDatabaseControl::create();
        control.set_actor_state(actor_id, &actor_load);
        control.set_strategy_state(strategy_id, &strategy_load);
        let config = BacktestEngineConfig {
            load_state: true,
            save_state: true,
            run_analysis: false,
            ..Default::default()
        };
        let mut engine = BacktestEngine::new(config).unwrap();
        engine
            .kernel
            .cache
            .borrow_mut()
            .set_database(Box::new(database));
        engine
            .add_actor(StateActor::new(
                actor_id,
                control.clone(),
                actor_save.clone(),
            ))
            .unwrap();
        engine
            .add_strategy(StateStrategy::new(
                strategy_id,
                control.clone(),
                strategy_save.clone(),
            ))
            .unwrap();

        engine
            .run(
                Some(UnixNanos::from(0)),
                Some(UnixNanos::from(1)),
                None,
                false,
            )
            .unwrap();
        engine.dispose();

        assert_eq!(
            control.events(),
            vec![
                "actor.load:BACKTEST-STATE-ACTOR",
                "actor.on_load",
                "strategy.load:BACKTEST-STATE-STRATEGY-001",
                "strategy.on_load",
                "actor.on_start",
                "strategy.on_start",
                "actor.on_stop",
                "strategy.on_stop",
                "actor.on_save",
                "actor.update:BACKTEST-STATE-ACTOR",
                "strategy.on_save",
                "strategy.update:BACKTEST-STATE-STRATEGY-001",
                "database.close",
            ]
        );
        assert_eq!(control.actor_state(&actor_id), Some(actor_save));
        assert_eq!(control.strategy_state(&strategy_id), Some(strategy_save));
        assert_eq!(engine.backtest_end, Some(UnixNanos::from(0)));
    }

    #[rstest]
    fn test_backtest_state_persistence_reports_callback_errors_after_shutdown() {
        let actor_id = ActorId::from("BACKTEST-FAIL-SAVE-ACTOR");
        let strategy_id = StrategyId::from("BACKTEST-FAIL-SAVE-STRATEGY-001");
        let (database, control) = TestCacheDatabaseControl::create();
        let config = BacktestEngineConfig {
            save_state: true,
            run_analysis: false,
            ..Default::default()
        };
        let mut engine = BacktestEngine::new(config).unwrap();
        engine
            .kernel
            .cache
            .borrow_mut()
            .set_database(Box::new(database));
        engine
            .add_actor(StateActor::new(actor_id, control.clone(), IndexMap::new()).with_fail_save())
            .unwrap();
        engine
            .add_strategy(
                StateStrategy::new(strategy_id, control.clone(), IndexMap::new()).with_fail_save(),
            )
            .unwrap();

        let error = engine
            .run(
                Some(UnixNanos::from(0)),
                Some(UnixNanos::from(1)),
                None,
                false,
            )
            .unwrap_err();
        engine.dispose();

        assert_eq!(
            error.to_string(),
            "Failed to save component state: actor BACKTEST-FAIL-SAVE-ACTOR callback: test actor \
             on_save failure; strategy BACKTEST-FAIL-SAVE-STRATEGY-001 callback: test strategy \
             on_save failure"
        );
        assert_eq!(
            control.events(),
            vec![
                "actor.on_start",
                "strategy.on_start",
                "actor.on_stop",
                "strategy.on_stop",
                "actor.on_save",
                "strategy.on_save",
                "database.close",
            ]
        );
        assert!(!engine.kernel.trader.borrow().is_running());
        assert_eq!(engine.backtest_end, Some(UnixNanos::from(0)));
    }

    #[rstest]
    #[case(None)]
    #[case(Some(true))]
    #[case(Some(false))]
    fn test_new_forces_drop_instruments_on_reset_false(
        crypto_perpetual_ethusdt: CryptoPerpetual,
        #[case] user_value: Option<bool>,
    ) {
        use nautilus_common::cache::CacheConfig;

        let config = match user_value {
            None => BacktestEngineConfig::builder().build(),
            Some(value) => BacktestEngineConfig::builder()
                .cache(
                    CacheConfig::builder()
                        .drop_instruments_on_reset(value)
                        .build()
                        .unwrap(),
                )
                .build(),
        };
        let mut engine = BacktestEngine::new(config).unwrap();

        let venue_config = SimulatedVenueConfig::builder()
            .venue(Venue::from("BINANCE"))
            .oms_type(OmsType::Netting)
            .account_type(AccountType::Margin)
            .book_type(BookType::L1_MBP)
            .starting_balances(vec![Money::from("1_000_000 USDT")])
            .build()
            .unwrap();
        engine.add_venue(venue_config).unwrap();

        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
        let instrument_id = instrument.id();
        engine.add_instrument(&instrument).unwrap();

        engine.reset().unwrap();

        assert!(
            engine
                .kernel()
                .cache
                .borrow()
                .instrument(&instrument_id)
                .is_some(),
            "instrument must survive engine.reset(); user-supplied \
             drop_instruments_on_reset={user_value:?} must not leak through",
        );
    }

    #[rstest]
    fn test_reset_resets_order_emulator_state(crypto_perpetual_ethusdt: CryptoPerpetual) {
        let mut engine = create_engine();
        let data_commands =
            register_data_command_handler("DataEngine.queue_execute.backtest_reset");
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt.clone());
        let instrument_id = instrument.id();
        engine.add_instrument(&instrument).unwrap();
        let order = create_stop_market_order(&crypto_perpetual_ethusdt);
        let command = create_submit_order_command(&order);
        engine
            .kernel
            .cache
            .borrow_mut()
            .add_order(order, None, None, false)
            .unwrap();
        let order_emulator = engine.kernel.order_emulator.emulator();
        let mut order_emulator = order_emulator.borrow_mut();
        order_emulator.cache_submit_order_command(command.clone());
        order_emulator.handle_submit_order(&command);
        drop(order_emulator);
        data_commands.clear();

        engine.reset().unwrap();

        let commands = data_commands.get_messages();
        let emulator = engine.kernel.order_emulator.get_emulator();
        assert!(emulator.subscribed_quotes().is_empty());
        assert!(emulator.subscribed_trades().is_empty());
        assert!(emulator.get_matching_core(&instrument_id).is_none());
        assert!(commands.iter().any(|command| matches!(
            command,
            DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(command))
                if command.instrument_id == instrument_id
        )));
    }

    #[rstest]
    fn test_route_data_to_exchange_instrument_status(crypto_perpetual_ethusdt: CryptoPerpetual) {
        let mut engine = create_engine();
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
        let instrument_id = instrument.id();
        engine.add_instrument(&instrument).unwrap();

        let status = InstrumentStatus::new(
            instrument_id,
            MarketStatusAction::Close,
            UnixNanos::from(1),
            UnixNanos::from(1),
            None,
            None,
            None,
            None,
            None,
        );

        BacktestEngine::route_data_to_exchange(
            &engine.venues,
            &mut engine.has_book_processed,
            &engine.kernel.clock,
            DataRef::InstrumentStatus(&status),
        )
        .unwrap();

        let exchange = engine.venues.get(&instrument_id.venue).unwrap().borrow();
        let market_status = exchange
            .get_matching_engine(&instrument_id)
            .unwrap()
            .market_status;
        assert_eq!(market_status, MarketStatus::Closed);
    }
}
