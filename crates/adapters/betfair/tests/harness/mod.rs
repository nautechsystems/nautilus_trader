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

//! Reusable engine-wired seam harness for Betfair live execution tests.
//!
//! Wires a real `RiskEngine` + `ExecutionEngine` + `Cache` to a real
//! `BetfairExecutionClient` pointed at the in-process mock venue, and routes the
//! client's emitted events back through the real `AsyncRunner` routing fork.

use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

use nautilus_betfair::{
    common::consts::{BETFAIR_CLIENT_ID, BETFAIR_VENUE},
    config::BetfairExecConfig,
    execution::BetfairExecutionClient,
};
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    clock::{Clock, TestClock},
    live::runner::{replace_data_event_sender, replace_exec_event_sender},
    messages::{
        ExecutionEvent,
        execution::{TradingCommand, modify::ModifyOrder, submit::SubmitOrder},
    },
    msgbus::{self, MessageBus, MessagingSwitchboard},
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_execution::engine::{ExecutionEngine, config::ExecutionEngineConfig};
use nautilus_live::{ExecutionClientCore, runner::AsyncRunner};
use nautilus_model::{
    data::QuoteTick,
    enums::{AccountType, OmsType, OrderSide, OrderType, TimeInForce},
    events::{OrderEventAny, OrderPendingCancel},
    identifiers::{AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, TraderId},
    instruments::{Instrument, InstrumentAny, stubs::betting},
    orders::{Order, OrderAny, builder::OrderTestBuilder},
    reports::ExecutionMassStatus,
    types::{Currency, Price, Quantity},
};
use nautilus_portfolio::Portfolio;
use nautilus_risk::engine::{RiskEngine, config::RiskEngineConfig};
use nautilus_testkit::testers::{ExecTester, ExecTesterConfig};
use nautilus_trading::strategy::Strategy;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::mpsc::{UnboundedReceiver, UnboundedSender},
};

use crate::common::{
    MockState, accept_and_auth, create_test_http_client, load_fixture, plain_stream_config,
    start_mock_http, start_mock_stream, test_credential,
};

// Some fields are held to keep engines and the mock alive or for future scenarios.
#[allow(
    dead_code,
    reason = "fields held for engine liveness and future scenarios"
)]
pub(crate) struct Harness {
    pub(crate) clock: Rc<RefCell<dyn Clock>>,
    pub(crate) cache: Rc<RefCell<Cache>>,
    pub(crate) risk_engine: Rc<RefCell<RiskEngine>>,
    pub(crate) exec_engine: Rc<RefCell<ExecutionEngine>>,
    pub(crate) exec_rx: UnboundedReceiver<ExecutionEvent>,
    pub(crate) routed: Vec<RoutedKind>,
    pub(crate) mock_state: MockState,
    pub(crate) feeder: StreamFeeder,
    pub(crate) trader_id: TraderId,
    pub(crate) account_id: AccountId,
    pub(crate) instrument_id: InstrumentId,
}

impl Harness {
    pub(crate) fn client_id(&self) -> ClientId {
        *BETFAIR_CLIENT_ID
    }

