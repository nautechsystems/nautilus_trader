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

//! Engine-wired support for live adapter integration tests.
//!
//! This module records whether adapter output follows the typed order-event path or the
//! reconciliation-report path before routing each event through the live runner.

#![warn(rustc::all)]
#![warn(clippy::pedantic)]
#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(nonstandard_style)]
#![deny(missing_debug_implementations)]
#![deny(rustdoc::broken_intra_doc_links)]
#![allow(
    clippy::missing_panics_doc,
    reason = "test support reports invalid setup and failed assertions by panicking"
)]

use std::{cell::RefCell, fmt::Debug, rc::Rc, time::Duration};

use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    clock::{Clock, TestClock},
    live::{
        dst,
        runner::{replace_data_event_sender, replace_exec_event_sender},
    },
    messages::{
        ExecutionEvent,
        execution::{
            TradingCommand, cancel::CancelOrder, modify::ModifyOrder, submit::SubmitOrder,
        },
    },
    msgbus::{self, MessageBus, MessagingSwitchboard},
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_execution::engine::{ExecutionEngine, config::ExecutionEngineConfig};
use nautilus_model::{
    events::{OrderEventAny, OrderPendingCancel, OrderPendingUpdate},
    identifiers::{AccountId, ClientId, InstrumentId, StrategyId, TraderId},
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    reports::ExecutionMassStatus,
    types::{Price, Quantity},
};
use nautilus_portfolio::Portfolio;
use nautilus_risk::engine::{RiskEngine, config::RiskEngineConfig};
use nautilus_testkit::testers::{ExecTester, ExecTesterConfig};
use nautilus_trading::strategy::StrategyNative;

use crate::runner::AsyncRunner;

/// Engine-wired state for deterministic live execution seam tests.
pub struct ExecutionHarness {
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
    risk_engine: Rc<RefCell<RiskEngine>>,
    exec_engine: Rc<RefCell<ExecutionEngine>>,
    exec_rx: tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    routed: Vec<RoutedKind>,
    trader_id: TraderId,
    client_id: ClientId,
    account_id: AccountId,
    instrument_id: InstrumentId,
}

impl Debug for ExecutionHarness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(ExecutionHarness))
            .field("trader_id", &self.trader_id)
            .field("client_id", &self.client_id)
            .field("account_id", &self.account_id)
            .field("instrument_id", &self.instrument_id)
            .field("routed", &self.routed)
            .finish_non_exhaustive()
    }
}

