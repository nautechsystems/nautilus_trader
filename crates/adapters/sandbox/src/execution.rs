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

//! Sandbox execution client implementation.

use std::{cell::RefCell, fmt::Debug, rc::Rc};

use ahash::AHashMap;
use async_trait::async_trait;
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    clock::Clock,
    messages::execution::{
        BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
        GenerateOrderStatusReport, GenerateOrderStatusReports, GeneratePositionStatusReports,
        ModifyOrder, QueryAccount, QueryOrder, SubmitOrder, SubmitOrderList,
    },
    msgbus::{self, MStr, Pattern, TypedHandler},
};
use nautilus_core::{UnixNanos, WeakCell};
use nautilus_execution::{
    client::base::ExecutionClientCore,
    matching_engine::adapter::OrderEngineAdapter,
    models::{
        fee::{FeeModelAny, MakerTakerFeeModel},
        fill::FillModel,
    },
};
use nautilus_model::{
    accounts::AccountAny,
    data::{Bar, OrderBookDeltas, QuoteTick, TradeTick},
    enums::OmsType,
    identifiers::{AccountId, ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
    orders::Order,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance, Money},
};

use crate::config::SandboxExecutionClientConfig;

/// Inner state for the sandbox execution client.
///
/// This is wrapped in `Rc<RefCell<>>` so message handlers can hold weak references.
struct SandboxInner {
    /// Matching engines per instrument.
    matching_engines: AHashMap<InstrumentId, OrderEngineAdapter>,
    /// Next raw ID assigned to a matching engine.
    next_engine_raw_id: u32,
    /// Current account balances.
    balances: AHashMap<String, Money>,
    /// Reference to the clock.
    clock: Rc<RefCell<dyn Clock>>,
    /// Reference to the cache.
    cache: Rc<RefCell<Cache>>,
    /// The sandbox configuration.
    config: SandboxExecutionClientConfig,
}

impl SandboxInner {
    /// Ensures a matching engine exists for the given instrument.
    fn ensure_matching_engine(&mut self, instrument: &InstrumentAny) {
        let instrument_id = instrument.id();

        if !self.matching_engines.contains_key(&instrument_id) {
            let engine_config = self.config.to_matching_engine_config();
            let fill_model = FillModel::default();
            let fee_model = FeeModelAny::MakerTaker(MakerTakerFeeModel);
            let raw_id = self.next_engine_raw_id;
            self.next_engine_raw_id = self.next_engine_raw_id.wrapping_add(1);

            let engine = OrderEngineAdapter::new(
                instrument.clone(),
                raw_id,
                fill_model,
                fee_model,
                self.config.book_type,
                self.config.oms_type,
                self.config.account_type,
                self.clock.clone(),
                self.cache.clone(),
                engine_config,
            );

            self.matching_engines.insert(instrument_id, engine);
        }
    }

    /// Processes a quote tick through the matching engine.
    fn process_quote_tick(&mut self, quote: &QuoteTick) {
        let instrument_id = quote.instrument_id;

        // Try to get instrument from cache, create engine if found
        let instrument = self.cache.borrow().instrument(&instrument_id).cloned();
        if let Some(instrument) = instrument {
            self.ensure_matching_engine(&instrument);
            if let Some(engine) = self.matching_engines.get_mut(&instrument_id) {
                engine.get_engine_mut().process_quote_tick(quote);
            }
        }
    }

    /// Processes a trade tick through the matching engine.
    fn process_trade_tick(&mut self, trade: &TradeTick) {
        if !self.config.trade_execution {
            return;
        }

        let instrument_id = trade.instrument_id;

        let instrument = self.cache.borrow().instrument(&instrument_id).cloned();
        if let Some(instrument) = instrument {
            self.ensure_matching_engine(&instrument);
            if let Some(engine) = self.matching_engines.get_mut(&instrument_id) {
                engine.get_engine_mut().process_trade_tick(trade);
            }
        }
    }

    /// Processes a bar through the matching engine.
    fn process_bar(&mut self, bar: &Bar) {
        if !self.config.bar_execution {
            return;
        }

        let instrument_id = bar.bar_type.instrument_id();

        let instrument = self.cache.borrow().instrument(&instrument_id).cloned();
        if let Some(instrument) = instrument {
            self.ensure_matching_engine(&instrument);
            if let Some(engine) = self.matching_engines.get_mut(&instrument_id) {
                engine.get_engine_mut().process_bar(bar);
            }
        }
    }
}

