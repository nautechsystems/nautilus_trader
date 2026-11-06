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

use std::{cell::RefCell, collections::BinaryHeap, fmt::Debug, rc::Rc};

use ahash::{AHashMap, AHashSet};
use async_trait::async_trait;
use nautilus_common::{
    cache::Cache,
    clients::{ExecutionClient, generate_mass_status},
    clock::Clock,
    factories::OrderEventFactory,
    live::try_get_exec_event_sender,
    messages::{
        ExecutionEvent,
        execution::{
            BatchCancelOrders, BatchModifyOrders, CancelAllOrders, CancelOrder,
            GenerateFillReports, GenerateOrderStatusReport, GenerateOrderStatusReports,
            GeneratePositionStatusReports, ModifyOrder, PositionOperationEffects,
            PositionOperationResult, PositionOperationStatus, PositionOperationTokenAmount,
            QueryAccount, QueryOrder, QueryPositionOperation, SubmitOrder, SubmitOrderList,
            SubmitPositionOperation, TradingCommand,
        },
    },
    msgbus::{
        self, MStr, MessagingSwitchboard, Pattern, TypedHandler,
        typed_handler::ShareableMessageHandler,
    },
    timer::{TimeEvent, TimeEventCallback},
};
use nautilus_core::{DurationNanos, Params, UUID4, UnixNanos, WeakCell};
use nautilus_execution::{
    client::core::ExecutionClientCore,
    matching_engine::{
        OrderMatchingEngine,
        inflight::InflightOrders,
        orders::{MatchingOrders, OtoCommand},
    },
    models::{fee::FeeModelHandle, fill::FillModelHandle, latency::LatencyModel},
};
use nautilus_model::{
    accounts::AccountAny,
    ctf::{
        MARKET_VERSION_KEY, MARKET_VERSION_V2, NegRiskMarketMetadata, base_units_to_decimal,
        convert_amounts, ensure_instrument_active, quantity_to_base_units,
    },
    data::{
        Bar, InstrumentClose, InstrumentStatus, OrderBookDeltas, OrderBookDepth, QuoteTick,
        TradeTick,
    },
    enums::{AccountType, OmsType},
    events::{OrderCancelDenied, OrderEventAny, OrderModifyDenied, OrderRejected, PositionEvent},
    identifiers::{AccountId, ClientId, ClientOrderId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    position_operation::PositionOperationType,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, MarginBalance, Money},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::config::SandboxExecutionClientConfig;

// Bounds retained state for quote-only instruments that expire without event-driven cleanup
const EXPIRED_ENGINE_SWEEP_INTERVAL: DurationNanos = DurationNanos::from_mins(1);

/// A sandbox execution client for paper trading against live market data.
///
/// The `SandboxExecutionClient` simulates order execution using the `OrderMatchingEngine`
/// to match orders against market data. This enables strategy testing in real-time
/// without actual order execution on exchanges.
///
/// Order status reports use cached orders with a venue order ID for this account and venue.
/// Full-history requests include retained closed orders.
pub struct SandboxExecutionClient {
    core: RefCell<ExecutionClientCore>,
    factory: OrderEventFactory,
    config: SandboxExecutionClientConfig,
    inner: Rc<RefCell<SandboxInner>>,
    handlers: RefCell<Option<RegisteredHandlers>>,
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
}

struct RegisteredHandlers {
    deltas_pattern: MStr<Pattern>,
    deltas_handler: TypedHandler<OrderBookDeltas>,
    depth_pattern: MStr<Pattern>,
    depth_handler: TypedHandler<OrderBookDepth>,
    quote_pattern: MStr<Pattern>,
    quote_handler: TypedHandler<QuoteTick>,
    trade_pattern: MStr<Pattern>,
    trade_handler: TypedHandler<TradeTick>,
    bar_pattern: MStr<Pattern>,
    bar_handler: TypedHandler<Bar>,
    status_pattern: MStr<Pattern>,
    status_handler: ShareableMessageHandler,
    close_pattern: MStr<Pattern>,
    close_handler: ShareableMessageHandler,
    position_pattern: MStr<Pattern>,
    position_handler: TypedHandler<PositionEvent>,
}

impl Debug for SandboxExecutionClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(SandboxExecutionClient))
            .field("venue", &self.config.venue)
            .field("account_id", &self.core.borrow().account_id)
            .field("connected", &self.core.borrow().is_connected())
            .field(
                "matching_engines",
                &self.inner.borrow().matching_engines.len(),
            )
            .finish()
    }
}

impl SandboxExecutionClient {
    /// Creates a new [`SandboxExecutionClient`] with an explicitly configured fee model.
    ///
    /// The fee model is resolved once and shared across all matching engines for
    /// this account, ensuring model state is shared within the account and isolated
    /// across accounts.
    ///
    /// # Errors
    ///
    /// Returns an error if starting balances repeat a currency or `config.fee_model` is `None`.
    /// A deliberate fee choice, including an explicit zero-fee model, is required.
    pub fn new(
        core: ExecutionClientCore,
        config: SandboxExecutionClientConfig,
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
    ) -> anyhow::Result<Self> {
        config.validate()?;

        let fee_model = config.fee_model.clone().map(FeeModelHandle::from).ok_or_else(|| {
            anyhow::anyhow!(
                "SandboxExecutionClientConfig requires an explicit fee_model, including an explicit zero-fee model"
            )
        })?;

        Self::new_with_fee_model(core, config, clock, cache, fee_model)
    }