    // Builds the fully wired stack with a connected client registered into the engine.
    pub(crate) async fn build() -> Self {
        let trader_id = TraderId::from("TESTER-001");
        let account_id = AccountId::from("BETFAIR-001");

        // Install a bus with our trader id before anything lazily creates the default.
        let _bus = MessageBus::new(trader_id, UUID4::new(), None, None).register_message_bus();

        let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));

        let instrument = InstrumentAny::Betting(betting());
        let instrument_id = instrument.id();
        cache.borrow_mut().add_instrument(instrument).unwrap();

        let portfolio = Portfolio::new(cache.clone(), clock.clone(), None);
        let risk_engine = Rc::new(RefCell::new(RiskEngine::new(
            RiskEngineConfig::default(),
            portfolio,
            clock.clone(),
            cache.clone(),
        )));
        RiskEngine::register_msgbus_handlers(&risk_engine);

        let exec_cfg = ExecutionEngineConfig::builder()
            .manage_own_order_books(true)
            .build()
            .expect("execution engine config should be valid");
        let exec_engine = Rc::new(RefCell::new(ExecutionEngine::new(
            clock.clone(),
            cache.clone(),
            Some(exec_cfg),
        )));
        ExecutionEngine::register_msgbus_handlers(&exec_engine);

        let (addr, mock_state) = start_mock_http().await;
        let (stream_port, listener) = start_mock_stream().await;

        let (tx, exec_rx) = tokio::sync::mpsc::unbounded_channel();
        replace_exec_event_sender(tx);
        let (data_tx, _data_rx) = tokio::sync::mpsc::unbounded_channel();
        replace_data_event_sender(data_tx);

        let core = ExecutionClientCore::new(
            trader_id,
            *BETFAIR_CLIENT_ID,
            *BETFAIR_VENUE,
            OmsType::Netting,
            account_id,
            AccountType::Betting,
            None,
            cache.clone(),
        );
        let mut client = BetfairExecutionClient::new(
            core,
            create_test_http_client(addr),
            test_credential(),
            plain_stream_config(stream_port),
            BetfairExecConfig::default(),
            Currency::GBP(),
        );
        client.start().unwrap();

        let feeder = StreamFeeder::spawn(listener);
        client.connect().await.unwrap();
        exec_engine
            .borrow_mut()
            .register_client(Box::new(client))
            .unwrap();

        Self {
            clock,
            cache,
            risk_engine,
            exec_engine,
            exec_rx,
            routed: Vec::new(),
            mock_state,
            feeder,
            trader_id,
            account_id,
            instrument_id,
        }
    }

    // Sends a SubmitOrder through the risk engine after caching the order, mirroring
    // how a strategy submits. ExecTester replaces this driver in a later task.
    pub(crate) fn submit_via_risk(&self, order: &OrderAny) {
        let cmd = SubmitOrder::from_order(
            order,
            self.trader_id,
            Some(self.client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
        );
        self.cache
            .borrow_mut()
            .add_order(order.clone(), None, Some(self.client_id()), false)
            .unwrap();
        msgbus::send_trading_command(
            MessagingSwitchboard::risk_engine_execute(),
            TradingCommand::SubmitOrder(cmd),
        );
    }

    // Sends a ModifyOrder through the risk engine, mirroring `submit_via_risk`. Reads the
    // venue_order_id from the cached (accepted) order so the adapter can target the bet:
    // a price change drives replaceOrders, a quantity reduction drives a partial cancel.
    pub(crate) fn modify_via_risk(
        &self,
        order: &OrderAny,
        price: Option<Price>,
        quantity: Option<Quantity>,
    ) {
        let venue_order_id = self
            .cache
            .borrow()
            .order(&order.client_order_id())
            .and_then(|cached| cached.venue_order_id());
        let cmd = ModifyOrder::new(
            self.trader_id,
            Some(self.client_id()),
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

    // Drains the client's emitted events with a per-recv timeout, taps each into
    // `routed`, and routes it through the real fork until `predicate` holds or the
    // deadline elapses. Returns whether the predicate held.
    pub(crate) async fn pump_until(
        &mut self,
        deadline: Duration,
        predicate: impl Fn(&Cache) -> bool,
    ) -> bool {
        let start = Instant::now();

        loop {
            if predicate(&self.cache.borrow()) {
                return true;
            }

            if start.elapsed() >= deadline {
                return false;
            }

            match tokio::time::timeout(Duration::from_millis(50), self.exec_rx.recv()).await {
                Ok(Some(evt)) => {
                    self.routed.push(RoutedKind::of(&evt));
                    AsyncRunner::handle_exec_event(evt);
                }
                Ok(None) => return predicate(&self.cache.borrow()),
                Err(_) => tokio::task::yield_now().await,
            }
        }
    }

    // Drains and routes events until one of `kind` has been routed, or the deadline
    // elapses. Used for the report path, where nothing changes in the cache.
    pub(crate) async fn pump_until_routed(&mut self, deadline: Duration, kind: RoutedKind) -> bool {
        let start = Instant::now();

        loop {
            if self.routed.contains(&kind) {
                return true;
            }

            if start.elapsed() >= deadline {
                return false;
            }

            match tokio::time::timeout(Duration::from_millis(50), self.exec_rx.recv()).await {
                Ok(Some(evt)) => {
                    self.routed.push(RoutedKind::of(&evt));
                    AsyncRunner::handle_exec_event(evt);
                }
                Ok(None) => return self.routed.contains(&kind),
                Err(_) => tokio::task::yield_now().await,
            }
        }
    }

    // Registers an ExecTester (config-only) against the harness clock/cache so it drives
    // submit -> risk -> exec through real strategy code. Reuse is via the `Strategy` trait's
    // `core_mut`; ExecTester itself is unchanged.
    pub(crate) fn register_exec_tester(&self, order_qty: &str) -> ExecTester {
        let mut config = ExecTesterConfig::new(
            StrategyId::from("S-001"),
            self.instrument_id,
            self.client_id(),
            Quantity::from(order_qty),
        );
        config.subscribe_quotes = false;
        config.subscribe_trades = false;
        config.enable_limit_sells = false;
        config.tob_offset_ticks = 1;
        config.cancel_orders_on_stop = false;
        config.close_positions_on_stop = false;

        let mut tester = ExecTester::new(config);
        let portfolio = Rc::new(RefCell::new(Portfolio::new(
            self.cache.clone(),
            self.clock.clone(),
            None,
        )));
        Strategy::core_mut(&mut tester)
            .register(
                self.trader_id,
                self.clock.clone(),
                self.cache.clone(),
                portfolio,
            )
            .unwrap();
        tester
    }

    // Transitions a tracked order to PendingCancel without a live venue confirmation,
    // mirroring `Strategy::mark_order_pending_cancel`. Scenarios stage the missed-cancel
    // recovery path with this, then let reconciliation resolve the order from the venue.
    pub(crate) fn mark_pending_cancel(&self, order: &OrderAny) {
        let cached = self
            .cache
            .borrow()
            .order(&order.client_order_id())
            .map(|cached| cached.clone())
            .expect("order must be cached before pending cancel");
        let ts_now = self.clock.borrow().timestamp_ns();
        let event = OrderEventAny::PendingCancel(OrderPendingCancel::new(
            cached.trader_id(),
            cached.strategy_id(),
            cached.instrument_id(),
            cached.client_order_id(),
            cached
                .account_id()
                .expect("accepted order must have an account_id"),
            UUID4::new(),
            ts_now,
            ts_now,
            false,
            cached.venue_order_id(),
        ));
        self.cache.borrow_mut().update_order(&event).unwrap();
    }

    // Overrides the mock venue's JSON-RPC `result` for a betting method using a fixture's
    // `result` object, so scenarios can return venue errors or reconciliation snapshots.
    pub(crate) fn override_betting_result(&self, method: &str, fixture_rel_path: &str) {
        let fixture = load_fixture(fixture_rel_path);
        let value: serde_json::Value = serde_json::from_str(&fixture).unwrap();
        self.mock_state
            .betting_overrides
            .lock()
            .unwrap()
            .insert(method.to_string(), value["result"].clone());
    }

    // Polls the venue via the registered client (HTTP listCurrentOrders) and reconciles the
    // resulting reports against the cache, mirroring the live engine's two-step reconcile.
    // Returns the mass status so scenarios can assert the generated reports end-to-end.
    #[allow(
        clippy::await_holding_refcell_ref,
        reason = "single-threaded harness; only the mock HTTP task runs during this await and it never borrows the engine"
    )]
    pub(crate) async fn reconcile_from_venue(&self) -> ExecutionMassStatus {
        let client_id = self.client_id();
        let mass_status = self
            .exec_engine
            .borrow_mut()
            .generate_mass_status(&client_id, None)
            .await
            .expect("generate_mass_status failed")
            .expect("mass status was None");
        self.exec_engine
            .borrow_mut()
            .reconcile_execution_mass_status(&mass_status);
        mass_status
    }
}

