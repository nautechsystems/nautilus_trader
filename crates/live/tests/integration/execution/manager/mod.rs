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

//! Integration tests for ExecutionManager.
//!
//! These tests focus on observable behavior through the public API.
//! Internal state tests are in the in-module tests in manager.rs.

use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    rc::Rc,
    sync::Once,
};

use ahash::AHashSet;
use async_trait::async_trait;
use indexmap::{IndexMap, IndexSet};
use log::{Level, LevelFilter, Log, Metadata, Record};
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    clock::{Clock, VirtualClock},
    live::dst,
    messages::{
        ExecutionReport,
        execution::{
            BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
            GenerateOrderStatusReport, GenerateOrderStatusReports, GeneratePositionStatusReports,
            ModifyOrder, QueryAccount, QueryOrder, SubmitOrder, SubmitOrderList, TradingCommand,
        },
    },
    msgbus::{
        self, MessagingSwitchboard, TypedHandler,
        stubs::{
            TypedMessageSavingHandler, get_any_saving_handler,
            get_typed_into_message_saving_handler, get_typed_message_saving_handler,
        },
        switchboard,
    },
};
use nautilus_core::{DurationNanos, Params, UUID4, UnixNanos};
use nautilus_execution::{
    engine::ExecutionEngine,
    order_emulator::emulator::OrderEmulator,
    reconciliation::{
        RECONCILIATION_ORDER_TAG, create_inferred_reconciliation_trade_id,
        create_position_reconciliation_venue_order_id, generate_external_order_status_events,
        process_mass_status_for_reconciliation,
        process_mass_status_for_reconciliation_without_synthetic_reports,
    },
};
use nautilus_live::{
    execution::submission::{
        SubmissionRecoveryExhausted, SubmissionRecoveryPolicy, SubmissionRecoverySource,
    },
    manager::{ExecutionManager, ExecutionManagerConfig, ReconciliationResult},
};
use nautilus_model::{
    accounts::{AccountAny, MarginAccount},
    enums::{
        AccountType, AvgPxReconciliation, ContingencyType, LiquiditySide, OmsType, OrderSide,
        OrderStatus, OrderType, PositionSide, TimeInForce, TriggerType,
    },
    events::{
        OrderEmulated, OrderEventAny, OrderFilled, PositionEvent,
        account::state::AccountState,
        order::spec::{
            OrderAcceptedSpec, OrderCancelRejectedSpec, OrderFillVoidedSpec,
            OrderModifyRejectedSpec, OrderPendingCancelSpec, OrderPendingUpdateSpec,
            OrderUpdatedSpec,
        },
    },
    identifiers::{
        AccountId, ClientId, ClientOrderId, ExecAlgorithmId, InstrumentId, PositionId, StrategyId,
        TradeId, TraderId, Venue, VenueOrderId,
    },
    instruments::{
        Instrument, InstrumentAny,
        stubs::{
            binary_option, btcusd_bybit, crypto_perpetual_ethusdt, currency_pair_btcusdt,
            equity_aapl,
        },
    },
    orders::{
        Order, OrderAny, OrderTestBuilder, RECONCILIATION_PRICE_UNKNOWN_TAG,
        stubs::{OrderFilledTestBuilder, TestOrderEventStubs},
    },
    position::Position,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    stubs::TestDefault,
    types::{AccountBalance, Currency, MarginBalance, Money, Price, Quantity},
};
use nautilus_testkit::cache::TestCacheDatabaseControl;
use parking_lot::Mutex;
use rstest::rstest;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use ustr::Ustr;

mod fills;
mod history;
mod inflight;
mod maintenance;
mod mass_status;
mod open_orders;
mod position_checks;
mod position_reports;
mod reports;
mod submission;

#[cfg(all(feature = "simulation", madsim))]
async fn advance_clock(d: dst::time::Duration) {
    madsim::time::advance(d);
    madsim::task::yield_now().await;
}

#[cfg(not(all(feature = "simulation", madsim)))]
async fn advance_clock(d: dst::time::Duration) {
    tokio::time::advance(d).await;
}

struct TestContext {
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    manager: ExecutionManager,
    exec_engine: Rc<RefCell<ExecutionEngine>>,
}

impl TestContext {
    fn new() -> Self {
        Self::with_config(ExecutionManagerConfig::default())
    }