    /// Creates a sandbox execution client with a runtime fee model.
    ///
    /// The supplied model takes precedence over `config.fee_model` and is shared
    /// across all matching engines for this client.
    ///
    /// # Errors
    ///
    /// Returns an error if starting balances repeat a currency.
    pub fn new_with_fee_model(
        core: ExecutionClientCore,
        config: SandboxExecutionClientConfig,
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
        fee_model: FeeModelHandle,
    ) -> anyhow::Result<Self> {
        config.validate()?;

        let mut balances = AHashMap::new();
        for money in &config.starting_balances {
            balances.insert(money.currency.code.to_string(), *money);
        }

        let fill_model = config
            .fill_model
            .clone()
            .map(FillModelHandle::from)
            .unwrap_or_default();

        let factory = OrderEventFactory::new(
            core.trader_id,
            core.account_id,
            core.account_type,
            core.base_currency,
        );

        let inner = Rc::new_cyclic(|weak: &std::rc::Weak<RefCell<SandboxInner>>| {
            RefCell::new(SandboxInner {
                clock: clock.clone(),
                cache: cache.clone(),
                config: config.clone(),
                fill_model,
                fee_model,
                matching_engines: AHashMap::new(),
                next_engine_raw_id: 0,
                balances,
                event_handler: None,
                inbound_queue: BinaryHeap::new(),
                inflight_orders: InflightOrders::default(),
                matching_orders: MatchingOrders::default(),
                inbound_seq: 0,
                client_id: core.client_id,
                account_id: core.account_id,
                factory: factory.clone(),
                position_operations: AHashMap::new(),
                pending_position_operation_cash: AHashMap::new(),
                self_weak: WeakCell::from(weak.clone()),
            })
        });

        Ok(Self {
            core: RefCell::new(core),
            factory,
            config,
            inner,
            handlers: RefCell::new(None),
            clock,
            cache,
        })
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

    fn dispatch_order_event(&self, event: OrderEventAny) {
        self.inner.borrow().dispatch_order_event(event);
    }

    fn register_message_handlers(&self) {
        if self.handlers.borrow().is_some() {
            log::warn!("Sandbox message handlers already registered");
            return;
        }

        let inner_weak = WeakCell::from(Rc::downgrade(&self.inner));
        let venue = self.config.venue;

        let deltas_handler = {
            let inner = inner_weak.clone();
            TypedHandler::from(move |deltas: &OrderBookDeltas| {
                if deltas.instrument_id.venue == venue
                    && let Some(inner_rc) = inner.upgrade()
                {
                    SandboxInner::on_order_book_deltas(&inner_rc, deltas);
                }
            })
        };

        let depth_handler = {
            let inner = inner_weak.clone();
            TypedHandler::from(move |depth: &OrderBookDepth| {
                SandboxInner::on_order_book_depth(&inner, venue, depth);
            })
        };

        let quote_handler = {
            let inner = inner_weak.clone();
            TypedHandler::from(move |quote: &QuoteTick| {
                if quote.instrument_id.venue == venue
                    && let Some(inner_rc) = inner.upgrade()
                {
                    SandboxInner::on_quote_tick(&inner_rc, quote);
                }
            })
        };

        let trade_handler = {
            let inner = inner_weak.clone();
            TypedHandler::from(move |trade: &TradeTick| {
                if trade.instrument_id.venue == venue
                    && let Some(inner_rc) = inner.upgrade()
                {
                    SandboxInner::on_trade_tick(&inner_rc, trade);
                }
            })
        };

        // Bar topics include the bar type, so filter by venue in the handler
        let bar_handler = {
            let inner = inner_weak.clone();
            TypedHandler::from(move |bar: &Bar| {
                if bar.bar_type.instrument_id().venue == venue
                    && let Some(inner_rc) = inner.upgrade()
                {
                    SandboxInner::on_bar(&inner_rc, bar);
                }
            })
        };

        let status_handler = {
            let inner = inner_weak.clone();
            ShareableMessageHandler::from_typed(move |status: &InstrumentStatus| {
                if status.instrument_id.venue == venue
                    && let Some(inner_rc) = inner.upgrade()
                {
                    SandboxInner::on_instrument_status(&inner_rc, status);
                }
            })
        };

        let close_handler = {
            let inner = inner_weak.clone();
            ShareableMessageHandler::from_typed(move |close: &InstrumentClose| {
                if close.instrument_id.venue == venue
                    && let Some(inner_rc) = inner.upgrade()
                {
                    SandboxInner::on_instrument_close(&inner_rc, close);
                }
            })
        };

        let position_handler = {
            TypedHandler::from(move |event: &PositionEvent| {
                if let Some(inner_rc) = inner_weak.upgrade() {
                    SandboxInner::on_position_event(&inner_rc, event);
                }
            })
        };

        let deltas_pattern: MStr<Pattern> = format!("data.book.deltas.{venue}.*").into();
        let depth_pattern: MStr<Pattern> = format!("data.book.depth.{venue}.*").into();
        let quote_pattern: MStr<Pattern> = format!("data.quotes.{venue}.*").into();
        let trade_pattern: MStr<Pattern> = format!("data.trades.{venue}.*").into();
        let bar_pattern: MStr<Pattern> = "data.bars.*".into();
        let status_pattern: MStr<Pattern> = format!("data.status.{venue}.*").into();
        let close_pattern: MStr<Pattern> = format!("data.close.{venue}.*").into();
        let position_pattern: MStr<Pattern> = "events.position.*".into();

        msgbus::subscribe_book_deltas(deltas_pattern, deltas_handler.clone(), Some(10));
        msgbus::subscribe_book_depth(depth_pattern, depth_handler.clone(), Some(10));
        msgbus::subscribe_quotes(quote_pattern, quote_handler.clone(), Some(10));
        msgbus::subscribe_trades(trade_pattern, trade_handler.clone(), Some(10));
        msgbus::subscribe_bars(bar_pattern, bar_handler.clone(), Some(10));
        msgbus::subscribe_any(status_pattern, status_handler.clone(), Some(10));
        msgbus::subscribe_instrument_close(close_pattern, close_handler.clone(), Some(10));
        msgbus::subscribe_position_events(position_pattern, position_handler.clone(), Some(10));

        *self.handlers.borrow_mut() = Some(RegisteredHandlers {
            deltas_pattern,
            deltas_handler,
            depth_pattern,
            depth_handler,
            quote_pattern,
            quote_handler,
            trade_pattern,
            trade_handler,
            bar_pattern,
            bar_handler,
            status_pattern,
            status_handler,
            close_pattern,
            close_handler,
            position_pattern,
            position_handler,
        });

        log::debug!(
            "Sandbox registered message handlers for venue={}",
            self.config.venue
        );
    }

    fn deregister_message_handlers(&self) {
        if let Some(handlers) = self.handlers.borrow_mut().take() {
            msgbus::unsubscribe_book_deltas(handlers.deltas_pattern, &handlers.deltas_handler);
            msgbus::unsubscribe_book_depth(handlers.depth_pattern, &handlers.depth_handler);
            msgbus::unsubscribe_quotes(handlers.quote_pattern, &handlers.quote_handler);
            msgbus::unsubscribe_trades(handlers.trade_pattern, &handlers.trade_handler);
            msgbus::unsubscribe_bars(handlers.bar_pattern, &handlers.bar_handler);
            msgbus::unsubscribe_any(handlers.status_pattern, &handlers.status_handler);
            msgbus::unsubscribe_instrument_close(handlers.close_pattern, &handlers.close_handler);
            msgbus::unsubscribe_position_events(
                handlers.position_pattern,
                &handlers.position_handler,
            );

            log::debug!(
                "Sandbox deregistered message handlers for venue={}",
                self.config.venue
            );
        }
    }

    fn expiry_sweep_timer_name(&self) -> String {
        format!("{}-sandbox-expiry-sweep", self.core.borrow().client_id)
    }

    fn register_expiry_sweep_timer(&self) {
        let inner_weak = WeakCell::from(Rc::downgrade(&self.inner));
        let callback: Rc<dyn Fn(TimeEvent)> = Rc::new(move |_event: TimeEvent| {
            let Some(inner_rc) = inner_weak.upgrade() else {
                return;
            };

            // The timer fires on the runner task, but a nested msgbus dispatch may already hold the
            // borrow; skipping is safe because the next interval retries.
            if let Ok(mut inner) = inner_rc.try_borrow_mut() {
                inner.sweep_expired_engines();
            } else {
                log::debug!("Skipping sandbox expiry sweep due to active borrow");
            }
        });

        let name = self.expiry_sweep_timer_name();

        if let Err(e) = self.clock.borrow_mut().set_timer_ns(
            &name,
            EXPIRED_ENGINE_SWEEP_INTERVAL,
            None,
            None,
            Some(TimeEventCallback::from(callback)),
            None,
            None,
        ) {
            log::error!("Failed to register sandbox expiry sweep timer: {e}");
        }
    }

    fn cancel_expiry_sweep_timer(&self) {
        self.clock
            .borrow_mut()
            .cancel_timer(&self.expiry_sweep_timer_name());
    }

    fn cancel_inbound_alert(&self) {
        let client_id = self.core.borrow().client_id;
        self.clock
            .borrow_mut()
            .cancel_timer(&inbound_alert_name(client_id));
    }

    fn get_current_account_balances(&self) -> Vec<AccountBalance> {
        let account_id = self.core.borrow().account_id;
        let cache = self.cache.borrow();

        if let Some(account) = cache.account(&account_id) {
            return account.balances().into_values().collect();
        }

        self.get_account_balances()
    }

    fn sync_cached_account_config(&self) -> anyhow::Result<()> {
        let Some(mut account) = self.get_account() else {
            return Ok(());
        };

        account.set_calculate_account_state(!self.config.frozen_account);

        if let AccountAny::Margin(margin_account) = &mut account {
            if let Some(model) = &self.config.margin_model {
                margin_account.set_margin_model(model.clone().into());
            }

            margin_account.set_default_leverage(self.config.default_leverage);
            for (instrument_id, leverage) in &self.config.leverages {
                margin_account.set_leverage(*instrument_id, *leverage);
            }
        }

        self.cache.borrow_mut().update_account(&account)
    }

    /// Processes a quote tick through the matching engine.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument is not found in the cache.
    pub fn process_quote_tick(&self, quote: &QuoteTick) -> anyhow::Result<()> {
        SandboxInner::drain_inbound(&self.inner);

        let instrument_id = quote.instrument_id;
        let instrument = self.cache.borrow().try_instrument(&instrument_id)?.clone();

        if !check_quote_or_drop("quote tick", quote, &instrument) {
            return Ok(());
        }

        let mut inner = self.inner.borrow_mut();
        inner.ensure_matching_engine(&instrument);
        if let Some(engine) = inner.matching_engines.get_mut(&instrument_id) {
            engine.process_quote_tick(quote);
        }

        inner.drain_oto_commands();
        Ok(())
    }

    /// Processes a trade tick through the matching engine.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument is not found in the cache.
    pub fn process_trade_tick(&self, trade: &TradeTick) -> anyhow::Result<()> {
        SandboxInner::drain_inbound(&self.inner);

        if !self.config.trade_execution {
            return Ok(());
        }

        let instrument_id = trade.instrument_id;
        let instrument = self.cache.borrow().try_instrument(&instrument_id)?.clone();

        if !check_trade_or_drop("trade tick", trade, &instrument) {
            return Ok(());
        }

        let mut inner = self.inner.borrow_mut();
        inner.ensure_matching_engine(&instrument);
        if let Some(engine) = inner.matching_engines.get_mut(&instrument_id) {
            engine.process_trade_tick(trade);
        }

        inner.drain_oto_commands();
        Ok(())
    }

    /// Processes a bar through the matching engine.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument is not found in the cache.
    pub fn process_bar(&self, bar: &Bar) -> anyhow::Result<()> {
        SandboxInner::drain_inbound(&self.inner);

        if !self.config.bar_execution {
            return Ok(());
        }

        let instrument_id = bar.bar_type.instrument_id();
        let instrument = self.cache.borrow().try_instrument(&instrument_id)?.clone();

        if !check_bar_or_drop("bar", bar, &instrument) {
            return Ok(());
        }

        let mut inner = self.inner.borrow_mut();
        inner.ensure_matching_engine(&instrument);
        if let Some(engine) = inner.matching_engines.get_mut(&instrument_id) {
            engine.process_bar(bar);
        }

        inner.drain_oto_commands();
        Ok(())
    }

    /// Processes order book deltas through the matching engine.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument is not found in the cache.
    pub fn process_order_book_deltas(&self, deltas: &OrderBookDeltas) -> anyhow::Result<()> {
        SandboxInner::drain_inbound(&self.inner);

        let instrument_id = deltas.instrument_id;
        let instrument = self.cache.borrow().try_instrument(&instrument_id)?.clone();

        let mut inner = self.inner.borrow_mut();
        inner.ensure_matching_engine(&instrument);
        if let Some(engine) = inner.matching_engines.get_mut(&instrument_id) {
            engine.process_order_book_deltas(deltas)?;
        }

        inner.drain_oto_commands();
        Ok(())
    }

    /// Resets the sandbox to its initial state.
    pub fn reset(&self) {
        let mut inner = self.inner.borrow_mut();
        for engine in inner.matching_engines.values_mut() {
            engine.reset();
        }

        inner.balances.clear();
        for money in &self.config.starting_balances {
            inner
                .balances
                .insert(money.currency.code.to_string(), *money);
        }

        inner.clear_inbound_queue();
        inner.position_operations.clear();
        inner.pending_position_operation_cash.clear();
        self.cancel_inbound_alert();

        log::info!(
            "Sandbox execution client reset: venue={}",
            self.config.venue
        );
    }

    fn get_account_balances(&self) -> Vec<AccountBalance> {
        self.inner
            .borrow()
            .balances
            .values()
            .map(|money| AccountBalance::new(*money, Money::zero(money.currency), *money))
            .collect()
    }

    fn get_order(&self, client_order_id: &ClientOrderId) -> anyhow::Result<OrderAny> {
        Ok(self.cache.borrow().try_order_owned(client_order_id)?)
    }
}

#[async_trait(?Send)]
impl ExecutionClient for SandboxExecutionClient {
    fn is_connected(&self) -> bool {
        self.core.borrow().is_connected()
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

    // Position reports are always empty, so an absent report is not evidence of a flat position
    fn provides_bulk_position_coverage(&self, _instrument_id: InstrumentId) -> bool {
        false
    }

    fn settles_contract_expirations(&self) -> bool {
        true
    }

    fn on_instrument(&mut self, instrument: InstrumentAny) {
        let instrument_id = instrument.id();
        let mut inner = self.inner.borrow_mut();
        if let Some(engine) = inner.matching_engines.get_mut(&instrument_id)
            && let Err(e) = engine.update_instrument(instrument)
        {
            log::error!("Failed to update instrument {instrument_id} in sandbox engine: {e}");
        }

        inner.drain_oto_commands();
    }

    fn get_account(&self) -> Option<AccountAny> {
        let account_id = self.core.borrow().account_id;
        self.cache.borrow().account_owned(&account_id)
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        Self::reset(self);
        Ok(())
    }

    fn generate_account_state(
        &self,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        reported: bool,
        ts_event: UnixNanos,
        info: Option<Params>,
    ) -> anyhow::Result<()> {
        let ts_init = self.clock.borrow().timestamp_ns();
        let state = self
            .factory
            .generate_account_state(balances, margins, reported, ts_event, ts_init, info);
        let endpoint = MessagingSwitchboard::portfolio_update_account();
        msgbus::send_account_state(endpoint, &state);
        self.sync_cached_account_config()?;
        Ok(())
    }

    fn start(&mut self) -> anyhow::Result<()> {
        if self.core.borrow().is_started() {
            return Ok(());
        }

        if let Some(sender) = try_get_exec_event_sender() {
            let handler: Rc<dyn Fn(OrderEventAny)> = Rc::new(move |event: OrderEventAny| {
                if let Err(e) = sender.send(ExecutionEvent::Order(event)) {
                    log::warn!("Failed to send order event: {e}");
                }
            });
            let mut inner = self.inner.borrow_mut();
            inner.event_handler = Some(handler.clone());
            for engine in inner.matching_engines.values_mut() {
                engine.set_event_handler(handler.clone());
            }
        }

        self.inner.borrow_mut().load_open_orders();

        self.register_message_handlers();
        self.register_expiry_sweep_timer();

        self.core.borrow().set_started();
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
        if self.core.borrow().is_stopped() {
            return Ok(());
        }

        self.deregister_message_handlers();
        self.cancel_expiry_sweep_timer();
        self.cancel_inbound_alert();

        // Command outcomes take the execution channel, so the engine stopping
        // this client processes them once its own borrow is released. Without a runner sender they
        // would dispatch synchronously into an engine `ExecutionEngine::stop` still holds mutably,
        // which is unsupported
        {
            let mut inner = self.inner.borrow_mut();
            let discarded = inner.take_inbound_queue();
            inner.refuse_discarded(discarded);
        }

        self.core.borrow().set_stopped();
        self.core.borrow().set_disconnected();
        log::info!(
            "Sandbox execution client stopped: venue={}",
            self.config.venue
        );
        Ok(())
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.core.borrow().is_connected() {
            return Ok(());
        }

        let balances = self.get_account_balances();
        let ts_event = self.clock.borrow().timestamp_ns();
        self.generate_account_state(balances, vec![], false, ts_event, None)?;

        self.core.borrow().set_connected();
        log::info!(
            "Sandbox execution client connected: venue={}",
            self.config.venue
        );
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if self.core.borrow().is_disconnected() {
            return Ok(());
        }

        self.core.borrow().set_disconnected();
        log::info!(
            "Sandbox execution client disconnected: venue={}",
            self.config.venue
        );
        Ok(())
    }

    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
        let order = self.get_order(&cmd.client_order_id)?;

        if order.is_closed() {
            log::warn!("Cannot submit closed order {}", order.client_order_id());
            return Ok(());
        }

        let ts_init = self.clock.borrow().timestamp_ns();
        let event = self.factory.generate_order_submitted(&order, ts_init);
        self.dispatch_order_event(event);

        let mut inner = self.inner.borrow_mut();
        if inner.config.latency_model.is_none() {
            inner.apply_submit_order(&cmd)?;
        } else {
            inner.defer_or_apply(TradingCommand::SubmitOrder(cmd));
        }
        Ok(())
    }