// Builds a passive limit order on the betting instrument for the harness strategy.
pub(crate) fn limit_order(instrument_id: &InstrumentId, client_order_id: &str) -> OrderAny {
    OrderTestBuilder::new(OrderType::Limit)
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(*instrument_id)
        .client_order_id(ClientOrderId::from(client_order_id))
        .side(OrderSide::Buy)
        .price(Price::from("3.0"))
        .quantity(Quantity::from("10.0"))
        .time_in_force(TimeInForce::Gtc)
        .build()
}

// Builds a quote tick for the betting instrument to drive ExecTester's order maintenance.
pub(crate) fn quote(instrument_id: &InstrumentId, bid: &str, ask: &str) -> QuoteTick {
    QuoteTick::new(
        *instrument_id,
        Price::from(bid),
        Price::from(ask),
        Quantity::from("100"),
        Quantity::from("100"),
        UnixNanos::default(),
        UnixNanos::default(),
    )
}

// Classifies how a routed event was dispatched by the fork: the event path
// (`Order`) vs the reconciliation path (`Report`). `ExecutionEvent` is not
// `Clone`, so scenarios assert on these tags rather than the events themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RoutedKind {
    Order,
    Report,
    Account,
}

impl RoutedKind {
    fn of(evt: &ExecutionEvent) -> Self {
        match evt {
            ExecutionEvent::Report(_) => Self::Report,
            ExecutionEvent::Account(_) => Self::Account,
            _ => Self::Order,
        }
    }
}