    fn with_config(config: ExecutionManagerConfig) -> Self {
        Self::with_cache(Cache::default(), config)
    }

    fn with_cache(cache: Cache, config: ExecutionManagerConfig) -> Self {
        let clock = Rc::new(RefCell::new(VirtualClock::new()));
        let cache = Rc::new(RefCell::new(cache));

        // Add test account to cache (required for position creation in ExecutionEngine)
        let account_state = AccountState::new(
            test_account_id(),
            AccountType::Margin,
            vec![AccountBalance::new(
                Money::from("1000000 USDT"),
                Money::from("0 USDT"),
                Money::from("1000000 USDT"),
            )],
            vec![],
            true,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            Some(Currency::USDT()),
        );

        let account = AccountAny::Margin(MarginAccount::new(account_state, true));
        cache.borrow_mut().add_account(account).unwrap();

        let manager =
            ExecutionManager::new(clock.clone(), cache.clone(), config).expect("valid config");
        let mut engine = ExecutionEngine::new(clock.clone(), cache.clone(), None);
        engine
            .register_client(Box::new(MockExecutionClient::new(Vec::new())))
            .expect("test execution client registers");

        // Register hedging mode for EXTERNAL strategy (used by external/reconciliation orders)
        engine.register_oms_type(StrategyId::from("EXTERNAL"), OmsType::Hedging);

        let exec_engine = Rc::new(RefCell::new(engine));

        Self {
            clock,
            cache,
            manager,
            exec_engine,
        }
    }

    fn advance_time(&self, delta_nanos: u64) {
        let current = self.clock.borrow().get_time_ns();
        self.clock
            .borrow_mut()
            .advance_time(UnixNanos::from(current.as_u64() + delta_nanos), true);
    }

    async fn advance_both(&self, d: dst::time::Duration) {
        let delta_nanos = u64::try_from(d.as_nanos()).expect("test duration fits in u64 nanos");
        self.advance_time(delta_nanos);
        advance_clock(d).await;
    }

    fn add_instrument(&self, instrument: InstrumentAny) {
        self.cache.borrow_mut().add_instrument(instrument).unwrap();
    }

    fn add_order(&self, order: OrderAny) {
        self.cache
            .borrow_mut()
            .add_order(order, None, Some(test_client_id()), false)
            .unwrap();
    }

    fn add_order_with_client_id(&self, order: OrderAny, client_id: ClientId) {
        self.cache
            .borrow_mut()
            .add_order(order, None, Some(client_id), false)
            .unwrap();
    }

    fn add_position(&self, position: &Position) {
        self.cache
            .borrow_mut()
            .add_position(position, OmsType::Hedging)
            .unwrap();
    }

    fn get_order(&self, client_order_id: &ClientOrderId) -> Option<OrderAny> {
        self.cache
            .borrow()
            .order(client_order_id)
            .map(|o| o.clone())
    }

    fn require_authoritative_fills(&self) {
        let mut client = MockExecutionClient::new(Vec::new());
        client.requires_authoritative_fills = true;
        client.oms_type = OmsType::Netting;
        let mut engine = self.exec_engine.borrow_mut();
        engine.deregister_client(test_client_id()).unwrap();
        engine.register_client(Box::new(client)).unwrap();
        engine.register_oms_type(StrategyId::external(), OmsType::Netting);
    }

    fn add_margin_account(&self, account_id: AccountId) {
        let account_state = AccountState::new(
            account_id,
            AccountType::Margin,
            vec![AccountBalance::new(
                Money::from("1000000 USDT"),
                Money::from("0 USDT"),
                Money::from("1000000 USDT"),
            )],
            vec![],
            true,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            Some(Currency::USDT()),
        );

        let account = AccountAny::Margin(MarginAccount::new(account_state, true));
        self.cache.borrow_mut().add_account(account).unwrap();
    }
}

fn test_instrument() -> InstrumentAny {
    InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt())
}

fn test_instrument_id() -> InstrumentId {
    crypto_perpetual_ethusdt().id()
}

fn test_instrument2() -> InstrumentAny {
    InstrumentAny::CryptoPerpetual(btcusd_bybit())
}

fn test_instrument_id2() -> InstrumentId {
    btcusd_bybit().id()
}