/// Registered message handlers for later deregistration.
struct RegisteredHandlers {
    quote_pattern: MStr<Pattern>,
    quote_handler: TypedHandler<QuoteTick>,
    trade_pattern: MStr<Pattern>,
    trade_handler: TypedHandler<TradeTick>,
    bar_pattern: MStr<Pattern>,
    bar_handler: TypedHandler<Bar>,
}

/// A sandbox execution client for paper trading against live market data.
///
/// The `SandboxExecutionClient` simulates order execution using the `OrderMatchingEngine`
/// to match orders against market data. This enables strategy testing in real-time
/// without actual order execution on exchanges.
pub struct SandboxExecutionClient {
    /// The core execution client functionality.
    core: RefCell<ExecutionClientCore>,
    /// The sandbox configuration.
    config: SandboxExecutionClientConfig,
    /// Inner state wrapped for handler access.
    inner: Rc<RefCell<SandboxInner>>,
    /// Registered message handlers for cleanup.
    handlers: RefCell<Option<RegisteredHandlers>>,
    /// Whether the client is started.
    started: RefCell<bool>,
    /// Whether the client is connected.
    connected: RefCell<bool>,
    /// Reference to the clock.
    clock: Rc<RefCell<dyn Clock>>,
    /// Reference to the cache.
    cache: Rc<RefCell<Cache>>,
}

impl Debug for SandboxExecutionClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(SandboxExecutionClient))
            .field("venue", &self.config.venue)
            .field("account_id", &self.core.borrow().account_id)
            .field("connected", &*self.connected.borrow())
            .field(
                "matching_engines",
                &self.inner.borrow().matching_engines.len(),
            )
            .finish()
    }
}

impl SandboxExecutionClient {
    /// Creates a new [`SandboxExecutionClient`] instance.
    #[must_use]
    pub fn new(
        core: ExecutionClientCore,
        config: SandboxExecutionClientConfig,
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
    ) -> Self {
        let mut balances = AHashMap::new();
        for money in &config.starting_balances {
            balances.insert(money.currency.code.to_string(), *money);
        }

        let inner = Rc::new(RefCell::new(SandboxInner {
            matching_engines: AHashMap::new(),
            next_engine_raw_id: 0,
            balances,
            clock: clock.clone(),
            cache: cache.clone(),
            config: config.clone(),
        }));

        Self {
            core: RefCell::new(core),
            config,
            inner,
            handlers: RefCell::new(None),
            started: RefCell::new(false),
            connected: RefCell::new(false),
            clock,
            cache,
        }
    }

    /// Returns a reference to the configuration.
    #[must_use]
    pub const fn config(&self) -> &SandboxExecutionClientConfig {
        &self.config
    }

    /// Returns the number of active matching engines.
    #[must_use]
    pub fn matching_engine_count(&self) -> usize {
        self.inner.borrow().matching_engines.len()
    }

    /// Registers message handlers for market data subscriptions.
    ///
    /// This subscribes to quotes, trades, and bars for the configured venue,
    /// routing all received data to the matching engines.
    fn register_message_handlers(&self) {
        if self.handlers.borrow().is_some() {
            log::warn!("Sandbox message handlers already registered");
            return;
        }

        let inner_weak = WeakCell::from(Rc::downgrade(&self.inner));
        let venue = self.config.venue;

        // Quote tick handler
        let quote_handler = {
            let inner = inner_weak.clone();
            TypedHandler::from(move |quote: &QuoteTick| {
                if quote.instrument_id.venue == venue
                    && let Some(inner_rc) = inner.upgrade()
                {
                    inner_rc.borrow_mut().process_quote_tick(quote);
                }
            })
        };

        // Trade tick handler
        let trade_handler = {
            let inner = inner_weak.clone();
            TypedHandler::from(move |trade: &TradeTick| {
                if trade.instrument_id.venue == venue
                    && let Some(inner_rc) = inner.upgrade()
                {
                    inner_rc.borrow_mut().process_trade_tick(trade);
                }
            })
        };

        // Bar handler (topic is data.bars.{bar_type}, filter by venue in handler)
        let bar_handler = {
            let inner = inner_weak;
            TypedHandler::from(move |bar: &Bar| {
                if bar.bar_type.instrument_id().venue == venue
                    && let Some(inner_rc) = inner.upgrade()
                {
                    inner_rc.borrow_mut().process_bar(bar);
                }
            })
        };

        // Subscribe patterns (bar topic is data.bars.{bar_type} so use wildcard)
        let quote_pattern: MStr<Pattern> = format!("data.quotes.{venue}.*").into();
        let trade_pattern: MStr<Pattern> = format!("data.trades.{venue}.*").into();
        let bar_pattern: MStr<Pattern> = "data.bars.*".into();

        msgbus::subscribe_quotes(quote_pattern, quote_handler.clone(), Some(10));
        msgbus::subscribe_trades(trade_pattern, trade_handler.clone(), Some(10));
        msgbus::subscribe_bars(bar_pattern, bar_handler.clone(), Some(10));

        // Store handlers for later deregistration
        *self.handlers.borrow_mut() = Some(RegisteredHandlers {
            quote_pattern,
            quote_handler,
            trade_pattern,
            trade_handler,
            bar_pattern,
            bar_handler,
        });

        log::info!(
            "Sandbox registered message handlers for venue={}",
            self.config.venue
        );
    }