impl ExecutionHarness {
    /// Creates a harness with real risk and execution engines and the supplied instrument.
    ///
    /// Replaces the current thread's message bus and live event senders.
    #[must_use]
    pub fn new(
        trader_id: TraderId,
        client_id: ClientId,
        account_id: AccountId,
        instrument: InstrumentAny,
    ) -> Self {
        let _bus = MessageBus::new(trader_id, UUID4::new(), None, None).register_message_bus();
        let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));
        let instrument_id = instrument.id();
        cache
            .borrow_mut()
            .add_instrument(instrument)
            .expect("instrument should be added to the harness cache");

        let portfolio = Portfolio::new(clock.clone(), cache.clone(), None);
        let risk_engine = Rc::new(RefCell::new(RiskEngine::new(
            RiskEngineConfig::default(),
            portfolio,
            clock.clone(),
            cache.clone(),
        )));
        RiskEngine::register_msgbus_handlers(&risk_engine);

        let exec_config = ExecutionEngineConfig::builder()
            .manage_own_order_books(true)
            .build()
            .expect("execution engine config should be valid");
        let exec_engine = Rc::new(RefCell::new(ExecutionEngine::new(
            clock.clone(),
            cache.clone(),
            Some(exec_config),
        )));
        ExecutionEngine::register_msgbus_handlers(&exec_engine);

        let (exec_tx, exec_rx) = tokio::sync::mpsc::unbounded_channel();
        replace_exec_event_sender(exec_tx);
        let (data_tx, _data_rx) = tokio::sync::mpsc::unbounded_channel();
        replace_data_event_sender(data_tx);

        Self {
            clock,
            cache,
            risk_engine,
            exec_engine,
            exec_rx,
            routed: Vec::new(),
            trader_id,
            client_id,
            account_id,
            instrument_id,
        }
    }

    /// Returns the clock shared by the harness components.
    #[must_use]
    pub const fn clock(&self) -> &Rc<RefCell<dyn Clock>> {
        &self.clock
    }

    /// Returns the cache shared by the harness components.
    #[must_use]
    pub const fn cache(&self) -> &Rc<RefCell<Cache>> {
        &self.cache
    }

    /// Returns the risk engine used to route trading commands.
    #[must_use]
    pub const fn risk_engine(&self) -> &Rc<RefCell<RiskEngine>> {
        &self.risk_engine
    }

    /// Returns the execution engine containing the adapter client under test.
    #[must_use]
    pub const fn exec_engine(&self) -> &Rc<RefCell<ExecutionEngine>> {
        &self.exec_engine
    }

    /// Returns the trader ID used by the harness.
    #[must_use]
    pub const fn trader_id(&self) -> TraderId {
        self.trader_id
    }

    /// Returns the execution client ID used by the harness.
    #[must_use]
    pub const fn client_id(&self) -> ClientId {
        self.client_id
    }

    /// Returns the account ID used by the harness.
    #[must_use]
    pub const fn account_id(&self) -> AccountId {
        self.account_id
    }

    /// Returns the instrument ID registered in the cache.
    #[must_use]
    pub const fn instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    /// Returns the routing kinds observed before live-runner dispatch.
    #[must_use]
    pub fn routed(&self) -> &[RoutedKind] {
        &self.routed
    }

    /// Asserts that the execution client is registered and its engine is ready.
    pub fn assert_engine_ready(&self) {
        let engine = self.exec_engine.borrow();
        assert!(engine.get_client(&self.client_id).is_some());
        assert!(engine.check_integrity());
        assert!(engine.check_connected());
    }

    /// Returns the total commands received by the risk engine.
    #[must_use]
    pub fn risk_command_count(&self) -> u64 {
        self.risk_engine.borrow().command_count()
    }

    /// Registers an adapter execution client with the real execution engine.
    ///
    /// # Errors
    ///
    /// Returns an error when the engine already contains the client ID or venue route.
    pub fn register_client(&self, client: Box<dyn ExecutionClient>) -> anyhow::Result<()> {
        self.exec_engine.borrow_mut().register_client(client)
    }

    /// Caches an order and sends its submission command through the risk engine.
    pub fn submit_via_risk(&self, order: &OrderAny) {
        let cmd = SubmitOrder::from_order(
            order,
            self.trader_id,
            Some(self.client_id),
            None,
            UUID4::new(),
            UnixNanos::default(),
        );
        self.cache
            .borrow_mut()
            .add_order(order.clone(), None, Some(self.client_id), false)
            .expect("order should be added to the harness cache");
        msgbus::send_trading_command(
            MessagingSwitchboard::risk_engine_execute(),
            TradingCommand::SubmitOrder(cmd),
        );
    }

    /// Marks an order pending update and sends its modification through the risk engine.
    pub fn modify_via_risk(
        &self,
        order: &OrderAny,
        price: Option<Price>,
        quantity: Option<Quantity>,
    ) {
        self.mark_pending_update(order);
        let venue_order_id = self
            .cache
            .borrow()
            .order(&order.client_order_id())
            .and_then(|cached| cached.venue_order_id());
        let cmd = ModifyOrder::new(
            self.trader_id,
            Some(self.client_id),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            venue_order_id,
            quantity,
            price,
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        );
        msgbus::send_trading_command(
            MessagingSwitchboard::risk_engine_execute(),
            TradingCommand::ModifyOrder(cmd),
        );
    }

    /// Marks an order pending cancel and sends its cancellation through the execution engine.
    pub fn cancel_via_execution(&self, order: &OrderAny) {
        self.mark_pending_cancel(order);
        let venue_order_id = self
            .cache
            .borrow()
            .order(&order.client_order_id())
            .and_then(|cached| cached.venue_order_id());
        let cmd = CancelOrder::new(
            self.trader_id,
            Some(self.client_id),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            venue_order_id,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        );
        msgbus::send_trading_command(
            MessagingSwitchboard::exec_engine_queue_execute(),
            TradingCommand::CancelOrder(cmd),
        );
    }

    /// Routes emitted execution events until the cache predicate holds or the deadline expires.
    pub async fn pump_until(
        &mut self,
        timeout: Duration,
        predicate: impl Fn(&Cache) -> bool,
    ) -> bool {
        self.pump_until_condition(timeout, |harness| predicate(&harness.cache.borrow()))
            .await
    }

    /// Routes emitted execution events until the expected routing kind is observed.
    pub async fn pump_until_routed(&mut self, timeout: Duration, kind: RoutedKind) -> bool {
        self.pump_until_condition(timeout, |harness| harness.routed.contains(&kind))
            .await
    }

    /// Routes every execution event received during the supplied duration.
    pub async fn pump_for(&mut self, duration: Duration) {
        let deadline = dst::time::Instant::now() + duration;

        while dst::time::Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(dst::time::Instant::now());
            match dst::time::timeout(remaining.min(EVENT_POLL_INTERVAL), self.exec_rx.recv()).await
            {
                Ok(Some(event)) => self.route_event(event),
                Ok(None) => return,
                Err(_) => dst::task::yield_now().await,
            }
        }
    }

    /// Registers an `ExecTester` against the harness clock and cache.
    #[must_use]
    pub fn register_exec_tester(&self, strategy_id: StrategyId, order_qty: Quantity) -> ExecTester {
        let mut config =
            ExecTesterConfig::new(strategy_id, self.instrument_id, self.client_id, order_qty);
        config.subscribe_quotes = false;
        config.subscribe_trades = false;
        config.enable_limit_sells = false;
        config.tob_offset_ticks = 1;
        config.cancel_orders_on_stop = false;
        config.close_positions_on_stop = false;

        let mut tester = ExecTester::new(config);
        let portfolio = Rc::new(RefCell::new(Portfolio::new(
            self.clock.clone(),
            self.cache.clone(),
            None,
        )));
        StrategyNative::strategy_core_mut(&mut tester)
            .register(
                self.trader_id,
                self.clock.clone(),
                self.cache.clone(),
                portfolio,
            )
            .expect("ExecTester should register against the harness");
        tester
    }

    /// Applies a pending-cancel event to the cached order.
    pub fn mark_pending_cancel(&self, order: &OrderAny) {
        let cached = self.cached_order(order);
        let ts_now = self.clock.borrow().timestamp_ns();
        let event = OrderEventAny::PendingCancel(OrderPendingCancel::new(
            cached.trader_id(),
            cached.strategy_id(),
            cached.instrument_id(),
            cached.client_order_id(),
            cached.account_id(),
            UUID4::new(),
            ts_now,
            ts_now,
            false,
            cached.venue_order_id(),
        ));
        self.apply_pending_event(&event);
    }

    /// Generates and applies execution mass status from the registered adapter client.
    #[allow(
        clippy::await_holding_refcell_ref,
        reason = "single-threaded test harness only runs mock venue tasks during the await"
    )]
    pub async fn reconcile_from_venue(&self) -> ExecutionMassStatus {
        let mass_status = self
            .exec_engine
            .borrow_mut()
            .generate_mass_status(&self.client_id, None)
            .await
            .expect("mass-status request should succeed")
            .expect("mass-status request should return a report");
        self.exec_engine
            .borrow_mut()
            .reconcile_execution_mass_status(&mass_status);
        mass_status
    }

    fn mark_pending_update(&self, order: &OrderAny) {
        let cached = self.cached_order(order);
        let ts_now = self.clock.borrow().timestamp_ns();
        let event = OrderEventAny::PendingUpdate(OrderPendingUpdate::new(
            cached.trader_id(),
            cached.strategy_id(),
            cached.instrument_id(),
            cached.client_order_id(),
            cached.account_id(),
            UUID4::new(),
            ts_now,
            ts_now,
            false,
            cached.venue_order_id(),
        ));
        self.apply_pending_event(&event);
    }

    fn cached_order(&self, order: &OrderAny) -> OrderAny {
        self.cache
            .borrow()
            .order(&order.client_order_id())
            .map(|cached| cached.clone())
            .expect("order must be cached before a pending transition")
    }

    fn apply_pending_event(&self, event: &OrderEventAny) {
        self.cache
            .borrow_mut()
            .update_order(event)
            .expect("pending event should update the cached order");
    }

    async fn pump_until_condition(
        &mut self,
        timeout: Duration,
        predicate: impl Fn(&Self) -> bool,
    ) -> bool {
        let start = dst::time::Instant::now();

        loop {
            if predicate(self) {
                return true;
            }

            if start.elapsed() >= timeout {
                return false;
            }

            match dst::time::timeout(EVENT_POLL_INTERVAL, self.exec_rx.recv()).await {
                Ok(Some(event)) => self.route_event(event),
                Ok(None) => return predicate(self),
                Err(_) => dst::task::yield_now().await,
            }
        }
    }

    fn route_event(&mut self, event: ExecutionEvent) {
        self.routed.push(RoutedKind::of(&event));
        AsyncRunner::handle_exec_event(event);
    }
}