fn test_account_id() -> AccountId {
    AccountId::from("BINANCE-001")
}

fn test_venue() -> Venue {
    Venue::from("BINANCE")
}

fn test_client_id() -> ClientId {
    ClientId::from("BINANCE")
}

fn create_limit_order(
    client_order_id: &str,
    instrument_id: InstrumentId,
    side: OrderSide,
    quantity: &str,
    price: &str,
) -> OrderAny {
    OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(ClientOrderId::from(client_order_id))
        .instrument_id(instrument_id)
        .side(side)
        .quantity(Quantity::from(quantity))
        .price(Price::from(price))
        .build()
}

/// Creates an order that has been submitted (has account_id set)
fn create_submitted_order(
    client_order_id: &str,
    instrument_id: InstrumentId,
    side: OrderSide,
    quantity: &str,
    price: &str,
) -> OrderAny {
    let mut order = create_limit_order(client_order_id, instrument_id, side, quantity, price);
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    order
}

fn create_order_report(
    client_order_id: Option<ClientOrderId>,
    venue_order_id: VenueOrderId,
    instrument_id: InstrumentId,
    status: OrderStatus,
    quantity: Quantity,
    filled_qty: Quantity,
) -> OrderStatusReport {
    create_order_report_for_side(
        client_order_id,
        venue_order_id,
        instrument_id,
        OrderSide::Buy,
        status,
        quantity,
        filled_qty,
    )
}

fn create_order_report_for_side(
    client_order_id: Option<ClientOrderId>,
    venue_order_id: VenueOrderId,
    instrument_id: InstrumentId,
    order_side: OrderSide,
    status: OrderStatus,
    quantity: Quantity,
    filled_qty: Quantity,
) -> OrderStatusReport {
    OrderStatusReport::new(
        test_account_id(),
        instrument_id,
        client_order_id,
        venue_order_id,
        order_side.into(),
        OrderType::Limit,
        TimeInForce::Gtc,
        status,
        quantity,
        filled_qty,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    )
    .with_price(Price::from("3000.00"))
}

fn create_fill_report(
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    instrument_id: InstrumentId,
    trade_id: TradeId,
    quantity: &str,
) -> FillReport {
    FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        trade_id,
        OrderSide::Buy,
        Quantity::from(quantity),
        Price::from("3000.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        Some(client_order_id),
        None,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    )
}

fn create_mass_status(
    order_reports: Vec<OrderStatusReport>,
    fill_reports: Vec<FillReport>,
) -> ExecutionMassStatus {
    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    mass_status.add_order_reports(order_reports);
    mass_status.add_fill_reports(fill_reports);
    mass_status
}

struct ManagerLogCapture {
    messages: Mutex<Vec<String>>,
}

impl Log for ManagerLogCapture {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= Level::Warn
            && (matches!(
                metadata.target(),
                "nautilus_live::execution::manager" | "nautilus_live::execution::reconciliation"
            ) || metadata.target() == "nautilus_common::cache"
                && metadata.level() == Level::Error)
    }

    fn log(&self, record: &Record<'_>) {
        if self.enabled(record.metadata()) {
            self.messages.lock().push(record.args().to_string());
        }
    }

    fn flush(&self) {}
}

static MANAGER_LOG_CAPTURE: ManagerLogCapture = ManagerLogCapture {
    messages: Mutex::new(Vec::new()),
};
static MANAGER_LOG_INIT: Once = Once::new();
static MANAGER_LOG_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn install_manager_log_capture() {
    MANAGER_LOG_INIT.call_once(|| {
        log::set_logger(&MANAGER_LOG_CAPTURE).expect("test logger already installed");
        log::set_max_level(LevelFilter::Warn);
    });

    MANAGER_LOG_CAPTURE.messages.lock().clear();
}