    fn submit_order_list(&self, cmd: SubmitOrderList) -> anyhow::Result<()> {
        let ts_init = self.clock.borrow().timestamp_ns();

        let orders: Vec<OrderAny> = self
            .cache
            .borrow()
            .orders_for_ids(&cmd.order_list.client_order_ids, &cmd);

        for order in &orders {
            if order.is_closed() {
                log::warn!("Cannot submit closed order {}", order.client_order_id());
                continue;
            }

            let event = self.factory.generate_order_submitted(order, ts_init);
            self.dispatch_order_event(event);
        }

        let mut inner = self.inner.borrow_mut();
        if inner.config.latency_model.is_none() {
            let _ = inner.apply_submit_order_list(&cmd);
        } else {
            inner.defer_or_apply(TradingCommand::SubmitOrderList(cmd));
        }
        Ok(())
    }

    fn modify_order(&self, cmd: ModifyOrder) -> anyhow::Result<()> {
        let mut inner = self.inner.borrow_mut();
        if inner.config.latency_model.is_none() {
            inner.apply_modify_order(&cmd);
        } else {
            inner.defer_or_apply(TradingCommand::ModifyOrder(cmd));
        }
        Ok(())
    }

    fn batch_modify_orders(&self, cmd: BatchModifyOrders) -> anyhow::Result<()> {
        let mut inner = self.inner.borrow_mut();
        if inner.config.latency_model.is_none() {
            inner.apply_batch_modify_orders(&cmd);
        } else {
            inner.defer_or_apply(TradingCommand::ModifyOrders(cmd));
        }
        Ok(())
    }

    fn cancel_order(&self, cmd: CancelOrder) -> anyhow::Result<()> {
        let mut inner = self.inner.borrow_mut();
        if inner.config.latency_model.is_none() {
            inner.apply_cancel_order(&cmd);
        } else {
            inner.defer_or_apply(TradingCommand::CancelOrder(cmd));
        }
        Ok(())
    }

    fn cancel_all_orders(&self, cmd: CancelAllOrders) -> anyhow::Result<()> {
        let mut inner = self.inner.borrow_mut();
        if inner.config.latency_model.is_none() {
            inner.apply_cancel_all_orders(&cmd);
        } else {
            inner.defer_or_apply(TradingCommand::CancelAllOrders(cmd));
        }
        Ok(())
    }

    fn batch_cancel_orders(&self, cmd: BatchCancelOrders) -> anyhow::Result<()> {
        let mut inner = self.inner.borrow_mut();
        if inner.config.latency_model.is_none() {
            inner.apply_batch_cancel_orders(&cmd);
        } else {
            inner.defer_or_apply(TradingCommand::CancelOrders(cmd));
        }
        Ok(())
    }

    fn query_account(&self, _cmd: QueryAccount) -> anyhow::Result<()> {
        let balances = self.get_current_account_balances();
        let ts_event = self.clock.borrow().timestamp_ns();
        self.generate_account_state(balances, vec![], false, ts_event, None)?;
        Ok(())
    }

    fn query_order(&self, _cmd: QueryOrder) -> anyhow::Result<()> {
        // Orders are tracked in the cache, no external query needed for sandbox
        Ok(())
    }

    fn submit_position_operation(&self, cmd: SubmitPositionOperation) -> anyhow::Result<()> {
        let mut inner = self.inner.borrow_mut();
        if inner.config.latency_model.is_none() {
            inner.apply_submit_position_operation(&cmd);
        } else {
            inner.defer_or_apply(TradingCommand::SubmitPositionOperation(cmd));
        }
        Ok(())
    }

    fn on_position_operation_applied(
        &self,
        operation_id: UUID4,
        _legs: &[nautilus_model::position_operation::PositionOperationLeg],
        _transaction_hash: Option<&str>,
    ) {
        let mut inner = self.inner.borrow_mut();
        if let Some((_, _, status)) = inner.position_operations.get_mut(&operation_id) {
            *status = PositionOperationStatus::Applied;
        }
        if let Err(e) = inner.apply_pending_position_operation_cash(operation_id) {
            log::error!(
                "Failed to apply simulated cash for position operation {operation_id}: {e}"
            );
        }
    }

    fn query_position_operation(&self, cmd: QueryPositionOperation) -> anyhow::Result<()> {
        let inner = self.inner.borrow();
        let Some((operation, instrument_ids, status)) =
            inner.position_operations.get(&cmd.operation_id).cloned()
        else {
            anyhow::bail!(
                "sandbox has no record for position operation {}",
                cmd.operation_id
            );
        };

        let Some(sender) = try_get_exec_event_sender() else {
            anyhow::bail!("execution event sender is unavailable");
        };
        let ts_event = self.clock.borrow().timestamp_ns();
        let ts_init = self.clock.borrow().timestamp_ns();
        let result = PositionOperationResult::new(
            cmd.trader_id,
            cmd.strategy_id,
            inner.client_id,
            inner.account_id,
            cmd.operation_id,
            operation,
            status,
            instrument_ids,
            None,
            None,
            None,
            None,
            UUID4::new(),
            ts_event,
            ts_init,
        );

        if let Err(e) = sender.send(ExecutionEvent::PositionOperation(result)) {
            log::error!(
                "Failed to send position operation {} query result: {e}",
                cmd.operation_id
            );
        }
        Ok(())
    }

    async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let cache = self.cache.borrow();

        let client_order_id = cmd.client_order_id.or_else(|| {
            cmd.venue_order_id
                .and_then(|id| cache.client_order_id(&id).copied())
        });

        let Some(order) = client_order_id.and_then(|id| cache.order(&id)) else {
            return Ok(None);
        };