const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Classifies which branch of the live execution routing fork handles an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutedKind {
    /// Typed order-event path.
    Order,
    /// Reconciliation-report path.
    Report,
    /// Account-state path.
    Account,
}

impl RoutedKind {
    fn of(event: &ExecutionEvent) -> Self {
        match event {
            ExecutionEvent::Report(_) => Self::Report,
            ExecutionEvent::Account(_) => Self::Account,
            _ => Self::Order,
        }
    }
}

/// Cross-layer invariants for live execution tests.
pub mod invariants {
    use nautilus_common::cache::Cache;
    use nautilus_model::{
        enums::OrderStatus,
        identifiers::{ClientOrderId, InstrumentId},
        orders::Order,
    };
    use rust_decimal::Decimal;

    use super::RoutedKind;

    /// Asserts that a tracked lifecycle used typed order events and no reports.
    pub fn assert_tracked_used_events(routed: &[RoutedKind]) {
        assert!(
            routed.contains(&RoutedKind::Order),
            "tracked lifecycle routed no typed order event: {routed:?}",
        );
        let reports = routed
            .iter()
            .filter(|kind| **kind == RoutedKind::Report)
            .count();
        assert_eq!(
            reports, 0,
            "tracked happy path routed {reports} report(s), expected 0: {routed:?}",
        );
    }