#[expect(
    clippy::too_many_arguments,
    reason = "test fixture sets distinct lifecycle fields"
)]
fn create_bounded_fill_lifecycle(
    instrument_id: InstrumentId,
    venue_order_id: VenueOrderId,
    trade_id: TradeId,
    order_side: OrderSide,
    quantity: &str,
    price: &str,
    reduce_only: bool,
    ts_event: UnixNanos,
) -> (OrderStatusReport, FillReport) {
    let order_report = OrderStatusReport::new(
        test_account_id(),
        instrument_id,
        None,
        venue_order_id,
        order_side.into(),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Filled,
        Quantity::from(quantity),
        Quantity::from(quantity),
        ts_event,
        ts_event,
        ts_event,
        None,
    )
    .with_price(Price::from(price))
    .with_avg_px(Decimal::from_str_exact(price).unwrap())
    .with_reduce_only(reduce_only);

    let fill_report = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        trade_id,
        order_side,
        Quantity::from(quantity),
        Price::from(price),
        Money::from("0.20 USDT"),
        LiquiditySide::Taker,
        None,
        None,
        ts_event,
        ts_event,
        None,
    );

    (order_report, fill_report)
}

/// Creates an accepted order with venue_order_id set
fn create_accepted_order(
    client_order_id: &str,
    instrument_id: InstrumentId,
    side: OrderSide,
    quantity: &str,
    price: &str,
    venue_order_id: VenueOrderId,
) -> OrderAny {
    let mut order = create_submitted_order(client_order_id, instrument_id, side, quantity, price);
    apply_submitted_and_accepted(&mut order, venue_order_id);
    order
}

fn apply_submitted_and_accepted(order: &mut OrderAny, venue_order_id: VenueOrderId) {
    if order.status() == OrderStatus::Initialized {
        let submitted = TestOrderEventStubs::submitted(order, test_account_id());
        order.apply(submitted).unwrap();
    }

    let accepted = TestOrderEventStubs::accepted(order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
}

fn create_pending_update_order(
    client_order_id: &str,
    instrument_id: InstrumentId,
    side: OrderSide,
    quantity: &str,
    price: &str,
    venue_order_id: VenueOrderId,
) -> OrderAny {
    let mut order = create_accepted_order(
        client_order_id,
        instrument_id,
        side,
        quantity,
        price,
        venue_order_id,
    );
    let pending = OrderEventAny::PendingUpdate(
        OrderPendingUpdateSpec::builder()
            .trader_id(order.trader_id())
            .strategy_id(order.strategy_id())
            .instrument_id(order.instrument_id())
            .client_order_id(order.client_order_id())
            .account_id(test_account_id())
            .venue_order_id(venue_order_id)
            .build(),
    );
    order.apply(pending).unwrap();
    order
}

/// Creates a test position from a fill.
fn create_test_position(
    instrument: &InstrumentAny,
    position_id: PositionId,
    side: OrderSide,
    qty: &str,
    price: &str,
) -> Position {
    create_test_position_for_account(instrument, position_id, side, qty, price, test_account_id())
}

fn create_test_position_for_account(
    instrument: &InstrumentAny,
    position_id: PositionId,
    side: OrderSide,
    qty: &str,
    price: &str,
    account_id: AccountId,
) -> Position {
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id())
        .side(side)
        .quantity(Quantity::from(qty))
        .build();

    let fill = TestOrderEventStubs::filled(
        &order,
        instrument,
        Some(TradeId::new("T-001")),
        Some(position_id),
        Some(Price::from(price)),
        Some(Quantity::from(qty)),
        None,
        None,
        None,
        Some(account_id),
    );

    let order_filled: OrderFilled = fill.into();
    Position::new(instrument, order_filled)
}

// Opens 1 at 50,000 and 1 at 60,000, then closes 1: the netting average stays 55,000 while a
// FIFO venue reports the surviving lot at 60,000.
fn create_fifo_divergent_position(instrument: &InstrumentAny, entry_side: OrderSide) -> Position {
    let mut position = create_test_position(
        instrument,
        PositionId::from("P-FIFO"),
        entry_side,
        "1.000",
        "50000.00",
    );

    let exit_side = match entry_side {
        OrderSide::Buy => OrderSide::Sell,
        OrderSide::Sell => OrderSide::Buy,
    };

    for (trade_id, side, price) in [
        ("T-002", entry_side, "60000.00"),
        ("T-003", exit_side, "65000.00"),
    ] {
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(side)
            .quantity(Quantity::from("1.000"))
            .build();
        let fill = TestOrderEventStubs::filled(
            &order,
            instrument,
            Some(TradeId::new(trade_id)),
            Some(position.id),
            Some(Price::from(price)),
            Some(Quantity::from("1.000")),
            None,
            None,
            None,
            Some(position.account_id),
        );
        let filled: OrderFilled = fill.into();
        position.apply(&filled);
    }

    position
}