        if order.account_id() != Some(self.core.borrow().account_id)
            || order.instrument_id().venue != self.config.venue
            || cmd
                .instrument_id
                .is_some_and(|id| id != order.instrument_id())
            || cmd
                .venue_order_id
                .is_some_and(|id| Some(id) != order.venue_order_id())
        {
            return Ok(None);
        }

        let ts_init = self.clock.borrow().timestamp_ns();
        Ok(order_status_report(&order, ts_init))
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let account_id = self.core.borrow().account_id;
        let ts_init = self.clock.borrow().timestamp_ns();
        let cache = self.cache.borrow();

        let reports = cache
            .orders_refs(
                Some(&self.config.venue),
                cmd.instrument_id.as_ref(),
                None,
                Some(&account_id),
                None,
            )
            .into_iter()
            .filter(|order| !cmd.open_only || order.is_open() || order.is_inflight())
            .filter(|order| {
                !order.is_closed()
                    || (cmd.start.is_none_or(|start| order.ts_last() >= start)
                        && cmd.end.is_none_or(|end| order.ts_last() <= end))
            })
            .filter_map(|order| order_status_report(&order, ts_init))
            .collect();

        Ok(reports)
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
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        let ts_init = self.clock.borrow().timestamp_ns();
        generate_mass_status(self, lookback_mins, ts_init).await
    }
}

fn order_status_report(order: &OrderAny, ts_init: UnixNanos) -> Option<OrderStatusReport> {
    let mut report = order.to_order_status_report(None)?;
    report.ts_init = ts_init;
    report.venue_position_id = None;

    for event in order.events() {
        if let OrderEventAny::Canceled(event) = event {
            report.cancel_reason = event.reason.map(|reason| reason.to_string());
        }
    }

    Some(report)
}

// Wrapped in `Rc<RefCell<>>` so message handlers can hold weak references
struct SandboxInner {
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
    config: SandboxExecutionClientConfig,
    fill_model: FillModelHandle,
    fee_model: FeeModelHandle,
    matching_engines: AHashMap<InstrumentId, OrderMatchingEngine>,
    next_engine_raw_id: u32,
    balances: AHashMap<String, Money>,
    /// Forwards order events to the runner's execution channel once `start` finds a sender; shared
    /// with every matching engine this client owns.
    event_handler: Option<Rc<dyn Fn(OrderEventAny)>>,
    /// Inbound commands deferred by latency, ordered as a min-heap by due time.
    inbound_queue: BinaryHeap<DelayedCommand>,
    inflight_orders: InflightOrders,
    matching_orders: MatchingOrders,
    /// Monotonic sequence providing FIFO tie-breaking for deferred commands sharing a due time, so
    /// no queued command can be overtaken by one enqueued after it.
    inbound_seq: u64,
    client_id: ClientId,
    account_id: AccountId,
    factory: OrderEventFactory,
    /// Operations submitted in this session, keyed by operation identity for deduplication.
    position_operations: AHashMap<
        UUID4,
        (
            PositionOperationType,
            Vec<InstrumentId>,
            PositionOperationStatus,
        ),
    >,
    /// Cash effects retained until the core confirms that it applied the operation.
    pending_position_operation_cash: AHashMap<UUID4, Decimal>,
    self_weak: WeakCell<Self>,
}

/// A [`TradingCommand`] deferred by inbound latency, ordered by `due_ns` then `seq` so the
/// `BinaryHeap` behaves as a min-heap for FIFO draining. Equality follows the same key.
#[derive(Debug)]
struct DelayedCommand {
    due_ns: UnixNanos,
    seq: u64,
    command: TradingCommand,
}

impl Ord for DelayedCommand {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other
            .due_ns
            .cmp(&self.due_ns)
            .then_with(|| other.seq.cmp(&self.seq))
    }
}

impl PartialOrd for DelayedCommand {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for DelayedCommand {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == std::cmp::Ordering::Equal
    }
}

impl Eq for DelayedCommand {}

impl SandboxInner {
    fn apply_submit_position_operation(&mut self, cmd: &SubmitPositionOperation) {
        if self.config.account_type != AccountType::Cash || self.config.oms_type != OmsType::Netting
        {
            self.send_position_operation_result(
                cmd,
                PositionOperationStatus::Rejected,
                cmd.instrument_ids.clone(),
                None,
                Some("sandbox position operations require a Cash/NETTING account".to_string()),
            );
            return;
        }

        if let Err(e) = cmd.validate() {
            self.send_position_operation_result(
                cmd,
                PositionOperationStatus::Rejected,
                cmd.instrument_ids.clone(),
                None,
                Some(e.to_string()),
            );
            return;
        }

        if cmd.operation != PositionOperationType::Convert {
            self.send_position_operation_result(
                cmd,
                PositionOperationStatus::Rejected,
                cmd.instrument_ids.clone(),
                None,
                Some(format!(
                    "sandbox supports only CTF convert, was {}",
                    cmd.operation
                )),
            );
            return;
        }

        if self.position_operations.contains_key(&cmd.operation_id) {
            return;
        }

        let (instrument_ids, effects, collateral) = match self.simulate_convert(cmd) {
            Ok(simulated) => simulated,
            Err(e) => {
                self.send_position_operation_result(
                    cmd,
                    PositionOperationStatus::Rejected,
                    cmd.instrument_ids.clone(),
                    None,
                    Some(e.to_string()),
                );
                return;
            }
        };

        if !self.config.frozen_account && !collateral.is_zero() {
            // Cash is applied only after the core confirms it applied the position changes, so a
            // rejected or partially applied operation cannot change the simulated balance.
            self.pending_position_operation_cash
                .insert(cmd.operation_id, collateral);
        }

        self.send_position_operation_result(
            cmd,
            PositionOperationStatus::Confirmed,
            instrument_ids.clone(),
            Some(effects),
            None,
        );
        self.position_operations.insert(
            cmd.operation_id,
            (
                cmd.operation,
                instrument_ids,
                PositionOperationStatus::Confirmed,
            ),
        );
    }

    fn simulate_convert(
        &self,
        cmd: &SubmitPositionOperation,
    ) -> anyhow::Result<(Vec<InstrumentId>, PositionOperationEffects, Decimal)> {
        let quantity = cmd
            .quantity
            .ok_or_else(|| anyhow::anyhow!("CTF convert requires a quantity"))?;
        let input_base = quantity_to_base_units(quantity)?;
        let now = self.clock.borrow().timestamp_ns();

        let mut market: Option<NegRiskMarketMetadata> = None;
        let mut indices: Vec<u8> = Vec::new();
        let mut spent = Vec::with_capacity(cmd.instrument_ids.len());

        {
            let cache = self.cache.borrow();

            for instrument_id in &cmd.instrument_ids {
                let instrument = cache.instrument(instrument_id).ok_or_else(|| {
                    anyhow::anyhow!("instrument {instrument_id} is not in the cache")
                })?;
                let InstrumentAny::BinaryOption(binary) = instrument else {
                    anyhow::bail!("position operations require binary option instruments");
                };
                let positions = cache.positions_open(
                    None,
                    Some(instrument_id),
                    Some(&cmd.strategy_id),
                    Some(&self.account_id),
                    None,
                );
                anyhow::ensure!(
                    positions.len() <= 1,
                    "multiple open positions for {instrument_id} in NETTING mode"
                );
                let available = positions.first().map_or(Decimal::ZERO, |position| {
                    position.signed_decimal_qty().max(Decimal::ZERO)
                });
                anyhow::ensure!(
                    available >= quantity.as_decimal(),
                    "sandbox convert requires {} available {instrument_id}, found {available}",
                    quantity.as_decimal()
                );
                ensure_instrument_active(
                    instrument_id,
                    binary.activation_ns,
                    binary.expiration_ns,
                    now,
                )?;

                let info = binary
                    .info
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("instrument {instrument_id} has no metadata"))?;
                anyhow::ensure!(
                    info.get_str(MARKET_VERSION_KEY) != Some(MARKET_VERSION_V2),
                    "sandbox does not support V2 CTF convert for {instrument_id}"
                );
                let parsed = NegRiskMarketMetadata::parse(info)?;

                if let Some(existing) = &market {
                    anyhow::ensure!(
                        existing == &parsed,
                        "conversion inputs span different negative-risk markets"
                    );
                } else {
                    market = Some(parsed);
                }

                let token_id = info
                    .get_str("token_id")
                    .ok_or_else(|| anyhow::anyhow!("instrument {instrument_id} has no token_id"))?;
                let condition_id = info.get_str("condition_id").ok_or_else(|| {
                    anyhow::anyhow!("instrument {instrument_id} has no condition_id")
                })?;
                let outcome = binary
                    .outcome
                    .map(|outcome| outcome.to_string())
                    .unwrap_or_default();
                anyhow::ensure!(
                    outcome.eq_ignore_ascii_case("no"),
                    "conversion inputs must be NO instruments"
                );

                let market = market.as_ref().expect("market was just set");
                let condition = market.condition_for_no_token(token_id).ok_or_else(|| {
                    anyhow::anyhow!(
                        "token {token_id} is not a NO outcome of negative-risk market {}",
                        market.market_id
                    )
                })?;
                anyhow::ensure!(
                    condition.condition_id == condition_id,
                    "instrument {instrument_id} condition does not match its negative-risk metadata"
                );
                anyhow::ensure!(
                    !indices.contains(&condition.question_index),
                    "conversion inputs select question index {} more than once",
                    condition.question_index
                );

                indices.push(condition.question_index);
                spent.push(PositionOperationTokenAmount {
                    token_id: token_id.to_string(),
                    quantity: base_units_to_decimal(input_base)?.to_string(),
                });
            }
        }