// Reusable cross-layer invariant checks. The routing contract is the load-bearing
// one: a tracked happy-path run must reach the engine via order events, never reports.
pub(crate) mod invariants {
    use nautilus_common::cache::Cache;
    use nautilus_model::{
        enums::OrderStatus,
        identifiers::{ClientOrderId, InstrumentId},
        orders::Order,
    };
    use rust_decimal::Decimal;

    use super::RoutedKind;

    pub(crate) fn assert_tracked_used_events(routed: &[RoutedKind]) {
        let reports = routed
            .iter()
            .filter(|kind| **kind == RoutedKind::Report)
            .count();
        assert_eq!(
            reports, 0,
            "tracked happy path routed {reports} report(s), expected 0 (routing-contract violation): {routed:?}",
        );
    }

    pub(crate) fn assert_order_status(cache: &Cache, id: &ClientOrderId, expected: OrderStatus) {
        let status = cache.order(id).map(|order| order.status());
        assert_eq!(
            status,
            Some(expected),
            "order {id} status was {status:?}, expected {expected:?}",
        );
    }

    // Asserts no closed (or missing) order lingers in the own order book: the
    // balloon regression the Betfair report-path bug produced.
    pub(crate) fn assert_own_book_consistent(cache: &Cache, instrument_id: &InstrumentId) {
        let Some(book) = cache.own_order_book(instrument_id) else {
            return;
        };
        let mut order_ids = book.bid_client_order_ids();
        order_ids.extend(book.ask_client_order_ids());
        for id in order_ids {
            let open = cache.order(&id).is_some_and(|order| !order.is_closed());
            assert!(
                open,
                "own order book retains closed or missing order {id} (balloon regression)",
            );
        }
    }

    pub(crate) fn assert_filled_qty(cache: &Cache, id: &ClientOrderId, expected: Decimal) {
        let filled = cache.order(id).map(|order| order.filled_qty().as_decimal());
        assert_eq!(
            filled,
            Some(expected),
            "order {id} filled_qty was {filled:?}, expected {expected}",
        );
    }

    pub(crate) fn assert_in_own_book(
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

// Writes OCM stream frames to the live mock socket on demand, so scenarios can
// deliver venue responses at controlled points after submits.
pub(crate) struct StreamFeeder {
    tx: UnboundedSender<String>,
}

impl StreamFeeder {
    pub(crate) fn spawn(listener: TcpListener) -> Self {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();

        tokio::spawn(async move {
            let (mut reader, mut write_half) = accept_and_auth(&listener).await;
            // Drain the order-subscription line the client sends after auth.
            let mut line = String::new();
            reader.read_line(&mut line).await.ok();

            while let Some(frame) = rx.recv().await {
                write_half
                    .write_all(format!("{}\r\n", frame.trim()).as_bytes())
                    .await
                    .unwrap();
            }
        });
        Self { tx }
    }

    pub(crate) fn feed(&self, fixture_rel_path: &str) {
        self.tx.send(load_fixture(fixture_rel_path)).unwrap();
    }
}