// The FIFO scenarios use Kraken-scale prices above the ETHUSDT stub's maximum price
fn fifo_test_instrument() -> InstrumentAny {
    let mut instrument = crypto_perpetual_ethusdt();
    instrument.max_price = Some(Price::from("100000.00"));
    InstrumentAny::CryptoPerpetual(instrument)
}

fn filled_events(events: &[OrderEventAny]) -> Vec<(OrderSide, Quantity, Price)> {
    events
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => Some((fill.order_side, fill.last_qty, fill.last_px)),
            _ => None,
        })
        .collect()
}

struct MockExecutionClient {
    order_venue_ids: IndexMap<ClientOrderId, Vec<VenueOrderId>>,
    bulk_position_coverage: bool,
    uncovered_instrument_ids: IndexSet<InstrumentId>,
    retain_unresolved_submissions: bool,
    requires_authoritative_fills: bool,
    client_id: ClientId,
    account_id: AccountId,
    venue: Venue,
    oms_type: OmsType,
    handled_venues: Option<IndexSet<Venue>>,
    order_report: RefCell<Option<OrderStatusReport>>,
    order_reports: RefCell<Vec<OrderStatusReport>>,
    fill_reports: Vec<FillReport>,
    fill_report_queries: RefCell<Vec<GenerateFillReports>>,
    fail_fill_reports: Cell<bool>,
    on_fill_reports_query: RefCell<Option<Box<dyn FnOnce()>>>,
    order_report_query_count: Cell<usize>,
    fail_order_report: bool,
    fail_order_reports: bool,
    commission: Option<Money>,
    commission_failure: Option<Rc<Cell<bool>>>,
    on_order_report_query: RefCell<Option<Box<dyn FnOnce()>>>,
    on_order_reports_query: RefCell<Option<Box<dyn FnOnce()>>>,
}

impl MockExecutionClient {
    fn new(order_reports: Vec<OrderStatusReport>) -> Self {
        Self {
            bulk_position_coverage: true,
            uncovered_instrument_ids: IndexSet::new(),
            retain_unresolved_submissions: false,
            requires_authoritative_fills: false,
            order_venue_ids: IndexMap::new(),
            client_id: test_client_id(),
            account_id: test_account_id(),
            venue: test_venue(),
            oms_type: OmsType::Hedging,
            handled_venues: None,
            order_report: RefCell::new(None),
            order_reports: RefCell::new(order_reports),
            fill_reports: Vec::new(),
            fill_report_queries: RefCell::new(Vec::new()),
            fail_fill_reports: Cell::new(false),
            on_fill_reports_query: RefCell::new(None),
            order_report_query_count: Cell::new(0),
            fail_order_report: false,
            fail_order_reports: false,
            commission: None,
            commission_failure: None,
            on_order_report_query: RefCell::new(None),
            on_order_reports_query: RefCell::new(None),
        }
    }

    fn for_venue(client_id: ClientId, venue: Venue, order_reports: Vec<OrderStatusReport>) -> Self {
        Self {
            bulk_position_coverage: true,
            uncovered_instrument_ids: IndexSet::new(),
            retain_unresolved_submissions: false,
            requires_authoritative_fills: false,
            order_venue_ids: IndexMap::new(),
            client_id,
            account_id: test_account_id(),
            venue,
            oms_type: OmsType::Hedging,
            handled_venues: None,
            order_report: RefCell::new(None),
            order_reports: RefCell::new(order_reports),
            fill_reports: Vec::new(),
            fill_report_queries: RefCell::new(Vec::new()),
            fail_fill_reports: Cell::new(false),
            on_fill_reports_query: RefCell::new(None),
            order_report_query_count: Cell::new(0),
            fail_order_report: false,
            fail_order_reports: false,
            commission: None,
            commission_failure: None,
            on_order_report_query: RefCell::new(None),
            on_order_reports_query: RefCell::new(None),
        }
    }