    /// Deregisters message handlers to stop receiving market data.
    fn deregister_message_handlers(&self) {
        if let Some(handlers) = self.handlers.borrow_mut().take() {
            msgbus::unsubscribe_quotes(handlers.quote_pattern, &handlers.quote_handler);
            msgbus::unsubscribe_trades(handlers.trade_pattern, &handlers.trade_handler);
            msgbus::unsubscribe_bars(handlers.bar_pattern, &handlers.bar_handler);

            log::info!(
                "Sandbox deregistered message handlers for venue={}",
                self.config.venue
            );
        }
    }

    /// Returns current account balances, preferring cache state over starting balances.
    fn get_current_account_balances(&self) -> Vec<AccountBalance> {
        let account_id = self.core.borrow().account_id;
        let cache = self.cache.borrow();

        // Use account from cache if available (updated by fill events)
        if let Some(account) = cache.account(&account_id) {
            return account.balances().into_values().collect();
        }

        // Fall back to starting balances
        self.get_account_balances()
    }

    /// Processes a quote tick through the matching engine.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument is not found in the cache.
    pub fn process_quote_tick(&self, quote: &QuoteTick) -> anyhow::Result<()> {
        let instrument_id = quote.instrument_id;
        let instrument = self
            .cache
            .borrow()
            .instrument(&instrument_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Instrument not found: {instrument_id}"))?;

        let mut inner = self.inner.borrow_mut();
        inner.ensure_matching_engine(&instrument);
        if let Some(engine) = inner.matching_engines.get_mut(&instrument_id) {
            engine.get_engine_mut().process_quote_tick(quote);
        }
        Ok(())
    }

    /// Processes a trade tick through the matching engine.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument is not found in the cache.
    pub fn process_trade_tick(&self, trade: &TradeTick) -> anyhow::Result<()> {
        if !self.config.trade_execution {
            return Ok(());
        }

        let instrument_id = trade.instrument_id;
        let instrument = self
            .cache
            .borrow()
            .instrument(&instrument_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Instrument not found: {instrument_id}"))?;

        let mut inner = self.inner.borrow_mut();
        inner.ensure_matching_engine(&instrument);
        if let Some(engine) = inner.matching_engines.get_mut(&instrument_id) {
            engine.get_engine_mut().process_trade_tick(trade);
        }
        Ok(())
    }

    /// Processes a bar through the matching engine.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument is not found in the cache.
    pub fn process_bar(&self, bar: &Bar) -> anyhow::Result<()> {
        if !self.config.bar_execution {
            return Ok(());
        }

        let instrument_id = bar.bar_type.instrument_id();
        let instrument = self
            .cache
            .borrow()
            .instrument(&instrument_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Instrument not found: {instrument_id}"))?;

        let mut inner = self.inner.borrow_mut();
        inner.ensure_matching_engine(&instrument);
        if let Some(engine) = inner.matching_engines.get_mut(&instrument_id) {
            engine.get_engine_mut().process_bar(bar);
        }
        Ok(())
    }

    /// Processes order book deltas through the matching engine.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument is not found in the cache.
    pub fn process_order_book_deltas(&self, deltas: &OrderBookDeltas) -> anyhow::Result<()> {
        let instrument_id = deltas.instrument_id;
        let instrument = self
            .cache
            .borrow()
            .instrument(&instrument_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Instrument not found: {instrument_id}"))?;

        let mut inner = self.inner.borrow_mut();
        inner.ensure_matching_engine(&instrument);
        if let Some(engine) = inner.matching_engines.get_mut(&instrument_id) {
            engine.get_engine_mut().process_order_book_deltas(deltas)?;
        }
        Ok(())
    }

    /// Resets the sandbox to its initial state.
    pub fn reset(&self) {
        let mut inner = self.inner.borrow_mut();
        for engine in inner.matching_engines.values_mut() {
            engine.get_engine_mut().reset();
        }

        inner.balances.clear();
        for money in &self.config.starting_balances {
            inner
                .balances
                .insert(money.currency.code.to_string(), *money);
        }

        log::info!(
            "Sandbox execution client reset: venue={}",
            self.config.venue
        );
    }

    /// Generates account balance entries from current balances.
    fn get_account_balances(&self) -> Vec<AccountBalance> {
        self.inner
            .borrow()
            .balances
            .values()
            .map(|money| AccountBalance::new(*money, Money::new(0.0, money.currency), *money))
            .collect()
    }
}

#[async_trait(?Send)]
impl ExecutionClient for SandboxExecutionClient {
    fn is_connected(&self) -> bool {
        *self.connected.borrow()
    }

    fn client_id(&self) -> ClientId {
        self.core.borrow().client_id
    }

    fn account_id(&self) -> AccountId {
        self.core.borrow().account_id
    }

    fn venue(&self) -> Venue {
        self.core.borrow().venue
    }

    fn oms_type(&self) -> OmsType {
        self.config.oms_type
    }

    fn get_account(&self) -> Option<AccountAny> {
        self.core.borrow().get_account()
    }

    fn generate_account_state(
        &self,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        reported: bool,
        ts_event: UnixNanos,
    ) -> anyhow::Result<()> {
        self.core
            .borrow()
            .generate_account_state(balances, margins, reported, ts_event)
    }

    fn start(&mut self) -> anyhow::Result<()> {
        if *self.started.borrow() {
            return Ok(());
        }

        // Register message handlers to receive market data
        self.register_message_handlers();

        *self.started.borrow_mut() = true;
        let core = self.core.borrow();
        log::info!(
            "Sandbox execution client started: venue={}, account_id={}, oms_type={:?}, account_type={:?}",
            self.config.venue,
            core.account_id,
            self.config.oms_type,
            self.config.account_type,
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        if !*self.started.borrow() {
            return Ok(());
        }

        // Deregister message handlers to stop receiving data
        self.deregister_message_handlers();

        *self.started.borrow_mut() = false;
        *self.connected.borrow_mut() = false;
        log::info!(
            "Sandbox execution client stopped: venue={}",
            self.config.venue
        );
        Ok(())
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if *self.connected.borrow() {
            return Ok(());
        }

        let balances = self.get_account_balances();
        let ts_event = self.clock.borrow().timestamp_ns();
        self.generate_account_state(balances, vec![], false, ts_event)?;

        *self.connected.borrow_mut() = true;
        self.core.borrow_mut().set_connected(true);
        log::info!(
            "Sandbox execution client connected: venue={}",
            self.config.venue
        );
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if !*self.connected.borrow() {
            return Ok(());
        }

        *self.connected.borrow_mut() = false;
        self.core.borrow_mut().set_connected(false);
        log::info!(
            "Sandbox execution client disconnected: venue={}",
            self.config.venue
        );
        Ok(())
    }

    fn submit_order(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        let core = self.core.borrow();
        let mut order = core.get_order(&cmd.client_order_id)?;

        if order.is_closed() {
            log::warn!("Cannot submit closed order {}", order.client_order_id());
            return Ok(());
        }

        core.generate_order_submitted(
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            cmd.ts_init,
        );

        let instrument_id = order.instrument_id();
        let instrument = self
            .cache
            .borrow()
            .instrument(&instrument_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Instrument not found: {instrument_id}"))?;

        drop(core); // Release borrow before mutable borrow

        let mut inner = self.inner.borrow_mut();
        inner.ensure_matching_engine(&instrument);

        // Update matching engine with latest market data from cache
        let cache = self.cache.borrow();
        if let Some(engine) = inner.matching_engines.get_mut(&instrument_id) {
            if let Some(quote) = cache.quote(&instrument_id) {
                engine.get_engine_mut().process_quote_tick(quote);
            }
            if self.config.trade_execution
                && let Some(trade) = cache.trade(&instrument_id)
            {
                engine.get_engine_mut().process_trade_tick(trade);
            }
        }
        drop(cache);

        let account_id = self.core.borrow().account_id;
        if let Some(engine) = inner.matching_engines.get_mut(&instrument_id) {
            engine
                .get_engine_mut()
                .process_order(&mut order, account_id);
        }

        Ok(())
    }

    fn submit_order_list(&self, cmd: &SubmitOrderList) -> anyhow::Result<()> {
        let core = self.core.borrow();

        for order in &cmd.order_list.orders {
            if order.is_closed() {
                log::warn!("Cannot submit closed order {}", order.client_order_id());
                continue;
            }

            core.generate_order_submitted(
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                cmd.ts_init,
            );
        }

        drop(core); // Release borrow before mutable operations

        let account_id = self.core.borrow().account_id;
        for order in &cmd.order_list.orders {
            if order.is_closed() {
                continue;
            }

            let instrument_id = order.instrument_id();
            let instrument = self.cache.borrow().instrument(&instrument_id).cloned();

            if let Some(instrument) = instrument {
                let mut inner = self.inner.borrow_mut();
                inner.ensure_matching_engine(&instrument);

                // Update with latest market data
                let cache = self.cache.borrow();
                if let Some(engine) = inner.matching_engines.get_mut(&instrument_id) {
                    if let Some(quote) = cache.quote(&instrument_id) {
                        engine.get_engine_mut().process_quote_tick(quote);
                    }
                    if self.config.trade_execution
                        && let Some(trade) = cache.trade(&instrument_id)
                    {
                        engine.get_engine_mut().process_trade_tick(trade);
                    }
                }
                drop(cache);

                if let Some(engine) = inner.matching_engines.get_mut(&instrument_id) {
                    let mut order_clone = order.clone();
                    engine
                        .get_engine_mut()
                        .process_order(&mut order_clone, account_id);
                }
            }
        }

        Ok(())
    }

    fn modify_order(&self, cmd: &ModifyOrder) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let account_id = self.core.borrow().account_id;

        let mut inner = self.inner.borrow_mut();
        if let Some(engine) = inner.matching_engines.get_mut(&instrument_id) {
            engine.get_engine_mut().process_modify(cmd, account_id);
        }
        Ok(())
    }

    fn cancel_order(&self, cmd: &CancelOrder) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let account_id = self.core.borrow().account_id;

        let mut inner = self.inner.borrow_mut();
        if let Some(engine) = inner.matching_engines.get_mut(&instrument_id) {
            engine.get_engine_mut().process_cancel(cmd, account_id);
        }
        Ok(())
    }

    fn cancel_all_orders(&self, cmd: &CancelAllOrders) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let account_id = self.core.borrow().account_id;

        let mut inner = self.inner.borrow_mut();
        if let Some(engine) = inner.matching_engines.get_mut(&instrument_id) {
            engine.get_engine_mut().process_cancel_all(cmd, account_id);
        }
        Ok(())
    }

    fn batch_cancel_orders(&self, cmd: &BatchCancelOrders) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let account_id = self.core.borrow().account_id;

        let mut inner = self.inner.borrow_mut();
        if let Some(engine) = inner.matching_engines.get_mut(&instrument_id) {
            engine
                .get_engine_mut()
                .process_batch_cancel(cmd, account_id);
        }
        Ok(())
    }

    fn query_account(&self, _cmd: &QueryAccount) -> anyhow::Result<()> {
        let balances = self.get_current_account_balances();
        let ts_event = self.clock.borrow().timestamp_ns();
        self.generate_account_state(balances, vec![], false, ts_event)?;
        Ok(())
    }

    fn query_order(&self, _cmd: &QueryOrder) -> anyhow::Result<()> {
        // Orders are tracked in the cache, no external query needed for sandbox
        Ok(())
    }

    async fn generate_order_status_report(
        &self,
        _cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        // Sandbox orders are tracked internally
        Ok(None)
    }

    async fn generate_order_status_reports(
        &self,
        _cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        // Sandbox orders are tracked internally
        Ok(Vec::new())
    }

    async fn generate_fill_reports(
        &self,
        _cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        // Sandbox fills are tracked internally
        Ok(Vec::new())
    }

    async fn generate_position_status_reports(
        &self,
        _cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        // Sandbox positions are tracked internally
        Ok(Vec::new())
    }

    async fn generate_mass_status(
        &self,
        _lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        // Sandbox doesn't need reconciliation
        Ok(None)
    }
}