    /// Asserts the exact status of a cached order.
    pub fn assert_order_status(cache: &Cache, id: &ClientOrderId, expected: OrderStatus) {
        let status = cache.order(id).map(|order| order.status());
        assert_eq!(
            status,
            Some(expected),
            "order {id} status was {status:?}, expected {expected:?}",
        );
    }

    /// Asserts that every order retained in an own order book remains open in the cache.
    pub fn assert_own_book_consistent(cache: &Cache, instrument_id: &InstrumentId) {
        let Some(book) = cache.own_order_book(instrument_id) else {
            return;
        };
        let mut order_ids = book.bid_client_order_ids();
        order_ids.extend(book.ask_client_order_ids());

        for id in order_ids {
            let open = cache.order(&id).is_some_and(|order| !order.is_closed());
            assert!(open, "own order book retains closed or missing order {id}");
        }
    }

    /// Asserts the exact cumulative filled quantity of a cached order.
    pub fn assert_filled_qty(cache: &Cache, id: &ClientOrderId, expected: Decimal) {
        let filled = cache.order(id).map(|order| order.filled_qty().as_decimal());
        assert_eq!(
            filled,
            Some(expected),
            "order {id} filled_qty was {filled:?}, expected {expected}",
        );
    }

    /// Asserts whether an order is present in the instrument's own order book.
    pub fn assert_in_own_book(
        cache: &Cache,
        instrument_id: &InstrumentId,
        id: &ClientOrderId,
        expected: bool,
    ) {
        let present = cache
            .own_order_book(instrument_id)
            .is_some_and(|book| book.is_order_in_book(id));
        assert_eq!(
            present, expected,
            "order {id} own-book membership was {present}, expected {expected}",
        );
    }
}