    fn failing(client_id: ClientId, venue: Venue) -> Self {
        Self {
            bulk_position_coverage: true,
            uncovered_instrument_ids: IndexSet::new(),
            retain_unresolved_submissions: false,
            requires_authoritative_fills: false,
            order_venue_ids: IndexMap::new(),
            client_id,
            account_id: test_account_id(),
            venue,
            oms_type: OmsType::Hedging,
            handled_venues: None,
            order_report: RefCell::new(None),
            order_reports: RefCell::new(Vec::new()),
            fill_reports: Vec::new(),
            fill_report_queries: RefCell::new(Vec::new()),
            fail_fill_reports: Cell::new(false),
            on_fill_reports_query: RefCell::new(None),
            order_report_query_count: Cell::new(0),
            fail_order_report: false,
            fail_order_reports: true,
            commission: None,
            commission_failure: None,
            on_order_report_query: RefCell::new(None),
            on_order_reports_query: RefCell::new(None),
        }
    }

    fn with_bulk_position_coverage(mut self, covered: bool) -> Self {
        self.bulk_position_coverage = covered;
        self
    }

    fn with_uncovered_instrument(mut self, instrument_id: InstrumentId) -> Self {
        self.uncovered_instrument_ids.insert(instrument_id);
        self
    }

    fn with_account_id(mut self, account_id: AccountId) -> Self {
        self.account_id = account_id;
        self
    }

    fn with_handled_venues(mut self, handled_venues: IndexSet<Venue>) -> Self {
        self.handled_venues = Some(handled_venues);
        self
    }

    fn with_order_report(self, report: OrderStatusReport) -> Self {
        *self.order_report.borrow_mut() = Some(report);
        self
    }

    fn with_fill_reports(mut self, reports: Vec<FillReport>) -> Self {
        self.fill_reports = reports;
        self
    }

    fn with_failed_order_report(mut self) -> Self {
        self.fail_order_report = true;
        self
    }

    fn with_commission(mut self, commission: Money, failure: Rc<Cell<bool>>) -> Self {
        self.commission = Some(commission);
        self.commission_failure = Some(failure);
        self
    }

    fn with_on_order_report_query(self, callback: Box<dyn FnOnce()>) -> Self {
        *self.on_order_report_query.borrow_mut() = Some(callback);
        self
    }

    fn with_on_order_reports_query(self, callback: Box<dyn FnOnce()>) -> Self {
        *self.on_order_reports_query.borrow_mut() = Some(callback);
        self
    }
}

#[async_trait(?Send)]
impl ExecutionClient for MockExecutionClient {
    fn requires_authoritative_fills(&self) -> bool {
        self.requires_authoritative_fills
    }

    fn provides_bulk_position_coverage(&self, instrument_id: InstrumentId) -> bool {
        self.bulk_position_coverage && !self.uncovered_instrument_ids.contains(&instrument_id)
    }

    fn venue_order_ids(
        &self,
        _mass_status: &ExecutionMassStatus,
    ) -> Option<IndexMap<ClientOrderId, Vec<VenueOrderId>>> {
        (!self.order_venue_ids.is_empty()).then(|| self.order_venue_ids.clone())
    }

    fn retain_unresolved_submissions(&self) -> bool {
        self.retain_unresolved_submissions
    }

    fn is_connected(&self) -> bool {
        true
    }

    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn account_id(&self) -> AccountId {
        self.account_id
    }

    fn venue(&self) -> Venue {
        self.venue
    }

    fn handles_order_venue(&self, venue: Venue) -> bool {
        self.handled_venues
            .as_ref()
            .map_or(self.venue == venue, |handled| handled.contains(&venue))
    }

    fn oms_type(&self) -> OmsType {
        self.oms_type
    }

    fn get_account(&self) -> Option<AccountAny> {
        None
    }