        let market = market
            .ok_or_else(|| anyhow::anyhow!("CTF convert requires at least one NO instrument"))?;
        let amounts = convert_amounts(input_base, market.fee_bips, indices.len())
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let mut received = Vec::new();
        let mut instrument_ids = cmd.instrument_ids.clone();
        {
            let cache = self.cache.borrow();
            let instruments = cache.instruments(&self.config.venue, None);

            for condition in &market.conditions {
                if indices.contains(&condition.question_index) {
                    continue;
                }

                let output = instruments
                    .iter()
                    .find_map(|instrument| {
                        let InstrumentAny::BinaryOption(binary) = instrument else {
                            return None;
                        };
                        let info = binary.info.as_ref()?;
                        (info.get_str("token_id") == Some(condition.yes_token_id.as_str()))
                            .then_some(*instrument)
                    })
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "missing YES instrument for conversion output token {}",
                            condition.yes_token_id
                        )
                    })?;
                let InstrumentAny::BinaryOption(binary) = output else {
                    unreachable!("output was matched as a binary option");
                };
                ensure_instrument_active(
                    &output.id(),
                    binary.activation_ns,
                    binary.expiration_ns,
                    now,
                )?;

                instrument_ids.push(output.id());
                received.push(PositionOperationTokenAmount {
                    token_id: condition.yes_token_id.clone(),
                    quantity: base_units_to_decimal(amounts.per_outcome)?.to_string(),
                });
            }
        }

        instrument_ids.sort();
        instrument_ids.dedup();

        let collateral = base_units_to_decimal(amounts.collateral_return)?;
        let effects = PositionOperationEffects {
            received,
            spent,
            collateral_delta: collateral.to_string(),
            payouts: Vec::new(),
        };

        Ok((instrument_ids, effects, collateral))
    }

    fn apply_simulated_collateral(&mut self, collateral: Decimal) -> anyhow::Result<()> {
        if self.config.frozen_account || collateral.is_zero() {
            return Ok(());
        }

        let currency = Currency::pUSD();
        let mut balances: Vec<AccountBalance> = {
            let cache = self.cache.borrow();
            match cache.account_owned(&self.account_id) {
                Some(account) => account.balances().into_values().collect(),
                None => self
                    .balances
                    .values()
                    .map(|money| AccountBalance::new(*money, Money::zero(money.currency), *money))
                    .collect(),
            }
        };

        let delta =
            Money::from_decimal(collateral, currency).map_err(|e| anyhow::anyhow!("{e}"))?;
        if let Some(balance) = balances
            .iter_mut()
            .find(|balance| balance.currency == currency)
        {
            balance.total = balance.total + delta;
            balance.free = balance.free + delta;
        } else {
            balances.push(AccountBalance::new(delta, Money::zero(currency), delta));
        }

        if let Some(total) = balances
            .iter()
            .find(|balance| balance.currency == currency)
            .map(|balance| balance.total)
        {
            self.balances.insert(currency.code.to_string(), total);
        }

        let ts_event = self.clock.borrow().timestamp_ns();
        let ts_init = self.clock.borrow().timestamp_ns();
        let state = self.factory.generate_account_state(
            balances,
            Vec::new(),
            false,
            ts_event,
            ts_init,
            None,
        );
        msgbus::send_account_state(MessagingSwitchboard::portfolio_update_account(), &state);
        Ok(())
    }

    fn apply_pending_position_operation_cash(&mut self, operation_id: UUID4) -> anyhow::Result<()> {
        if let Some(collateral) = self.pending_position_operation_cash.remove(&operation_id)
            && let Err(e) = self.apply_simulated_collateral(collateral)
        {
            self.pending_position_operation_cash
                .insert(operation_id, collateral);
            return Err(e);
        }
        Ok(())
    }

    fn send_position_operation_result(
        &self,
        cmd: &SubmitPositionOperation,
        status: PositionOperationStatus,
        instrument_ids: Vec<InstrumentId>,
        effects: Option<PositionOperationEffects>,
        message: Option<String>,
    ) {
        let Some(sender) = try_get_exec_event_sender() else {
            log::error!(
                "Cannot send position operation {} result: execution event sender is unavailable",
                cmd.operation_id
            );
            return;
        };
        let ts_event = self.clock.borrow().timestamp_ns();
        let ts_init = self.clock.borrow().timestamp_ns();
        let result = PositionOperationResult::new(
            cmd.trader_id,
            cmd.strategy_id,
            self.client_id,
            self.account_id,
            cmd.operation_id,
            cmd.operation,
            status,
            instrument_ids,
            None,
            None,
            message,
            effects,
            UUID4::new(),
            ts_event,
            ts_init,
        );

        if let Err(e) = sender.send(ExecutionEvent::PositionOperation(result)) {
            log::error!(
                "Failed to send position operation {} result: {e}",
                cmd.operation_id
            );
        }
    }
}

impl SandboxInner {
    fn load_open_orders(&mut self) {
        let venue = self.config.venue;
        let account_id = self.account_id;
        let mut open_orders: Vec<(OrderAny, AccountId)> = {
            let cache = self.cache.borrow();
            cache
                .orders_open(Some(&venue), None, None, Some(&account_id), None)
                .into_iter()
                .filter(|order| !order.is_emulated())
                .filter_map(|order| {
                    order
                        .account_id()
                        .map(|account_id| (order.clone(), account_id))
                })
                .collect()
        };

        open_orders.sort_by(|(a, _), (b, _)| {
            a.ts_init()
                .cmp(&b.ts_init())
                .then_with(|| a.client_order_id().cmp(&b.client_order_id()))
        });

        for (order, account_id) in open_orders {
            let instrument_id = order.instrument_id();
            let instrument = self.cache.borrow().instrument(&instrument_id).cloned();
            match instrument {
                Some(instrument) => {
                    self.ensure_matching_engine(&instrument);

                    if let Some(engine) = self.matching_engines.get_mut(&instrument_id) {
                        engine.restore_open_order(&order, account_id);
                    }
                }
                None => {
                    log::warn!(
                        "No instrument for {instrument_id} to restore open order {}",
                        order.client_order_id()
                    );
                }
            }
        }
    }