    fn generate_account_state(
        &self,
        _balances: Vec<AccountBalance>,
        _margins: Vec<MarginBalance>,
        _reported: bool,
        _ts_event: UnixNanos,
        _info: Option<Params>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn start(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn submit_order(&self, _cmd: SubmitOrder) -> anyhow::Result<()> {
        Ok(())
    }

    fn submit_order_list(&self, _cmd: SubmitOrderList) -> anyhow::Result<()> {
        Ok(())
    }

    fn modify_order(&self, _cmd: ModifyOrder) -> anyhow::Result<()> {
        Ok(())
    }

    fn cancel_order(&self, _cmd: CancelOrder) -> anyhow::Result<()> {
        Ok(())
    }

    fn cancel_all_orders(&self, _cmd: CancelAllOrders) -> anyhow::Result<()> {
        Ok(())
    }

    fn batch_cancel_orders(&self, _cmd: BatchCancelOrders) -> anyhow::Result<()> {
        Ok(())
    }

    fn query_account(&self, _cmd: QueryAccount) -> anyhow::Result<()> {
        Ok(())
    }

    fn query_order(&self, _cmd: QueryOrder) -> anyhow::Result<()> {
        Ok(())
    }

    fn calculate_commission(
        &self,
        _instrument: &InstrumentAny,
        _last_qty: Quantity,
        _last_px: Price,
        _liquidity_side: LiquiditySide,
    ) -> anyhow::Result<Option<Money>> {
        if self
            .commission_failure
            .as_ref()
            .is_some_and(|failure| failure.get())
        {
            anyhow::bail!("commission unavailable");
        }

        Ok(self.commission)
    }

    async fn generate_order_status_report(
        &self,
        _cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        self.order_report_query_count
            .set(self.order_report_query_count.get() + 1);

        if let Some(callback) = self.on_order_report_query.borrow_mut().take() {
            callback();
        }

        if self.fail_order_report {
            anyhow::bail!("order report unavailable");
        }

        Ok(self.order_report.borrow().clone())
    }

    async fn generate_order_status_reports(
        &self,
        _cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        if let Some(callback) = self.on_order_reports_query.borrow_mut().take() {
            callback();
        }

        if self.fail_order_reports {
            anyhow::bail!("order reports unavailable");
        }

        Ok(self.order_reports.borrow().clone())
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        self.fill_report_queries.borrow_mut().push(cmd);

        if let Some(callback) = self.on_fill_reports_query.borrow_mut().take() {
            callback();
        }

        if self.fail_fill_reports.get() {
            anyhow::bail!("fill reports unavailable");
        }

        Ok(self.fill_reports.clone())
    }
}

fn insert_accepted_limit_order(
    ctx: &TestContext,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    client_id: ClientId,
) {
    insert_accepted_limit_order_for_instrument(
        ctx,
        client_order_id,
        venue_order_id,
        test_instrument_id(),
        client_id,
    );
}

fn insert_accepted_limit_order_for_instrument(
    ctx: &TestContext,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    instrument_id: InstrumentId,
    client_id: ClientId,
) {
    let order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .quantity(Quantity::from("10.0"))
        .price(Price::from("100.0"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    ctx.add_order_with_client_id(order, client_id);
    let order = ctx.cache.borrow_mut().update_order(&submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    ctx.cache.borrow_mut().update_order(&accepted).unwrap();
}

enum PositionRequestMutation {
    Add {
        cache: Rc<RefCell<Cache>>,
        position: Position,
    },
    Update {
        cache: Rc<RefCell<Cache>>,
        position: Position,
    },
}

struct MockPositionExecutionClient {
    client_id: ClientId,
    account_id: AccountId,
    venue: Venue,
    handled_venues: Option<IndexSet<Venue>>,
    order_reports: RefCell<Vec<OrderStatusReport>>,
    position_reports: RefCell<Vec<PositionStatusReport>>,
    position_request_mutation: RefCell<Option<PositionRequestMutation>>,
    fail_position_reports: bool,
    position_reconciliation_tolerance: Decimal,
}

impl MockPositionExecutionClient {
    fn new(
        order_reports: Vec<OrderStatusReport>,
        position_reports: Vec<PositionStatusReport>,
    ) -> Self {
        Self {
            client_id: test_client_id(),
            account_id: test_account_id(),
            venue: test_venue(),
            handled_venues: None,
            order_reports: RefCell::new(order_reports),
            position_reports: RefCell::new(position_reports),
            position_request_mutation: RefCell::new(None),
            fail_position_reports: false,
            position_reconciliation_tolerance: dec!(0.00000001),
        }
    }

    fn with_position_reconciliation_tolerance(mut self, tolerance: Decimal) -> Self {
        self.position_reconciliation_tolerance = tolerance;
        self
    }

    fn with_position_reports(mut self, position_reports: Vec<PositionStatusReport>) -> Self {
        self.position_reports = RefCell::new(position_reports);
        self
    }

    fn with_position_request_mutation(mut self, mutation: PositionRequestMutation) -> Self {
        self.position_request_mutation = RefCell::new(Some(mutation));
        self
    }

    fn failing_position_reports() -> Self {
        Self {
            client_id: test_client_id(),
            account_id: test_account_id(),
            venue: test_venue(),
            handled_venues: None,
            order_reports: RefCell::new(Vec::new()),
            position_reports: RefCell::new(Vec::new()),
            position_request_mutation: RefCell::new(None),
            fail_position_reports: true,
            position_reconciliation_tolerance: dec!(0.00000001),
        }
    }

    fn configured(
        client_id: ClientId,
        account_id: AccountId,
        venue: Venue,
        handled_venues: IndexSet<Venue>,
        fail_position_reports: bool,
    ) -> Self {
        Self {
            client_id,
            account_id,
            venue,
            handled_venues: Some(handled_venues),
            order_reports: RefCell::new(Vec::new()),
            position_reports: RefCell::new(Vec::new()),
            position_request_mutation: RefCell::new(None),
            fail_position_reports,
            position_reconciliation_tolerance: dec!(0.00000001),
        }
    }
}

#[async_trait(?Send)]
impl ExecutionClient for MockPositionExecutionClient {
    fn is_connected(&self) -> bool {
        true
    }

    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn account_id(&self) -> AccountId {
        self.account_id
    }

    fn venue(&self) -> Venue {
        self.venue
    }

    fn handles_order_venue(&self, venue: Venue) -> bool {
        self.handled_venues
            .as_ref()
            .map_or(self.venue == venue, |handled| handled.contains(&venue))
    }

    fn oms_type(&self) -> OmsType {
        OmsType::Hedging
    }

    fn get_account(&self) -> Option<AccountAny> {
        None
    }

    fn position_reconciliation_tolerance(&self) -> Decimal {
        self.position_reconciliation_tolerance
    }

    fn generate_account_state(
        &self,
        _balances: Vec<AccountBalance>,
        _margins: Vec<MarginBalance>,
        _reported: bool,
        _ts_event: UnixNanos,
        _info: Option<Params>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn start(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn submit_order(&self, _cmd: SubmitOrder) -> anyhow::Result<()> {
        Ok(())
    }

    fn submit_order_list(&self, _cmd: SubmitOrderList) -> anyhow::Result<()> {
        Ok(())
    }

    fn modify_order(&self, _cmd: ModifyOrder) -> anyhow::Result<()> {
        Ok(())
    }

    fn cancel_order(&self, _cmd: CancelOrder) -> anyhow::Result<()> {
        Ok(())
    }

    fn cancel_all_orders(&self, _cmd: CancelAllOrders) -> anyhow::Result<()> {
        Ok(())
    }

    fn batch_cancel_orders(&self, _cmd: BatchCancelOrders) -> anyhow::Result<()> {
        Ok(())
    }

    fn query_account(&self, _cmd: QueryAccount) -> anyhow::Result<()> {
        Ok(())
    }

    fn query_order(&self, _cmd: QueryOrder) -> anyhow::Result<()> {
        Ok(())
    }

    async fn generate_order_status_reports(
        &self,
        _cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        Ok(self.order_reports.borrow().clone())
    }

    async fn generate_position_status_reports(
        &self,
        _cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let mutation = { self.position_request_mutation.borrow_mut().take() };
        if let Some(mutation) = mutation {
            dst::time::sleep(dst::time::Duration::from_secs(1)).await;

            match mutation {
                PositionRequestMutation::Add { cache, position } => cache
                    .borrow_mut()
                    .add_position(&position, OmsType::Hedging)
                    .unwrap(),
                PositionRequestMutation::Update { cache, position } => {
                    cache.borrow_mut().update_position(&position).unwrap();
                }
            }
        }

        if self.fail_position_reports {
            anyhow::bail!("position reports unavailable");
        }

        Ok(self.position_reports.borrow().clone())
    }
}

fn create_test_position_report(
    account_id: AccountId,
    instrument_id: InstrumentId,
    position_side: PositionSide,
    quantity: &str,
    venue_position_id: &str,
    avg_px_open: Decimal,
) -> PositionStatusReport {
    PositionStatusReport::new(
        account_id,
        instrument_id,
        position_side,
        Quantity::from(quantity),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(PositionId::from(venue_position_id)),
        Some(avg_px_open),
    )
}