    fn ensure_matching_engine(&mut self, instrument: &InstrumentAny) {
        let instrument_id = instrument.id();

        if !self.matching_engines.contains_key(&instrument_id) {
            let engine_config = self.config.to_matching_engine_config();
            let fill_model = self.fill_model.clone();
            let fee_model = self.fee_model.clone();
            let raw_id = self.next_engine_raw_id;
            self.next_engine_raw_id = self.next_engine_raw_id.wrapping_add(1);

            let mut engine = OrderMatchingEngine::new(
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

            if let Some(handler) = &self.event_handler {
                engine.set_event_handler(handler.clone());
            }

            engine.set_inflight_orders(self.inflight_orders.clone());
            engine.set_matching_orders(self.matching_orders.clone());
            self.matching_engines.insert(instrument_id, engine);
        }
    }

    fn process_quote_tick(&mut self, quote: &QuoteTick) {
        let instrument_id = quote.instrument_id;

        let instrument = self.cache.borrow().instrument(&instrument_id).cloned();
        if let Some(instrument) = instrument {
            if !check_quote_or_drop("quote tick", quote, &instrument) {
                return;
            }

            self.ensure_matching_engine(&instrument);

            if let Some(engine) = self.matching_engines.get_mut(&instrument_id) {
                engine.process_quote_tick(quote);
            }
        }

        self.drain_oto_commands();
    }

    fn process_trade_tick(&mut self, trade: &TradeTick) {
        if !self.config.trade_execution {
            return;
        }

        let instrument_id = trade.instrument_id;

        let instrument = self.cache.borrow().instrument(&instrument_id).cloned();
        if let Some(instrument) = instrument {
            if !check_trade_or_drop("trade tick", trade, &instrument) {
                return;
            }

            self.ensure_matching_engine(&instrument);

            if let Some(engine) = self.matching_engines.get_mut(&instrument_id) {
                engine.process_trade_tick(trade);
            }
        }

        self.drain_oto_commands();
    }

    fn process_bar(&mut self, bar: &Bar) {
        if !self.config.bar_execution {
            return;
        }

        let instrument_id = bar.bar_type.instrument_id();

        let instrument = self.cache.borrow().instrument(&instrument_id).cloned();
        if let Some(instrument) = instrument {
            if !check_bar_or_drop("bar", bar, &instrument) {
                return;
            }

            self.ensure_matching_engine(&instrument);

            if let Some(engine) = self.matching_engines.get_mut(&instrument_id) {
                engine.process_bar(bar);
            }
        }

        self.drain_oto_commands();
    }

    fn process_order_book_deltas(&mut self, deltas: &OrderBookDeltas) {
        let instrument_id = deltas.instrument_id;

        let instrument = self.cache.borrow().instrument(&instrument_id).cloned();
        if let Some(instrument) = instrument {
            self.ensure_matching_engine(&instrument);

            if let Some(engine) = self.matching_engines.get_mut(&instrument_id)
                && let Err(e) = engine.process_order_book_deltas(deltas)
            {
                log::error!("Error processing order book deltas: {e}");
            }
        }

        self.drain_oto_commands();
    }

    fn process_order_book_depth(&mut self, depth: &OrderBookDepth) {
        let instrument_id = depth.instrument_id;

        let instrument = self.cache.borrow().instrument(&instrument_id).cloned();
        if let Some(instrument) = instrument {
            self.ensure_matching_engine(&instrument);

            if let Some(engine) = self.matching_engines.get_mut(&instrument_id)
                && let Err(e) = engine.process_order_book_depth(depth)
            {
                log::error!("Error processing order book depth: {e}");
            }
        }

        self.drain_oto_commands();
    }

    fn process_instrument_status(&mut self, status: &InstrumentStatus) {
        let instrument_id = status.instrument_id;

        if let Some(engine) = self.matching_engines.get_mut(&instrument_id) {
            engine.process_status(status.action);
            return;
        }

        let instrument = self.cache.borrow().instrument(&instrument_id).cloned();
        if let Some(instrument) = instrument {
            self.ensure_matching_engine(&instrument);

            if let Some(engine) = self.matching_engines.get_mut(&instrument_id) {
                engine.process_status(status.action);
            }
        } else {
            log::warn!(
                "Ignoring instrument status for {instrument_id}: instrument missing from cache",
            );
        }
    }

    fn process_instrument_close(&mut self, close: &InstrumentClose) {
        let instrument_id = close.instrument_id;

        // A delayed close belongs to an existing exposure lifecycle. Unlike an
        // instrument status update, it must not recreate execution state from
        // cache after rotation/unsubscribe; pending-settlement ownership stays
        // with the already-initialized matching engine.
        if let Some(engine) = self.matching_engines.get_mut(&instrument_id) {
            engine.process_instrument_close(*close);
            self.sync_expired_cleanup(instrument_id);
        } else {
            log::warn!(
                "Ignoring instrument close for {instrument_id}: no existing matching engine",
            );
        }

        self.drain_oto_commands();
    }

    fn is_expired_now(&self, instrument_id: InstrumentId) -> bool {
        let Some(engine) = self.matching_engines.get(&instrument_id) else {
            return false;
        };

        let now_ns = self.clock.borrow().timestamp_ns();
        engine
            .instrument
            .expiration_ns()
            .is_some_and(|ns| now_ns >= ns)
    }

    fn has_open_orders(&self, instrument_id: InstrumentId) -> bool {
        self.cache.borrow().has_orders_open(
            Some(&self.config.venue),
            Some(&instrument_id),
            None,
            None,
            None,
        )
    }

    fn sync_expired_cleanup(&mut self, instrument_id: InstrumentId) {
        if !self.is_expired_now(instrument_id) {
            return;
        }

        if self
            .matching_engines
            .get_mut(&instrument_id)
            .is_some_and(OrderMatchingEngine::has_pending_fills)
        {
            return;
        }

        let has_open_positions = self.cache.borrow().has_positions_open(
            Some(&self.config.venue),
            Some(&instrument_id),
            None,
            None,
            None,
        );

        if has_open_positions {
            return;
        }

        self.matching_engines.remove(&instrument_id);
        self.cache
            .borrow_mut()
            .purge_instrument_skip_order_guard(instrument_id);
    }

    fn sync_expired_cleanup_many(&mut self, instrument_ids: &[InstrumentId]) {
        for &instrument_id in instrument_ids {
            self.sync_expired_cleanup(instrument_id);
        }
    }

    // Retain open-order engines until terminal events apply, otherwise retry settlement
    // when a synchronous position callback could not borrow the sandbox.
    fn sweep_expired_engines(&mut self) {
        let expired_ids: Vec<InstrumentId> = self
            .matching_engines
            .keys()
            .copied()
            .filter(|instrument_id| {
                self.is_expired_now(*instrument_id) && !self.has_open_orders(*instrument_id)
            })
            .collect();

        let ts_now = self.clock.borrow().timestamp_ns();

        for instrument_id in expired_ids {
            if let Some(engine) = self.matching_engines.get_mut(&instrument_id) {
                engine.process_instrument_expiration(ts_now);
            }

            self.sync_expired_cleanup(instrument_id);
        }

        self.drain_oto_commands();
    }

    /// Routes a deferred [`TradingCommand`] to its venue-side apply helper.
    fn apply_trading_command(&mut self, cmd: &TradingCommand) -> anyhow::Result<()> {
        self.inflight_orders.remove(cmd);

        // Only a deferred command can overtake the submit that would have created the engine, so
        // build it here and let the venue raise the rejection.
        if matches!(
            cmd,
            TradingCommand::ModifyOrder(_)
                | TradingCommand::ModifyOrders(_)
                | TradingCommand::CancelOrder(_)
                | TradingCommand::CancelOrders(_)
        ) && !self.ensure_engine_for(cmd.instrument_id())
        {
            self.refuse_command(cmd, "No matching engine for instrument");
            return Ok(());
        }

        match cmd {
            TradingCommand::SubmitOrder(cmd) => self.apply_submit_order(cmd)?,
            TradingCommand::SubmitOrderList(cmd) => {
                // Only a deferred leg has an `OrderSubmitted` out that nothing else would resolve
                for order in self.apply_submit_order_list(cmd) {
                    self.reject_submit_leg(cmd, &order, "No instrument for order");
                }
            }
            TradingCommand::ModifyOrder(cmd) => self.apply_modify_order(cmd),
            TradingCommand::ModifyOrders(cmd) => self.apply_batch_modify_orders(cmd),
            TradingCommand::CancelOrder(cmd) => self.apply_cancel_order(cmd),
            TradingCommand::CancelOrders(cmd) => self.apply_batch_cancel_orders(cmd),
            TradingCommand::CancelAllOrders(cmd) => self.apply_cancel_all_orders(cmd),
            TradingCommand::QueryOrder(_) | TradingCommand::QueryAccount(_) => {}
            TradingCommand::SubmitPositionOperation(cmd) => {
                self.apply_submit_position_operation(cmd);
            }
            TradingCommand::QueryPositionOperation(_) => {
                // Queries are answered directly by the client from retained operation state.
            }
        }
        Ok(())
    }

    fn refuse_command(&self, command: &TradingCommand, reason: &str) {
        self.refuse_command_deduped(command, reason, &mut AHashSet::new());
    }

    fn refuse_command_deduped(
        &self,
        command: &TradingCommand,
        reason: &str,
        pending_denied: &mut AHashSet<(ClientOrderId, UUID4)>,
    ) {
        let ts_now = self.clock.borrow().timestamp_ns();
        let account_id = self.account_id;
        let reason = Ustr::from(reason);

        let reject_submit = |trader_id, strategy_id, instrument_id, client_order_id| {
            self.dispatch_order_event(OrderEventAny::Rejected(OrderRejected::new(
                trader_id,
                strategy_id,
                instrument_id,
                client_order_id,
                account_id,
                reason,
                UUID4::new(),
                ts_now,
                ts_now,
                false,
                false,
            )));
        };

        match command {
            TradingCommand::SubmitOrder(cmd) => reject_submit(
                cmd.trader_id,
                cmd.strategy_id,
                cmd.instrument_id,
                cmd.client_order_id,
            ),
            TradingCommand::SubmitOrderList(cmd) => {
                // Keyed per leg: the list's `instrument_id` is only representative
                let in_flight: Vec<(ClientOrderId, InstrumentId)> = self
                    .cache
                    .borrow()
                    .orders_for_ids(&cmd.order_list.client_order_ids, cmd)
                    .iter()
                    .filter(|order| !order.is_closed())
                    .map(|order| (order.client_order_id(), order.instrument_id()))
                    .collect();

                for (client_order_id, instrument_id) in in_flight {
                    reject_submit(
                        cmd.trader_id,
                        cmd.strategy_id,
                        instrument_id,
                        client_order_id,
                    );
                }
            }
            TradingCommand::ModifyOrder(cmd) => {
                if pending_denied.insert((cmd.client_order_id, cmd.command_id)) {
                    self.deny_modify(cmd, reason, ts_now);
                }
            }
            TradingCommand::ModifyOrders(cmd) => {
                for modify in &cmd.modifies {
                    if pending_denied.insert((modify.client_order_id, modify.command_id)) {
                        self.deny_modify(modify, reason, ts_now);
                    }
                }
            }
            TradingCommand::CancelOrder(cmd) => {
                if pending_denied.insert((cmd.client_order_id, cmd.command_id)) {
                    self.deny_cancel(cmd, reason, ts_now);
                }
            }
            TradingCommand::CancelOrders(cmd) => {
                for cancel in &cmd.cancels {
                    if pending_denied.insert((cancel.client_order_id, cancel.command_id)) {
                        self.deny_cancel(cancel, reason, ts_now);
                    }
                }
            }
            // Cancel-all carries no individual command IDs to deny
            TradingCommand::CancelAllOrders(_) => {}
            TradingCommand::QueryOrder(_)
            | TradingCommand::QueryAccount(_)
            | TradingCommand::SubmitPositionOperation(_)
            | TradingCommand::QueryPositionOperation(_) => {}
        }
    }

    fn deny_modify(&self, cmd: &ModifyOrder, reason: Ustr, ts_now: UnixNanos) {
        let mut event = OrderModifyDenied::new(
            cmd.trader_id,
            cmd.strategy_id,
            cmd.instrument_id,
            cmd.client_order_id,
            reason,
            UUID4::new(),
            ts_now,
            ts_now,
            cmd.venue_order_id,
            Some(self.account_id),
        );
        event.causation_id = Some(cmd.command_id);
        self.dispatch_order_event(OrderEventAny::ModifyDenied(event));
    }

    fn deny_cancel(&self, cmd: &CancelOrder, reason: Ustr, ts_now: UnixNanos) {
        let mut event = OrderCancelDenied::new(
            cmd.trader_id,
            cmd.strategy_id,
            cmd.instrument_id,
            cmd.client_order_id,
            reason,
            UUID4::new(),
            ts_now,
            ts_now,
            cmd.venue_order_id,
            Some(self.account_id),
        );
        event.causation_id = Some(cmd.command_id);
        self.dispatch_order_event(OrderEventAny::CancelDenied(event));
    }

    fn dispatch_order_event(&self, event: OrderEventAny) {
        if let Some(handler) = &self.event_handler {
            handler(event);
        } else {
            msgbus::send_order_event(MessagingSwitchboard::exec_engine_process(), event);
        }
    }

    /// Creates the matching engine from the cached instrument when it does not exist yet, so it can
    /// answer for an order it has never seen. Returns whether an engine now exists.
    fn ensure_engine_for(&mut self, instrument_id: InstrumentId) -> bool {
        if !self.matching_engines.contains_key(&instrument_id) {
            let instrument = self.cache.borrow().instrument(&instrument_id).cloned();
            let Some(instrument) = instrument else {
                log::warn!(
                    "Cannot process command for {instrument_id}: instrument missing from cache",
                );
                return false;
            };
            self.ensure_matching_engine(&instrument);
        }

        self.matching_engines.contains_key(&instrument_id)
    }

    fn apply_submit_order(&mut self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        let mut order = self.cache.borrow().try_order_owned(&cmd.client_order_id)?;

        let instrument_id = order.instrument_id();
        let instrument = self.cache.borrow().try_instrument(&instrument_id)?.clone();

        self.ensure_matching_engine(&instrument);

        self.seed_cached_market_data(instrument_id);

        if let Some(engine) = self.matching_engines.get_mut(&instrument_id) {
            engine.process_order(&mut order, self.account_id);
            self.sync_expired_cleanup(instrument_id);
        }

        self.drain_oto_commands();
        Ok(())
    }

    /// Applies a submit-order-list command to the matching engines (venue-side), less the per-order
    /// `OrderSubmitted` dispatch kept by the client handler, returning the legs that could not
    /// reach a matching engine.
    fn apply_submit_order_list(&mut self, cmd: &SubmitOrderList) -> Vec<OrderAny> {
        let orders: Vec<OrderAny> = self
            .cache
            .borrow()
            .orders_for_ids(&cmd.order_list.client_order_ids, cmd);

        let mut cleanup_instrument_ids = Vec::new();
        let mut unresolved: Vec<OrderAny> = Vec::new();

        for order in &orders {
            if order.is_closed() {
                continue;
            }

            let instrument_id = order.instrument_id();
            if !cleanup_instrument_ids.contains(&instrument_id) {
                cleanup_instrument_ids.push(instrument_id);
            }
            let instrument = self.cache.borrow().instrument(&instrument_id).cloned();

            let Some(instrument) = instrument else {
                // Skipped per leg rather than failing the whole command: the legs that did reach
                // the venue are live orders.
                unresolved.push(order.clone());
                continue;
            };

            self.ensure_matching_engine(&instrument);

            self.seed_cached_market_data(instrument_id);

            if let Some(engine) = self.matching_engines.get_mut(&instrument_id) {
                let mut order_clone = order.clone();
                engine.process_order(&mut order_clone, self.account_id);
            }
        }

        if !cleanup_instrument_ids.is_empty() {
            self.sync_expired_cleanup_many(&cleanup_instrument_ids);
        }

        self.drain_oto_commands();
        unresolved
    }

    fn drain_oto_commands(&mut self) {
        while let Some(command) = self.matching_orders.pop_command() {
            let instrument_id = command.instrument_id();
            if self.ensure_engine_for(instrument_id) {
                if matches!(command, OtoCommand::Release { .. }) {
                    self.seed_cached_market_data(instrument_id);
                }

                if let Some(engine) = self.matching_engines.get_mut(&instrument_id) {
                    engine.process_oto_command(&command);
                }
            }

            if let OtoCommand::Release {
                parent_quantity: Some(quantity),
                parent_order_id,
                parent_instrument_id,
                ..
            } = command
                && let Some(engine) = self.matching_engines.get_mut(&parent_instrument_id)
            {
                engine.process_oto_resize(parent_order_id, quantity);
            }
        }
    }

    /// Rejects a single leg of `cmd` the venue could not accept, leaving its siblings untouched.
    fn reject_submit_leg(&self, cmd: &SubmitOrderList, order: &OrderAny, reason: &str) {
        let ts_now = self.clock.borrow().timestamp_ns();
        self.dispatch_order_event(OrderEventAny::Rejected(OrderRejected::new(
            cmd.trader_id,
            cmd.strategy_id,
            order.instrument_id(),
            order.client_order_id(),
            self.account_id,
            Ustr::from(reason),
            UUID4::new(),
            ts_now,
            ts_now,
            false,
            false,
        )));
    }

    fn seed_cached_market_data(&mut self, instrument_id: InstrumentId) {
        let Some(engine) = self.matching_engines.get_mut(&instrument_id) else {
            return;
        };

        // Cached ticks initialize an empty engine; replay would replenish consumed liquidity
        if self.config.liquidity_consumption
            && (engine.get_book().update_count != 0 || engine.get_core().last.is_some())
        {
            return;
        }

        let (quote, trade) = {
            let cache = self.cache.borrow();
            (
                cache.quote(&instrument_id).copied(),
                cache.trade(&instrument_id).copied(),
            )
        };

        if let Some(quote) = quote
            && check_quote_or_drop("cached quote tick", &quote, &engine.instrument)
        {
            engine.process_quote_tick(&quote);
        }

        if self.config.trade_execution
            && let Some(trade) = trade
            && check_trade_or_drop("cached trade tick", &trade, &engine.instrument)
        {
            engine.process_trade_tick(&trade);
        }
    }

    fn apply_modify_order(&mut self, cmd: &ModifyOrder) {
        let account_id = self.account_id;
        if let Some(engine) = self.matching_engines.get_mut(&cmd.instrument_id) {
            engine.process_modify(cmd, account_id);
        } else {
            let ts_now = self.clock.borrow().timestamp_ns();
            self.deny_modify(cmd, "No matching engine for instrument".into(), ts_now);
        }

        self.drain_oto_commands();
    }

    fn apply_batch_modify_orders(&mut self, cmd: &BatchModifyOrders) {
        let account_id = self.account_id;
        if let Some(engine) = self.matching_engines.get_mut(&cmd.instrument_id) {
            engine.process_batch_modify(cmd, account_id);
        } else {
            let ts_now = self.clock.borrow().timestamp_ns();
            for command in &cmd.modifies {
                self.deny_modify(command, "No matching engine for instrument".into(), ts_now);
            }
        }

        self.drain_oto_commands();
    }

    fn apply_cancel_order(&mut self, cmd: &CancelOrder) {
        let account_id = self.account_id;
        if let Some(engine) = self.matching_engines.get_mut(&cmd.instrument_id) {
            engine.process_cancel(cmd, account_id);
        } else {
            let ts_now = self.clock.borrow().timestamp_ns();
            self.deny_cancel(cmd, "No matching engine for instrument".into(), ts_now);
        }

        self.drain_oto_commands();
    }

    fn apply_cancel_all_orders(&mut self, cmd: &CancelAllOrders) {
        let instrument_id = cmd.instrument_id;
        if let Some(engine) = self.matching_engines.get_mut(&instrument_id) {
            engine.process_cancel_all(cmd, self.account_id);
        } else {
            log::debug!("No open orders to cancel for {instrument_id}: no matching engine");
        }

        self.drain_oto_commands();
    }

    fn apply_batch_cancel_orders(&mut self, cmd: &BatchCancelOrders) {
        let account_id = self.account_id;
        if let Some(engine) = self.matching_engines.get_mut(&cmd.instrument_id) {
            engine.process_batch_cancel(cmd, account_id);
        } else {
            let ts_now = self.clock.borrow().timestamp_ns();
            for command in &cmd.cancels {
                self.deny_cancel(command, "No matching engine for instrument".into(), ts_now);
            }
        }

        self.drain_oto_commands();
    }

    /// Enqueues a trading command to be applied after its inbound latency elapses, as backtest
    /// `generate_inflight_command` does, but keyed off arrival rather than `command.ts_init()`.
    fn enqueue(&mut self, command: TradingCommand, now_ns: UnixNanos, due_ns: UnixNanos) {
        let seq = self.inbound_seq;
        self.inbound_seq += 1;

        self.inflight_orders.insert(&command);
        self.inbound_queue.push(DelayedCommand {
            due_ns,
            seq,
            command,
        });

        self.arm_inbound_alert(now_ns);
    }

    /// Defers `command` by its inbound latency leg, or applies it inline when that leg is zero.
    ///
    /// A zero-leg command is still queued behind a head already due, so the two apply in
    /// `(due_ns, seq)` order.
    fn defer_or_apply(&mut self, command: TradingCommand) {
        let now_ns = self.clock.borrow().timestamp_ns();

        let leg_latency = match self.command_leg_latency(&command, now_ns) {
            Ok(latency) => latency,
            Err(e) => {
                let reason = format!("Latency model failed: {e:#}");
                log::error!("{reason}");
                self.refuse_command(&command, &reason);
                return;
            }
        };

        let Some(due_ns) = now_ns.checked_add(leg_latency) else {
            let reason = "Latency arrival timestamp exceeds u64 nanoseconds";
            log::error!("{reason}");
            self.refuse_command(&command, reason);
            return;
        };

        let head_is_due = self
            .inbound_queue
            .peek()
            .is_some_and(|delayed| delayed.due_ns <= now_ns);
        if head_is_due || leg_latency > DurationNanos::ZERO {
            self.enqueue(command, now_ns, due_ns);
            return;
        }

        if let Err(e) = self.apply_trading_command(&command) {
            log::error!("Error applying command: {e}");
            self.refuse_command(&command, "Command could not be applied at the venue");
        }
    }

    /// Returns the inbound latency leg for `command`, or zero when no model is set.
    fn command_leg_latency(
        &self,
        command: &TradingCommand,
        ts_now: UnixNanos,
    ) -> anyhow::Result<DurationNanos> {
        let Some(latency_model) = self.config.latency_model.as_ref() else {
            return Ok(DurationNanos::ZERO);
        };

        match command {
            TradingCommand::SubmitOrder(_) | TradingCommand::SubmitOrderList(_) => {
                latency_model.get_insert_latency(command, ts_now)
            }
            TradingCommand::ModifyOrder(_) | TradingCommand::ModifyOrders(_) => {
                latency_model.get_update_latency(command, ts_now)
            }
            TradingCommand::CancelOrder(_)
            | TradingCommand::CancelOrders(_)
            | TradingCommand::CancelAllOrders(_) => {
                latency_model.get_delete_latency(command, ts_now)
            }
            TradingCommand::QueryOrder(_) | TradingCommand::QueryAccount(_) => {
                Ok(DurationNanos::ZERO)
            }
            TradingCommand::SubmitPositionOperation(_) => {
                latency_model.get_insert_latency(command, ts_now)
            }
            TradingCommand::QueryPositionOperation(_) => Ok(DurationNanos::ZERO),
        }
    }

    fn pop_due(&mut self, now_ns: UnixNanos) -> Option<DelayedCommand> {
        self.inbound_queue
            .peek()
            .is_some_and(|delayed| delayed.due_ns <= now_ns)
            .then(|| self.inbound_queue.pop().expect("peek returned Some"))
    }

    fn on_quote_tick(inner: &Rc<RefCell<Self>>, quote: &QuoteTick) {
        Self::drain_inbound(inner);
        inner.borrow_mut().process_quote_tick(quote);
    }

    fn on_trade_tick(inner: &Rc<RefCell<Self>>, trade: &TradeTick) {
        Self::drain_inbound(inner);
        inner.borrow_mut().process_trade_tick(trade);
    }

    fn on_bar(inner: &Rc<RefCell<Self>>, bar: &Bar) {
        Self::drain_inbound(inner);
        inner.borrow_mut().process_bar(bar);
    }

    fn on_order_book_deltas(inner: &Rc<RefCell<Self>>, deltas: &OrderBookDeltas) {
        Self::drain_inbound(inner);
        inner.borrow_mut().process_order_book_deltas(deltas);
    }

    fn on_order_book_depth(inner: &WeakCell<Self>, venue: Venue, depth: &OrderBookDepth) {
        if depth.instrument_id.venue != venue {
            return;
        }

        let Some(inner_rc) = inner.upgrade() else {
            return;
        };

        Self::drain_inbound(&inner_rc);
        inner_rc.borrow_mut().process_order_book_depth(depth);
    }

    fn on_instrument_status(inner: &Rc<RefCell<Self>>, status: &InstrumentStatus) {
        Self::drain_inbound(inner);
        inner.borrow_mut().process_instrument_status(status);
    }

    fn on_instrument_close(inner: &Rc<RefCell<Self>>, close: &InstrumentClose) {
        Self::drain_inbound(inner);
        inner.borrow_mut().process_instrument_close(close);
    }

    fn on_position_event(inner: &Rc<RefCell<Self>>, event: &PositionEvent) {
        let instrument_id = event.instrument_id();

        let Ok(mut inner) = inner.try_borrow_mut() else {
            log::debug!(
                "Skipping immediate expiration retry for {instrument_id} due to active sandbox borrow",
            );
            return;
        };

        if instrument_id.venue != inner.config.venue || event.account_id() != inner.account_id {
            return;
        }

        // Position events observe the cache after the fill has been applied
        let ts_now = inner.clock.borrow().timestamp_ns();
        if let Some(engine) = inner.matching_engines.get_mut(&instrument_id) {
            engine.process_instrument_expiration(ts_now);
        }

        inner.sync_expired_cleanup(instrument_id);
        inner.drain_oto_commands();
    }

    /// Applies every inbound command whose latency has elapsed, in `(due_ns, seq)` order, then
    /// arms the alert for the earliest command still queued.
    ///
    /// Called at the top of each data handler and public `process_*` method, so a command due
    /// before a tick is processed is applied before that tick, and by the alert when no data is
    /// flowing.
    fn drain_inbound(inner: &Rc<RefCell<Self>>) {
        // The alert fires on the runner task, where a nested msgbus dispatch may already hold the
        // borrow; the next data tick or public `process_*` call releases the queue instead.
        let Ok(mut this) = inner.try_borrow_mut() else {
            log::debug!("Skipping sandbox inbound drain due to active borrow");
            return;
        };

        if this.config.latency_model.is_none() {
            return;
        }

        let now_ns = this.clock.borrow().timestamp_ns();

        while let Some(delayed) = this.pop_due(now_ns) {
            if let Err(e) = this.apply_trading_command(&delayed.command) {
                log::error!("Error applying deferred command: {e}");
                this.refuse_command(
                    &delayed.command,
                    "Command could not be applied at the venue",
                );
            }
        }

        this.arm_inbound_alert(now_ns);
    }

    /// Takes every command still deferred by inbound latency, ordered for [`Self::refuse_discarded`]
    /// to unwind last-issued-first.
    fn take_inbound_queue(&mut self) -> Vec<DelayedCommand> {
        self.inflight_orders.clear();

        if self.inbound_queue.is_empty() {
            return Vec::new();
        }

        log::warn!(
            "Discarding {} command(s) still in flight at stop",
            self.inbound_queue.len(),
        );

        // Publish command outcomes before rejecting the submit they were queued behind
        let mut discarded = std::mem::take(&mut self.inbound_queue).into_vec();
        discarded.sort_unstable_by_key(|delayed| std::cmp::Reverse(delayed.seq));
        discarded
    }

    fn refuse_discarded(&self, discarded: Vec<DelayedCommand>) {
        let mut pending_denied = AHashSet::new();

        for delayed in discarded {
            self.refuse_command_deduped(
                &delayed.command,
                "Client stopped before the command was sent",
                &mut pending_denied,
            );
        }
    }

    /// Clears every command still deferred by inbound latency, without rejecting any of them:
    /// `reset` discards all client state, so there is nothing to release.
    fn clear_inbound_queue(&mut self) {
        self.inbound_queue.clear();
        self.inflight_orders.clear();
    }

    /// (Re)arms the `LiveClock` alert for the earliest queued `due_ns` while that is still ahead
    /// of `now_ns`.
    ///
    /// A due time already reached is not armed for: the drain releasing it is already pending on
    /// the runner, and an alert at a time the clock has passed only asks the clock to warn. The
    /// clock reads its own time again when arming, so a leg shorter than that gap can still warn.
    fn arm_inbound_alert(&self, now_ns: UnixNanos) {
        let Some(earliest_due) = self.inbound_queue.peek().map(|delayed| delayed.due_ns) else {
            return;
        };

        if earliest_due <= now_ns {
            return;
        }

        let name = inbound_alert_name(self.client_id);
        let armed_ns = self.clock.borrow().next_time_ns(&name);

        match armed_ns {
            // Already armed no later than the new earliest due, so that alert still wakes the drain
            Some(armed_ns) if armed_ns <= earliest_due => return,
            // Canceling first avoids the warning `replace_existing_timer` would log
            Some(_) => self.clock.borrow_mut().cancel_timer(&name),
            None => {}
        }

        let inner_weak = self.self_weak.clone();
        let alert_name = name.clone();

        let callback: Rc<dyn Fn(TimeEvent)> = Rc::new(move |_event: TimeEvent| {
            let Some(inner_rc) = inner_weak.upgrade() else {
                return;
            };

            // Retire the spent one-shot before the drain's exit path reads it back as still armed
            if let Ok(this) = inner_rc.try_borrow() {
                this.clock.borrow_mut().cancel_timer(&alert_name);
            }

            // The pass arms for whatever it leaves queued
            Self::drain_inbound(&inner_rc);
        });

        if let Err(e) = self.clock.borrow_mut().set_time_alert_ns(
            &name,
            earliest_due,
            Some(TimeEventCallback::from(callback)),
            Some(true),
        ) {
            log::error!("Failed to arm sandbox inbound alert '{name}': {e}");
        }
    }
}

fn inbound_alert_name(client_id: ClientId) -> String {
    format!("{client_id}-sandbox-inbound-alert")
}

fn check_quote_or_drop(context: &str, quote: &QuoteTick, instrument: &InstrumentAny) -> bool {
    if quote_matches_instrument_precision(quote, instrument) {
        return true;
    }

    log::warn!(
        "Dropping {context} for {} due to precision mismatch \
         (bid_px={}, ask_px={}, bid_sz={}, ask_sz={}, expected_price={}, expected_size={})",
        instrument.id(),
        quote.bid_price.precision,
        quote.ask_price.precision,
        quote.bid_size.precision,
        quote.ask_size.precision,
        instrument.price_precision(),
        instrument.size_precision(),
    );
    false
}

fn check_trade_or_drop(context: &str, trade: &TradeTick, instrument: &InstrumentAny) -> bool {
    if trade_matches_instrument_precision(trade, instrument) {
        return true;
    }

    log::warn!(
        "Dropping {context} for {} due to precision mismatch \
         (px={}, sz={}, expected_price={}, expected_size={})",
        instrument.id(),
        trade.price.precision,
        trade.size.precision,
        instrument.price_precision(),
        instrument.size_precision(),
    );
    false
}

fn check_bar_or_drop(context: &str, bar: &Bar, instrument: &InstrumentAny) -> bool {
    if bar_matches_instrument_precision(bar, instrument) {
        return true;
    }

    log::warn!(
        "Dropping {context} for {} due to precision mismatch \
         (open={}, high={}, low={}, close={}, volume={}, expected_price={}, expected_size={})",
        instrument.id(),
        bar.open.precision,
        bar.high.precision,
        bar.low.precision,
        bar.close.precision,
        bar.volume.precision,
        instrument.price_precision(),
        instrument.size_precision(),
    );
    false
}

fn quote_matches_instrument_precision(quote: &QuoteTick, instrument: &InstrumentAny) -> bool {
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    quote.bid_price.precision == price_precision
        && quote.ask_price.precision == price_precision
        && quote.bid_size.precision == size_precision
        && quote.ask_size.precision == size_precision
}

fn trade_matches_instrument_precision(trade: &TradeTick, instrument: &InstrumentAny) -> bool {
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    trade.price.precision == price_precision && trade.size.precision == size_precision
}

fn bar_matches_instrument_precision(bar: &Bar, instrument: &InstrumentAny) -> bool {
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    bar.open.precision == price_precision
        && bar.high.precision == price_precision
        && bar.low.precision == price_precision
        && bar.close.precision == price_precision
        && bar.volume.precision == size_precision
}
