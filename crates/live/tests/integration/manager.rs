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
    sync::{Arc, Once},
};

use ahash::{AHashMap, AHashSet};
use async_trait::async_trait;
use bytes::Bytes;
use indexmap::IndexSet;
use log::{Level, LevelFilter, Log, Metadata, Record};
use nautilus_common::{
    cache::{
        Cache,
        database::{CacheDatabaseAdapter, CacheMap},
    },
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
    signal::Signal,
};
use nautilus_core::{DurationNanos, Params, UUID4, UnixNanos};
use nautilus_execution::{
    engine::ExecutionEngine,
    reconciliation::{
        create_position_reconciliation_venue_order_id, process_mass_status_for_reconciliation,
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
    data::{
        Bar, CustomData, DataType, FundingRateUpdate, InstrumentClose, QuoteTick, TradeTick,
        greeks::{GreeksData, YieldCurveData},
    },
    enums::{
        AccountType, AvgPxReconciliation, ContingencyType, LiquiditySide, OmsType, OrderSide,
        OrderStatus, OrderType, PositionSide, TimeInForce, TriggerType,
    },
    events::{
        OrderEventAny, OrderFilled, OrderSnapshot, PositionEvent,
        account::state::AccountState,
        order::spec::{
            OrderAcceptedSpec, OrderCancelRejectedSpec, OrderModifyRejectedSpec,
            OrderPendingCancelSpec, OrderPendingUpdateSpec, OrderUpdatedSpec,
        },
        position::snapshot::PositionSnapshot,
    },
    identifiers::{
        AccountId, ActorId, ClientId, ClientOrderId, ExecAlgorithmId, InstrumentId, PositionId,
        StrategyId, TradeId, TraderId, Venue, VenueOrderId,
    },
    instruments::{
        Instrument, InstrumentAny, SyntheticInstrument,
        stubs::{binary_option, btcusd_bybit, crypto_perpetual_ethusdt, currency_pair_btcusdt},
    },
    orderbook::OrderBook,
    orders::{
        Order, OrderAny, OrderTestBuilder,
        stubs::{OrderFilledTestBuilder, TestOrderEventStubs},
    },
    position::Position,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, MarginBalance, Money, Price, Quantity},
};
use parking_lot::Mutex;
use rstest::rstest;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use ustr::Ustr;

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

    /// Builds a context around a prepared cache, such as one restored through `cache_all`.
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
            && matches!(
                metadata.target(),
                "nautilus_live::execution::manager" | "nautilus_live::execution::reconciliation"
            )
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

#[rstest]
fn test_fill_deduplication_new_fill_not_processed() {
    let ctx = TestContext::new();
    let trade_id = TradeId::from("T-001");

    assert!(!ctx.manager.is_fill_recently_processed(
        test_account_id(),
        test_instrument_id(),
        trade_id
    ));
}

#[rstest]
fn test_fill_deduplication_tracks_processed_fill() {
    let mut ctx = TestContext::new();
    let trade_id = TradeId::from("T-001");

    ctx.manager
        .mark_fill_processed(test_account_id(), test_instrument_id(), trade_id);

    assert!(ctx.manager.is_fill_recently_processed(
        test_account_id(),
        test_instrument_id(),
        trade_id
    ));
    assert!(!ctx.manager.is_fill_recently_processed(
        test_account_id(),
        test_instrument_id2(),
        trade_id
    ));
    assert!(!ctx.manager.is_fill_recently_processed(
        AccountId::from("BINANCE-002"),
        test_instrument_id(),
        trade_id
    ));
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_fill_deduplication_prune_removes_expired() {
    let mut ctx = TestContext::new();
    let old_trade = TradeId::from("T-OLD");
    let new_trade = TradeId::from("T-NEW");

    ctx.manager
        .mark_fill_processed(test_account_id(), test_instrument_id(), old_trade);
    ctx.advance_both(dst::time::Duration::from_secs(120)).await;
    ctx.manager
        .mark_fill_processed(test_account_id(), test_instrument_id(), new_trade);

    ctx.manager.prune_recent_fills_cache(60.0); // 60 second TTL

    assert!(!ctx.manager.is_fill_recently_processed(
        test_account_id(),
        test_instrument_id(),
        old_trade
    ));
    assert!(ctx.manager.is_fill_recently_processed(
        test_account_id(),
        test_instrument_id(),
        new_trade
    ));
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_fill_deduplication_prune_uses_monotonic_ttl() {
    let mut ctx = TestContext::new();
    let trade_id = TradeId::from("T-MONOTONIC");

    ctx.manager
        .mark_fill_processed(test_account_id(), test_instrument_id(), trade_id);
    ctx.advance_time(120_000_000_000);
    ctx.manager.prune_recent_fills_cache(60.0);
    assert!(ctx.manager.is_fill_recently_processed(
        test_account_id(),
        test_instrument_id(),
        trade_id
    ));

    advance_clock(dst::time::Duration::from_secs(61)).await;
    ctx.manager.prune_recent_fills_cache(60.0);
    assert!(!ctx.manager.is_fill_recently_processed(
        test_account_id(),
        test_instrument_id(),
        trade_id
    ));
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_prune_recent_fills_cache_keeps_all_on_overflow_ttl() {
    // Positive-overflow and infinity TTLs must saturate to keep-all.
    let mut ctx = TestContext::new();
    let trade_id = TradeId::from("T-KEEP");
    ctx.manager
        .mark_fill_processed(test_account_id(), test_instrument_id(), trade_id);
    advance_clock(dst::time::Duration::from_nanos(1)).await;

    ctx.manager.prune_recent_fills_cache(1.0e30);
    assert!(
        ctx.manager
            .is_fill_recently_processed(test_account_id(), test_instrument_id(), trade_id),
        "overflowing TTL must keep entries, not prune them",
    );

    ctx.manager.prune_recent_fills_cache(f64::INFINITY);
    assert!(
        ctx.manager
            .is_fill_recently_processed(test_account_id(), test_instrument_id(), trade_id),
        "infinite TTL must keep entries",
    );
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_prune_recent_fills_cache_prunes_all_on_negative_or_nan_ttl() {
    // Negative and NaN TTLs fall back to a zero TTL, matching the old cast.
    let mut ctx = TestContext::new();

    ctx.manager.mark_fill_processed(
        test_account_id(),
        test_instrument_id(),
        TradeId::from("T-NEG"),
    );
    advance_clock(dst::time::Duration::from_nanos(1)).await;
    ctx.manager.prune_recent_fills_cache(-1.0);
    assert!(!ctx.manager.is_fill_recently_processed(
        test_account_id(),
        test_instrument_id(),
        TradeId::from("T-NEG"),
    ));

    ctx.manager.mark_fill_processed(
        test_account_id(),
        test_instrument_id(),
        TradeId::from("T-NAN"),
    );
    advance_clock(dst::time::Duration::from_nanos(1)).await;
    ctx.manager.prune_recent_fills_cache(f64::NAN);
    assert!(!ctx.manager.is_fill_recently_processed(
        test_account_id(),
        test_instrument_id(),
        TradeId::from("T-NAN"),
    ));
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_observe_order_report_clears_inflight_tracking() {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 5,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");

    ctx.add_instrument(test_instrument());
    let order = create_submitted_order("O-001", instrument_id, OrderSide::Buy, "1.0", "3000.00");
    ctx.add_order(order);

    // Register as inflight
    ctx.manager.register_inflight(client_order_id);

    // Simulate venue responding with an OrderStatusReport
    let order_report = create_order_report(
        Some(client_order_id),
        VenueOrderId::from("V-001"),
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from(0),
    );
    let report = ExecutionReport::Order(Box::new(order_report));

    ctx.manager.observe_execution_report(&report);

    // Advance time past threshold so inflight check would trigger if still tracked
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;

    // Inflight check should return empty - tracking was cleared by observation
    let result = ctx.manager.check_inflight_orders();
    assert!(result.events.is_empty());
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_observe_pending_order_report_keeps_inflight_tracking() {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 3,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-PENDING");

    ctx.add_instrument(test_instrument());
    let order =
        create_submitted_order("O-PENDING", instrument_id, OrderSide::Buy, "1.0", "3000.00");
    ctx.add_order(order);

    ctx.manager.register_inflight(client_order_id);

    // Venue reports PendingUpdate (interim acknowledgement, not terminal)
    let order_report = create_order_report(
        Some(client_order_id),
        VenueOrderId::from("V-001"),
        instrument_id,
        OrderStatus::PendingUpdate,
        Quantity::from("1.0"),
        Quantity::from(0),
    );
    let report = ExecutionReport::Order(Box::new(order_report));

    ctx.manager.observe_execution_report(&report);

    // Inflight tracking should still be active
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;
    let result = ctx.manager.check_inflight_orders();

    // Order is still tracked: should generate a query (retry 1 < max 3)
    assert_eq!(result.queries.len(), 1);
    assert!(result.events.is_empty());
}

#[rstest]
fn test_observe_fill_report_does_not_mark_fill_processed() {
    // Incoming fill reports record activity only. Normal-live fill dedup marking
    // follows the separate order-event observation path.
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let trade_id = TradeId::from("T-001");

    ctx.add_instrument(test_instrument());

    let fill_report = FillReport::new(
        test_account_id(),
        instrument_id,
        VenueOrderId::from("V-001"),
        trade_id,
        OrderSide::Buy,
        Quantity::from("1.0"),
        Price::from("3000.00"),
        Money::new(0.0, Currency::USDT()),
        LiquiditySide::Maker,
        Some(ClientOrderId::from("O-001")),
        None, // venue_position_id
        UnixNanos::from(2_000_000),
        UnixNanos::from(2_000_000),
        None, // report_id
    );
    let report = ExecutionReport::Fill(Box::new(fill_report));

    ctx.manager.observe_execution_report(&report);

    // observe_execution_report should not mark fills as processed.
    assert!(
        !ctx.manager
            .is_fill_recently_processed(test_account_id(), instrument_id, trade_id)
    );
}

#[rstest]
fn test_observe_position_report_records_activity() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();

    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("1.0"),
        UnixNanos::from(3_000_000),
        UnixNanos::from(3_000_000),
        None, // report_id
        None, // venue_position_id
        Some(dec!(3000.0)),
    );
    let report = ExecutionReport::Position(Box::new(position_report));

    // Should complete without panic - position activity is recorded internally
    ctx.manager.observe_execution_report(&report);
}

#[tokio::test]
async fn test_reconcile_mass_status_with_empty_reports() {
    let mut ctx = TestContext::new();

    let mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert!(result.events.is_empty());
}

#[tokio::test]
async fn test_reconcile_mass_status_creates_external_order_accepted() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_id = test_client_id();

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        None, // No client_order_id = external order
        VenueOrderId::from("V-EXT-001"),
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));

    // Verify order was added to cache
    let client_order_id = ClientOrderId::from("V-EXT-001");
    let order = ctx.get_order(&client_order_id);
    assert!(order.is_some());
    assert_eq!(
        ctx.cache.borrow().client_id(&client_order_id),
        Some(&client_id)
    );
}

#[tokio::test]
async fn test_reconcile_mass_status_materializes_restored_close_position_order() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("close-long");
    ctx.add_instrument(test_instrument());

    let report = OrderStatusReport::new(
        test_account_id(),
        instrument_id,
        Some(client_order_id),
        VenueOrderId::from("123456790"),
        OrderSide::Sell.into(),
        OrderType::StopMarket,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        Quantity::from("0.005"),
        Quantity::from("0.000"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    )
    .with_trigger_price(Price::from("2500.00"))
    .with_trigger_type(TriggerType::MarkPrice)
    .with_reduce_only(true);
    let mut mass_status = create_mass_status(vec![report], Vec::new());
    mass_status.add_position_reports(vec![PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("0.005"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3000.0)),
    )]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);
    let order = ctx.get_order(&client_order_id).unwrap();

    assert_eq!(
        result
            .events
            .iter()
            .filter(|event| matches!(
                event,
                OrderEventAny::Accepted(accepted)
                    if accepted.client_order_id == client_order_id
            ))
            .count(),
        1
    );
    assert_eq!(result.external_orders.len(), 1);
    assert_eq!(order.order_type(), OrderType::StopMarket);
    assert_eq!(order.order_side(), OrderSide::Sell);
    assert_eq!(order.quantity(), Quantity::from("0.005"));
    assert_eq!(order.trigger_price(), Some(Price::from("2500.00")));
    assert!(order.is_reduce_only());
}

#[tokio::test]
async fn test_reconcile_mass_status_rejects_external_order_with_zero_quantity() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("ordinary-zero");
    ctx.add_instrument(test_instrument());

    let report = create_order_report(
        Some(client_order_id),
        VenueOrderId::from("123456792"),
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("0.000"),
        Quantity::from("0.000"),
    );
    let mass_status = create_mass_status(vec![report], Vec::new());

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert!(result.events.is_empty());
    assert!(result.external_orders.is_empty());
    assert!(ctx.get_order(&client_order_id).is_none());
}

#[rstest]
#[case(None, true)]
#[case(None, false)]
#[case(Some("BINANCE"), true)]
#[case(Some("BINANCE"), false)]
#[case(Some("OTHER"), true)]
#[case(Some("OTHER"), false)]
#[tokio::test]
async fn test_reconcile_mass_status_warns_on_untrusted_cached_client_origin(
    #[case] cached_client_id: Option<&str>,
    #[case] report_has_client_order_id: bool,
) {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-CACHED-SOURCE");
    let venue_order_id = VenueOrderId::from("V-CACHED-SOURCE");
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .quantity(Quantity::from("1.0"))
        .price(Price::from("3000.00"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();

    ctx.add_instrument(test_instrument());
    ctx.cache
        .borrow_mut()
        .add_order(order, None, cached_client_id.map(ClientId::from), false)
        .unwrap();

    if !report_has_client_order_id {
        ctx.cache
            .borrow_mut()
            .add_venue_order_id(&client_order_id, &venue_order_id, false)
            .unwrap();
    }

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );
    mass_status.add_order_reports(vec![create_order_report(
        report_has_client_order_id.then_some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Canceled,
        Quantity::from("1.0"),
        Quantity::from("0"),
    )]);

    let raw_topic = MessagingSwitchboard::reconciliation_raw_order_status_report_topic();
    let raw_pattern: msgbus::MStr<msgbus::Pattern> = raw_topic.into();
    let (raw_handler, raw_saver) = get_any_saving_handler::<OrderStatusReport>(None);
    msgbus::subscribe_any(raw_pattern, raw_handler.clone(), None);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    msgbus::unsubscribe_any(raw_pattern, &raw_handler);

    assert_eq!(result.events.len(), 1);
    assert!(result.external_orders.is_empty());
    assert_eq!(
        ctx.get_order(&client_order_id).unwrap().status(),
        OrderStatus::Canceled
    );
    assert_eq!(raw_saver.get_messages().len(), 1);
    assert_eq!(
        ctx.cache.borrow().client_id(&client_order_id).copied(),
        cached_client_id.map(ClientId::from),
    );
}

#[tokio::test]
async fn test_reconcile_mass_status_warns_on_venue_only_fill_from_other_client() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-CACHED-FILL-SOURCE");
    let venue_order_id = VenueOrderId::from("V-CACHED-FILL-SOURCE");
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .quantity(Quantity::from("1.0"))
        .price(Price::from("3000.00"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();

    ctx.add_instrument(test_instrument());
    ctx.cache
        .borrow_mut()
        .add_order(order, None, Some(ClientId::from("OTHER")), false)
        .unwrap();
    ctx.cache
        .borrow_mut()
        .add_venue_order_id(&client_order_id, &venue_order_id, false)
        .unwrap();

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );
    mass_status.add_fill_reports(vec![FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-CACHED-FILL-SOURCE"),
        OrderSide::Buy,
        Quantity::from("1.0"),
        Price::from("3000.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        None,
        None,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    )]);

    let raw_topic = MessagingSwitchboard::reconciliation_raw_fill_report_topic();
    let raw_pattern: msgbus::MStr<msgbus::Pattern> = raw_topic.into();
    let (raw_handler, raw_saver) = get_any_saving_handler::<FillReport>(None);
    msgbus::subscribe_any(raw_pattern, raw_handler.clone(), None);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    msgbus::unsubscribe_any(raw_pattern, &raw_handler);

    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Filled(_)));
    assert!(result.external_orders.is_empty());
    assert_eq!(raw_saver.get_messages().len(), 1);
    let cached_order = ctx.get_order(&client_order_id).unwrap();
    assert_eq!(cached_order.status(), OrderStatus::Filled);
    assert_eq!(cached_order.filled_qty(), Quantity::from("1.0"));
}

#[rstest]
fn test_reconcile_order_status_report_publishes_external_order_initialized() {
    let ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let strategy_id = StrategyId::from("EXT-PUBLISH-DIRECT");

    ctx.add_instrument(test_instrument());
    ctx.exec_engine
        .borrow_mut()
        .register_external_order_claims(strategy_id, &HashSet::from([instrument_id]))
        .unwrap();

    let topic = switchboard::get_event_order_topic(strategy_id);
    let (handler, event_messages): (_, TypedMessageSavingHandler<OrderEventAny>) =
        get_typed_message_saving_handler(None);
    msgbus::subscribe_order_events(topic.into(), handler.clone(), None);

    let report = create_order_report(
        None,
        VenueOrderId::from("V-EXT-PUBLISH-DIRECT"),
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0"),
    );
    ctx.exec_engine
        .borrow_mut()
        .reconcile_order_status_report(&report);

    msgbus::unsubscribe_order_events(topic.into(), &handler);

    let messages = event_messages.get_messages();
    assert_eq!(messages.len(), 2);

    match &messages[0] {
        OrderEventAny::Initialized(initialized) => {
            assert_eq!(
                initialized.client_order_id,
                ClientOrderId::from("V-EXT-PUBLISH-DIRECT")
            );
            assert_eq!(initialized.strategy_id, strategy_id);
            assert!(initialized.reconciliation);
        }
        event => panic!("Expected OrderInitialized event, was {event:?}"),
    }

    assert!(matches!(messages[1], OrderEventAny::Accepted(_)));
}

#[tokio::test]
async fn test_reconcile_mass_status_publishes_external_order_initialized() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let strategy_id = StrategyId::from("EXT-PUBLISH-MASS");

    ctx.add_instrument(test_instrument());
    ctx.manager
        .claim_external_orders(instrument_id, strategy_id)
        .unwrap();

    let topic = switchboard::get_event_order_topic(strategy_id);
    let (handler, event_messages): (_, TypedMessageSavingHandler<OrderEventAny>) =
        get_typed_message_saving_handler(None);
    msgbus::subscribe_order_events(topic.into(), handler.clone(), None);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );
    let report = create_order_report(
        None,
        VenueOrderId::from("V-EXT-PUBLISH-MASS"),
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    msgbus::unsubscribe_order_events(topic.into(), &handler);

    let messages = event_messages.get_messages();
    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));
    assert_eq!(messages.len(), 2);

    match &messages[0] {
        OrderEventAny::Initialized(initialized) => {
            assert_eq!(
                initialized.client_order_id,
                ClientOrderId::from("V-EXT-PUBLISH-MASS")
            );
            assert_eq!(initialized.strategy_id, strategy_id);
            assert!(initialized.reconciliation);
        }
        event => panic!("Expected OrderInitialized event, was {event:?}"),
    }

    assert!(matches!(messages[1], OrderEventAny::Accepted(_)));
}

#[rstest]
fn test_order_fill_replay_propagates_position_id_to_oto_contingent_order() {
    let ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = test_instrument_id();
    let primary_id = ClientOrderId::from("O-OTO-PRIMARY");
    let contingent_id = ClientOrderId::from("O-OTO-CONTINGENT");
    let position_id = PositionId::from("P-OTO-001");

    ctx.add_instrument(instrument.clone());

    let mut primary_order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(primary_id)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.0"))
        .price(Price::from("3000.00"))
        .contingency_type(ContingencyType::Oto)
        .linked_order_ids(vec![contingent_id])
        .build();
    apply_submitted_and_accepted(&mut primary_order, VenueOrderId::from("V-OTO-PRIMARY"));

    let mut contingent_order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(contingent_id)
        .instrument_id(instrument_id)
        .side(OrderSide::Sell)
        .quantity(Quantity::from("1.0"))
        .price(Price::from("3100.00"))
        .build();
    apply_submitted_and_accepted(
        &mut contingent_order,
        VenueOrderId::from("V-OTO-CONTINGENT"),
    );

    let strategy_id = primary_order.strategy_id();
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(strategy_id, OmsType::Hedging);
    ctx.add_order(primary_order.clone());
    ctx.add_order(contingent_order);

    let fill = TestOrderEventStubs::filled(
        &primary_order,
        &instrument,
        Some(TradeId::from("T-OTO-001")),
        Some(position_id),
        Some(Price::from("3000.00")),
        Some(Quantity::from("1.0")),
        Some(LiquiditySide::Taker),
        Some(Money::from("0 USDT")),
        Some(UnixNanos::from(2_000_000)),
        Some(test_account_id()),
    );
    ctx.exec_engine.borrow_mut().process(&fill);

    let cache = ctx.cache.borrow();
    let primary_after = cache.order(&primary_id).unwrap();
    let contingent_after = cache.order(&contingent_id).unwrap();

    assert_eq!(primary_after.position_id(), Some(position_id));
    assert_eq!(contingent_after.position_id(), Some(position_id));
    assert_eq!(cache.position_id(&contingent_id), Some(&position_id));
}

#[rstest]
fn test_order_fill_replay_propagates_position_id_to_exec_spawn_primary_order() {
    let ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = test_instrument_id();
    let primary_id = ClientOrderId::from("O-SPAWN-PRIMARY");
    let spawned_id = ClientOrderId::from("O-SPAWN-CHILD");
    let position_id = PositionId::from("P-SPAWN-001");

    ctx.add_instrument(instrument.clone());

    let mut primary_order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(primary_id)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.0"))
        .price(Price::from("3000.00"))
        .build();
    apply_submitted_and_accepted(&mut primary_order, VenueOrderId::from("V-SPAWN-PRIMARY"));

    let mut spawned_order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(spawned_id)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.0"))
        .price(Price::from("3000.00"))
        .exec_algorithm_id(ExecAlgorithmId::from("ALG-SPAWN"))
        .exec_spawn_id(primary_id)
        .build();
    apply_submitted_and_accepted(&mut spawned_order, VenueOrderId::from("V-SPAWN-CHILD"));

    let strategy_id = spawned_order.strategy_id();
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(strategy_id, OmsType::Hedging);
    ctx.add_order(primary_order);
    ctx.add_order(spawned_order.clone());

    let fill = TestOrderEventStubs::filled(
        &spawned_order,
        &instrument,
        Some(TradeId::from("T-SPAWN-001")),
        Some(position_id),
        Some(Price::from("3000.00")),
        Some(Quantity::from("1.0")),
        Some(LiquiditySide::Taker),
        Some(Money::from("0 USDT")),
        Some(UnixNanos::from(2_000_000)),
        Some(test_account_id()),
    );
    ctx.exec_engine.borrow_mut().process(&fill);

    let cache = ctx.cache.borrow();
    let primary_after = cache.order(&primary_id).unwrap();
    let spawned_after = cache.order(&spawned_id).unwrap();

    assert_eq!(primary_after.position_id(), Some(position_id));
    assert_eq!(spawned_after.position_id(), Some(position_id));
    assert_eq!(cache.position_id(&primary_id), Some(&position_id));
}

#[tokio::test]
async fn test_reconcile_mass_status_creates_external_order_canceled() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        None,
        VenueOrderId::from("V-EXT-002"),
        instrument_id,
        OrderStatus::Canceled,
        Quantity::from("1.0"),
        Quantity::from("0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 2);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));
    assert!(matches!(result.events[1], OrderEventAny::Canceled(_)));
}

#[tokio::test]
async fn test_external_order_canceled_with_partial_fill() {
    // Test that external orders with Canceled status and partial fills
    // have both the fill and canceled events generated (matching Python behavior)
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let venue_order_id = VenueOrderId::from("V-EXT-PARTIAL");

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Order was partially filled (0.5 of 1.0) then canceled
    let report = create_order_report(
        None,
        venue_order_id,
        instrument_id,
        OrderStatus::Canceled,
        Quantity::from("1.0"),
        Quantity::from("0.5"),
    )
    .with_avg_px(dec!(3000.00));
    mass_status.add_order_reports(vec![report]);

    // Add fill report for the partial fill
    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-PARTIAL-001"),
        OrderSide::Buy,
        Quantity::from("0.5"),
        Price::from("3000.00"),
        Money::from("0.25 USDT"),
        LiquiditySide::Maker,
        None, // No client_order_id for external order
        None,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );
    mass_status.add_fill_reports(vec![fill]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should have: Accepted, Filled, Canceled (in ts_event order)
    assert_eq!(result.events.len(), 3);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));
    assert!(matches!(result.events[1], OrderEventAny::Filled(_)));
    assert!(matches!(result.events[2], OrderEventAny::Canceled(_)));

    if let OrderEventAny::Filled(filled) = &result.events[1] {
        assert_eq!(filled.last_qty, Quantity::from("0.5"));
        assert_eq!(filled.trade_id, TradeId::from("T-PARTIAL-001"));
    }

    // Verify order state in cache
    let cache = ctx.cache.borrow();
    let orders = cache.orders(None, None, None, None, None);
    assert_eq!(orders.len(), 1);
    let order = &orders[0];
    assert_eq!(order.status(), OrderStatus::Canceled);
    assert_eq!(order.filled_qty(), Quantity::from("0.5"));
}

#[rstest]
#[case(OrderStatus::Canceled)]
#[case(OrderStatus::Expired)]
#[tokio::test]
async fn test_external_terminal_order_with_incomplete_real_fills_infers_residual(
    #[case] terminal_status: OrderStatus,
) {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let venue_order_id = VenueOrderId::from("V-EXT-TERMINAL-RESIDUAL");

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );
    let report = create_order_report(
        None,
        venue_order_id,
        instrument_id,
        terminal_status,
        Quantity::from("2.0"),
        Quantity::from("1.5"),
    )
    .with_avg_px(dec!(3000.00));
    mass_status.add_order_reports(vec![report]);

    let real_trade_id = TradeId::from("T-TERMINAL-RESIDUAL-001");

    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        real_trade_id,
        OrderSide::Buy,
        Quantity::from("1.0"),
        Price::from("3000.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        None,
        None,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );
    mass_status.add_fill_reports(vec![fill]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 4);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));
    assert!(matches!(
        (&result.events[3], terminal_status),
        (OrderEventAny::Canceled(_), OrderStatus::Canceled)
            | (OrderEventAny::Expired(_), OrderStatus::Expired),
    ));

    let OrderEventAny::Filled(real_fill) = &result.events[1] else {
        panic!("Expected real Filled event, was {:?}", result.events[1]);
    };

    let OrderEventAny::Filled(inferred_fill) = &result.events[2] else {
        panic!("Expected inferred Filled event, was {:?}", result.events[2]);
    };

    assert_eq!(real_fill.trade_id, real_trade_id);
    assert_eq!(real_fill.last_qty, Quantity::from("1.0"));
    assert_ne!(inferred_fill.trade_id, real_trade_id);
    assert_eq!(inferred_fill.last_qty, Quantity::from("0.5"));

    let cache = ctx.cache.borrow();
    let orders = cache.orders(None, None, None, None, None);
    assert_eq!(orders.len(), 1);
    assert_eq!(orders[0].status(), terminal_status);
    assert_eq!(orders[0].filled_qty(), Quantity::from("1.5"));
}

#[tokio::test]
async fn test_cached_order_canceled_with_fills() {
    // Test that a cached order transitioning to Canceled has fills applied
    // BEFORE the Canceled event (matching Python behavior)
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-CANCEL-FILL");
    let venue_order_id = VenueOrderId::from("V-CANCEL-FILL");

    ctx.add_instrument(test_instrument());

    // Create and cache an accepted order
    let mut order = create_submitted_order(
        "O-CANCEL-FILL",
        instrument_id,
        OrderSide::Buy,
        "2.0",
        "3000.00",
    );
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Venue reports order was partially filled then canceled
    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Canceled,
        Quantity::from("2.0"),
        Quantity::from("1.0"),
    )
    .with_avg_px(dec!(3000.00));
    mass_status.add_order_reports(vec![report]);

    // Add fill report
    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-CACHED-001"),
        OrderSide::Buy,
        Quantity::from("1.0"),
        Price::from("3000.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        Some(client_order_id),
        None,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );
    mass_status.add_fill_reports(vec![fill]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should have: Filled, Canceled (order already accepted)
    assert_eq!(result.events.len(), 2);
    assert!(matches!(result.events[0], OrderEventAny::Filled(_)));
    assert!(matches!(result.events[1], OrderEventAny::Canceled(_)));

    // Verify order state
    let cached_order = ctx.get_order(&client_order_id).unwrap();
    assert_eq!(cached_order.status(), OrderStatus::Canceled);
    assert_eq!(cached_order.filled_qty(), Quantity::from("1.0"));
}

#[tokio::test]
async fn test_triggered_event_generated_before_canceled() {
    // Test that Triggered event is generated before Canceled when ts_triggered is set
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-TRIG-CANCEL");
    let venue_order_id = VenueOrderId::from("V-TRIG-CANCEL");

    ctx.add_instrument(test_instrument());

    // Create and cache an accepted StopLimit order (must be triggerable)
    let mut order = OrderTestBuilder::new(OrderType::StopLimit)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.0"))
        .price(Price::from("3000.00"))
        .trigger_price(Price::from("3100.00"))
        .trigger_type(TriggerType::LastPrice)
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Venue reports order was triggered then canceled
    let mut report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Canceled,
        Quantity::from("1.0"),
        Quantity::from("0"),
    );
    report.ts_triggered = Some(UnixNanos::from(500_000));
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should have: Triggered, Canceled
    assert_eq!(result.events.len(), 2);
    assert!(matches!(result.events[0], OrderEventAny::Triggered(_)));
    assert!(matches!(result.events[1], OrderEventAny::Canceled(_)));
}

#[tokio::test]
async fn test_reconcile_mass_status_creates_external_order_filled() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        None,
        VenueOrderId::from("V-EXT-003"),
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("1.0"),
        Quantity::from("1.0"),
    )
    .with_avg_px(dec!(3000.50));
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 2);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));
    assert!(matches!(result.events[1], OrderEventAny::Filled(_)));

    if let OrderEventAny::Filled(filled) = &result.events[1] {
        assert_eq!(filled.last_qty, Quantity::from("1.0"));
        assert!(filled.reconciliation);
    }
}

#[tokio::test]
async fn test_external_order_filled_uses_real_fills() {
    // Test that external orders with Filled status use real fill reports
    // instead of inferred fills, preserving trade-level details
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let venue_order_id = VenueOrderId::from("V-EXT-FILLED");

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Order is fully filled (2.0 of 2.0)
    let report = create_order_report(
        None,
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("2.0"),
        Quantity::from("2.0"),
    )
    .with_avg_px(dec!(3000.00));
    mass_status.add_order_reports(vec![report]);

    // Add two separate fill reports (multi-fill execution)
    let fill1 = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-FILL-001"),
        OrderSide::Buy,
        Quantity::from("1.0"),
        Price::from("2999.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        None,
        None,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );

    let fill2 = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-FILL-002"),
        OrderSide::Buy,
        Quantity::from("1.0"),
        Price::from("3001.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Taker,
        None,
        None,
        UnixNanos::from(2_000_000),
        UnixNanos::from(2_000_000),
        None,
    );
    mass_status.add_fill_reports(vec![fill1, fill2]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should have: Accepted, Fill1, Fill2 (real fills, not inferred)
    assert_eq!(result.events.len(), 3);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));
    assert!(matches!(result.events[1], OrderEventAny::Filled(_)));
    assert!(matches!(result.events[2], OrderEventAny::Filled(_)));

    // Verify we got the real trade IDs, not inferred
    if let OrderEventAny::Filled(filled1) = &result.events[1] {
        assert_eq!(filled1.trade_id, TradeId::from("T-FILL-001"));
        assert_eq!(filled1.last_qty, Quantity::from("1.0"));
        assert_eq!(filled1.last_px, Price::from("2999.00"));
    }

    if let OrderEventAny::Filled(filled2) = &result.events[2] {
        assert_eq!(filled2.trade_id, TradeId::from("T-FILL-002"));
        assert_eq!(filled2.last_qty, Quantity::from("1.0"));
        assert_eq!(filled2.last_px, Price::from("3001.00"));
    }

    // Verify order state in cache
    {
        let cache = ctx.cache.borrow();
        let orders = cache.orders(None, None, None, None, None);
        assert_eq!(orders.len(), 1);
        let order = &orders[0];
        assert_eq!(order.status(), OrderStatus::Filled);
        assert_eq!(order.filled_qty(), Quantity::from("2.0"));
    }

    ctx.manager = ExecutionManager::new(
        ctx.clock.clone(),
        ctx.cache.clone(),
        ExecutionManagerConfig::default(),
    )
    .expect("valid config");
    let replay = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);
    let cache = ctx.cache.borrow();

    assert!(replay.events.is_empty());
    assert_eq!(cache.orders(None, None, None, None, None).len(), 1);
    assert_eq!(
        cache.orders(None, None, None, None, None)[0].filled_qty(),
        Quantity::from("2.0"),
    );
}

#[tokio::test]
async fn test_external_order_filled_with_acceptance_after_fills() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-EXT-LATE-ACCEPT");
    let venue_order_id = VenueOrderId::from("V-EXT-LATE-ACCEPT");

    ctx.add_instrument(test_instrument());

    let mut report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("2.000"),
        Quantity::from("2.000"),
    )
    .with_avg_px(dec!(3000.00));

    // Venues without an acceptance time report the reconciliation time instead
    report.ts_accepted = UnixNanos::from(3_000_000);
    report.ts_last = UnixNanos::from(3_000_000);

    let fill1 = create_fill_report(
        client_order_id,
        venue_order_id,
        instrument_id,
        TradeId::from("T-LATE-001"),
        "1.000",
    );
    let mut fill2 = create_fill_report(
        client_order_id,
        venue_order_id,
        instrument_id,
        TradeId::from("T-LATE-002"),
        "1.000",
    );
    fill2.ts_event = UnixNanos::from(2_000_000);
    let mass_status = create_mass_status(vec![report], vec![fill1, fill2]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let sequence: Vec<(&str, Option<TradeId>, UnixNanos)> = result
        .events
        .iter()
        .map(|event| match event {
            OrderEventAny::Accepted(accepted) => ("accepted", None, accepted.ts_event),
            OrderEventAny::Filled(filled) => ("filled", Some(filled.trade_id), filled.ts_event),
            _ => ("other", None, event.ts_event()),
        })
        .collect();

    assert_eq!(
        sequence,
        vec![
            ("accepted", None, UnixNanos::from(3_000_000)),
            (
                "filled",
                Some(TradeId::from("T-LATE-001")),
                UnixNanos::from(1_000_000),
            ),
            (
                "filled",
                Some(TradeId::from("T-LATE-002")),
                UnixNanos::from(2_000_000),
            ),
        ]
    );

    let cache = ctx.cache.borrow();
    let orders = cache.orders(None, None, None, None, None);
    assert_eq!(orders.len(), 1);
    let order = &orders[0];
    assert_eq!(order.status(), OrderStatus::Filled);
    assert!(!order.is_open());
    assert_eq!(order.filled_qty(), Quantity::from("2.000"));
    assert_eq!(
        order.trade_ids(),
        vec![&TradeId::from("T-LATE-001"), &TradeId::from("T-LATE-002")]
    );
    assert!(cache.orders_open(None, None, None, None, None).is_empty());
    let positions = cache.positions_open(None, None, None, None, None);
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].quantity, Quantity::from("2.000"));
}

#[tokio::test]
async fn test_external_order_replaced_leg_fills_do_not_double_count() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-EXT-REPLACED");
    let old_venue_order_id = VenueOrderId::from("V-EXT-OLD");
    let new_venue_order_id = VenueOrderId::from("V-EXT-NEW");

    ctx.add_instrument(test_instrument());

    // The successor's report carries the replaced leg's filled quantity
    let mut report = create_order_report(
        Some(client_order_id),
        new_venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.000"),
        Quantity::from("5.000"),
    );
    report.ts_accepted = UnixNanos::from(2_000_000);
    report.ts_last = UnixNanos::from(3_000_000);

    let old_fill = create_fill_report(
        client_order_id,
        old_venue_order_id,
        instrument_id,
        TradeId::from("T-OLD"),
        "2.000",
    );
    let mut new_fill = create_fill_report(
        client_order_id,
        new_venue_order_id,
        instrument_id,
        TradeId::from("T-NEW"),
        "3.000",
    );
    new_fill.ts_event = UnixNanos::from(3_000_000);
    let mass_status = create_mass_status(vec![report], vec![old_fill, new_fill]);

    ctx.manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let cache = ctx.cache.borrow();
    let order = cache.order(&client_order_id).unwrap();
    assert_eq!(order.status(), OrderStatus::PartiallyFilled);
    assert_eq!(order.filled_qty(), Quantity::from("5.000"));
    assert_eq!(order.trade_ids().len(), 2);
    assert_eq!(order.trade_ids()[0], &TradeId::from("T-NEW"));
    assert!(!order.trade_ids().contains(&&TradeId::from("T-OLD")));
    let positions = cache.positions_open(None, None, None, None, None);
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].quantity, Quantity::from("5.000"));
}

#[tokio::test]
async fn test_external_order_filled_with_partial_fills_generates_inferred() {
    // Test that external filled orders with incomplete fill reports
    // still get an inferred fill for the remaining quantity
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let venue_order_id = VenueOrderId::from("V-EXT-PARTIAL-INFER");

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Order is fully filled (3.0 of 3.0) according to report
    // Use precision 3 to match instrument size precision
    let report = create_order_report(
        None,
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("3.000"),
        Quantity::from("3.000"),
    )
    .with_avg_px(dec!(3000.00));
    mass_status.add_order_reports(vec![report]);

    // But we only have fill reports for 2.0 (missing 1.0)
    // Use precision 3 to match instrument size precision
    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-PARTIAL-001"),
        OrderSide::Buy,
        Quantity::from("2.000"),
        Price::from("2999.00"),
        Money::from("1.00 USDT"),
        LiquiditySide::Maker,
        None,
        None,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );
    mass_status.add_fill_reports(vec![fill]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should have: Accepted, RealFill (2.0), InferredFill (1.0)
    assert_eq!(result.events.len(), 3);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));
    assert!(matches!(result.events[1], OrderEventAny::Filled(_)));
    assert!(matches!(result.events[2], OrderEventAny::Filled(_)));

    // First fill is real
    if let OrderEventAny::Filled(filled1) = &result.events[1] {
        assert_eq!(filled1.trade_id, TradeId::from("T-PARTIAL-001"));
        assert_eq!(filled1.last_qty, Quantity::from("2.000"));
    }

    // Second fill is inferred (has UUID format trade_id, not the known fill report trade ID)
    if let OrderEventAny::Filled(filled2) = &result.events[2] {
        assert_ne!(
            filled2.trade_id.as_str(),
            "T-PARTIAL-001",
            "Expected inferred trade ID (UUID), was known fill report trade ID"
        );
        assert_eq!(filled2.trade_id.as_str().len(), 36);
        assert_eq!(filled2.last_qty, Quantity::from("1.000"));
    }

    // Verify order is fully filled
    let cache = ctx.cache.borrow();
    let orders = cache.orders(None, None, None, None, None);
    assert_eq!(orders.len(), 1);
    let order = &orders[0];
    assert_eq!(order.filled_qty(), Quantity::from("3.000"));
}

#[rstest]
#[case::venue_id(None)]
#[case::client_id(Some(ClientOrderId::from("O-UNCLAIMED")))]
#[tokio::test]
async fn test_reconcile_mass_status_skips_external_when_filtered(
    #[case] client_order_id: Option<ClientOrderId>,
) {
    let config = ExecutionManagerConfig {
        filter_unclaimed_external: true,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        client_order_id,
        VenueOrderId::from("V-EXT-001"),
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert!(result.events.is_empty());
    assert!(
        ctx.cache
            .borrow()
            .orders(None, None, None, None, None)
            .is_empty()
    );
}

#[tokio::test]
async fn test_synthetic_orders_bypass_filter_unclaimed_external() {
    let config = ExecutionManagerConfig {
        filter_unclaimed_external: true,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // S- prefix indicates synthetic order, should bypass filter_unclaimed_external
    let report = create_order_report(
        None,
        VenueOrderId::from("S-abc123-def456"),
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("1.0"),
        Quantity::from("1.0"),
    )
    .with_avg_px(dec!(100.0));
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert!(!result.events.is_empty());
    assert!(
        result
            .events
            .iter()
            .any(|e| matches!(e, OrderEventAny::Filled(_)))
    );

    if let OrderEventAny::Accepted(accepted) = &result.events[0] {
        let order = ctx
            .get_order(&accepted.client_order_id)
            .expect("Order should exist");
        let tags = order.tags().expect("Order should have tags");
        assert!(
            tags.contains(&ustr::Ustr::from("RECONCILIATION")),
            "Synthetic order should have RECONCILIATION tag, was {tags:?}",
        );
    } else {
        panic!("Expected Accepted event first, was {:?}", result.events[0]);
    }

    let client_order_id = result.events[0].client_order_id();
    assert_eq!(ctx.cache.borrow().client_id(&client_order_id), None);
}

#[rstest]
#[case::unfiltered_venue_id(false, None)]
#[case::filtered_venue_id(true, None)]
#[case::unfiltered_client_id(false, Some(ClientOrderId::from("O-CLAIMED")))]
#[case::filtered_client_id(true, Some(ClientOrderId::from("O-CLAIMED")))]
#[tokio::test]
async fn test_reconcile_mass_status_uses_claimed_strategy(
    #[case] filter_unclaimed_external: bool,
    #[case] client_order_id: Option<ClientOrderId>,
) {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        filter_unclaimed_external,
        ..Default::default()
    });

    let instrument_id = test_instrument_id();
    let strategy_id = StrategyId::from("MY-STRATEGY");

    ctx.add_instrument(test_instrument());
    ctx.manager
        .claim_external_orders(instrument_id, strategy_id)
        .unwrap();

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        client_order_id,
        VenueOrderId::from("V-EXT-001"),
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 1);

    let client_order_id = client_order_id.unwrap_or_else(|| ClientOrderId::from("V-EXT-001"));
    let order = ctx.get_order(&client_order_id).unwrap();
    assert_eq!(order.strategy_id(), strategy_id);
    assert_eq!(order.status(), OrderStatus::Accepted);
    assert_eq!(order.quantity(), Quantity::from("1.0"));
    assert_eq!(order.tags(), None);
}

#[rstest]
#[case::empty(false)]
#[case::partial(true)]
#[tokio::test]
async fn test_claimed_terminal_order_defers_unexplained_fills(#[case] partial: bool) {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        filter_unclaimed_external: true,
        ..Default::default()
    });

    let instrument = test_instrument();
    let strategy_id = StrategyId::from("CLAIMED-001");
    let client_order_id = ClientOrderId::from("O-CLAIMED-TERMINAL");
    let venue_order_id = VenueOrderId::from("V-CLAIMED-TERMINAL");
    ctx.add_instrument(instrument.clone());
    ctx.manager
        .claim_external_orders(instrument.id(), strategy_id)
        .unwrap();
    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument.id(),
        OrderStatus::Canceled,
        Quantity::from("10.0"),
        Quantity::from("2.0"),
    )
    .with_avg_px(dec!(3000.0));
    let first = create_fill_report(
        client_order_id,
        venue_order_id,
        instrument.id(),
        TradeId::from("T-CLAIMED-1"),
        "1.0",
    );
    let second = create_fill_report(
        client_order_id,
        venue_order_id,
        instrument.id(),
        TradeId::from("T-CLAIMED-2"),
        "1.0",
    );
    ctx.manager.reconcile_execution_mass_status(
        &create_mass_status(
            vec![report.clone()],
            if partial {
                vec![first.clone()]
            } else {
                Vec::new()
            },
        ),
        &ctx.exec_engine,
    );

    let deferred = ctx.get_order(&client_order_id);

    ctx.manager.reconcile_execution_mass_status(
        &create_mass_status(vec![report], vec![first.clone(), second.clone()]),
        &ctx.exec_engine,
    );
    ctx.exec_engine.borrow_mut().reconcile_fill_report(&first);
    ctx.exec_engine.borrow_mut().reconcile_fill_report(&second);
    let recovered = ctx.get_order(&client_order_id).unwrap();
    let cache = ctx.cache.borrow();
    let position = cache
        .position(cache.position_id(&client_order_id).unwrap())
        .unwrap();

    if partial {
        let deferred = deferred.unwrap();
        assert_eq!(deferred.status(), OrderStatus::PartiallyFilled);
        assert_eq!(deferred.filled_qty(), Quantity::from("1.0"));
        assert_eq!(deferred.strategy_id(), strategy_id);
    } else {
        assert!(deferred.is_none());
    }

    assert_eq!(recovered.status(), OrderStatus::Canceled);
    assert_eq!(recovered.filled_qty(), Quantity::from("2.0"));
    assert_eq!(recovered.strategy_id(), strategy_id);
    assert_eq!(
        recovered.trade_ids(),
        vec![&first.trade_id, &second.trade_id]
    );
    assert_eq!(
        recovered.commissions().get(&Currency::USDT()),
        Some(&Money::from("1.00 USDT"))
    );
    assert_eq!(position.quantity, Quantity::from("2.0"));
    assert_eq!(position.strategy_id, strategy_id);
}

#[tokio::test]
async fn test_claim_external_orders_duplicate_fails_without_overwriting() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let strategy_id = StrategyId::from("MY-STRATEGY");
    let duplicate_strategy_id = StrategyId::from("OTHER-STRATEGY");

    ctx.add_instrument(test_instrument());
    ctx.manager
        .claim_external_orders(instrument_id, strategy_id)
        .unwrap();

    let result = ctx
        .manager
        .claim_external_orders(instrument_id, duplicate_strategy_id);

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("already exists for MY-STRATEGY")
    );

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );
    let report = create_order_report(
        None,
        VenueOrderId::from("V-EXT-DUPLICATE"),
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 1);

    let client_order_id = ClientOrderId::from("V-EXT-DUPLICATE");
    let order = ctx.get_order(&client_order_id).unwrap();
    assert_eq!(order.strategy_id(), strategy_id);
}

#[tokio::test]
async fn test_reconcile_mass_status_processes_fills_for_cached_order() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");

    ctx.add_instrument(test_instrument());
    let order = create_limit_order("O-001", instrument_id, OrderSide::Buy, "2.0", "3000.00");
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-001"),
        OrderSide::Buy,
        Quantity::from("1.0"),
        Price::from("3000.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        Some(client_order_id),
        None,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );
    mass_status.add_fill_reports(vec![fill]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Filled(_)));
}

#[tokio::test]
async fn test_reconcile_mass_status_deduplicates_fills() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let trade_id = TradeId::from("T-001");

    ctx.add_instrument(test_instrument());
    let order = create_limit_order("O-001", instrument_id, OrderSide::Buy, "2.0", "3000.00");
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add same fill twice
    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        trade_id,
        OrderSide::Buy,
        Quantity::from("1.0"),
        Price::from("3000.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        Some(client_order_id),
        None,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );
    mass_status.add_fill_reports(vec![fill.clone(), fill]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Only one fill should be processed
    assert_eq!(result.events.len(), 1);
}

#[rstest]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_canonical_duplicate_reconciliation_fill_is_committed() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-DUPLICATE-COMMIT");
    let venue_order_id = VenueOrderId::from("V-DUPLICATE-COMMIT");
    let retry_client_order_id = ClientOrderId::from("O-DUPLICATE-COMMIT-2");
    let retry_venue_order_id = VenueOrderId::from("V-DUPLICATE-COMMIT-2");
    let trade_id = TradeId::from("T-DUPLICATE-COMMIT");

    let instrument = test_instrument();
    ctx.add_instrument(instrument.clone());
    let mut order = create_accepted_order(
        client_order_id.as_str(),
        instrument_id,
        OrderSide::Buy,
        "2.0",
        "3000.00",
        venue_order_id,
    );
    let existing_fill = TestOrderEventStubs::filled(
        &order,
        &instrument,
        Some(trade_id),
        None,
        Some(Price::from("3000.00")),
        Some(Quantity::from("1.0")),
        Some(LiquiditySide::Maker),
        None,
        None,
        Some(test_account_id()),
    );
    order.apply(existing_fill).unwrap();
    ctx.add_order(order);

    let duplicate_report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("2.0"),
        Quantity::from("2.0"),
    );
    let duplicate_fill = create_fill_report(
        client_order_id,
        venue_order_id,
        instrument_id,
        trade_id,
        "1.0",
    );
    let duplicate = ctx.manager.reconcile_execution_mass_status(
        &create_mass_status(vec![duplicate_report], vec![duplicate_fill]),
        &ctx.exec_engine,
    );

    let [OrderEventAny::Filled(residual)] = duplicate.events.as_slice() else {
        panic!(
            "Expected one residual fill, received {:?}",
            duplicate.events
        );
    };

    assert_eq!(residual.client_order_id, client_order_id);
    assert_eq!(residual.last_qty, Quantity::from("1.0"));
    assert_ne!(residual.trade_id, trade_id);
    let reconciled = ctx.get_order(&client_order_id).unwrap();
    assert_eq!(reconciled.status(), OrderStatus::Filled);
    assert_eq!(reconciled.filled_qty(), Quantity::from("2.0"));

    ctx.add_order(create_accepted_order(
        retry_client_order_id.as_str(),
        instrument_id,
        OrderSide::Buy,
        "1.0",
        "3000.00",
        retry_venue_order_id,
    ));
    let retry_fill = create_fill_report(
        retry_client_order_id,
        retry_venue_order_id,
        instrument_id,
        trade_id,
        "1.0",
    );
    let retry = ctx.manager.reconcile_execution_mass_status(
        &create_mass_status(vec![], vec![retry_fill]),
        &ctx.exec_engine,
    );

    assert!(retry.events.is_empty());
    assert!(
        ctx.get_order(&retry_client_order_id)
            .unwrap()
            .trade_ids()
            .is_empty()
    );
}

#[rstest]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_rejected_reconciliation_fill_is_retried() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-REJECT-RETRY");
    let venue_order_id = VenueOrderId::from("V-REJECT-RETRY");
    let retry_client_order_id = ClientOrderId::from("O-REJECT-RETRY-2");
    let retry_venue_order_id = VenueOrderId::from("V-REJECT-RETRY-2");
    let existing_trade_id = TradeId::from("T-REJECT-EXISTING");
    let trade_id = TradeId::from("T-REJECT-RETRY");

    let instrument = test_instrument();
    ctx.add_instrument(instrument.clone());
    let mut order = create_accepted_order(
        client_order_id.as_str(),
        instrument_id,
        OrderSide::Buy,
        "1.0",
        "3000.00",
        venue_order_id,
    );
    let existing_fill = TestOrderEventStubs::filled(
        &order,
        &instrument,
        Some(existing_trade_id),
        None,
        Some(Price::from("3000.00")),
        Some(Quantity::from("1.0")),
        Some(LiquiditySide::Maker),
        None,
        None,
        Some(test_account_id()),
    );
    order.apply(existing_fill).unwrap();
    ctx.add_order(order);

    let rejected_report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("2.0"),
        Quantity::from("2.0"),
    );
    let rejected_fill = create_fill_report(
        client_order_id,
        venue_order_id,
        instrument_id,
        trade_id,
        "1.0",
    );
    let rejected = ctx.manager.reconcile_execution_mass_status(
        &create_mass_status(vec![rejected_report], vec![rejected_fill]),
        &ctx.exec_engine,
    );

    assert!(rejected.events.is_empty());
    let rejected_order = ctx.get_order(&client_order_id).unwrap();
    assert_eq!(rejected_order.trade_ids(), vec![&existing_trade_id]);

    ctx.add_order(create_accepted_order(
        retry_client_order_id.as_str(),
        instrument_id,
        OrderSide::Buy,
        "1.0",
        "3000.00",
        retry_venue_order_id,
    ));
    let retry_fill = create_fill_report(
        retry_client_order_id,
        retry_venue_order_id,
        instrument_id,
        trade_id,
        "1.0",
    );
    let retry = ctx.manager.reconcile_execution_mass_status(
        &create_mass_status(vec![], vec![retry_fill]),
        &ctx.exec_engine,
    );

    assert_eq!(retry.events.len(), 1);
    assert!(matches!(retry.events[0], OrderEventAny::Filled(_)));
    let retry_order = ctx.get_order(&retry_client_order_id).unwrap();
    assert_eq!(retry_order.trade_ids(), vec![&trade_id]);
}

#[rstest]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_reconciliation_fill_dispatch_rejection_does_not_commit() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-DISPATCH-RETRY");
    let venue_order_id = VenueOrderId::from("V-DISPATCH-RETRY");
    let trade_id = TradeId::from("T-DISPATCH-RETRY");

    ctx.add_instrument(test_instrument());
    ctx.add_order(create_limit_order(
        client_order_id.as_str(),
        instrument_id,
        OrderSide::Buy,
        "1.0",
        "3000.00",
    ));

    let fill = create_fill_report(
        client_order_id,
        venue_order_id,
        instrument_id,
        trade_id,
        "1.0",
    );
    // An orphan report queues without the order-report working.apply projection.
    let rejected = ctx.manager.reconcile_execution_mass_status(
        &create_mass_status(vec![], vec![fill.clone()]),
        &ctx.exec_engine,
    );

    assert_eq!(
        rejected
            .events
            .iter()
            .filter(|event| matches!(event, OrderEventAny::Filled(_)))
            .count(),
        1
    );
    assert!(
        !ctx.get_order(&client_order_id)
            .unwrap()
            .trade_ids()
            .contains(&&trade_id)
    );

    let order = ctx.get_order(&client_order_id).unwrap();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    let order = ctx.cache.borrow_mut().update_order(&submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    ctx.cache.borrow_mut().update_order(&accepted).unwrap();

    let retry = ctx
        .manager
        .reconcile_execution_mass_status(&create_mass_status(vec![], vec![fill]), &ctx.exec_engine);

    assert_eq!(
        retry
            .events
            .iter()
            .filter(|event| matches!(event, OrderEventAny::Filled(_)))
            .count(),
        1
    );
    assert!(
        ctx.get_order(&client_order_id)
            .unwrap()
            .trade_ids()
            .contains(&&trade_id)
    );
}

#[rstest]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_applied_reconciliation_fill_is_committed() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let trade_id = TradeId::from("T-COMMITTED");
    let first_client_order_id = ClientOrderId::from("O-COMMITTED-1");
    let first_venue_order_id = VenueOrderId::from("V-COMMITTED-1");
    let second_client_order_id = ClientOrderId::from("O-COMMITTED-2");
    let second_venue_order_id = VenueOrderId::from("V-COMMITTED-2");

    ctx.add_instrument(test_instrument());
    ctx.add_order(create_accepted_order(
        first_client_order_id.as_str(),
        instrument_id,
        OrderSide::Buy,
        "1.0",
        "3000.00",
        first_venue_order_id,
    ));
    ctx.add_order(create_accepted_order(
        second_client_order_id.as_str(),
        instrument_id,
        OrderSide::Buy,
        "1.0",
        "3000.00",
        second_venue_order_id,
    ));

    let first = ctx.manager.reconcile_execution_mass_status(
        &create_mass_status(
            vec![],
            vec![create_fill_report(
                first_client_order_id,
                first_venue_order_id,
                instrument_id,
                trade_id,
                "1.0",
            )],
        ),
        &ctx.exec_engine,
    );
    assert_eq!(first.events.len(), 1);

    let duplicate = ctx.manager.reconcile_execution_mass_status(
        &create_mass_status(
            vec![],
            vec![create_fill_report(
                second_client_order_id,
                second_venue_order_id,
                instrument_id,
                trade_id,
                "1.0",
            )],
        ),
        &ctx.exec_engine,
    );

    assert!(duplicate.events.is_empty());
    assert!(
        ctx.get_order(&second_client_order_id)
            .unwrap()
            .trade_ids()
            .is_empty()
    );
}

#[rstest]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_same_cycle_cross_order_fill_is_queued_once() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let trade_id = TradeId::from("T-CROSS-ORDER");
    let first_client_order_id = ClientOrderId::from("O-CROSS-ORDER-1");
    let first_venue_order_id = VenueOrderId::from("V-CROSS-ORDER-1");
    let second_client_order_id = ClientOrderId::from("O-CROSS-ORDER-2");
    let second_venue_order_id = VenueOrderId::from("V-CROSS-ORDER-2");

    ctx.add_instrument(test_instrument());
    ctx.add_order(create_accepted_order(
        first_client_order_id.as_str(),
        instrument_id,
        OrderSide::Buy,
        "1.0",
        "3000.00",
        first_venue_order_id,
    ));
    ctx.add_order(create_accepted_order(
        second_client_order_id.as_str(),
        instrument_id,
        OrderSide::Buy,
        "1.0",
        "3000.00",
        second_venue_order_id,
    ));

    let result = ctx.manager.reconcile_execution_mass_status(
        &create_mass_status(
            vec![],
            vec![
                create_fill_report(
                    first_client_order_id,
                    first_venue_order_id,
                    instrument_id,
                    trade_id,
                    "1.0",
                ),
                create_fill_report(
                    second_client_order_id,
                    second_venue_order_id,
                    instrument_id,
                    trade_id,
                    "1.0",
                ),
            ],
        ),
        &ctx.exec_engine,
    );

    assert_eq!(
        result
            .events
            .iter()
            .filter(|event| matches!(event, OrderEventAny::Filled(_)))
            .count(),
        1
    );

    let applied_orders = [first_client_order_id, second_client_order_id]
        .iter()
        .filter(|client_order_id| {
            ctx.get_order(client_order_id)
                .unwrap()
                .trade_ids()
                .contains(&&trade_id)
        })
        .count();

    assert_eq!(applied_orders, 1);
}

#[rstest]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_inferred_fill_is_not_committed_as_reported_fill() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let external_venue_order_id = VenueOrderId::from("V-INFERRED-SOURCE");
    ctx.add_instrument(test_instrument());

    let order_report = create_order_report(
        None,
        external_venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("2.0"),
        Quantity::from("2.0"),
    )
    .with_avg_px(dec!(3000.0));
    let real_trade_id = TradeId::from("T-INFERRED-SOURCE");
    let source = ctx.manager.reconcile_execution_mass_status(
        &create_mass_status(
            vec![order_report],
            vec![create_fill_report(
                ClientOrderId::from(external_venue_order_id.as_str()),
                external_venue_order_id,
                instrument_id,
                real_trade_id,
                "1.0",
            )],
        ),
        &ctx.exec_engine,
    );

    let inferred_trade_id = source
        .events
        .iter()
        .find_map(|event| match event {
            OrderEventAny::Filled(fill) if fill.trade_id != real_trade_id => Some(fill.trade_id),
            _ => None,
        })
        .expect("expected inferred fill");

    let client_order_id = ClientOrderId::from("O-INFERRED-REUSE");
    let venue_order_id = VenueOrderId::from("V-INFERRED-REUSE");
    ctx.add_order(create_accepted_order(
        client_order_id.as_str(),
        instrument_id,
        OrderSide::Buy,
        "1.0",
        "3000.00",
        venue_order_id,
    ));
    let reused = ctx.manager.reconcile_execution_mass_status(
        &create_mass_status(
            vec![],
            vec![create_fill_report(
                client_order_id,
                venue_order_id,
                instrument_id,
                inferred_trade_id,
                "1.0",
            )],
        ),
        &ctx.exec_engine,
    );

    assert_eq!(reused.events.len(), 1);
    assert!(matches!(reused.events[0], OrderEventAny::Filled(_)));
}

#[rstest]
#[case(Some(0), 60, true)]
#[case(Some(2), 120, true)]
#[case(None, 86_400, false)]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_processed_fill_retention(
    #[case] lookback_mins: Option<u64>,
    #[case] horizon_secs: u64,
    #[case] prunes_past_horizon: bool,
) {
    let config = ExecutionManagerConfig {
        lookback_mins,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    let trade_id = TradeId::from("T-RETENTION");
    let first_client_order_id = ClientOrderId::from("O-RETENTION-1");
    let first_venue_order_id = VenueOrderId::from("V-RETENTION-1");
    let second_client_order_id = ClientOrderId::from("O-RETENTION-2");
    let second_venue_order_id = VenueOrderId::from("V-RETENTION-2");

    ctx.add_instrument(test_instrument());
    ctx.add_order(create_accepted_order(
        first_client_order_id.as_str(),
        instrument_id,
        OrderSide::Buy,
        "1.0",
        "3000.00",
        first_venue_order_id,
    ));
    ctx.add_order(create_accepted_order(
        second_client_order_id.as_str(),
        instrument_id,
        OrderSide::Buy,
        "1.0",
        "3000.00",
        second_venue_order_id,
    ));

    let first = ctx.manager.reconcile_execution_mass_status(
        &create_mass_status(
            vec![],
            vec![create_fill_report(
                first_client_order_id,
                first_venue_order_id,
                instrument_id,
                trade_id,
                "1.0",
            )],
        ),
        &ctx.exec_engine,
    );
    assert_eq!(first.events.len(), 1);

    ctx.advance_both(dst::time::Duration::from_secs(horizon_secs))
        .await;
    ctx.manager.prune_processed_fills();
    let at_horizon = ctx.manager.reconcile_execution_mass_status(
        &create_mass_status(
            vec![],
            vec![create_fill_report(
                second_client_order_id,
                second_venue_order_id,
                instrument_id,
                trade_id,
                "1.0",
            )],
        ),
        &ctx.exec_engine,
    );
    assert!(at_horizon.events.is_empty());

    ctx.advance_both(dst::time::Duration::from_nanos(1)).await;
    ctx.manager.prune_processed_fills();
    let past_horizon = ctx.manager.reconcile_execution_mass_status(
        &create_mass_status(
            vec![],
            vec![create_fill_report(
                second_client_order_id,
                second_venue_order_id,
                instrument_id,
                trade_id,
                "1.0",
            )],
        ),
        &ctx.exec_engine,
    );
    assert_eq!(past_horizon.events.len(), usize::from(prunes_past_horizon));
}

#[rstest]
#[case(OmsType::Netting, true, true, true)]
#[case(OmsType::Netting, true, false, true)]
#[case(OmsType::Netting, false, true, true)]
#[case(OmsType::Netting, false, false, true)]
#[case(OmsType::Hedging, true, true, true)]
#[case(OmsType::Hedging, true, false, true)]
#[case(OmsType::Hedging, false, true, true)]
#[case(OmsType::Hedging, false, false, true)]
#[case(OmsType::Netting, true, false, false)]
#[case(OmsType::Netting, false, false, false)]
#[case(OmsType::Hedging, true, false, false)]
#[case(OmsType::Hedging, false, false, false)]
#[tokio::test]
async fn test_retained_fill_projects_missing_order_without_reapplying(
    #[case] oms_type: OmsType,
    #[case] generate_missing_orders: bool,
    #[case] include_fill_report: bool,
    #[case] include_client_order_id: bool,
) {
    let config = ExecutionManagerConfig {
        generate_missing_orders,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let strategy_id = StrategyId::from("STRATEGY-001");
    let client_order_id = ClientOrderId::from("O-RETAINED-001");
    let venue_order_id = VenueOrderId::from("V-RETAINED-001");
    let trade_id = TradeId::from("T-RETAINED-001");

    let position_id = match oms_type {
        OmsType::Hedging => PositionId::from("P-RETAINED-001"),
        _ => PositionId::new(format!("{instrument_id}-{strategy_id}")),
    };

    ctx.add_instrument(instrument.clone());
    ctx.manager
        .claim_external_orders(instrument_id, strategy_id)
        .unwrap();
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(strategy_id, oms_type);

    let mut restored_order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .strategy_id(strategy_id)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .price(Price::from("3000.00"))
        .build();
    apply_submitted_and_accepted(&mut restored_order, venue_order_id);
    let restored_fill = TestOrderEventStubs::filled(
        &restored_order,
        &instrument,
        Some(trade_id),
        Some(position_id),
        Some(Price::from("3000.00")),
        Some(Quantity::from("1.000")),
        Some(LiquiditySide::Maker),
        Some(Money::from("0.50 USDT")),
        Some(UnixNanos::from(1_000_000)),
        Some(test_account_id()),
    );
    let position = Position::new(&instrument, restored_fill.into());
    ctx.cache
        .borrow_mut()
        .add_position(&position, oms_type)
        .unwrap();

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );
    mass_status.set_report_window(Some(UnixNanos::from(2_000_000)), true);
    let mut order_report = create_order_report(
        include_client_order_id.then_some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("1.000"),
        Quantity::from("1.000"),
    )
    .with_avg_px(dec!(3000.0));
    let venue_position_id = (oms_type == OmsType::Hedging).then_some(position_id);
    if let Some(position_id) = venue_position_id {
        order_report = order_report.with_venue_position_id(position_id);
    }

    let fill_report = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        trade_id,
        OrderSide::Buy,
        Quantity::from("1.000"),
        Price::from("3000.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        Some(client_order_id),
        venue_position_id,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );

    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("1.000"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        venue_position_id,
        Some(dec!(3000.00)),
    );
    mass_status.add_order_reports(vec![order_report]);

    if include_fill_report {
        mass_status.add_fill_reports(vec![fill_report]);
    }

    mass_status.add_position_reports(vec![position_report]);

    ctx.manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let cache = ctx.cache.borrow();

    let reconciled_order_id = if include_client_order_id {
        client_order_id
    } else {
        ClientOrderId::from(venue_order_id.as_str())
    };

    let order = cache.order(&reconciled_order_id).unwrap();
    let position = cache.position(&position_id).unwrap();

    assert_eq!(cache.oms_type(&position_id), Some(oms_type));
    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.filled_qty(), Quantity::from("1.000"));
    assert_eq!(position.quantity, Quantity::from("1.000"));
    assert_eq!(position.realized_pnl, Some(Money::from("-0.50 USDT")));
    assert_eq!(position.commissions(), vec![Money::from("0.50 USDT")]);
    assert_eq!(position.trade_ids.len(), 1);
    assert!(position.trade_ids.contains(&trade_id));
}

#[rstest]
#[case::unbounded_missing_orders(true, false)]
#[case::unbounded_no_missing_orders(false, false)]
#[case::bounded_missing_orders(true, true)]
#[case::bounded_no_missing_orders(false, true)]
#[tokio::test]
async fn test_inferred_delta_for_retained_order_applies_new_economics(
    #[case] generate_missing_orders: bool,
    #[case] bounded: bool,
) {
    let config = ExecutionManagerConfig {
        generate_missing_orders,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let strategy_id = StrategyId::from("STRATEGY-001");
    let client_order_id = ClientOrderId::from("O-RETAINED-PARTIAL");
    let venue_order_id = VenueOrderId::from("V-RETAINED-PARTIAL");
    let known_trade_id = TradeId::from("T-RETAINED-PARTIAL");
    let position_id = PositionId::new(format!("{instrument_id}-{strategy_id}"));

    ctx.add_instrument(instrument.clone());
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(strategy_id, OmsType::Netting);

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .strategy_id(strategy_id)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("10.000"))
        .price(Price::from("3000.00"))
        .build();
    apply_submitted_and_accepted(&mut order, venue_order_id);
    let known_fill = TestOrderEventStubs::filled(
        &order,
        &instrument,
        Some(known_trade_id),
        Some(position_id),
        Some(Price::from("3000.00")),
        Some(Quantity::from("5.000")),
        Some(LiquiditySide::Maker),
        Some(Money::from("0.50 USDT")),
        Some(UnixNanos::from(1_000_000)),
        Some(test_account_id()),
    );
    order.apply(known_fill.clone()).unwrap();
    let position = Position::new(&instrument, known_fill.into());
    ctx.add_order(order);
    ctx.cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );
    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("10.000"),
        Quantity::from("10.000"),
    )
    .with_avg_px(dec!(3000.0));

    if bounded {
        mass_status.set_report_window(Some(UnixNanos::from(500_000)), true);
        mass_status.add_position_reports(vec![PositionStatusReport::new(
            test_account_id(),
            instrument_id,
            PositionSide::Long,
            Quantity::from("10.000"),
            UnixNanos::from(2_000_000),
            UnixNanos::from(2_000_000),
            None,
            None,
            Some(dec!(3000.00)),
        )]);
    }

    mass_status.add_order_reports(vec![report]);

    ctx.manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let cache = ctx.cache.borrow();
    let order = cache.order(&client_order_id).unwrap();
    let position = cache.position(&position_id).unwrap();

    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.filled_qty(), Quantity::from("10.000"));
    assert_eq!(position.quantity, Quantity::from("10.000"));
    assert_eq!(position.realized_pnl, Some(Money::from("-0.50 USDT")));
    assert_eq!(position.commissions(), vec![Money::from("0.50 USDT")]);
    assert_eq!(position.trade_ids.len(), 2);
    assert!(position.trade_ids.contains(&known_trade_id));
}

#[tokio::test]
async fn test_missing_venue_order_id_collision_is_scoped_by_instrument() {
    let mut ctx = TestContext::new();
    let retained_instrument = test_instrument();
    let retained_instrument_id = retained_instrument.id();
    let new_instrument = test_instrument2();
    let new_instrument_id = new_instrument.id();
    let strategy_id = StrategyId::from("STRATEGY-001");
    let venue_order_id = VenueOrderId::from("V-COLLISION");
    let retained_position_id = PositionId::new(format!("{retained_instrument_id}-{strategy_id}"));

    ctx.add_instrument(retained_instrument.clone());
    ctx.add_instrument(new_instrument);
    ctx.manager
        .claim_external_orders(new_instrument_id, strategy_id)
        .unwrap();
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(strategy_id, OmsType::Netting);

    let mut retained_order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(ClientOrderId::from("O-RETAINED-COLLISION"))
        .strategy_id(strategy_id)
        .instrument_id(retained_instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .price(Price::from("3000.00"))
        .build();
    apply_submitted_and_accepted(&mut retained_order, venue_order_id);
    let retained_fill = TestOrderEventStubs::filled(
        &retained_order,
        &retained_instrument,
        Some(TradeId::from("T-RETAINED-COLLISION")),
        Some(retained_position_id),
        Some(Price::from("3000.00")),
        Some(Quantity::from("1.000")),
        Some(LiquiditySide::Maker),
        Some(Money::from("0.50 USDT")),
        Some(UnixNanos::from(1_000_000)),
        Some(test_account_id()),
    );
    let retained_position = Position::new(&retained_instrument, retained_fill.into());
    ctx.cache
        .borrow_mut()
        .add_position(&retained_position, OmsType::Netting)
        .unwrap();

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );
    let report = create_order_report(
        None,
        venue_order_id,
        new_instrument_id,
        OrderStatus::Filled,
        Quantity::from("1.000"),
        Quantity::from("1.000"),
    )
    .with_avg_px(dec!(3000.0));
    mass_status.add_order_reports(vec![report]);

    ctx.manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let new_position_id = PositionId::new(format!("{new_instrument_id}-{strategy_id}"));
    let cache = ctx.cache.borrow();
    let order = cache
        .order(&ClientOrderId::from(venue_order_id.as_str()))
        .unwrap();
    let position = cache.position(&new_position_id).unwrap();

    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(position.quantity, Quantity::from("1.000"));
    assert_eq!(position.trade_ids.len(), 1);
}

#[rstest]
#[case::unbounded_missing_orders(true, false)]
#[case::unbounded_no_missing_orders(false, false)]
#[case::bounded_missing_orders(true, true)]
#[case::bounded_no_missing_orders(false, true)]
#[tokio::test]
async fn test_partially_known_fills_apply_only_new_economics(
    #[case] generate_missing_orders: bool,
    #[case] bounded: bool,
) {
    let config = ExecutionManagerConfig {
        generate_missing_orders,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let strategy_id = StrategyId::from("STRATEGY-001");
    let client_order_id = ClientOrderId::from("O-RETAINED-002");
    let venue_order_id = VenueOrderId::from("V-RETAINED-002");
    let known_trade_id = TradeId::from("T-RETAINED-002-A");
    let new_trade_id = TradeId::from("T-RETAINED-002-B");
    let position_id = PositionId::new(format!("{instrument_id}-{strategy_id}"));

    ctx.add_instrument(instrument.clone());
    ctx.manager
        .claim_external_orders(instrument_id, strategy_id)
        .unwrap();
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(strategy_id, OmsType::Netting);

    let mut restored_order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .strategy_id(strategy_id)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("2.000"))
        .price(Price::from("3000.00"))
        .build();
    apply_submitted_and_accepted(&mut restored_order, venue_order_id);
    let known_fill = TestOrderEventStubs::filled(
        &restored_order,
        &instrument,
        Some(known_trade_id),
        Some(position_id),
        Some(Price::from("3000.00")),
        Some(Quantity::from("2.000")),
        Some(LiquiditySide::Maker),
        Some(Money::from("1.00 USDT")),
        Some(UnixNanos::from(1_000_000)),
        Some(test_account_id()),
    );
    let position = Position::new(&instrument, known_fill.into());
    ctx.cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    if bounded {
        mass_status.set_report_window(Some(UnixNanos::from(500_000)), true);
    }

    let order_report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("3.000"),
        Quantity::from("3.000"),
    )
    .with_avg_px(dec!(3033.333333));

    let known_fill_report = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        known_trade_id,
        OrderSide::Buy,
        Quantity::from("2.000"),
        Price::from("3000.00"),
        Money::from("1.00 USDT"),
        LiquiditySide::Maker,
        Some(client_order_id),
        None,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );

    let new_fill_report = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        new_trade_id,
        OrderSide::Buy,
        Quantity::from("1.000"),
        Price::from("3100.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Taker,
        Some(client_order_id),
        None,
        UnixNanos::from(2_000_000),
        UnixNanos::from(2_000_000),
        None,
    );

    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("3.000"),
        UnixNanos::from(2_000_000),
        UnixNanos::from(2_000_000),
        None,
        None,
        Some(dec!(3033.333333)),
    );
    mass_status.add_order_reports(vec![order_report]);
    mass_status.add_fill_reports(vec![known_fill_report, new_fill_report]);
    mass_status.add_position_reports(vec![position_report]);

    ctx.manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let cache = ctx.cache.borrow();
    let order = cache.order(&client_order_id).unwrap();
    let position = cache.position(&position_id).unwrap();

    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.filled_qty(), Quantity::from("3.000"));
    assert_eq!(position.quantity, Quantity::from("3.000"));
    assert_eq!(position.realized_pnl, Some(Money::from("-1.50 USDT")));
    assert_eq!(position.commissions(), vec![Money::from("1.50 USDT")]);
    assert_eq!(position.trade_ids.len(), 2);
    assert!(position.trade_ids.contains(&known_trade_id));
    assert!(position.trade_ids.contains(&new_trade_id));
}

#[rstest]
#[case(true)]
#[case(false)]
#[tokio::test]
async fn test_partial_window_known_fill_does_not_reapply_economics(
    #[case] generate_missing_orders: bool,
) {
    let config = ExecutionManagerConfig {
        generate_missing_orders,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let strategy_id = StrategyId::from("STRATEGY-001");
    let opening_order_id = ClientOrderId::from("O-PARTIAL-WINDOW-OPEN");
    let closing_order_id = ClientOrderId::from("O-PARTIAL-WINDOW-CLOSE");
    let closing_venue_order_id = VenueOrderId::from("V-PARTIAL-WINDOW-CLOSE");
    let opening_trade_id = TradeId::from("T-PARTIAL-WINDOW-OPEN");
    let closing_trade_id = TradeId::from("T-PARTIAL-WINDOW-CLOSE");
    let position_id = PositionId::new(format!("{instrument_id}-{strategy_id}"));

    ctx.add_instrument(instrument.clone());
    ctx.manager
        .claim_external_orders(instrument_id, strategy_id)
        .unwrap();
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(strategy_id, OmsType::Netting);

    let mut opening_order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(opening_order_id)
        .strategy_id(strategy_id)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("5.000"))
        .price(Price::from("3000.00"))
        .build();
    apply_submitted_and_accepted(
        &mut opening_order,
        VenueOrderId::from("V-PARTIAL-WINDOW-OPEN"),
    );
    let opening_fill = TestOrderEventStubs::filled(
        &opening_order,
        &instrument,
        Some(opening_trade_id),
        Some(position_id),
        Some(Price::from("3000.00")),
        Some(Quantity::from("5.000")),
        Some(LiquiditySide::Maker),
        Some(Money::from("2.50 USDT")),
        Some(UnixNanos::from(1_000_000)),
        Some(test_account_id()),
    );
    let mut position = Position::new(&instrument, opening_fill.into());

    let mut closing_order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(closing_order_id)
        .strategy_id(strategy_id)
        .instrument_id(instrument_id)
        .side(OrderSide::Sell)
        .quantity(Quantity::from("2.000"))
        .price(Price::from("3100.00"))
        .build();
    apply_submitted_and_accepted(&mut closing_order, closing_venue_order_id);
    let closing_fill = TestOrderEventStubs::filled(
        &closing_order,
        &instrument,
        Some(closing_trade_id),
        Some(position_id),
        Some(Price::from("3100.00")),
        Some(Quantity::from("2.000")),
        Some(LiquiditySide::Taker),
        Some(Money::from("1.00 USDT")),
        Some(UnixNanos::from(2_000_000)),
        Some(test_account_id()),
    );
    position.apply(&closing_fill.into());
    ctx.cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );
    let order_report = create_order_report_for_side(
        Some(closing_order_id),
        closing_venue_order_id,
        instrument_id,
        OrderSide::Sell,
        OrderStatus::Filled,
        Quantity::from("2.000"),
        Quantity::from("2.000"),
    )
    .with_avg_px(dec!(3100.0));

    let fill_report = FillReport::new(
        test_account_id(),
        instrument_id,
        closing_venue_order_id,
        closing_trade_id,
        OrderSide::Sell,
        Quantity::from("2.000"),
        Price::from("3100.00"),
        Money::from("1.00 USDT"),
        LiquiditySide::Taker,
        Some(closing_order_id),
        None,
        UnixNanos::from(2_000_000),
        UnixNanos::from(2_000_000),
        None,
    );

    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("3.000"),
        UnixNanos::from(2_000_000),
        UnixNanos::from(2_000_000),
        None,
        None,
        Some(dec!(3000.00)),
    );
    mass_status.add_order_reports(vec![order_report]);
    mass_status.add_fill_reports(vec![fill_report]);
    mass_status.add_position_reports(vec![position_report]);

    ctx.manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let cache = ctx.cache.borrow();
    let order = cache.order(&closing_order_id).unwrap();
    let position = cache.position(&position_id).unwrap();

    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(position.quantity, Quantity::from("3.000"));
    assert_eq!(position.realized_pnl, Some(Money::from("196.50 USDT")));
    assert_eq!(position.commissions(), vec![Money::from("3.50 USDT")]);
    assert_eq!(position.trade_ids.len(), 2);
    assert!(position.trade_ids.contains(&opening_trade_id));
    assert!(position.trade_ids.contains(&closing_trade_id));
}

#[tokio::test]
async fn test_split_lighter_reduce_only_lifecycle_preserves_explicit_flat() {
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let strategy_id = StrategyId::from("EXTERNAL");
    let venue_order_id = VenueOrderId::from("V-LIGHTER-SPLIT-CLOSE");
    let trade_id = TradeId::from("T-LIGHTER-SPLIT-CLOSE");
    let cutoff = UnixNanos::from(1_000_000_000_000);
    let close_ts = UnixNanos::from(1_000_001_000_000);

    ctx.add_instrument(instrument);
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(strategy_id, OmsType::Netting);

    let (portfolio_handler, portfolio_events) =
        get_typed_into_message_saving_handler::<OrderEventAny>(None);
    let portfolio_endpoint = MessagingSwitchboard::portfolio_update_order();
    msgbus::register_order_event_endpoint(portfolio_endpoint, portfolio_handler);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        close_ts,
        Some(UUID4::new()),
    );
    mass_status.set_report_window(Some(cutoff), true);
    let order_report = OrderStatusReport::new(
        test_account_id(),
        instrument_id,
        None,
        venue_order_id,
        OrderSide::Sell.into(),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Filled,
        Quantity::from("1.000"),
        Quantity::from("1.000"),
        close_ts,
        close_ts,
        close_ts,
        None,
    )
    .with_price(Price::from("3000.00"))
    .with_avg_px(dec!(3000.00))
    .with_reduce_only(true);

    let fill_report = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        trade_id,
        OrderSide::Sell,
        Quantity::from("1.000"),
        Price::from("3000.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Taker,
        None,
        None,
        close_ts,
        close_ts,
        None,
    );

    let flat_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Flat,
        Quantity::from("0"),
        close_ts,
        close_ts,
        None,
        None,
        None,
    );
    mass_status.add_order_reports(vec![order_report]);
    mass_status.add_fill_reports(vec![fill_report]);
    mass_status.add_position_reports(vec![flat_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    msgbus::deregister_any(portfolio_endpoint);

    assert_eq!(cutoff, UnixNanos::from(1_000_000_000_000));
    assert_eq!(close_ts, UnixNanos::from(1_000_001_000_000));
    assert_eq!(result.events.len(), 2);

    let OrderEventAny::Accepted(accepted) = &result.events[0] else {
        panic!("Expected Accepted event, was {:?}", result.events[0]);
    };

    let OrderEventAny::Filled(filled) = &result.events[1] else {
        panic!("Expected Filled event, was {:?}", result.events[1]);
    };

    assert_eq!(accepted.strategy_id, strategy_id);
    assert_eq!(accepted.instrument_id, instrument_id);
    assert_eq!(accepted.client_order_id.as_str(), venue_order_id.as_str());
    assert_eq!(accepted.venue_order_id, venue_order_id);
    assert_eq!(accepted.account_id, test_account_id());
    assert_eq!(accepted.ts_event, close_ts);
    assert_eq!(filled.strategy_id, strategy_id);
    assert_eq!(filled.instrument_id, instrument_id);
    assert_eq!(filled.client_order_id.as_str(), venue_order_id.as_str());
    assert_eq!(filled.venue_order_id, venue_order_id);
    assert_eq!(filled.account_id, test_account_id());
    assert_eq!(filled.trade_id, trade_id);
    assert_eq!(filled.position_id, None);
    assert_eq!(filled.order_side, OrderSide::Sell);
    assert_eq!(filled.order_type, OrderType::Limit);
    assert_eq!(filled.last_qty, Quantity::from("1.000"));
    assert_eq!(filled.last_px, Price::from("3000.00"));
    assert_eq!(filled.commission, Some(Money::from("0.50 USDT")));
    assert_eq!(filled.liquidity_side, LiquiditySide::Taker);
    assert_eq!(filled.ts_event, close_ts);
    assert_eq!(result.external_orders.len(), 1);
    assert_eq!(
        result.external_orders[0].client_order_id.as_str(),
        venue_order_id.as_str()
    );
    assert_eq!(result.external_orders[0].venue_order_id, venue_order_id);
    assert_eq!(result.external_orders[0].instrument_id, instrument_id);
    assert_eq!(result.external_orders[0].strategy_id, strategy_id);
    let order = ctx
        .cache
        .borrow()
        .order_owned(&ClientOrderId::from(venue_order_id.as_str()))
        .expect("reconciled terminal order");
    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.filled_qty(), Quantity::from("1.000"));
    assert!(order.is_reduce_only());
    assert_eq!(
        ctx.cache
            .borrow()
            .orders(None, None, None, None, None)
            .len(),
        1
    );
    assert_eq!(
        ctx.cache
            .borrow()
            .positions(None, None, None, None, None)
            .len(),
        0
    );
    let portfolio_events = portfolio_events.get_messages();
    assert_eq!(portfolio_events.len(), 2);
    assert!(matches!(portfolio_events[0], OrderEventAny::Accepted(_)));
    assert!(matches!(portfolio_events[1], OrderEventAny::Filled(_)));
    assert!(result.unresolved_positions.is_empty());
}

#[tokio::test]
async fn test_bounded_complete_lifecycle_applies_beside_split_close() {
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let strategy_id = StrategyId::from("EXTERNAL");
    let opening_venue_order_id = VenueOrderId::from("V-INSIDE-OPEN");
    let closing_venue_order_id = VenueOrderId::from("V-INSIDE-CLOSE");
    let split_venue_order_id = VenueOrderId::from("V-SPLIT-CLOSE");
    let opening_trade_id = TradeId::from("T-INSIDE-OPEN-1");
    let opening_trade_id_2 = TradeId::from("T-INSIDE-OPEN-2");
    let closing_trade_id = TradeId::from("T-INSIDE-CLOSE");
    let split_trade_id = TradeId::from("T-SPLIT-CLOSE");
    let cutoff = UnixNanos::from(1_000_000_000_000);
    let opening_ts = UnixNanos::from(1_000_001_000_000);
    let closing_ts = UnixNanos::from(1_000_002_000_000);
    let split_ts = UnixNanos::from(1_000_003_000_000);

    ctx.add_instrument(instrument);
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(strategy_id, OmsType::Netting);

    let (portfolio_handler, portfolio_events) =
        get_typed_into_message_saving_handler::<OrderEventAny>(None);
    let portfolio_endpoint = MessagingSwitchboard::portfolio_update_order();
    msgbus::register_order_event_endpoint(portfolio_endpoint, portfolio_handler);

    let (mut opening_order, opening_fill) = create_bounded_fill_lifecycle(
        instrument_id,
        opening_venue_order_id,
        opening_trade_id,
        OrderSide::Buy,
        "0.400",
        "3000.00",
        false,
        opening_ts,
    );
    opening_order.quantity = Quantity::from("1.000");
    opening_order.filled_qty = Quantity::from("1.000");
    let (_, opening_fill_2) = create_bounded_fill_lifecycle(
        instrument_id,
        opening_venue_order_id,
        opening_trade_id_2,
        OrderSide::Buy,
        "0.600",
        "3000.00",
        false,
        UnixNanos::from(opening_ts.as_u64() + 1),
    );
    let (closing_order, closing_fill) = create_bounded_fill_lifecycle(
        instrument_id,
        closing_venue_order_id,
        closing_trade_id,
        OrderSide::Sell,
        "1.000",
        "3100.00",
        true,
        closing_ts,
    );
    let (split_order, split_fill) = create_bounded_fill_lifecycle(
        instrument_id,
        split_venue_order_id,
        split_trade_id,
        OrderSide::Sell,
        "1.000",
        "3200.00",
        true,
        split_ts,
    );

    let flat_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Flat,
        Quantity::from("0"),
        split_ts,
        split_ts,
        None,
        None,
        None,
    );

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        split_ts,
        Some(UUID4::new()),
    );
    mass_status.set_report_window(Some(cutoff), true);
    mass_status.add_order_reports(vec![opening_order, closing_order, split_order]);
    mass_status.add_fill_reports(vec![opening_fill, opening_fill_2, closing_fill, split_fill]);
    mass_status.add_position_reports(vec![flat_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    msgbus::deregister_any(portfolio_endpoint);

    let actual_events: Vec<(&str, VenueOrderId)> = result
        .events
        .iter()
        .map(|event| match event {
            OrderEventAny::Accepted(event) => ("accepted", event.venue_order_id),
            OrderEventAny::Filled(event) => ("filled", event.venue_order_id),
            event => panic!("Unexpected reconciliation event: {event:?}"),
        })
        .collect();

    assert_eq!(
        actual_events,
        vec![
            ("accepted", opening_venue_order_id),
            ("filled", opening_venue_order_id),
            ("filled", opening_venue_order_id),
            ("accepted", closing_venue_order_id),
            ("filled", closing_venue_order_id),
            ("accepted", split_venue_order_id),
            ("filled", split_venue_order_id),
        ]
    );
    assert_eq!(result.external_orders.len(), 3);
    assert!(result.unresolved_positions.is_empty());

    let position_id = PositionId::new(format!("{instrument_id}-{strategy_id}"));
    let cache = ctx.cache.borrow();
    let position = cache
        .position(&position_id)
        .expect("closed inside lifecycle");
    assert!(position.is_closed());
    assert_eq!(position.quantity, Quantity::zero(3));
    assert_eq!(position.trade_ids.len(), 3);
    assert!(position.trade_ids.contains(&opening_trade_id));
    assert!(position.trade_ids.contains(&opening_trade_id_2));
    assert!(position.trade_ids.contains(&closing_trade_id));
    assert!(!position.trade_ids.contains(&split_trade_id));
    assert_eq!(position.commissions(), vec![Money::from("0.60 USDT")]);
    assert_eq!(cache.orders(None, None, None, None, None).len(), 3);

    for venue_order_id in [
        opening_venue_order_id,
        closing_venue_order_id,
        split_venue_order_id,
    ] {
        let order = cache
            .order(&ClientOrderId::from(venue_order_id.as_str()))
            .expect("reconciled terminal order");
        assert_eq!(order.status(), OrderStatus::Filled);
        assert_eq!(order.filled_qty(), Quantity::from("1.000"));
    }

    drop(position);
    drop(cache);

    let portfolio_events = portfolio_events.get_messages();

    let actual_portfolio_events: Vec<(&str, VenueOrderId)> = portfolio_events
        .iter()
        .map(|event| match event {
            OrderEventAny::Accepted(event) => ("accepted", event.venue_order_id),
            OrderEventAny::Filled(event) => ("filled", event.venue_order_id),
            event => panic!("Unexpected portfolio event: {event:?}"),
        })
        .collect();

    assert_eq!(
        actual_portfolio_events,
        vec![
            ("accepted", opening_venue_order_id),
            ("filled", opening_venue_order_id),
            ("filled", opening_venue_order_id),
            ("accepted", closing_venue_order_id),
            ("filled", closing_venue_order_id),
            ("accepted", split_venue_order_id),
            ("filled", split_venue_order_id),
        ]
    );
}

#[tokio::test]
async fn test_bounded_hedge_fill_applies_economics_once() {
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let strategy_id = StrategyId::from("EXTERNAL");
    let venue_order_id = VenueOrderId::from("V-BOUNDED-HEDGE");
    let venue_position_id = PositionId::from("P-BOUNDED-HEDGE");
    let trade_id = TradeId::from("T-BOUNDED-HEDGE");
    let cutoff = UnixNanos::from(1_500_000_000_000);
    let fill_ts = UnixNanos::from(1_500_001_000_000);

    ctx.add_instrument(instrument);

    let (mut order_report, mut fill_report) = create_bounded_fill_lifecycle(
        instrument_id,
        venue_order_id,
        trade_id,
        OrderSide::Buy,
        "1.000",
        "3000.00",
        false,
        fill_ts,
    );
    order_report = order_report.with_venue_position_id(venue_position_id);
    fill_report.venue_position_id = Some(venue_position_id);

    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("1.000"),
        fill_ts,
        fill_ts,
        None,
        Some(venue_position_id),
        Some(dec!(3000.00)),
    );

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        fill_ts,
        Some(UUID4::new()),
    );
    mass_status.set_report_window(Some(cutoff), true);
    mass_status.add_order_reports(vec![order_report]);
    mass_status.add_fill_reports(vec![fill_report]);
    mass_status.add_position_reports(vec![position_report]);

    let (portfolio_handler, portfolio_events) =
        get_typed_into_message_saving_handler::<OrderEventAny>(None);
    let portfolio_endpoint = MessagingSwitchboard::portfolio_update_order();
    msgbus::register_order_event_endpoint(portfolio_endpoint, portfolio_handler);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    msgbus::deregister_any(portfolio_endpoint);

    assert_eq!(result.events.len(), 2);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));

    let OrderEventAny::Filled(fill) = &result.events[1] else {
        panic!("Expected Filled event, was {:?}", result.events[1]);
    };

    assert_eq!(fill.strategy_id, strategy_id);
    assert_eq!(fill.venue_order_id, venue_order_id);
    assert_eq!(fill.position_id, Some(venue_position_id));
    assert_eq!(fill.trade_id, trade_id);
    assert_eq!(fill.last_qty, Quantity::from("1.000"));
    assert_eq!(result.external_orders.len(), 1);

    let cache = ctx.cache.borrow();
    let order = cache
        .order(&ClientOrderId::from(venue_order_id.as_str()))
        .expect("bounded hedge order");
    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.filled_qty(), Quantity::from("1.000"));
    let position = cache
        .position(&venue_position_id)
        .expect("bounded hedge position");
    assert_eq!(cache.oms_type(&venue_position_id), Some(OmsType::Hedging));
    assert!(position.is_open());
    assert!(position.is_long());
    assert_eq!(position.quantity, Quantity::from("1.000"));
    assert_eq!(position.trade_ids.len(), 1);
    assert!(position.trade_ids.contains(&trade_id));
    assert_eq!(position.commissions(), vec![Money::from("0.20 USDT")]);
    drop(position);
    drop(order);
    drop(cache);

    let portfolio_events = portfolio_events.get_messages();
    assert_eq!(portfolio_events.len(), 2);
    assert!(matches!(portfolio_events[0], OrderEventAny::Accepted(_)));
    assert!(matches!(portfolio_events[1], OrderEventAny::Filled(_)));
}

#[rstest]
#[case::sufficient_long_external(
    "STRATEGY-CLOSE",
    "1.000",
    false,
    OrderSide::Buy,
    OrderSide::Sell,
    true
)]
#[case::sufficient_long_cached(
    "STRATEGY-CLOSE",
    "1.000",
    true,
    OrderSide::Buy,
    OrderSide::Sell,
    true
)]
#[case::undersized_long_cached(
    "STRATEGY-CLOSE",
    "0.500",
    true,
    OrderSide::Buy,
    OrderSide::Sell,
    false
)]
#[case::unrelated_long_external(
    "STRATEGY-OTHER",
    "1.000",
    false,
    OrderSide::Buy,
    OrderSide::Sell,
    false
)]
#[case::sufficient_short_cached(
    "STRATEGY-CLOSE",
    "1.000",
    true,
    OrderSide::Sell,
    OrderSide::Buy,
    true
)]
#[case::undersized_short_cached(
    "STRATEGY-CLOSE",
    "0.500",
    true,
    OrderSide::Sell,
    OrderSide::Buy,
    false
)]
#[tokio::test]
async fn test_bounded_reduce_only_history_reconciles_flat_or_remains_unresolved(
    #[case] position_strategy: &str,
    #[case] position_qty: &str,
    #[case] cached_order: bool,
    #[case] opening_side: OrderSide,
    #[case] closing_side: OrderSide,
    #[case] closes_predecessor: bool,
) {
    let config = ExecutionManagerConfig {
        generate_missing_orders: false,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let close_strategy_id = StrategyId::from("STRATEGY-CLOSE");
    let position_strategy_id = StrategyId::from(position_strategy);
    let opening_order_id = ClientOrderId::from("O-CACHED-OPEN");
    let opening_venue_order_id = VenueOrderId::from("V-CACHED-OPEN");
    let opening_trade_id = TradeId::from("T-CACHED-OPEN");
    let closing_order_id = ClientOrderId::from("O-BOUNDED-CLOSE");
    let closing_venue_order_id = VenueOrderId::from("V-BOUNDED-CLOSE");
    let closing_trade_id = TradeId::from("T-BOUNDED-CLOSE");
    let position_id = PositionId::new(format!("{instrument_id}-{position_strategy_id}"));
    let close_position_id = PositionId::new(format!("{instrument_id}-{close_strategy_id}"));
    let cutoff = UnixNanos::from(2_000_000_000_000);
    let closing_ts = UnixNanos::from(2_000_001_000_000);

    ctx.add_instrument(instrument.clone());
    ctx.manager
        .claim_external_orders(instrument_id, close_strategy_id)
        .unwrap();
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(close_strategy_id, OmsType::Netting);
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(position_strategy_id, OmsType::Netting);

    let mut opening_order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(opening_order_id)
        .strategy_id(position_strategy_id)
        .instrument_id(instrument_id)
        .side(opening_side)
        .quantity(Quantity::from(position_qty))
        .price(Price::from("3000.00"))
        .build();
    apply_submitted_and_accepted(&mut opening_order, opening_venue_order_id);
    let opening_fill = TestOrderEventStubs::filled(
        &opening_order,
        &instrument,
        Some(opening_trade_id),
        Some(position_id),
        Some(Price::from("3000.00")),
        Some(Quantity::from(position_qty)),
        Some(LiquiditySide::Maker),
        Some(Money::from("0.10 USDT")),
        Some(UnixNanos::from(1_000_000_000_000)),
        Some(test_account_id()),
    );
    let position = Position::new(&instrument, opening_fill.into());
    ctx.cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();

    if cached_order {
        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .client_order_id(closing_order_id)
            .strategy_id(close_strategy_id)
            .instrument_id(instrument_id)
            .side(closing_side)
            .quantity(Quantity::from("1.000"))
            .price(Price::from("3100.00"))
            .reduce_only(true)
            .build();
        apply_submitted_and_accepted(&mut order, closing_venue_order_id);
        ctx.add_order(order);
    }

    let (mut closing_order, closing_fill) = create_bounded_fill_lifecycle(
        instrument_id,
        closing_venue_order_id,
        closing_trade_id,
        closing_side,
        "1.000",
        "3100.00",
        true,
        closing_ts,
    );

    if cached_order {
        closing_order.client_order_id = Some(closing_order_id);
    }

    let flat_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Flat,
        Quantity::from("0"),
        closing_ts,
        closing_ts,
        None,
        None,
        None,
    );

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        closing_ts,
        Some(UUID4::new()),
    );
    mass_status.set_report_window(Some(cutoff), true);
    mass_status.add_order_reports(vec![closing_order]);
    mass_status.add_fill_reports(vec![closing_fill]);
    mass_status.add_position_reports(vec![flat_report]);

    let (portfolio_handler, portfolio_events) =
        get_typed_into_message_saving_handler::<OrderEventAny>(None);
    let portfolio_endpoint = MessagingSwitchboard::portfolio_update_order();
    msgbus::register_order_event_endpoint(portfolio_endpoint, portfolio_handler);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    msgbus::deregister_any(portfolio_endpoint);

    assert_eq!(result.events.len(), if cached_order { 1 } else { 2 });

    if !cached_order {
        assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));
    }

    let OrderEventAny::Filled(fill) = result.events.last().unwrap() else {
        panic!(
            "Expected final Filled event, was {:?}",
            result.events.last()
        );
    };

    assert_eq!(fill.strategy_id, close_strategy_id);
    assert_eq!(fill.venue_order_id, closing_venue_order_id);
    assert_eq!(fill.trade_id, closing_trade_id);
    assert_eq!(fill.last_qty, Quantity::from("1.000"));
    assert_eq!(result.external_orders.len(), usize::from(!cached_order));

    let cache = ctx.cache.borrow();

    let reconciled_order_id = if cached_order {
        closing_order_id
    } else {
        ClientOrderId::from(closing_venue_order_id.as_str())
    };

    let order = cache
        .order(&reconciled_order_id)
        .expect("reconciled terminal order");
    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.filled_qty(), Quantity::from("1.000"));
    drop(order);
    let position = cache.position(&position_id).expect("cached predecessor");

    if closes_predecessor {
        assert!(position.is_closed());
        assert_eq!(position.quantity, Quantity::zero(3));
        assert_eq!(position.trade_ids.len(), 2);
        assert!(position.trade_ids.contains(&opening_trade_id));
        assert!(position.trade_ids.contains(&closing_trade_id));
        assert_eq!(position.commissions(), vec![Money::from("0.30 USDT")]);
    } else if position_strategy_id == close_strategy_id {
        assert!(position.is_open());
        assert_eq!(position.is_long(), closing_side == OrderSide::Buy);
        assert_eq!(position.quantity, Quantity::from("0.500"));
        assert_eq!(position.avg_px_open, 3100.0);
        assert_eq!(position.trade_ids.len(), 1);
        assert!(position.trade_ids.contains(&closing_trade_id));
        assert_eq!(position.commissions(), vec![Money::from("0.10 USDT")]);
    } else {
        assert!(position.is_open());
        assert_eq!(position.is_long(), opening_side == OrderSide::Buy);
        assert_eq!(position.quantity, Quantity::from(position_qty));
        assert_eq!(position.trade_ids.len(), 1);
        assert!(position.trade_ids.contains(&opening_trade_id));
        assert_eq!(position.commissions(), vec![Money::from("0.10 USDT")]);
        assert!(cache.position(&close_position_id).is_none());
    }

    assert_eq!(
        result.unresolved_positions.len(),
        usize::from(!closes_predecessor)
    );

    drop(position);
    drop(cache);

    let portfolio_events = portfolio_events.get_messages();
    assert_eq!(portfolio_events.len(), usize::from(!cached_order) + 1);

    if !cached_order {
        assert!(matches!(portfolio_events[0], OrderEventAny::Accepted(_)));
    }

    assert!(matches!(
        portfolio_events.last(),
        Some(OrderEventAny::Filled(_))
    ));
}

#[rstest]
#[case::reported_fill(true)]
#[case::inferred_fill(false)]
#[tokio::test]
async fn test_incomplete_bounded_reports_project_fills_order_only(#[case] has_fill_report: bool) {
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let venue_order_id = VenueOrderId::from("V-INCOMPLETE-FILL");
    let trade_id = TradeId::from("T-INCOMPLETE-FILL");
    let cutoff = UnixNanos::from(3_000_000_000_000);
    let fill_ts = UnixNanos::from(3_000_001_000_000);

    ctx.add_instrument(instrument);
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(StrategyId::from("EXTERNAL"), OmsType::Netting);

    let (order_report, fill_report) = create_bounded_fill_lifecycle(
        instrument_id,
        venue_order_id,
        trade_id,
        OrderSide::Buy,
        "1.000",
        "3000.00",
        false,
        fill_ts,
    );

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        fill_ts,
        Some(UUID4::new()),
    );
    mass_status.set_report_window(Some(cutoff), false);
    mass_status.add_order_reports(vec![order_report]);

    if has_fill_report {
        mass_status.add_fill_reports(vec![fill_report]);
    }

    let (portfolio_handler, portfolio_events) =
        get_typed_into_message_saving_handler::<OrderEventAny>(None);
    let portfolio_endpoint = MessagingSwitchboard::portfolio_update_order();
    msgbus::register_order_event_endpoint(portfolio_endpoint, portfolio_handler);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    msgbus::deregister_any(portfolio_endpoint);

    assert_eq!(result.events.len(), 2);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));

    let OrderEventAny::Filled(fill) = &result.events[1] else {
        panic!("Expected Filled event, was {:?}", result.events[1]);
    };

    assert_eq!(fill.venue_order_id, venue_order_id);
    assert_eq!(fill.instrument_id, instrument_id);
    assert_eq!(fill.order_side, OrderSide::Buy);
    assert_eq!(fill.last_qty, Quantity::from("1.000"));
    assert_eq!(fill.last_px, Price::from("3000.00"));
    assert_eq!(fill.trade_id == trade_id, has_fill_report);
    assert!(fill.reconciliation);
    assert_eq!(result.external_orders.len(), 1);

    let cache = ctx.cache.borrow();
    let order = cache
        .order(&ClientOrderId::from(venue_order_id.as_str()))
        .expect("reconciled terminal order");
    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.filled_qty(), Quantity::from("1.000"));
    assert_eq!(cache.positions(None, None, None, None, None).len(), 0);
    drop(order);
    drop(cache);

    let portfolio_events = portfolio_events.get_messages();
    assert_eq!(portfolio_events.len(), 1);
    assert!(matches!(portfolio_events[0], OrderEventAny::Accepted(_)));
}

#[tokio::test]
async fn test_bounded_active_partial_order_preserves_order_and_reconciles_flat() {
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let venue_order_id = VenueOrderId::from("V-ACTIVE-PARTIAL");
    let cutoff = UnixNanos::from(4_000_000_000_000);
    let fill_ts = UnixNanos::from(4_000_001_000_000);

    ctx.add_instrument(instrument);
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(StrategyId::from("EXTERNAL"), OmsType::Netting);

    let order_report = OrderStatusReport::new(
        test_account_id(),
        instrument_id,
        None,
        venue_order_id,
        OrderSide::Buy.into(),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::PartiallyFilled,
        Quantity::from("1.000"),
        Quantity::from("0.400"),
        fill_ts,
        fill_ts,
        fill_ts,
        None,
    )
    .with_price(Price::from("3000.00"))
    .with_avg_px(dec!(3000.00));

    let flat_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Flat,
        Quantity::from("0"),
        fill_ts,
        fill_ts,
        None,
        None,
        None,
    );

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        fill_ts,
        Some(UUID4::new()),
    );
    mass_status.set_report_window(Some(cutoff), true);
    mass_status.add_order_reports(vec![order_report]);
    mass_status.add_position_reports(vec![flat_report]);

    let (portfolio_handler, portfolio_events) =
        get_typed_into_message_saving_handler::<OrderEventAny>(None);
    let portfolio_endpoint = MessagingSwitchboard::portfolio_update_order();
    msgbus::register_order_event_endpoint(portfolio_endpoint, portfolio_handler);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    msgbus::deregister_any(portfolio_endpoint);

    assert_eq!(result.events.len(), 4);
    assert!(result.unresolved_positions.is_empty());
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));

    let OrderEventAny::Filled(fill) = &result.events[1] else {
        panic!("Expected inferred Filled event, was {:?}", result.events[1]);
    };

    assert_eq!(fill.venue_order_id, venue_order_id);
    assert_eq!(fill.last_qty, Quantity::from("0.400"));
    assert_eq!(fill.last_px, Price::from("3000.00"));
    assert!(fill.reconciliation);
    assert_eq!(result.external_orders.len(), 1);

    let cache = ctx.cache.borrow();
    let order = cache
        .order(&ClientOrderId::from(venue_order_id.as_str()))
        .expect("active partially filled order");
    assert_eq!(order.status(), OrderStatus::PartiallyFilled);
    assert_eq!(order.quantity(), Quantity::from("1.000"));
    assert_eq!(order.filled_qty(), Quantity::from("0.400"));
    assert_eq!(cache.orders_open(None, None, None, None, None).len(), 1);
    assert_eq!(cache.positions_open(None, None, None, None, None).len(), 0);
    assert_eq!(cache.positions(None, None, None, None, None).len(), 1);
    drop(order);
    drop(cache);

    let portfolio_events = portfolio_events.get_messages();
    assert_eq!(portfolio_events.len(), 4);

    let closing_fill = match &portfolio_events[3] {
        OrderEventAny::Filled(fill) => fill,
        event => panic!("Expected closing fill, was {event:?}"),
    };

    assert_eq!(closing_fill.order_side, OrderSide::Sell);
    assert_eq!(closing_fill.last_qty, Quantity::from("0.400"));
    assert_eq!(closing_fill.last_px, Price::from("3000.00"));
    assert!(matches!(portfolio_events[0], OrderEventAny::Accepted(_)));
}

#[rstest]
#[case::coherent_open(OrderSide::Buy, false, true)]
#[case::isolated_reduce_only_close(OrderSide::Sell, true, false)]
#[tokio::test]
async fn test_bounded_nonflat_position_requires_coherent_historical_fill(
    #[case] order_side: OrderSide,
    #[case] reduce_only: bool,
    #[case] expected_applied: bool,
) {
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let strategy_id = StrategyId::from("EXTERNAL");
    let venue_order_id = VenueOrderId::from("V-BOUNDED-NONFLAT");
    let trade_id = TradeId::from("T-BOUNDED-NONFLAT");
    let cutoff = UnixNanos::from(5_000_000_000_000);
    let fill_ts = UnixNanos::from(5_000_001_000_000);

    ctx.add_instrument(instrument);
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(strategy_id, OmsType::Netting);

    let (order_report, fill_report) = create_bounded_fill_lifecycle(
        instrument_id,
        venue_order_id,
        trade_id,
        order_side,
        "1.000",
        "3000.00",
        reduce_only,
        fill_ts,
    );

    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("1.000"),
        fill_ts,
        fill_ts,
        None,
        None,
        Some(dec!(3000.00)),
    );

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        fill_ts,
        Some(UUID4::new()),
    );
    mass_status.set_report_window(Some(cutoff), true);
    mass_status.add_order_reports(vec![order_report]);
    mass_status.add_fill_reports(vec![fill_report]);
    mass_status.add_position_reports(vec![position_report]);

    let (portfolio_handler, portfolio_events) =
        get_typed_into_message_saving_handler::<OrderEventAny>(None);
    let portfolio_endpoint = MessagingSwitchboard::portfolio_update_order();
    msgbus::register_order_event_endpoint(portfolio_endpoint, portfolio_handler);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    msgbus::deregister_any(portfolio_endpoint);

    let historical_fill = result.events.iter().find_map(|event| match event {
        OrderEventAny::Filled(fill) if fill.venue_order_id == venue_order_id => Some(fill),
        _ => None,
    });

    let historical_fill = historical_fill.expect("historical fill event");
    assert_eq!(historical_fill.trade_id, trade_id);
    assert_eq!(historical_fill.order_side, order_side);
    assert_eq!(historical_fill.last_qty, Quantity::from("1.000"));
    assert_eq!(historical_fill.last_px, Price::from("3000.00"));
    assert!(result.unresolved_positions.is_empty());
    assert_eq!(result.events.len(), if expected_applied { 2 } else { 4 });
    assert_eq!(
        result.external_orders.len(),
        if expected_applied { 1 } else { 2 }
    );

    let cache = ctx.cache.borrow();
    let order = cache
        .order(&ClientOrderId::from(venue_order_id.as_str()))
        .expect("historical order");
    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.filled_qty(), Quantity::from("1.000"));
    assert_eq!(order.is_reduce_only(), reduce_only);
    let position_id = PositionId::new(format!("{instrument_id}-{strategy_id}"));
    let position = cache
        .position(&position_id)
        .expect("authoritative open position");
    assert!(position.is_open());
    assert!(position.is_long());
    assert_eq!(position.quantity, Quantity::from("1.000"));
    assert_eq!(position.avg_px_open, 3000.0);
    assert!(position.trade_ids.contains(&trade_id));
    assert_eq!(
        position.trade_ids.len(),
        if expected_applied { 1 } else { 2 }
    );
    drop(position);
    drop(order);
    drop(cache);

    let portfolio_events = portfolio_events.get_messages();
    let historical_portfolio_fill = portfolio_events.iter().any(
        |event| matches!(event, OrderEventAny::Filled(fill) if fill.venue_order_id == venue_order_id),
    );
    assert!(historical_portfolio_fill);
    assert_eq!(portfolio_events.len(), if expected_applied { 2 } else { 4 });
}

#[rstest]
#[case::missing(0)]
#[case::duplicate_flat(2)]
#[tokio::test]
async fn test_bounded_history_distinguishes_missing_and_explicit_flat_reports(
    #[case] position_report_count: usize,
) {
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let strategy_id = StrategyId::from("EXTERNAL");
    let venue_order_id = VenueOrderId::from("V-BOUNDED-NO-POSITION");
    let trade_id = TradeId::from("T-BOUNDED-NO-POSITION");
    let cutoff = UnixNanos::from(6_000_000_000_000);
    let fill_ts = UnixNanos::from(6_000_001_000_000);

    ctx.add_instrument(instrument);
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(strategy_id, OmsType::Netting);

    let (order_report, fill_report) = create_bounded_fill_lifecycle(
        instrument_id,
        venue_order_id,
        trade_id,
        OrderSide::Buy,
        "1.000",
        "3000.00",
        false,
        fill_ts,
    );

    let position_reports = (0..position_report_count)
        .map(|_| {
            PositionStatusReport::new(
                test_account_id(),
                instrument_id,
                PositionSide::Flat,
                Quantity::zero(3),
                fill_ts,
                fill_ts,
                None,
                None,
                None,
            )
        })
        .collect();

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        fill_ts,
        Some(UUID4::new()),
    );
    mass_status.set_report_window(Some(cutoff), true);
    mass_status.add_order_reports(vec![order_report]);
    mass_status.add_fill_reports(vec![fill_report]);
    mass_status.add_position_reports(position_reports);

    let (portfolio_handler, portfolio_events) =
        get_typed_into_message_saving_handler::<OrderEventAny>(None);
    let portfolio_endpoint = MessagingSwitchboard::portfolio_update_order();
    msgbus::register_order_event_endpoint(portfolio_endpoint, portfolio_handler);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    msgbus::deregister_any(portfolio_endpoint);

    assert_eq!(
        result.events.len(),
        if position_report_count == 0 { 2 } else { 4 }
    );
    assert!(result.unresolved_positions.is_empty());
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));

    let OrderEventAny::Filled(fill) = &result.events[1] else {
        panic!("Expected Filled event, was {:?}", result.events[1]);
    };

    assert_eq!(fill.venue_order_id, venue_order_id);
    assert_eq!(fill.trade_id, trade_id);
    assert_eq!(fill.last_qty, Quantity::from("1.000"));
    assert_eq!(result.external_orders.len(), 1);

    let cache = ctx.cache.borrow();
    let order = cache
        .order(&ClientOrderId::from(venue_order_id.as_str()))
        .expect("historical order");
    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.filled_qty(), Quantity::from("1.000"));
    assert_eq!(cache.positions_open(None, None, None, None, None).len(), 0);
    assert_eq!(
        cache.positions(None, None, None, None, None).len(),
        usize::from(position_report_count != 0)
    );
    drop(order);
    drop(cache);

    let portfolio_events = portfolio_events.get_messages();
    assert_eq!(
        portfolio_events.len(),
        if position_report_count == 0 { 1 } else { 4 }
    );
    assert!(matches!(portfolio_events[0], OrderEventAny::Accepted(_)));
}

#[tokio::test]
async fn test_bounded_interleaved_multi_fill_orders_reconcile_explicit_flat() {
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let strategy_id = StrategyId::from("EXTERNAL");
    let opening_venue_order_id = VenueOrderId::from("V-INTERLEAVED-OPEN");
    let closing_venue_order_id = VenueOrderId::from("V-INTERLEAVED-CLOSE");
    let opening_trade_id = TradeId::from("T-INTERLEAVED-OPEN-1");
    let opening_trade_id_2 = TradeId::from("T-INTERLEAVED-OPEN-2");
    let closing_trade_id = TradeId::from("T-INTERLEAVED-CLOSE");
    let cutoff = UnixNanos::from(7_000_000_000_000);
    let opening_ts = UnixNanos::from(7_000_001_000_000);
    let closing_ts = UnixNanos::from(7_000_002_000_000);
    let opening_ts_2 = UnixNanos::from(7_000_003_000_000);

    ctx.add_instrument(instrument);
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(strategy_id, OmsType::Netting);

    let (mut opening_order, opening_fill) = create_bounded_fill_lifecycle(
        instrument_id,
        opening_venue_order_id,
        opening_trade_id,
        OrderSide::Buy,
        "0.500",
        "3000.00",
        false,
        opening_ts,
    );
    opening_order.quantity = Quantity::from("1.000");
    opening_order.filled_qty = Quantity::from("1.000");
    let (_, opening_fill_2) = create_bounded_fill_lifecycle(
        instrument_id,
        opening_venue_order_id,
        opening_trade_id_2,
        OrderSide::Buy,
        "0.500",
        "3000.00",
        false,
        opening_ts_2,
    );
    let (closing_order, closing_fill) = create_bounded_fill_lifecycle(
        instrument_id,
        closing_venue_order_id,
        closing_trade_id,
        OrderSide::Sell,
        "1.000",
        "3100.00",
        true,
        closing_ts,
    );

    let flat_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Flat,
        Quantity::zero(3),
        opening_ts_2,
        opening_ts_2,
        None,
        None,
        None,
    );

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        opening_ts_2,
        Some(UUID4::new()),
    );
    mass_status.set_report_window(Some(cutoff), true);
    mass_status.add_order_reports(vec![opening_order, closing_order]);
    mass_status.add_fill_reports(vec![opening_fill, opening_fill_2, closing_fill]);
    mass_status.add_position_reports(vec![flat_report]);

    let (portfolio_handler, portfolio_events) =
        get_typed_into_message_saving_handler::<OrderEventAny>(None);
    let portfolio_endpoint = MessagingSwitchboard::portfolio_update_order();
    msgbus::register_order_event_endpoint(portfolio_endpoint, portfolio_handler);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    msgbus::deregister_any(portfolio_endpoint);

    let actual_events: Vec<(&str, VenueOrderId, Option<TradeId>)> = result
        .events
        .iter()
        .map(|event| match event {
            OrderEventAny::Accepted(event) => ("accepted", event.venue_order_id, None),
            OrderEventAny::Filled(event) => ("filled", event.venue_order_id, Some(event.trade_id)),
            event => panic!("Unexpected reconciliation event: {event:?}"),
        })
        .collect();

    assert_eq!(
        actual_events,
        vec![
            ("accepted", opening_venue_order_id, None),
            ("filled", opening_venue_order_id, Some(opening_trade_id)),
            ("accepted", closing_venue_order_id, None),
            ("filled", closing_venue_order_id, Some(closing_trade_id)),
            ("filled", opening_venue_order_id, Some(opening_trade_id_2)),
        ]
    );
    assert_eq!(result.external_orders.len(), 2);

    let cache = ctx.cache.borrow();

    for venue_order_id in [opening_venue_order_id, closing_venue_order_id] {
        let order = cache
            .order(&ClientOrderId::from(venue_order_id.as_str()))
            .expect("historical order");
        assert_eq!(order.status(), OrderStatus::Filled);
        assert_eq!(order.filled_qty(), Quantity::from("1.000"));
    }

    assert!(result.unresolved_positions.is_empty());
    assert_eq!(cache.positions_open(None, None, None, None, None).len(), 0);
    assert_eq!(cache.positions(None, None, None, None, None).len(), 1);
    drop(cache);

    let portfolio_events = portfolio_events.get_messages();
    let position_id = PositionId::new(format!("{instrument_id}-{strategy_id}"));

    let expected_portfolio_events: Vec<_> = result
        .events
        .iter()
        .cloned()
        .map(|mut event| {
            if let OrderEventAny::Filled(fill) = &mut event {
                fill.position_id = Some(position_id);
            }

            event
        })
        .collect();

    assert_eq!(portfolio_events, expected_portfolio_events);
}

#[tokio::test]
async fn test_bounded_same_timestamp_orders_reconcile_explicit_flat() {
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let strategy_id = StrategyId::from("EXTERNAL");
    let opening_venue_order_id = VenueOrderId::from("V-SAME-TS-OPEN");
    let closing_venue_order_id = VenueOrderId::from("V-SAME-TS-CLOSE");
    let opening_trade_id = TradeId::from("T-SAME-TS-OPEN");
    let closing_trade_id = TradeId::from("T-SAME-TS-CLOSE");
    let cutoff = UnixNanos::from(8_000_000_000_000);
    let fill_ts = UnixNanos::from(8_000_001_000_000);

    ctx.add_instrument(instrument);
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(strategy_id, OmsType::Netting);

    let (opening_order, opening_fill) = create_bounded_fill_lifecycle(
        instrument_id,
        opening_venue_order_id,
        opening_trade_id,
        OrderSide::Buy,
        "1.000",
        "3000.00",
        false,
        fill_ts,
    );
    let (closing_order, closing_fill) = create_bounded_fill_lifecycle(
        instrument_id,
        closing_venue_order_id,
        closing_trade_id,
        OrderSide::Sell,
        "1.000",
        "3100.00",
        true,
        fill_ts,
    );

    let flat_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Flat,
        Quantity::zero(3),
        fill_ts,
        fill_ts,
        None,
        None,
        None,
    );

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        fill_ts,
        Some(UUID4::new()),
    );
    mass_status.set_report_window(Some(cutoff), true);
    mass_status.add_order_reports(vec![opening_order, closing_order]);
    mass_status.add_fill_reports(vec![opening_fill, closing_fill]);
    mass_status.add_position_reports(vec![flat_report]);

    let (portfolio_handler, portfolio_events) =
        get_typed_into_message_saving_handler::<OrderEventAny>(None);
    let portfolio_endpoint = MessagingSwitchboard::portfolio_update_order();
    msgbus::register_order_event_endpoint(portfolio_endpoint, portfolio_handler);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    msgbus::deregister_any(portfolio_endpoint);

    let actual_events: Vec<(&str, VenueOrderId, Option<TradeId>)> = result
        .events
        .iter()
        .map(|event| match event {
            OrderEventAny::Accepted(event) => ("accepted", event.venue_order_id, None),
            OrderEventAny::Filled(event) => ("filled", event.venue_order_id, Some(event.trade_id)),
            event => panic!("Unexpected reconciliation event: {event:?}"),
        })
        .collect();

    assert_eq!(
        actual_events,
        vec![
            ("accepted", opening_venue_order_id, None),
            ("filled", opening_venue_order_id, Some(opening_trade_id)),
            ("accepted", closing_venue_order_id, None),
            ("filled", closing_venue_order_id, Some(closing_trade_id)),
        ]
    );
    assert_eq!(result.external_orders.len(), 2);

    let cache = ctx.cache.borrow();

    for venue_order_id in [opening_venue_order_id, closing_venue_order_id] {
        let order = cache
            .order(&ClientOrderId::from(venue_order_id.as_str()))
            .expect("same-timestamp historical order");
        assert_eq!(order.status(), OrderStatus::Filled);
        assert_eq!(order.filled_qty(), Quantity::from("1.000"));
    }

    assert!(result.unresolved_positions.is_empty());
    assert_eq!(cache.positions_open(None, None, None, None, None).len(), 0);
    assert_eq!(cache.positions(None, None, None, None, None).len(), 1);
    drop(cache);

    let portfolio_events = portfolio_events.get_messages();
    let position_id = PositionId::new(format!("{instrument_id}-{strategy_id}"));

    let expected_portfolio_events: Vec<_> = result
        .events
        .iter()
        .cloned()
        .map(|mut event| {
            if let OrderEventAny::Filled(fill) = &mut event {
                fill.position_id = Some(position_id);
            }

            event
        })
        .collect();

    assert_eq!(portfolio_events, expected_portfolio_events);
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

#[derive(Debug, Default)]
struct PersistedCacheState {
    general: AHashMap<String, Bytes>,
    orders: AHashMap<ClientOrderId, OrderAny>,
    order_client: AHashMap<ClientOrderId, ClientId>,
    positions: AHashMap<PositionId, Position>,
    order_position: AHashMap<ClientOrderId, PositionId>,
}

/// Cache database adapter that keeps orders, positions, position OMS entries and the
/// order-position index in shared memory so a fresh `Cache` restores them through `cache_all`.
#[derive(Debug)]
struct PersistingCacheDatabase {
    state: Arc<Mutex<PersistedCacheState>>,
}

fn persisting_cache(state: &Arc<Mutex<PersistedCacheState>>) -> Cache {
    Cache::new(
        None,
        Some(Box::new(PersistingCacheDatabase {
            state: state.clone(),
        })),
    )
}

#[async_trait]
impl CacheDatabaseAdapter for PersistingCacheDatabase {
    fn close(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn flush(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn load_all(&self) -> anyhow::Result<CacheMap> {
        let state = self.state.lock();
        Ok(CacheMap {
            orders: state.orders.clone(),
            positions: state.positions.clone(),
            ..Default::default()
        })
    }

    fn load(&self) -> anyhow::Result<AHashMap<String, Bytes>> {
        Ok(self.state.lock().general.clone())
    }

    async fn load_currencies(&self) -> anyhow::Result<AHashMap<Ustr, Currency>> {
        Ok(AHashMap::new())
    }

    async fn load_instruments(&self) -> anyhow::Result<AHashMap<InstrumentId, InstrumentAny>> {
        Ok(AHashMap::new())
    }

    async fn load_instrument_closes(
        &self,
    ) -> anyhow::Result<AHashMap<InstrumentId, InstrumentClose>> {
        Ok(AHashMap::new())
    }

    async fn load_synthetics(&self) -> anyhow::Result<AHashMap<InstrumentId, SyntheticInstrument>> {
        Ok(AHashMap::new())
    }

    async fn load_accounts(&self) -> anyhow::Result<AHashMap<AccountId, AccountAny>> {
        Ok(AHashMap::new())
    }

    async fn load_orders(&self) -> anyhow::Result<AHashMap<ClientOrderId, OrderAny>> {
        Ok(self.state.lock().orders.clone())
    }

    async fn load_positions(&self) -> anyhow::Result<AHashMap<PositionId, Position>> {
        Ok(self.state.lock().positions.clone())
    }

    fn load_index_order_position(&self) -> anyhow::Result<AHashMap<ClientOrderId, PositionId>> {
        Ok(self.state.lock().order_position.clone())
    }

    fn load_index_order_client(&self) -> anyhow::Result<AHashMap<ClientOrderId, ClientId>> {
        Ok(self.state.lock().order_client.clone())
    }

    async fn load_currency(&self, _code: &Ustr) -> anyhow::Result<Option<Currency>> {
        Ok(None)
    }

    async fn load_instrument(
        &self,
        _instrument_id: &InstrumentId,
    ) -> anyhow::Result<Option<InstrumentAny>> {
        Ok(None)
    }

    async fn load_synthetic(
        &self,
        _instrument_id: &InstrumentId,
    ) -> anyhow::Result<Option<SyntheticInstrument>> {
        Ok(None)
    }

    async fn load_account(&self, _account_id: &AccountId) -> anyhow::Result<Option<AccountAny>> {
        Ok(None)
    }

    async fn load_order(
        &self,
        client_order_id: &ClientOrderId,
    ) -> anyhow::Result<Option<OrderAny>> {
        Ok(self.state.lock().orders.get(client_order_id).cloned())
    }

    async fn load_position(&self, position_id: &PositionId) -> anyhow::Result<Option<Position>> {
        Ok(self.state.lock().positions.get(position_id).cloned())
    }

    fn load_actor(&self, _actor_id: &ActorId) -> anyhow::Result<AHashMap<String, Bytes>> {
        Ok(AHashMap::new())
    }

    fn load_strategy(&self, _strategy_id: &StrategyId) -> anyhow::Result<AHashMap<String, Bytes>> {
        Ok(AHashMap::new())
    }

    fn load_signals(&self, _name: &str) -> anyhow::Result<Vec<Signal>> {
        Ok(Vec::new())
    }

    fn load_custom_data(&self, _data_type: &DataType) -> anyhow::Result<Vec<CustomData>> {
        Ok(Vec::new())
    }

    fn load_order_snapshot(
        &self,
        _client_order_id: &ClientOrderId,
    ) -> anyhow::Result<Option<OrderSnapshot>> {
        Ok(None)
    }

    fn load_position_snapshot(
        &self,
        _position_id: &PositionId,
    ) -> anyhow::Result<Option<PositionSnapshot>> {
        Ok(None)
    }

    fn load_quotes(&self, _instrument_id: &InstrumentId) -> anyhow::Result<Vec<QuoteTick>> {
        Ok(Vec::new())
    }

    fn load_trades(&self, _instrument_id: &InstrumentId) -> anyhow::Result<Vec<TradeTick>> {
        Ok(Vec::new())
    }

    fn load_funding_rates(
        &self,
        _instrument_id: &InstrumentId,
    ) -> anyhow::Result<Vec<FundingRateUpdate>> {
        Ok(Vec::new())
    }

    fn load_bars(&self, _instrument_id: &InstrumentId) -> anyhow::Result<Vec<Bar>> {
        Ok(Vec::new())
    }

    fn add(&self, key: String, value: Bytes) -> anyhow::Result<()> {
        self.state.lock().general.insert(key, value);
        Ok(())
    }

    fn add_currency(&self, _currency: &Currency) -> anyhow::Result<()> {
        Ok(())
    }

    fn add_instrument(&self, _instrument: &InstrumentAny) -> anyhow::Result<()> {
        Ok(())
    }

    fn add_instrument_close(&self, _close: &InstrumentClose) -> anyhow::Result<()> {
        Ok(())
    }

    fn add_synthetic(&self, _synthetic: &SyntheticInstrument) -> anyhow::Result<()> {
        Ok(())
    }

    fn add_account(&self, _account: &AccountAny) -> anyhow::Result<()> {
        Ok(())
    }

    fn add_order(&self, order: &OrderAny, client_id: Option<ClientId>) -> anyhow::Result<()> {
        let mut state = self.state.lock();
        state.orders.insert(order.client_order_id(), order.clone());

        if let Some(client_id) = client_id {
            state
                .order_client
                .insert(order.client_order_id(), client_id);
        }

        Ok(())
    }

    fn add_order_snapshot(&self, _snapshot: &OrderSnapshot) -> anyhow::Result<()> {
        Ok(())
    }

    fn add_position(&self, position: &Position) -> anyhow::Result<()> {
        self.state
            .lock()
            .positions
            .insert(position.id, position.clone());
        Ok(())
    }

    fn add_position_snapshot(&self, _snapshot: &PositionSnapshot) -> anyhow::Result<()> {
        Ok(())
    }

    fn add_order_book(&self, _order_book: &OrderBook) -> anyhow::Result<()> {
        Ok(())
    }

    fn add_signal(&self, _signal: &Signal) -> anyhow::Result<()> {
        Ok(())
    }

    fn add_custom_data(&self, _data: &CustomData) -> anyhow::Result<()> {
        Ok(())
    }

    fn add_quote(&self, _quote: &QuoteTick) -> anyhow::Result<()> {
        Ok(())
    }

    fn add_trade(&self, _trade: &TradeTick) -> anyhow::Result<()> {
        Ok(())
    }

    fn add_funding_rate(&self, _funding_rate: &FundingRateUpdate) -> anyhow::Result<()> {
        Ok(())
    }

    fn add_bar(&self, _bar: &Bar) -> anyhow::Result<()> {
        Ok(())
    }

    fn add_greeks(&self, _greeks: &GreeksData) -> anyhow::Result<()> {
        Ok(())
    }

    fn add_yield_curve(&self, _yield_curve: &YieldCurveData) -> anyhow::Result<()> {
        Ok(())
    }

    fn delete_actor(&self, _actor_id: &ActorId) -> anyhow::Result<()> {
        Ok(())
    }

    fn delete_strategy(&self, _component_id: &StrategyId) -> anyhow::Result<()> {
        Ok(())
    }

    fn delete_order(&self, client_order_id: &ClientOrderId) -> anyhow::Result<()> {
        let mut state = self.state.lock();
        state.orders.remove(client_order_id);
        state.order_client.remove(client_order_id);
        state.order_position.remove(client_order_id);
        Ok(())
    }

    fn delete_position(&self, position_id: &PositionId) -> anyhow::Result<()> {
        self.state.lock().positions.remove(position_id);
        Ok(())
    }

    fn delete_account_event(&self, _account_id: &AccountId, _event_id: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn index_venue_order_id(
        &self,
        _client_order_id: ClientOrderId,
        _venue_order_id: VenueOrderId,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn index_order_position(
        &self,
        client_order_id: ClientOrderId,
        position_id: PositionId,
    ) -> anyhow::Result<()> {
        self.state
            .lock()
            .order_position
            .insert(client_order_id, position_id);
        Ok(())
    }

    fn update_actor(
        &self,
        _actor_id: &ActorId,
        _actor_state: &AHashMap<String, Bytes>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn update_strategy(
        &self,
        _strategy_id: &StrategyId,
        _strategy_state: &AHashMap<String, Bytes>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn update_account(&self, _account: &AccountAny) -> anyhow::Result<()> {
        Ok(())
    }

    fn update_order(&self, order_event: &OrderEventAny) -> anyhow::Result<()> {
        let client_order_id = order_event.client_order_id();
        let mut state = self.state.lock();
        let order = state
            .orders
            .get_mut(&client_order_id)
            .ok_or_else(|| anyhow::anyhow!("order {client_order_id} is not persisted"))?;
        order.apply(order_event.clone())?;
        Ok(())
    }

    fn update_position(&self, position: &Position) -> anyhow::Result<()> {
        self.state
            .lock()
            .positions
            .insert(position.id, position.clone());
        Ok(())
    }

    fn snapshot_order_state(&self, _order: &OrderAny) -> anyhow::Result<()> {
        Ok(())
    }

    fn snapshot_position_state(
        &self,
        _position: &Position,
        _ts_snapshot: UnixNanos,
        _unrealized_pnl: Option<Money>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn heartbeat(&self, _timestamp: UnixNanos) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Restores a cache from the persisted state the way a node does on startup.
async fn restore_persisted_cache(state: &Arc<Mutex<PersistedCacheState>>) -> Cache {
    let mut cache = persisting_cache(state);
    cache.cache_all().await.expect("persisted cache loads");
    cache.build_index();
    cache
}

const CLOSING_SCENARIO_OPENING_TS: u64 = 1_000_000_000_000;
const CLOSING_SCENARIO_WINDOW_START: u64 = 2_000_000_000_000;
const CLOSING_SCENARIO_ACCEPTED_TS: u64 = 2_000_000_500_000;
const CLOSING_SCENARIO_FILL_TS_1: u64 = 2_000_001_000_000;
const CLOSING_SCENARIO_FILL_TS_2: u64 = 2_000_002_000_000;

fn closing_scenario_strategy_id() -> StrategyId {
    StrategyId::from("STRATEGY-001")
}

fn closing_scenario_position_id() -> PositionId {
    PositionId::new(format!(
        "{}-{}",
        test_instrument_id(),
        closing_scenario_strategy_id()
    ))
}

fn closing_scenario_venue_order_id() -> VenueOrderId {
    VenueOrderId::from("V-CLOSE")
}

/// Registers Netting for the strategy and for EXTERNAL. Kraken spot, whose margin positions
/// this scenario models, reports under a Netting OMS; the `TestContext` default of Hedging for
/// EXTERNAL would key an unclaimed external fill as `P-*` instead of `{instrument}-EXTERNAL`.
fn register_closing_scenario_oms(ctx: &TestContext) {
    let mut engine = ctx.exec_engine.borrow_mut();
    engine.register_oms_type(closing_scenario_strategy_id(), OmsType::Netting);
    engine.register_oms_type(StrategyId::from("EXTERNAL"), OmsType::Netting);
}

/// Caches the filled opening order `O-OPEN` and the open LONG 1.000 @ 3000.00 it created,
/// with the 0.60 USDT opening commission already recorded on the position.
fn cache_closing_scenario_open_long(ctx: &TestContext, instrument: &InstrumentAny) {
    cache_closing_scenario_open_long_with(ctx, instrument, test_account_id(), OmsType::Netting);
}

/// Caches the opening order and the open LONG for the given account under the given OMS.
fn cache_closing_scenario_open_long_with(
    ctx: &TestContext,
    instrument: &InstrumentAny,
    account_id: AccountId,
    oms_type: OmsType,
) {
    let position_id = closing_scenario_position_id();
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(ClientOrderId::from("O-OPEN"))
        .strategy_id(closing_scenario_strategy_id())
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .price(Price::from("3000.00"))
        .build();
    apply_submitted_and_accepted(&mut order, VenueOrderId::from("V-OPEN"));
    let filled = TestOrderEventStubs::filled(
        &order,
        instrument,
        Some(TradeId::from("T-OPEN")),
        Some(position_id),
        Some(Price::from("3000.00")),
        Some(Quantity::from("1.000")),
        Some(LiquiditySide::Maker),
        Some(Money::from("0.60 USDT")),
        Some(UnixNanos::from(CLOSING_SCENARIO_OPENING_TS)),
        Some(account_id),
    );
    order.apply(filled.clone()).unwrap();
    let position = Position::new(instrument, filled.into());
    assert_eq!(position.realized_pnl, Some(Money::from("-0.60 USDT")));

    let mut cache = ctx.cache.borrow_mut();
    cache
        .add_order(order, Some(position_id), Some(test_client_id()), false)
        .unwrap();
    cache.add_position(&position, oms_type).unwrap();
}

/// Caches the accepted closing order `O-CLOSE` (SELL 1.000 limit 3100.00, venue `V-CLOSE`).
fn cache_closing_scenario_closing_order(ctx: &TestContext) -> ClientOrderId {
    cache_closing_scenario_closing_order_for(ctx, test_client_id())
}

/// Caches the accepted closing order `O-CLOSE` as submitted through `client_id`.
fn cache_closing_scenario_closing_order_for(
    ctx: &TestContext,
    client_id: ClientId,
) -> ClientOrderId {
    let client_order_id = ClientOrderId::from("O-CLOSE");
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .strategy_id(closing_scenario_strategy_id())
        .instrument_id(test_instrument_id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from("1.000"))
        .price(Price::from("3100.00"))
        .build();
    apply_submitted_and_accepted(&mut order, closing_scenario_venue_order_id());
    ctx.add_order_with_client_id(order, client_id);
    client_order_id
}

fn closing_scenario_opening_order_report() -> OrderStatusReport {
    let ts = UnixNanos::from(CLOSING_SCENARIO_OPENING_TS);
    OrderStatusReport::new(
        test_account_id(),
        test_instrument_id(),
        Some(ClientOrderId::from("O-OPEN")),
        VenueOrderId::from("V-OPEN"),
        OrderSide::Buy.into(),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Filled,
        Quantity::from("1.000"),
        Quantity::from("1.000"),
        ts,
        ts,
        ts,
        None,
    )
    .with_price(Price::from("3000.00"))
    .with_avg_px(dec!(3000.00))
}

fn closing_scenario_opening_fill_report() -> FillReport {
    let ts = UnixNanos::from(CLOSING_SCENARIO_OPENING_TS);
    FillReport::new(
        test_account_id(),
        test_instrument_id(),
        VenueOrderId::from("V-OPEN"),
        TradeId::from("T-OPEN"),
        OrderSide::Buy,
        Quantity::from("1.000"),
        Price::from("3000.00"),
        Money::from("0.60 USDT"),
        LiquiditySide::Maker,
        Some(ClientOrderId::from("O-OPEN")),
        None,
        ts,
        ts,
        None,
    )
}

fn closing_scenario_closing_order_report(
    client_order_id: Option<ClientOrderId>,
) -> OrderStatusReport {
    OrderStatusReport::new(
        test_account_id(),
        test_instrument_id(),
        client_order_id,
        closing_scenario_venue_order_id(),
        OrderSide::Sell.into(),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Filled,
        Quantity::from("1.000"),
        Quantity::from("1.000"),
        UnixNanos::from(CLOSING_SCENARIO_ACCEPTED_TS),
        UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2),
        UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2),
        None,
    )
    .with_price(Price::from("3100.00"))
    .with_avg_px(dec!(3112.00))
}

/// Two closing fills: 0.400 @ 3100.00 fee 0.25 USDT and 0.600 @ 3120.00 fee 0.37 USDT.
fn closing_scenario_closing_fill_reports(
    client_order_id: Option<ClientOrderId>,
) -> Vec<FillReport> {
    [
        (
            "T-CLOSE-1",
            "0.400",
            "3100.00",
            "0.25 USDT",
            CLOSING_SCENARIO_FILL_TS_1,
        ),
        (
            "T-CLOSE-2",
            "0.600",
            "3120.00",
            "0.37 USDT",
            CLOSING_SCENARIO_FILL_TS_2,
        ),
    ]
    .into_iter()
    .map(|(trade_id, quantity, price, commission, ts)| {
        FillReport::new(
            test_account_id(),
            test_instrument_id(),
            closing_scenario_venue_order_id(),
            TradeId::from(trade_id),
            OrderSide::Sell,
            Quantity::from(quantity),
            Price::from(price),
            Money::from(commission),
            LiquiditySide::Taker,
            client_order_id,
            None,
            UnixNanos::from(ts),
            UnixNanos::from(ts),
            None,
        )
    })
    .collect()
}

/// Builds the venue data without any position report for the instrument. A bounded status
/// declares a window that starts after the opening fill and carries only the closing order and
/// fills; an unbounded status carries the opening order and fill as well.
fn closing_scenario_mass_status(
    bounded: bool,
    closing_client_order_id: Option<ClientOrderId>,
) -> ExecutionMassStatus {
    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2 + 1_000_000),
        Some(UUID4::new()),
    );
    let mut order_reports = vec![closing_scenario_closing_order_report(
        closing_client_order_id,
    )];
    let mut fill_reports = closing_scenario_closing_fill_reports(closing_client_order_id);

    if bounded {
        mass_status.set_report_window(Some(UnixNanos::from(CLOSING_SCENARIO_WINDOW_START)), true);
    } else {
        order_reports.insert(0, closing_scenario_opening_order_report());
        fill_reports.insert(0, closing_scenario_opening_fill_report());
    }

    mass_status.add_order_reports(order_reports);
    mass_status.add_fill_reports(fill_reports);
    mass_status
}

fn capture_position_events() -> (Rc<RefCell<Vec<PositionEvent>>>, TypedHandler<PositionEvent>) {
    let received = Rc::new(RefCell::new(Vec::<PositionEvent>::new()));
    let handler = TypedHandler::from({
        let received = received.clone();
        move |event: &PositionEvent| received.borrow_mut().push(event.clone())
    });
    msgbus::subscribe_position_events("events.position.*".into(), handler.clone(), None);
    (received, handler)
}

fn release_position_events(handler: &TypedHandler<PositionEvent>) {
    msgbus::unsubscribe_position_events("events.position.*".into(), handler);
}

fn count_filled_events(events: &[OrderEventAny]) -> usize {
    events
        .iter()
        .filter(|event| matches!(event, OrderEventAny::Filled(_)))
        .count()
}

/// Asserts that the closing order is FILLED with both trades and that the restored position
/// is closed exactly once by them: realized PnL 110.78 USDT, commissions 1.22 USDT, three fill
/// events, and no second position for the instrument.
fn assert_closing_scenario_recorded_once(cache: &Cache, closing_order_id: ClientOrderId) {
    let instrument_id = test_instrument_id();
    let account_id = test_account_id();

    assert_closing_scenario_order_filled(cache, closing_order_id);

    let position_ids = cache
        .positions(None, Some(&instrument_id), None, Some(&account_id), None)
        .iter()
        .map(|position| position.id)
        .collect::<Vec<_>>();
    assert_eq!(
        position_ids,
        vec![closing_scenario_position_id()],
        "a closing fill must not open an opposite position"
    );
    let position = cache
        .position(&closing_scenario_position_id())
        .expect("restored position cached");
    assert!(
        position.is_closed(),
        "restored position must be closed by its closing fills, found side {:?} quantity {} realized_pnl {:?} trade_ids {:?}",
        position.side,
        position.quantity,
        position.realized_pnl,
        position.trade_ids,
    );
    assert_eq!(position.side, PositionSide::Flat);
    assert_eq!(position.quantity, Quantity::from("0.000"));
    assert_eq!(position.signed_decimal_qty(), dec!(0));
    assert_eq!(position.realized_pnl, Some(Money::from("110.78 USDT")));
    assert_eq!(position.commissions(), vec![Money::from("1.22 USDT")]);
    assert_eq!(
        position.trade_ids,
        [
            TradeId::from("T-OPEN"),
            TradeId::from("T-CLOSE-1"),
            TradeId::from("T-CLOSE-2"),
        ]
        .into_iter()
        .collect::<AHashSet<_>>()
    );
    assert_eq!(position.events.len(), 3);
    assert_eq!(position.closing_order_id, Some(closing_order_id));
    assert_eq!(
        position.ts_closed,
        Some(UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2))
    );
    drop(position);

    assert_eq!(
        cache
            .positions_open(None, Some(&instrument_id), None, Some(&account_id), None)
            .len(),
        0
    );
    assert_eq!(
        cache
            .positions_closed(None, Some(&instrument_id), None, Some(&account_id), None)
            .len(),
        1
    );
}

/// Asserts the position event sequence of a first reconciliation that closes the restored
/// position with two fills: one `PositionChanged` followed by one `PositionClosed`.
fn assert_closing_scenario_position_events(events: &[PositionEvent]) {
    assert_eq!(
        events.len(),
        2,
        "expected PositionChanged then PositionClosed, found {events:?}"
    );
    assert!(matches!(events[0], PositionEvent::PositionChanged(_)));
    assert!(matches!(events[1], PositionEvent::PositionClosed(_)));
}

/// Asserts that the closing order is FILLED with both trades.
fn assert_closing_scenario_order_filled(cache: &Cache, closing_order_id: ClientOrderId) {
    let order = cache
        .order(&closing_order_id)
        .expect("closing order cached");
    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.filled_qty(), Quantity::from("1.000"));
    assert_eq!(
        order.trade_ids().into_iter().copied().collect::<Vec<_>>(),
        vec![TradeId::from("T-CLOSE-1"), TradeId::from("T-CLOSE-2")]
    );
}

/// Asserts that the restored position keeps its opening state: open LONG 1.000, realized PnL
/// -0.60 USDT, commissions 0.60 USDT and the opening trade only.
fn assert_closing_scenario_position_unchanged(cache: &Cache) {
    let position = cache
        .position(&closing_scenario_position_id())
        .expect("restored position cached");
    assert!(position.is_open());
    assert_eq!(position.side, PositionSide::Long);
    assert_eq!(position.quantity, Quantity::from("1.000"));
    assert_eq!(position.realized_pnl, Some(Money::from("-0.60 USDT")));
    assert_eq!(position.commissions(), vec![Money::from("0.60 USDT")]);
    assert_eq!(
        position.trade_ids,
        [TradeId::from("T-OPEN")]
            .into_iter()
            .collect::<AHashSet<_>>()
    );
    assert_eq!(position.events.len(), 1);
}

/// Returns the identifiers of every position for the instrument, across accounts, sorted.
fn closing_scenario_instrument_position_ids(cache: &Cache) -> Vec<PositionId> {
    let mut position_ids: Vec<PositionId> = cache
        .positions(None, Some(&test_instrument_id()), None, None, None)
        .iter()
        .map(|position| position.id)
        .collect();
    position_ids.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    position_ids
}

/// Asserts that the closing fills stay on the closing order only: the order is FILLED with both
/// trades, the restored position keeps its opening state, and the instrument has no other
/// position.
fn assert_closing_scenario_order_only(cache: &Cache, closing_order_id: ClientOrderId) {
    assert_closing_scenario_order_filled(cache, closing_order_id);
    assert_closing_scenario_position_unchanged(cache);
    assert_eq!(
        closing_scenario_instrument_position_ids(cache),
        vec![closing_scenario_position_id()]
    );
}

/// How the closing order reaches reconciliation.
#[derive(Clone, Copy, Debug)]
enum ClosingOrder {
    /// The strategy's order `O-CLOSE` is cached before reconciliation.
    Cached,
    /// The order is placed outside the node and its instrument is claimed for the strategy.
    Claimed,
    /// The order is placed outside the node and its instrument is not claimed.
    Unclaimed,
}

/// Builds a closing-scenario context whose execution client declares the given bulk position
/// coverage.
fn closing_scenario_context(
    cache: Cache,
    config: ExecutionManagerConfig,
    covered: bool,
) -> TestContext {
    let ctx = TestContext::with_cache(cache, config);
    {
        let mut engine = ctx.exec_engine.borrow_mut();
        engine.deregister_client(test_client_id()).unwrap();
        engine
            .register_client(Box::new(
                MockExecutionClient::new(Vec::new()).with_bulk_position_coverage(covered),
            ))
            .unwrap();
    }
    ctx.add_instrument(test_instrument());
    register_closing_scenario_oms(&ctx);
    ctx
}

/// Claims the instrument for the strategy when the closing order is claimed.
fn claim_closing_scenario_instrument(ctx: &mut TestContext, closing: ClosingOrder) {
    if matches!(closing, ClosingOrder::Claimed) {
        ctx.manager
            .claim_external_orders(test_instrument_id(), closing_scenario_strategy_id())
            .unwrap();
    }
}

/// Prepares the closing order and returns the client order ID it is cached under after
/// reconciliation, with the client order ID the venue reports for it.
fn prepare_closing_scenario_order(
    ctx: &mut TestContext,
    closing: ClosingOrder,
) -> (ClientOrderId, Option<ClientOrderId>) {
    match closing {
        ClosingOrder::Cached => {
            let client_order_id = cache_closing_scenario_closing_order(ctx);
            (client_order_id, Some(client_order_id))
        }
        ClosingOrder::Claimed | ClosingOrder::Unclaimed => {
            claim_closing_scenario_instrument(ctx, closing);
            (
                ClientOrderId::from(closing_scenario_venue_order_id().as_str()),
                None,
            )
        }
    }
}

/// Reconciles the mass status and returns the result with the position events it published.
fn reconcile_closing_scenario(
    ctx: &mut TestContext,
    mass_status: &ExecutionMassStatus,
) -> (ReconciliationResult, Vec<PositionEvent>) {
    let (position_events, handler) = capture_position_events();
    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, &ctx.exec_engine);
    release_position_events(&handler);
    let position_events = position_events.borrow().clone();
    (result, position_events)
}

/// A cached or claimed closing order whose fills arrive with no position report, from a client
/// without bulk position coverage, closes the restored position with the fills' realized PnL
/// and fees, whether or not the mass status declares a lookback window that excludes the opening
/// fill.
#[rstest]
#[case::cached_unbounded(ClosingOrder::Cached, false)]
#[case::cached_bounded(ClosingOrder::Cached, true)]
#[case::claimed_unbounded(ClosingOrder::Claimed, false)]
#[case::claimed_bounded(ClosingOrder::Claimed, true)]
fn test_closing_order_fills_close_restored_position_without_coverage(
    #[case] closing: ClosingOrder,
    #[case] bounded: bool,
) {
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    cache_closing_scenario_open_long(&ctx, &test_instrument());
    let (closing_order_id, reported_order_id) = prepare_closing_scenario_order(&mut ctx, closing);

    let mass_status = closing_scenario_mass_status(bounded, reported_order_id);
    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert_eq!(
        result.external_orders.len(),
        usize::from(matches!(closing, ClosingOrder::Claimed))
    );
    assert_eq!(count_filled_events(&result.events), 2);
    assert_closing_scenario_recorded_once(&ctx.cache.borrow(), closing_order_id);
    assert_closing_scenario_position_events(&position_events);
}

/// After the reconciled state is persisted and restored into a fresh node, reconciling the
/// same venue data again leaves the position closed and records the cached or claimed closing
/// order's fills, fees and realized PnL exactly once, for a client without bulk position
/// coverage.
#[rstest]
#[case::cached_unbounded(ClosingOrder::Cached, false)]
#[case::cached_bounded(ClosingOrder::Cached, true)]
#[case::claimed_unbounded(ClosingOrder::Claimed, false)]
#[case::claimed_bounded(ClosingOrder::Claimed, true)]
#[tokio::test]
async fn test_closing_order_fills_recorded_once_across_restart_without_coverage(
    #[case] closing: ClosingOrder,
    #[case] bounded: bool,
) {
    let persisted = Arc::new(Mutex::new(PersistedCacheState::default()));
    let (closing_order_id, reported_order_id) = {
        let mut ctx = closing_scenario_context(
            persisting_cache(&persisted),
            ExecutionManagerConfig::default(),
            false,
        );
        cache_closing_scenario_open_long(&ctx, &test_instrument());
        let (closing_order_id, reported_order_id) =
            prepare_closing_scenario_order(&mut ctx, closing);

        let mass_status = closing_scenario_mass_status(bounded, reported_order_id);
        let (result, _) = reconcile_closing_scenario(&mut ctx, &mass_status);
        assert_eq!(count_filled_events(&result.events), 2);
        assert_closing_scenario_recorded_once(&ctx.cache.borrow(), closing_order_id);
        (closing_order_id, reported_order_id)
    };

    let mut ctx = closing_scenario_context(
        restore_persisted_cache(&persisted).await,
        ExecutionManagerConfig::default(),
        false,
    );
    claim_closing_scenario_instrument(&mut ctx, closing);

    let mass_status = closing_scenario_mass_status(bounded, reported_order_id);
    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert!(result.external_orders.is_empty());
    assert_eq!(count_filled_events(&result.events), 0);
    assert!(
        position_events.is_empty(),
        "restart must not re-apply fills, found {position_events:?}"
    );
    assert_closing_scenario_recorded_once(&ctx.cache.borrow(), closing_order_id);
}

/// Asserts the state an unclaimed closing order leaves: the order is FILLED and the restored
/// position keeps its opening state. Unbounded, the fills also open an opposite SHORT 1.000
/// `EXTERNAL` position carrying their 0.62 USDT commissions; bounded, they open nothing.
fn assert_unclaimed_closing_scenario_state(cache: &Cache, bounded: bool) {
    let closing_order_id = ClientOrderId::from(closing_scenario_venue_order_id().as_str());

    if bounded {
        assert_closing_scenario_order_only(cache, closing_order_id);
        return;
    }

    assert_closing_scenario_order_filled(cache, closing_order_id);
    assert_closing_scenario_position_unchanged(cache);

    let external_position_id = PositionId::new(format!("{}-EXTERNAL", test_instrument_id()));
    assert_eq!(
        closing_scenario_instrument_position_ids(cache),
        vec![external_position_id, closing_scenario_position_id()]
    );
    let external_position = cache
        .position(&external_position_id)
        .expect("external position cached");
    assert!(external_position.is_open());
    assert_eq!(external_position.side, PositionSide::Short);
    assert_eq!(external_position.quantity, Quantity::from("1.000"));
    assert_eq!(
        external_position.commissions(),
        vec![Money::from("0.62 USDT")]
    );
    assert_eq!(
        external_position.trade_ids,
        [TradeId::from("T-CLOSE-1"), TradeId::from("T-CLOSE-2")]
            .into_iter()
            .collect::<AHashSet<_>>()
    );
}

/// Control for external attribution, which this reconciliation rule does not change: a closing
/// order placed outside the node on an unclaimed instrument belongs to `EXTERNAL`, not to the
/// strategy that holds the restored position. Without bulk position coverage, an unbounded mass
/// status applies its fills to an opposite `EXTERNAL` position, and a bounded one keeps them on
/// the order only. Either way the restored position keeps its opening state, and a restart
/// reproduces the same state without re-applying a fill.
#[rstest]
#[case::unbounded(false)]
#[case::bounded(true)]
#[tokio::test]
async fn test_unclaimed_external_closing_order_fills_leave_restored_position_open(
    #[case] bounded: bool,
) {
    let persisted = Arc::new(Mutex::new(PersistedCacheState::default()));
    {
        let mut ctx = closing_scenario_context(
            persisting_cache(&persisted),
            ExecutionManagerConfig::default(),
            false,
        );
        cache_closing_scenario_open_long(&ctx, &test_instrument());
        let (_, reported_order_id) =
            prepare_closing_scenario_order(&mut ctx, ClosingOrder::Unclaimed);

        let mass_status = closing_scenario_mass_status(bounded, reported_order_id);
        let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

        assert!(result.unresolved_positions.is_empty());
        assert_eq!(result.external_orders.len(), 1);
        assert_eq!(count_filled_events(&result.events), 2);
        assert_unclaimed_closing_scenario_state(&ctx.cache.borrow(), bounded);

        if bounded {
            assert!(position_events.is_empty(), "found {position_events:?}");
        } else {
            assert_eq!(position_events.len(), 2, "found {position_events:?}");
            assert!(matches!(
                position_events[0],
                PositionEvent::PositionOpened(_)
            ));
            assert!(matches!(
                position_events[1],
                PositionEvent::PositionChanged(_)
            ));
        }
    }

    let mut ctx = closing_scenario_context(
        restore_persisted_cache(&persisted).await,
        ExecutionManagerConfig::default(),
        false,
    );

    let mass_status = closing_scenario_mass_status(bounded, None);
    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert!(result.external_orders.is_empty());
    assert_eq!(count_filled_events(&result.events), 0);
    assert!(
        position_events.is_empty(),
        "restart must not re-apply fills, found {position_events:?}"
    );
    assert_unclaimed_closing_scenario_state(&ctx.cache.borrow(), bounded);
}

/// A complete bounded status from a client without bulk position coverage closes the restored
/// position with the cached or claimed closing order's fills when the window also holds the
/// opening order and its fill, which the cache has already applied: an order with no unapplied
/// quantity neither qualifies for nor withholds the position it opened.
#[rstest]
#[case::cached(ClosingOrder::Cached)]
#[case::claimed(ClosingOrder::Claimed)]
fn test_bounded_closing_fills_close_position_with_applied_opening_order_in_window(
    #[case] closing: ClosingOrder,
) {
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    cache_closing_scenario_open_long(&ctx, &test_instrument());
    let (closing_order_id, reported_order_id) = prepare_closing_scenario_order(&mut ctx, closing);

    let mut mass_status = closing_scenario_mass_status(false, reported_order_id);
    mass_status.set_report_window(Some(UnixNanos::from(CLOSING_SCENARIO_OPENING_TS - 1)), true);
    assert!(
        mass_status
            .fill_reports()
            .contains_key(&VenueOrderId::from("V-OPEN"))
    );
    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert_eq!(count_filled_events(&result.events), 2);
    assert_closing_scenario_recorded_once(&ctx.cache.borrow(), closing_order_id);
    assert_closing_scenario_position_events(&position_events);
}

/// Bounded closing fills from a client without bulk position coverage stay on the order only
/// when the engine routes the strategy's fills under HEDGING while the retained position is
/// cached under NETTING, since the engine would key them to a new position rather than reduce
/// the retained one. A warning names the position and both OMS types.
#[rstest]
#[case::cached(ClosingOrder::Cached)]
#[case::claimed(ClosingOrder::Claimed)]
#[tokio::test]
async fn test_bounded_closing_fills_stay_order_only_when_engine_oms_is_hedging(
    #[case] closing: ClosingOrder,
) {
    let _log_guard = MANAGER_LOG_TEST_LOCK.lock().await;
    install_manager_log_capture();
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    cache_closing_scenario_open_long(&ctx, &test_instrument());
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(closing_scenario_strategy_id(), OmsType::Hedging);
    let (closing_order_id, reported_order_id) = prepare_closing_scenario_order(&mut ctx, closing);

    let mass_status = closing_scenario_mass_status(true, reported_order_id);
    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert_eq!(count_filled_events(&result.events), 2);
    assert!(position_events.is_empty(), "found {position_events:?}");
    assert_closing_scenario_order_only(&ctx.cache.borrow(), closing_order_id);

    let expected = format!(
        "leave retained position {} unchanged: order {} resolves to it with engine OMS HEDGING \
         and cached OMS NETTING, not NETTING",
        closing_scenario_position_id(),
        closing_scenario_venue_order_id(),
    );
    let messages = MANAGER_LOG_CAPTURE.messages.lock().clone();
    assert!(
        messages.iter().any(|message| message.contains(&expected)),
        "expected warning containing {expected:?}, found {messages:?}"
    );
}

/// Bounded closing fills from a client without bulk position coverage stay on the order only
/// when a claimed order for the same position reports no side and no fills: its direction is
/// unknown, so it may extend the position as readily as reduce it.
#[rstest]
fn test_bounded_closing_fills_with_unknown_side_order_stay_order_only() {
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    cache_closing_scenario_open_long(&ctx, &test_instrument());
    let (closing_order_id, reported_order_id) =
        prepare_closing_scenario_order(&mut ctx, ClosingOrder::Claimed);

    let ts = UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2 + 500_000);
    let mut unknown_side_report = OrderStatusReport::new(
        test_account_id(),
        test_instrument_id(),
        None,
        VenueOrderId::from("V-UNKNOWN"),
        OrderSide::Buy.into(),
        OrderType::Market,
        TimeInForce::Gtc,
        OrderStatus::Filled,
        Quantity::from("0.500"),
        Quantity::from("0.500"),
        ts,
        ts,
        ts,
        None,
    )
    .with_avg_px(dec!(3130.00));
    unknown_side_report.order_side = None;

    let mut mass_status = closing_scenario_mass_status(true, reported_order_id);
    mass_status.add_order_reports(vec![unknown_side_report]);
    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert_eq!(count_filled_events(&result.events), 2);
    assert!(position_events.is_empty(), "found {position_events:?}");
    assert_closing_scenario_order_only(&ctx.cache.borrow(), closing_order_id);
}

/// Bounded closing fills stay on the order only when the strategy has no registered OMS and the
/// cached closing order carries a HEDGING client as its origin, though the reporting client
/// without bulk position coverage is NETTING: the engine routes a fill by the order's own
/// client, so it would key the fills to a new position rather than reduce the retained one.
#[rstest]
fn test_bounded_closing_fills_stay_order_only_when_the_orders_client_is_hedging() {
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    {
        let mut engine = ctx.exec_engine.borrow_mut();
        engine.deregister_client(test_client_id()).unwrap();
        engine
            .register_client(Box::new(
                MockExecutionClient::new(Vec::new())
                    .with_bulk_position_coverage(false)
                    .with_oms_type(OmsType::Netting),
            ))
            .unwrap();
        engine
            .register_client(Box::new(MockExecutionClient::for_venue(
                ClientId::from("SIM-HEDGING"),
                test_venue(),
                Vec::new(),
            )))
            .unwrap();
        engine.register_oms_type(closing_scenario_strategy_id(), OmsType::Unspecified);
    }
    cache_closing_scenario_open_long(&ctx, &test_instrument());
    let closing_order_id =
        cache_closing_scenario_closing_order_for(&ctx, ClientId::from("SIM-HEDGING"));

    let mass_status = closing_scenario_mass_status(true, Some(closing_order_id));
    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert_eq!(count_filled_events(&result.events), 2);
    assert!(position_events.is_empty(), "found {position_events:?}");
    assert_closing_scenario_order_only(&ctx.cache.borrow(), closing_order_id);
}

/// Bounded closing fills from a client without bulk position coverage stay on the order only
/// when one of the closing order's fills reports the opening side, since the engine applies
/// each fill with its own side and that fill would extend the position.
#[rstest]
#[case::cached(ClosingOrder::Cached)]
#[case::claimed(ClosingOrder::Claimed)]
fn test_bounded_closing_fills_with_an_opening_side_fill_stay_order_only(
    #[case] closing: ClosingOrder,
) {
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    cache_closing_scenario_open_long(&ctx, &test_instrument());
    let (_, reported_order_id) = prepare_closing_scenario_order(&mut ctx, closing);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2 + 1_000_000),
        Some(UUID4::new()),
    );
    mass_status.set_report_window(Some(UnixNanos::from(CLOSING_SCENARIO_WINDOW_START)), true);
    mass_status.add_order_reports(vec![closing_scenario_closing_order_report(
        reported_order_id,
    )]);
    let mut fill_reports = closing_scenario_closing_fill_reports(reported_order_id);
    fill_reports[1].order_side = OrderSide::Buy;
    mass_status.add_fill_reports(fill_reports);
    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert!(position_events.is_empty(), "found {position_events:?}");
    let cache = ctx.cache.borrow();
    let position = cache.position(&closing_scenario_position_id()).unwrap();
    assert!(position.is_long(), "found {position:?}");
    assert_eq!(position.quantity, Quantity::from("1.000"));
}

/// Bounded closing fills for an instrument with no position report stay on the order only, and
/// the restored position keeps its opening state, when the venue data cannot stand in for the
/// missing report:
///
/// - A client with bulk position coverage reports every position it holds, so a missing report
///   is that client's account of the instrument and the retained position is not consulted.
/// - Incomplete history may omit fills that already changed the position, so the reported fills
///   cannot establish what the closing order leaves open. This matches the contract that
///   incomplete bounded fills with no position report stay order-only.
/// - Position-report filtering makes every bounded fill order-only.
#[rstest]
#[case::covered_cached(ClosingOrder::Cached, true, true, false)]
#[case::covered_claimed(ClosingOrder::Claimed, true, true, false)]
#[case::incomplete_cached(ClosingOrder::Cached, false, false, false)]
#[case::incomplete_claimed(ClosingOrder::Claimed, false, false, false)]
#[case::filtered_cached(ClosingOrder::Cached, false, true, true)]
#[case::filtered_claimed(ClosingOrder::Claimed, false, true, true)]
fn test_bounded_closing_order_fills_stay_order_only(
    #[case] closing: ClosingOrder,
    #[case] covered: bool,
    #[case] reports_complete: bool,
    #[case] filter_position_reports: bool,
) {
    let config = ExecutionManagerConfig {
        filter_position_reports,
        ..ExecutionManagerConfig::default()
    };
    let mut ctx = closing_scenario_context(Cache::default(), config, covered);
    cache_closing_scenario_open_long(&ctx, &test_instrument());
    let (closing_order_id, reported_order_id) = prepare_closing_scenario_order(&mut ctx, closing);

    let mut mass_status = closing_scenario_mass_status(true, reported_order_id);
    mass_status.set_report_window(
        Some(UnixNanos::from(CLOSING_SCENARIO_WINDOW_START)),
        reports_complete,
    );
    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert_eq!(count_filled_events(&result.events), 2);
    assert!(position_events.is_empty(), "found {position_events:?}");
    assert_closing_scenario_order_only(&ctx.cache.borrow(), closing_order_id);
}

/// Bounded closing fills from a client without bulk position coverage open no position when
/// no position is retained for them: the closing order reaches FILLED and the instrument has no
/// position.
#[rstest]
#[case::cached(ClosingOrder::Cached)]
#[case::claimed(ClosingOrder::Claimed)]
fn test_bounded_closing_order_fills_without_retained_position_stay_order_only(
    #[case] closing: ClosingOrder,
) {
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    let (closing_order_id, reported_order_id) = prepare_closing_scenario_order(&mut ctx, closing);

    let mass_status = closing_scenario_mass_status(true, reported_order_id);
    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert_eq!(count_filled_events(&result.events), 2);
    assert!(position_events.is_empty(), "found {position_events:?}");
    assert_closing_scenario_order_filled(&ctx.cache.borrow(), closing_order_id);
    assert!(closing_scenario_instrument_position_ids(&ctx.cache.borrow()).is_empty());
}

/// Bounded fills from a client without bulk position coverage close a retained position only
/// when every cached or claimed order resolving to it is on its closing side and together they
/// do not exceed its quantity. A second closing order that would take the position short, or a
/// same-side order that would extend it, keeps every fill for that position on its order only.
#[rstest]
#[case::crossing(OrderSide::Sell)]
#[case::extending(OrderSide::Buy)]
fn test_bounded_fills_beyond_retained_position_stay_order_only(#[case] extra_side: OrderSide) {
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    cache_closing_scenario_open_long(&ctx, &test_instrument());
    let (closing_order_id, reported_order_id) =
        prepare_closing_scenario_order(&mut ctx, ClosingOrder::Cached);

    let extra_order_id = ClientOrderId::from("O-EXTRA");
    let extra_venue_order_id = VenueOrderId::from("V-EXTRA");
    let mut extra_order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(extra_order_id)
        .strategy_id(closing_scenario_strategy_id())
        .instrument_id(test_instrument_id())
        .side(extra_side)
        .quantity(Quantity::from("0.500"))
        .price(Price::from("3130.00"))
        .build();
    apply_submitted_and_accepted(&mut extra_order, extra_venue_order_id);
    ctx.add_order(extra_order);

    let (mut extra_report, mut extra_fill) = create_bounded_fill_lifecycle(
        test_instrument_id(),
        extra_venue_order_id,
        TradeId::from("T-EXTRA"),
        extra_side,
        "0.500",
        "3130.00",
        false,
        UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2 + 500_000),
    );
    extra_report.client_order_id = Some(extra_order_id);
    extra_fill.client_order_id = Some(extra_order_id);

    let mut mass_status = closing_scenario_mass_status(true, reported_order_id);
    mass_status.add_order_reports(vec![extra_report]);
    mass_status.add_fill_reports(vec![extra_fill]);
    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert_eq!(count_filled_events(&result.events), 3);
    assert!(position_events.is_empty(), "found {position_events:?}");
    assert_closing_scenario_order_only(&ctx.cache.borrow(), closing_order_id);
    let extra_order = ctx.get_order(&extra_order_id).expect("extra order cached");
    assert_eq!(extra_order.status(), OrderStatus::Filled);
    assert_eq!(extra_order.filled_qty(), Quantity::from("0.500"));
}

/// Bounded closing fills from a client without bulk position coverage stay on the order only
/// when the open position they share an ID with is not the account's NETTING position: a
/// HEDGING position does not receive a fill without an assigned position ID, and a position
/// held by another account is not the reporting account's exposure.
#[rstest]
#[case::hedging(test_account_id(), OmsType::Hedging)]
#[case::other_account(AccountId::from("BINANCE-002"), OmsType::Netting)]
fn test_bounded_closing_order_fills_for_unresolved_position_stay_order_only(
    #[case] account_id: AccountId,
    #[case] oms_type: OmsType,
) {
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(closing_scenario_strategy_id(), oms_type);
    cache_closing_scenario_open_long_with(&ctx, &test_instrument(), account_id, oms_type);
    let (closing_order_id, reported_order_id) =
        prepare_closing_scenario_order(&mut ctx, ClosingOrder::Cached);

    let mass_status = closing_scenario_mass_status(true, reported_order_id);
    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert_eq!(count_filled_events(&result.events), 2);
    assert!(position_events.is_empty(), "found {position_events:?}");
    assert_closing_scenario_order_only(&ctx.cache.borrow(), closing_order_id);
}

/// A complete bounded status from a client without bulk position coverage and without a
/// position report for the instrument applies a partial closing fill to the restored position,
/// which stays open LONG 0.600 with the fill's realized PnL and fee.
///
/// The cases follow the report shapes of a client that reports no positions for an account it
/// shares, as a session-scoped Polymarket client does:
/// - A cached order still open at the venue arrives as a PARTIALLY_FILLED report with its fill,
///   which may predate the window when the client recovers the trades of an open order.
/// - A claimed order that closed at the venue arrives as a FILLED market report built from its
///   fills, with no client order ID or limit price.
#[rstest]
#[case::cached_open_in_window(ClosingOrder::Cached, CLOSING_SCENARIO_FILL_TS_1)]
#[case::cached_open_before_window(ClosingOrder::Cached, CLOSING_SCENARIO_WINDOW_START - 1)]
#[case::claimed_closed(ClosingOrder::Claimed, CLOSING_SCENARIO_FILL_TS_1)]
fn test_bounded_partial_closing_fill_reduces_restored_position_without_coverage(
    #[case] closing: ClosingOrder,
    #[case] fill_ts: u64,
) {
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    cache_closing_scenario_open_long(&ctx, &test_instrument());
    let (closing_order_id, reported_order_id) = prepare_closing_scenario_order(&mut ctx, closing);

    let fill_ts = UnixNanos::from(fill_ts);
    let (order_type, order_status, quantity) = match closing {
        ClosingOrder::Cached => (
            OrderType::Limit,
            OrderStatus::PartiallyFilled,
            Quantity::from("1.000"),
        ),
        ClosingOrder::Claimed | ClosingOrder::Unclaimed => (
            OrderType::Market,
            OrderStatus::Filled,
            Quantity::from("0.400"),
        ),
    };
    let mut order_report = OrderStatusReport::new(
        test_account_id(),
        test_instrument_id(),
        reported_order_id,
        closing_scenario_venue_order_id(),
        OrderSide::Sell.into(),
        order_type,
        TimeInForce::Gtc,
        order_status,
        quantity,
        Quantity::from("0.400"),
        fill_ts.min(UnixNanos::from(CLOSING_SCENARIO_ACCEPTED_TS)),
        fill_ts,
        UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2),
        None,
    )
    .with_avg_px(dec!(3100.00));

    if order_type == OrderType::Limit {
        order_report = order_report.with_price(Price::from("3100.00"));
    }

    let fill_report = FillReport::new(
        test_account_id(),
        test_instrument_id(),
        closing_scenario_venue_order_id(),
        TradeId::from("T-CLOSE-1"),
        OrderSide::Sell,
        Quantity::from("0.400"),
        Price::from("3100.00"),
        Money::from("0.25 USDT"),
        LiquiditySide::Taker,
        reported_order_id,
        None,
        fill_ts,
        fill_ts,
        None,
    );
    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2),
        Some(UUID4::new()),
    );
    mass_status.set_report_window(Some(UnixNanos::from(CLOSING_SCENARIO_WINDOW_START)), true);
    mass_status.add_order_reports(vec![order_report]);
    mass_status.add_fill_reports(vec![fill_report]);
    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert_eq!(count_filled_events(&result.events), 1);
    assert_eq!(position_events.len(), 1, "found {position_events:?}");
    assert!(matches!(
        position_events[0],
        PositionEvent::PositionChanged(_)
    ));

    let cache = ctx.cache.borrow();
    let order = cache
        .order(&closing_order_id)
        .expect("closing order cached");
    assert_eq!(order.status(), order_status);
    assert_eq!(order.filled_qty(), Quantity::from("0.400"));
    assert_eq!(
        closing_scenario_instrument_position_ids(&cache),
        vec![closing_scenario_position_id()]
    );
    let position = cache
        .position(&closing_scenario_position_id())
        .expect("restored position cached");
    assert!(position.is_open());
    assert_eq!(position.side, PositionSide::Long);
    assert_eq!(position.quantity, Quantity::from("0.600"));
    assert_eq!(position.realized_pnl, Some(Money::from("39.15 USDT")));
    assert_eq!(position.commissions(), vec![Money::from("0.85 USDT")]);
    assert_eq!(
        position.trade_ids,
        [TradeId::from("T-OPEN"), TradeId::from("T-CLOSE-1")]
            .into_iter()
            .collect::<AHashSet<_>>()
    );
}

/// A closing trade reported more than once for its order counts once toward the restored
/// position's quantity, as event processing applies each trade of an order once: a complete
/// bounded status from a client without bulk position coverage that repeats a closing fill
/// closes the restored position with each trade, fee and realized PnL recorded once, with or
/// without an order report for the closing order.
#[rstest]
#[case::cached_with_report(ClosingOrder::Cached, true)]
#[case::claimed_with_report(ClosingOrder::Claimed, true)]
#[case::cached_fills_only(ClosingOrder::Cached, false)]
fn test_bounded_repeated_closing_fill_counts_once_toward_restored_position(
    #[case] closing: ClosingOrder,
    #[case] with_order_report: bool,
) {
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    cache_closing_scenario_open_long(&ctx, &test_instrument());
    let (closing_order_id, reported_order_id) = prepare_closing_scenario_order(&mut ctx, closing);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2 + 1_000_000),
        Some(UUID4::new()),
    );
    mass_status.set_report_window(Some(UnixNanos::from(CLOSING_SCENARIO_WINDOW_START)), true);

    if with_order_report {
        mass_status.add_order_reports(vec![closing_scenario_closing_order_report(
            reported_order_id,
        )]);
    }

    let mut fill_reports = closing_scenario_closing_fill_reports(reported_order_id);
    fill_reports.push(fill_reports[1].clone());
    mass_status.add_fill_reports(fill_reports);
    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert_eq!(
        result.external_orders.len(),
        usize::from(matches!(closing, ClosingOrder::Claimed))
    );
    assert_eq!(count_filled_events(&result.events), 2);
    assert_closing_scenario_recorded_once(&ctx.cache.borrow(), closing_order_id);
    assert_closing_scenario_position_events(&position_events);
}

/// The copies of a closing trade count toward the restored position's open quantity at no less
/// than event processing can apply: the largest copy for an order that is cached or reported,
/// since it applies one copy of each trade, and every copy for an order reconciliation creates
/// from fills alone, since it fills that order by every copy and infers the copies it does not
/// apply. A complete bounded status from a client without bulk position coverage reduces the
/// restored position when that count fits the open quantity, and otherwise leaves it unchanged
/// with a warning naming the position.
#[rstest]
#[case::claimed_fills_only_one_copy(ClosingOrder::Claimed, &["0.600"], Some("0.400"))]
#[case::claimed_fills_only_two_copies(ClosingOrder::Claimed, &["0.600", "0.600"], None)]
#[case::cached_largest_copy_between_smaller(
    ClosingOrder::Cached,
    &["0.300", "1.200", "0.500"],
    None
)]
#[tokio::test]
async fn test_bounded_closing_trade_copies_count_toward_restored_position_as_applied(
    #[case] closing: ClosingOrder,
    #[case] copy_qtys: &[&str],
    #[case] remaining_qty: Option<&str>,
) {
    let _log_guard = MANAGER_LOG_TEST_LOCK.lock().await;
    install_manager_log_capture();
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    cache_closing_scenario_open_long(&ctx, &test_instrument());
    let (_, reported_order_id) = prepare_closing_scenario_order(&mut ctx, closing);

    // Reconciliation creates an order from fills alone only when they name a venue position
    let fill_ts = UnixNanos::from(CLOSING_SCENARIO_FILL_TS_1);
    let fill_reports = copy_qtys
        .iter()
        .map(|qty| {
            FillReport::new(
                test_account_id(),
                test_instrument_id(),
                closing_scenario_venue_order_id(),
                TradeId::from("T-CLOSE"),
                OrderSide::Sell,
                Quantity::from(*qty),
                Price::from("3100.00"),
                Money::from("0.25 USDT"),
                LiquiditySide::Taker,
                reported_order_id,
                Some(PositionId::from("P-VENUE")),
                fill_ts,
                fill_ts,
                None,
            )
        })
        .collect();
    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2),
        Some(UUID4::new()),
    );
    mass_status.set_report_window(Some(UnixNanos::from(CLOSING_SCENARIO_WINDOW_START)), true);
    mass_status.add_fill_reports(fill_reports);
    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());

    if let Some(remaining_qty) = remaining_qty {
        assert_eq!(position_events.len(), 1, "found {position_events:?}");
        let cache = ctx.cache.borrow();
        let position = cache
            .position(&closing_scenario_position_id())
            .expect("restored position cached");
        assert_eq!(position.side, PositionSide::Long);
        assert_eq!(position.quantity, Quantity::from(remaining_qty));
        assert_eq!(
            position.trade_ids,
            [TradeId::from("T-OPEN"), TradeId::from("T-CLOSE")]
                .into_iter()
                .collect::<AHashSet<_>>()
        );
    } else {
        assert!(position_events.is_empty(), "found {position_events:?}");
        assert_closing_scenario_position_unchanged(&ctx.cache.borrow());

        let expected = format!(
            "leave retained position {} unchanged: unapplied closing quantity 1.200 exceeds open \
             quantity 1.000",
            closing_scenario_position_id(),
        );
        let messages = MANAGER_LOG_CAPTURE.messages.lock().clone();
        assert!(
            messages.iter().any(|message| message.contains(&expected)),
            "expected warning containing {expected:?}, found {messages:?}"
        );
    }
}

/// Bounded closing fills from a client without bulk position coverage stay on the order only
/// when an unapplied fill of the closing order carries another account than the retained
/// position: the engine applies each fill to the resolved position under the fill's own
/// account, so that fill would change the position with another account's trade. A warning
/// names the position, the fill and both accounts.
#[rstest]
#[case::cached_with_report(ClosingOrder::Cached, true)]
#[case::cached_without_report(ClosingOrder::Cached, false)]
#[case::claimed_with_report(ClosingOrder::Claimed, true)]
#[tokio::test]
async fn test_bounded_closing_fills_with_another_accounts_fill_stay_order_only(
    #[case] closing: ClosingOrder,
    #[case] with_report: bool,
) {
    let _log_guard = MANAGER_LOG_TEST_LOCK.lock().await;
    install_manager_log_capture();
    let other_account_id = AccountId::from("BINANCE-002");
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    // The engine changes a position only for a fill whose account is cached
    ctx.add_margin_account(other_account_id);
    cache_closing_scenario_open_long(&ctx, &test_instrument());
    let (closing_order_id, reported_order_id) = prepare_closing_scenario_order(&mut ctx, closing);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2 + 1_000_000),
        Some(UUID4::new()),
    );
    mass_status.set_report_window(Some(UnixNanos::from(CLOSING_SCENARIO_WINDOW_START)), true);

    if with_report {
        mass_status.add_order_reports(vec![closing_scenario_closing_order_report(
            reported_order_id,
        )]);
    }

    let mut fill_reports = closing_scenario_closing_fill_reports(reported_order_id);
    fill_reports[1].account_id = other_account_id;
    mass_status.add_fill_reports(fill_reports);
    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert_eq!(count_filled_events(&result.events), 2);
    assert!(position_events.is_empty(), "found {position_events:?}");
    assert_closing_scenario_order_only(&ctx.cache.borrow(), closing_order_id);

    let expected = format!(
        "leave retained position {} unchanged: order {} fill T-CLOSE-2 account \
         {other_account_id} is not position account {}",
        closing_scenario_position_id(),
        closing_scenario_venue_order_id(),
        test_account_id(),
    );
    let messages = MANAGER_LOG_CAPTURE.messages.lock().clone();
    assert!(
        messages.iter().any(|message| message.contains(&expected)),
        "expected warning containing {expected:?}, found {messages:?}"
    );
}

/// Bounded closing fills from a client without bulk position coverage stay on the order only
/// when the closing order's report names another account than the retained position while its
/// fills and the account any fill is inferred under name the position's: an order naming two
/// accounts does not establish which account's position it moves. A warning names the position,
/// the report and both accounts.
#[rstest]
#[case::cached(ClosingOrder::Cached)]
#[case::claimed(ClosingOrder::Claimed)]
#[tokio::test]
async fn test_bounded_closing_fills_with_another_accounts_report_stay_order_only(
    #[case] closing: ClosingOrder,
) {
    let _log_guard = MANAGER_LOG_TEST_LOCK.lock().await;
    install_manager_log_capture();
    let other_account_id = AccountId::from("BINANCE-002");
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    // The engine changes a position only for a fill whose account is cached
    ctx.add_margin_account(other_account_id);
    cache_closing_scenario_open_long(&ctx, &test_instrument());
    let (closing_order_id, reported_order_id) = prepare_closing_scenario_order(&mut ctx, closing);

    let mut order_report = closing_scenario_closing_order_report(reported_order_id);
    order_report.account_id = other_account_id;
    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2 + 1_000_000),
        Some(UUID4::new()),
    );
    mass_status.set_report_window(Some(UnixNanos::from(CLOSING_SCENARIO_WINDOW_START)), true);
    mass_status.add_order_reports(vec![order_report]);
    mass_status.add_fill_reports(closing_scenario_closing_fill_reports(reported_order_id));
    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert_eq!(count_filled_events(&result.events), 2);
    assert!(position_events.is_empty(), "found {position_events:?}");
    assert_closing_scenario_order_only(&ctx.cache.borrow(), closing_order_id);

    let expected = format!(
        "leave retained position {} unchanged: order {} report account {other_account_id} is not \
         position account {}",
        closing_scenario_position_id(),
        closing_scenario_venue_order_id(),
        test_account_id(),
    );
    let messages = MANAGER_LOG_CAPTURE.messages.lock().clone();
    assert!(
        messages.iter().any(|message| message.contains(&expected)),
        "expected warning containing {expected:?}, found {messages:?}"
    );
}

/// Bounded closing fills from a client without bulk position coverage stay on the order only
/// when the cached closing order is held under another account than the retained position,
/// whether its fills are reported or inferred from its report: the engine infers a fill under
/// the cached order's account, and an order the cache holds for another account does not
/// establish which account's position its reported fills reduce. A warning names the position
/// and both accounts.
#[rstest]
#[case::reported_fills(true)]
#[case::inferred_fill(false)]
#[tokio::test]
async fn test_bounded_closing_fills_of_another_accounts_cached_order_stay_order_only(
    #[case] with_fills: bool,
) {
    let _log_guard = MANAGER_LOG_TEST_LOCK.lock().await;
    install_manager_log_capture();
    let other_account_id = AccountId::from("BINANCE-002");
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    // The engine changes a position only for a fill whose account is cached
    ctx.add_margin_account(other_account_id);
    cache_closing_scenario_open_long(&ctx, &test_instrument());

    let closing_order_id = ClientOrderId::from("O-CLOSE");
    let mut closing_order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(closing_order_id)
        .strategy_id(closing_scenario_strategy_id())
        .instrument_id(test_instrument_id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from("1.000"))
        .price(Price::from("3100.00"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&closing_order, other_account_id);
    closing_order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(
        &closing_order,
        other_account_id,
        closing_scenario_venue_order_id(),
    );
    closing_order.apply(accepted).unwrap();
    ctx.add_order(closing_order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2 + 1_000_000),
        Some(UUID4::new()),
    );
    mass_status.set_report_window(Some(UnixNanos::from(CLOSING_SCENARIO_WINDOW_START)), true);
    mass_status.add_order_reports(vec![closing_scenario_closing_order_report(Some(
        closing_order_id,
    ))]);

    if with_fills {
        mass_status.add_fill_reports(closing_scenario_closing_fill_reports(Some(
            closing_order_id,
        )));
    }

    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert_eq!(
        count_filled_events(&result.events),
        if with_fills { 2 } else { 1 }
    );
    assert!(position_events.is_empty(), "found {position_events:?}");
    {
        let cache = ctx.cache.borrow();
        assert_closing_scenario_position_unchanged(&cache);
        assert_eq!(
            closing_scenario_instrument_position_ids(&cache),
            vec![closing_scenario_position_id()]
        );
        let order = cache
            .order(&closing_order_id)
            .expect("closing order cached");
        assert_eq!(order.status(), OrderStatus::Filled);
        assert_eq!(order.filled_qty(), Quantity::from("1.000"));
    }

    let expected = format!(
        "leave retained position {} unchanged: order {} account {other_account_id} is not \
         position account {}",
        closing_scenario_position_id(),
        closing_scenario_venue_order_id(),
        test_account_id(),
    );
    let messages = MANAGER_LOG_CAPTURE.messages.lock().clone();
    assert!(
        messages.iter().any(|message| message.contains(&expected)),
        "expected warning containing {expected:?}, found {messages:?}"
    );
}

/// A claimed closing order reported by a client without bulk position coverage stays on the
/// order only when the mass status reports for another account than the retained position: the
/// engine materializes the order under the reporting account and infers its fill under that
/// account, so the fill would change the position with another account's trade. A warning names
/// the position and both accounts.
#[rstest]
#[tokio::test]
async fn test_bounded_claimed_closing_fill_for_another_reporting_account_stays_order_only() {
    let _log_guard = MANAGER_LOG_TEST_LOCK.lock().await;
    install_manager_log_capture();
    let other_account_id = AccountId::from("BINANCE-002");
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    // The engine changes a position only for a fill whose account is cached
    ctx.add_margin_account(other_account_id);
    cache_closing_scenario_open_long(&ctx, &test_instrument());
    let (closing_order_id, reported_order_id) =
        prepare_closing_scenario_order(&mut ctx, ClosingOrder::Claimed);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        other_account_id,
        test_venue(),
        UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2 + 1_000_000),
        Some(UUID4::new()),
    );
    mass_status.set_report_window(Some(UnixNanos::from(CLOSING_SCENARIO_WINDOW_START)), true);
    mass_status.add_order_reports(vec![closing_scenario_closing_order_report(
        reported_order_id,
    )]);
    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert_eq!(result.external_orders.len(), 1);
    assert_eq!(count_filled_events(&result.events), 1);
    assert!(position_events.is_empty(), "found {position_events:?}");
    {
        let cache = ctx.cache.borrow();
        assert_closing_scenario_position_unchanged(&cache);
        assert_eq!(
            closing_scenario_instrument_position_ids(&cache),
            vec![closing_scenario_position_id()]
        );
        let order = cache
            .order(&closing_order_id)
            .expect("closing order cached");
        assert_eq!(order.status(), OrderStatus::Filled);
        assert_eq!(order.account_id(), Some(other_account_id));
    }

    let expected = format!(
        "leave retained position {} unchanged: order {} account {other_account_id} is not \
         position account {}",
        closing_scenario_position_id(),
        closing_scenario_venue_order_id(),
        test_account_id(),
    );
    let messages = MANAGER_LOG_CAPTURE.messages.lock().clone();
    assert!(
        messages.iter().any(|message| message.contains(&expected)),
        "expected warning containing {expected:?}, found {messages:?}"
    );
}

/// Bounded closing fills from a client without bulk position coverage leave the retained
/// position unchanged beside an order that names the position's account and another one. An
/// order names the account of its report, of each unapplied fill and of any fill the engine
/// infers for it (the cached order's, else the reporting account), and an order naming two
/// accounts does not establish which account's position it moves, so the closing order alone
/// does not close the position. A warning names the position and the first other account.
#[rstest]
#[case::cached_report_with_fill(
    Some("BINANCE-001"),
    Some("BINANCE-002"),
    &["BINANCE-001"],
    "report account BINANCE-002"
)]
#[case::cached_report_with_inferred_fill(
    Some("BINANCE-001"),
    Some("BINANCE-002"),
    &[],
    "report account BINANCE-002"
)]
#[case::cached_report_and_fill(
    Some("BINANCE-001"),
    Some("BINANCE-002"),
    &["BINANCE-002"],
    "report account BINANCE-002"
)]
#[case::claimed_report(None, Some("BINANCE-002"), &[], "report account BINANCE-002")]
#[case::cached_fills_only(
    Some("BINANCE-001"),
    None,
    &["BINANCE-002", "BINANCE-001"],
    "fill T-ADD-1 account BINANCE-002"
)]
#[case::cached_fill_only(
    Some("BINANCE-001"),
    None,
    &["BINANCE-002"],
    "fill T-ADD-1 account BINANCE-002"
)]
#[case::other_accounts_cached_fill_only(
    Some("BINANCE-002"),
    None,
    &["BINANCE-001"],
    "account BINANCE-002"
)]
#[tokio::test]
async fn test_bounded_closing_fills_beside_an_order_naming_two_accounts_stay_order_only(
    #[case] cached_account: Option<&str>,
    #[case] report_account: Option<&str>,
    #[case] fill_accounts: &[&str],
    #[case] other_account: &str,
) {
    let _log_guard = MANAGER_LOG_TEST_LOCK.lock().await;
    install_manager_log_capture();
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    // The engine changes a position only for a fill whose account is cached
    ctx.add_margin_account(AccountId::from("BINANCE-002"));
    cache_closing_scenario_open_long(&ctx, &test_instrument());
    let closing_order_id = cache_closing_scenario_closing_order(&ctx);

    let add_venue_order_id = VenueOrderId::from("V-ADD");
    let reported_add_order_id = if let Some(cached_account) = cached_account {
        let cached_account = AccountId::from(cached_account);
        let client_order_id = ClientOrderId::from("O-ADD");
        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .client_order_id(client_order_id)
            .strategy_id(closing_scenario_strategy_id())
            .instrument_id(test_instrument_id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("0.500"))
            .price(Price::from("3050.00"))
            .build();
        let submitted = TestOrderEventStubs::submitted(&order, cached_account);
        order.apply(submitted).unwrap();
        let accepted = TestOrderEventStubs::accepted(&order, cached_account, add_venue_order_id);
        order.apply(accepted).unwrap();
        ctx.add_order(order);
        Some(client_order_id)
    } else {
        claim_closing_scenario_instrument(&mut ctx, ClosingOrder::Claimed);
        None
    };

    let mut mass_status = closing_scenario_mass_status(true, Some(closing_order_id));
    let fill_ts = UnixNanos::from(CLOSING_SCENARIO_FILL_TS_1 + 500_000);

    if let Some(report_account) = report_account {
        mass_status.add_order_reports(vec![
            OrderStatusReport::new(
                AccountId::from(report_account),
                test_instrument_id(),
                reported_add_order_id,
                add_venue_order_id,
                OrderSide::Buy.into(),
                OrderType::Limit,
                TimeInForce::Gtc,
                OrderStatus::Filled,
                Quantity::from("0.500"),
                Quantity::from("0.500"),
                UnixNanos::from(CLOSING_SCENARIO_ACCEPTED_TS),
                fill_ts,
                fill_ts,
                None,
            )
            .with_price(Price::from("3050.00"))
            .with_avg_px(dec!(3050.00)),
        ]);
    }

    let fill_qty = if fill_accounts.len() > 1 {
        "0.250"
    } else {
        "0.500"
    };
    let fill_reports = fill_accounts
        .iter()
        .zip([
            ("T-ADD-1", CLOSING_SCENARIO_FILL_TS_1 + 200_000),
            ("T-ADD-2", CLOSING_SCENARIO_FILL_TS_1 + 500_000),
        ])
        .map(|(account, (trade_id, ts))| {
            FillReport::new(
                AccountId::from(*account),
                test_instrument_id(),
                add_venue_order_id,
                TradeId::from(trade_id),
                OrderSide::Buy,
                Quantity::from(fill_qty),
                Price::from("3050.00"),
                Money::from("0.15 USDT"),
                LiquiditySide::Maker,
                reported_add_order_id,
                None,
                UnixNanos::from(ts),
                UnixNanos::from(ts),
                None,
            )
        })
        .collect();
    mass_status.add_fill_reports(fill_reports);
    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert!(position_events.is_empty(), "found {position_events:?}");
    {
        let cache = ctx.cache.borrow();
        assert_closing_scenario_order_only(&cache, closing_order_id);
        let add_order_id = reported_add_order_id
            .unwrap_or_else(|| ClientOrderId::from(add_venue_order_id.as_str()));
        let add_order = cache.order(&add_order_id).expect("added order cached");
        assert_eq!(add_order.filled_qty(), Quantity::from("0.500"));
    }

    let expected = format!(
        "leave retained position {} unchanged: order {add_venue_order_id} {other_account} is not \
         position account {}",
        closing_scenario_position_id(),
        test_account_id(),
    );
    let messages = MANAGER_LOG_CAPTURE.messages.lock().clone();
    assert!(
        messages.iter().any(|message| message.contains(&expected)),
        "expected warning containing {expected:?}, found {messages:?}"
    );
}

/// Bounded closing fills from a client without bulk position coverage close the retained
/// position beside an order whose report, fill and cached account all name another account:
/// that order moves another account's position, so it neither counts toward the retained
/// position nor withholds it.
#[rstest]
#[tokio::test]
async fn test_bounded_closing_fills_close_position_beside_another_accounts_order() {
    let _log_guard = MANAGER_LOG_TEST_LOCK.lock().await;
    install_manager_log_capture();
    let other_account_id = AccountId::from("BINANCE-002");
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    // The engine changes a position only for a fill whose account is cached
    ctx.add_margin_account(other_account_id);
    cache_closing_scenario_open_long(&ctx, &test_instrument());
    let closing_order_id = cache_closing_scenario_closing_order(&ctx);

    let other_order_id = ClientOrderId::from("O-OTHER");
    let other_venue_order_id = VenueOrderId::from("V-OTHER");
    let mut other_order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(other_order_id)
        .strategy_id(closing_scenario_strategy_id())
        .instrument_id(test_instrument_id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("0.500"))
        .price(Price::from("3050.00"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&other_order, other_account_id);
    other_order.apply(submitted).unwrap();
    let accepted =
        TestOrderEventStubs::accepted(&other_order, other_account_id, other_venue_order_id);
    other_order.apply(accepted).unwrap();
    ctx.add_order(other_order);

    let (mut other_report, mut other_fill) = create_bounded_fill_lifecycle(
        test_instrument_id(),
        other_venue_order_id,
        TradeId::from("T-OTHER"),
        OrderSide::Buy,
        "0.500",
        "3050.00",
        false,
        UnixNanos::from(CLOSING_SCENARIO_FILL_TS_1 + 500_000),
    );
    other_report.account_id = other_account_id;
    other_report.client_order_id = Some(other_order_id);
    other_fill.account_id = other_account_id;
    other_fill.client_order_id = Some(other_order_id);

    let mut mass_status = closing_scenario_mass_status(true, Some(closing_order_id));
    mass_status.add_order_reports(vec![other_report]);
    mass_status.add_fill_reports(vec![other_fill]);
    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert_eq!(count_filled_events(&result.events), 3);
    assert_closing_scenario_recorded_once(&ctx.cache.borrow(), closing_order_id);
    assert_closing_scenario_position_events(&position_events);

    let messages = MANAGER_LOG_CAPTURE.messages.lock().clone();
    assert!(
        !messages
            .iter()
            .any(|message| message.contains("leave retained position")),
        "found {messages:?}"
    );
}

/// Bounded closing fills for an uncached claimed order stay on the order only when the reporting
/// client without bulk position coverage is HEDGING and the strategy has no registered OMS,
/// whether the account has several clients on the venue or none: materialization records the
/// reporting client as the order's origin, so the engine would key the fills to a new HEDGING
/// position rather than reduce the retained NETTING one. A warning names the position and both
/// OMS types.
#[rstest]
#[case::account_clients_ambiguous(true)]
#[case::account_client_absent(false)]
#[tokio::test]
async fn test_bounded_claimed_closing_fills_stay_order_only_when_the_reporting_client_is_hedging(
    #[case] ambiguous: bool,
) {
    let _log_guard = MANAGER_LOG_TEST_LOCK.lock().await;
    install_manager_log_capture();
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    {
        let mut engine = ctx.exec_engine.borrow_mut();

        if ambiguous {
            engine
                .register_client(Box::new(
                    MockExecutionClient::for_venue(
                        ClientId::from("SIM-NETTING"),
                        test_venue(),
                        Vec::new(),
                    )
                    .with_oms_type(OmsType::Netting),
                ))
                .unwrap();
        } else {
            engine.deregister_client(test_client_id()).unwrap();
            engine
                .register_client(Box::new(
                    MockExecutionClient::new(Vec::new())
                        .with_bulk_position_coverage(false)
                        .with_account_id(AccountId::from("SIM-002")),
                ))
                .unwrap();
        }
        engine.register_oms_type(closing_scenario_strategy_id(), OmsType::Unspecified);
    }
    cache_closing_scenario_open_long(&ctx, &test_instrument());
    let (closing_order_id, reported_order_id) =
        prepare_closing_scenario_order(&mut ctx, ClosingOrder::Claimed);

    let mass_status = closing_scenario_mass_status(true, reported_order_id);
    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert_eq!(count_filled_events(&result.events), 2);
    assert_eq!(
        ctx.cache.borrow().client_id(&closing_order_id),
        Some(&test_client_id())
    );
    assert!(position_events.is_empty(), "found {position_events:?}");
    assert_closing_scenario_order_only(&ctx.cache.borrow(), closing_order_id);

    let expected = format!(
        "leave retained position {} unchanged: order {} resolves to it with engine OMS HEDGING \
         and cached OMS NETTING, not NETTING",
        closing_scenario_position_id(),
        closing_scenario_venue_order_id(),
    );
    let messages = MANAGER_LOG_CAPTURE.messages.lock().clone();
    assert!(
        messages.iter().any(|message| message.contains(&expected)),
        "expected warning containing {expected:?}, found {messages:?}"
    );
}

/// Bounded closing fills for an uncached claimed order close the retained position when the
/// reporting client without bulk position coverage is NETTING and the strategy has no registered
/// OMS, though the only client registered for the report's account is HEDGING, whether or not
/// the venue reports a client order ID: materialization records the reporting client as the
/// order's origin, so the engine reduces the retained NETTING position.
#[rstest]
#[case::without_client_order_id(None)]
#[case::with_client_order_id(Some(ClientOrderId::from("O-EXT-CLOSE")))]
fn test_bounded_claimed_closing_fills_close_position_when_the_reporting_client_is_netting(
    #[case] reported_order_id: Option<ClientOrderId>,
) {
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    {
        let mut engine = ctx.exec_engine.borrow_mut();
        engine.deregister_client(test_client_id()).unwrap();
        engine
            .register_client(Box::new(
                MockExecutionClient::new(Vec::new())
                    .with_bulk_position_coverage(false)
                    .with_oms_type(OmsType::Netting)
                    .with_account_id(AccountId::from("SIM-002")),
            ))
            .unwrap();
        engine
            .register_client(Box::new(MockExecutionClient::for_venue(
                ClientId::from("SIM-HEDGING"),
                test_venue(),
                Vec::new(),
            )))
            .unwrap();
        engine.register_oms_type(closing_scenario_strategy_id(), OmsType::Unspecified);
    }
    cache_closing_scenario_open_long(&ctx, &test_instrument());
    let (materialized_order_id, _) =
        prepare_closing_scenario_order(&mut ctx, ClosingOrder::Claimed);
    let closing_order_id = reported_order_id.unwrap_or(materialized_order_id);

    let mass_status = closing_scenario_mass_status(true, reported_order_id);
    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert_eq!(result.external_orders.len(), 1);
    assert_eq!(count_filled_events(&result.events), 2);
    assert_closing_scenario_recorded_once(&ctx.cache.borrow(), closing_order_id);
    assert_closing_scenario_position_events(&position_events);
}

/// Bounded closing fills for an uncached claimed order reported without a client order ID under
/// a synthetic `S-` venue order ID stay on the order only when the strategy has no registered OMS
/// and the only client registered for the report's account is HEDGING, though the reporting
/// client without bulk position coverage is NETTING: materialization records no origin for a
/// synthetic order, so the engine routes its fills by the account's client.
#[rstest]
fn test_bounded_synthetic_claimed_closing_fills_follow_the_account_client() {
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    {
        let mut engine = ctx.exec_engine.borrow_mut();
        engine.deregister_client(test_client_id()).unwrap();
        engine
            .register_client(Box::new(
                MockExecutionClient::new(Vec::new())
                    .with_bulk_position_coverage(false)
                    .with_oms_type(OmsType::Netting)
                    .with_account_id(AccountId::from("SIM-002")),
            ))
            .unwrap();
        engine
            .register_client(Box::new(MockExecutionClient::for_venue(
                ClientId::from("SIM-HEDGING"),
                test_venue(),
                Vec::new(),
            )))
            .unwrap();
        engine.register_oms_type(closing_scenario_strategy_id(), OmsType::Unspecified);
    }
    cache_closing_scenario_open_long(&ctx, &test_instrument());
    prepare_closing_scenario_order(&mut ctx, ClosingOrder::Claimed);
    let venue_order_id = VenueOrderId::from("S-CLOSE");
    let mut order_report = closing_scenario_closing_order_report(None);
    order_report.venue_order_id = venue_order_id;
    let fill_reports: Vec<FillReport> = closing_scenario_closing_fill_reports(None)
        .into_iter()
        .map(|mut fill| {
            fill.venue_order_id = venue_order_id;
            fill
        })
        .collect();

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2 + 1_000_000),
        Some(UUID4::new()),
    );
    mass_status.set_report_window(Some(UnixNanos::from(CLOSING_SCENARIO_WINDOW_START)), true);
    mass_status.add_order_reports(vec![order_report]);
    mass_status.add_fill_reports(fill_reports);
    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    let closing_order_id = ClientOrderId::from(venue_order_id.as_str());
    assert!(result.unresolved_positions.is_empty());
    assert_eq!(count_filled_events(&result.events), 2);
    assert_eq!(ctx.cache.borrow().client_id(&closing_order_id), None);
    assert!(position_events.is_empty(), "found {position_events:?}");
    assert_closing_scenario_order_only(&ctx.cache.borrow(), closing_order_id);
}

/// Bounded closing fills of an uncached claimed order reported without an order report follow
/// the OMS of the reporting client without bulk position coverage, not that of the account's
/// single client on the venue, when the strategy has no registered OMS: reconciliation creates
/// the order from its fills with the reporting client as its origin, so the engine routes the
/// fills by that client. Under a HEDGING reporting client the fills stay order-only with a
/// warning naming the position and both OMS types, and under a NETTING reporting client they
/// close the retained position.
#[rstest]
#[case::hedging_reporting_client(OmsType::Hedging, OmsType::Netting)]
#[case::netting_reporting_client(OmsType::Netting, OmsType::Hedging)]
#[tokio::test]
async fn test_bounded_claimed_closing_fills_without_order_report_follow_the_reporting_client(
    #[case] reporting_oms_type: OmsType,
    #[case] account_oms_type: OmsType,
) {
    let _log_guard = MANAGER_LOG_TEST_LOCK.lock().await;
    install_manager_log_capture();
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    {
        let mut engine = ctx.exec_engine.borrow_mut();
        engine.deregister_client(test_client_id()).unwrap();
        engine
            .register_client(Box::new(
                MockExecutionClient::new(Vec::new())
                    .with_bulk_position_coverage(false)
                    .with_oms_type(reporting_oms_type)
                    .with_account_id(AccountId::from("SIM-002")),
            ))
            .unwrap();
        engine
            .register_client(Box::new(
                MockExecutionClient::for_venue(
                    ClientId::from("SIM-ACCOUNT"),
                    test_venue(),
                    Vec::new(),
                )
                .with_oms_type(account_oms_type),
            ))
            .unwrap();
        engine.register_oms_type(closing_scenario_strategy_id(), OmsType::Unspecified);
    }
    cache_closing_scenario_open_long(&ctx, &test_instrument());
    let (closing_order_id, _) = prepare_closing_scenario_order(&mut ctx, ClosingOrder::Claimed);

    // Reconciliation creates an order from fills alone only when they name a venue position
    let fill_reports: Vec<FillReport> = closing_scenario_closing_fill_reports(None)
        .into_iter()
        .map(|mut fill| {
            fill.venue_position_id = Some(PositionId::from("P-VENUE"));
            fill
        })
        .collect();
    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2 + 1_000_000),
        Some(UUID4::new()),
    );
    mass_status.set_report_window(Some(UnixNanos::from(CLOSING_SCENARIO_WINDOW_START)), true);
    mass_status.add_fill_reports(fill_reports);
    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert_eq!(result.external_orders.len(), 1);
    assert_eq!(count_filled_events(&result.events), 2);
    assert_eq!(
        ctx.cache.borrow().client_id(&closing_order_id),
        Some(&test_client_id())
    );

    if reporting_oms_type == OmsType::Netting {
        assert_closing_scenario_recorded_once(&ctx.cache.borrow(), closing_order_id);
        assert_closing_scenario_position_events(&position_events);
    } else {
        assert!(position_events.is_empty(), "found {position_events:?}");
        assert_closing_scenario_order_only(&ctx.cache.borrow(), closing_order_id);

        let expected = format!(
            "leave retained position {} unchanged: order {} resolves to it with engine OMS \
             HEDGING and cached OMS NETTING, not NETTING",
            closing_scenario_position_id(),
            closing_scenario_venue_order_id(),
        );
        let messages = MANAGER_LOG_CAPTURE.messages.lock().clone();
        assert!(
            messages.iter().any(|message| message.contains(&expected)),
            "expected warning containing {expected:?}, found {messages:?}"
        );
    }
}

/// Bounded fills from a client without bulk position coverage stay on the order only, and a
/// warning names the retained position, when the cached order's side opens the position while
/// its report or fills show the closing side: every side the order is known by must close the
/// position. A cached side the venue's fills contradict is inconsistent evidence of which way the
/// venue moved the position, and with a report the engine infers the rest of the reported filled
/// quantity with the cached order's side.
#[rstest]
#[case::report_only(Some(OrderSide::Sell), Some("0.400"), None)]
#[case::report_beyond_fill(Some(OrderSide::Sell), Some("0.600"), Some("0.400"))]
#[case::sideless_report_beyond_fill(None, Some("0.600"), Some("0.400"))]
#[case::fill_only(None, None, Some("0.400"))]
#[tokio::test]
async fn test_bounded_fills_for_an_opening_side_cached_order_stay_order_only(
    #[case] report_side: Option<OrderSide>,
    #[case] report_filled_qty: Option<&str>,
    #[case] fill_qty: Option<&str>,
) {
    let _log_guard = MANAGER_LOG_TEST_LOCK.lock().await;
    install_manager_log_capture();
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    cache_closing_scenario_open_long(&ctx, &test_instrument());

    let closing_order_id = ClientOrderId::from("O-CLOSE");
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(closing_order_id)
        .strategy_id(closing_scenario_strategy_id())
        .instrument_id(test_instrument_id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .price(Price::from("3100.00"))
        .build();
    apply_submitted_and_accepted(&mut order, closing_scenario_venue_order_id());
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2),
        Some(UUID4::new()),
    );
    mass_status.set_report_window(Some(UnixNanos::from(CLOSING_SCENARIO_WINDOW_START)), true);

    if let Some(filled_qty) = report_filled_qty {
        let mut order_report = OrderStatusReport::new(
            test_account_id(),
            test_instrument_id(),
            Some(closing_order_id),
            closing_scenario_venue_order_id(),
            OrderSide::Sell.into(),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::PartiallyFilled,
            Quantity::from("1.000"),
            Quantity::from(filled_qty),
            UnixNanos::from(CLOSING_SCENARIO_ACCEPTED_TS),
            UnixNanos::from(CLOSING_SCENARIO_FILL_TS_1),
            UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2),
            None,
        )
        .with_price(Price::from("3100.00"))
        .with_avg_px(dec!(3100.00));
        order_report.order_side = report_side;
        mass_status.add_order_reports(vec![order_report]);
    }

    if let Some(fill_qty) = fill_qty {
        let fill_ts = UnixNanos::from(CLOSING_SCENARIO_FILL_TS_1);
        mass_status.add_fill_reports(vec![FillReport::new(
            test_account_id(),
            test_instrument_id(),
            closing_scenario_venue_order_id(),
            TradeId::from("T-CLOSE-1"),
            OrderSide::Sell,
            Quantity::from(fill_qty),
            Price::from("3100.00"),
            Money::from("0.25 USDT"),
            LiquiditySide::Taker,
            Some(closing_order_id),
            None,
            fill_ts,
            fill_ts,
            None,
        )]);
    }

    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert!(position_events.is_empty(), "found {position_events:?}");
    {
        let cache = ctx.cache.borrow();
        assert_closing_scenario_position_unchanged(&cache);
        assert_eq!(
            closing_scenario_instrument_position_ids(&cache),
            vec![closing_scenario_position_id()]
        );
        let order = cache
            .order(&closing_order_id)
            .expect("closing order cached");
        assert_eq!(
            order.filled_qty(),
            Quantity::from(report_filled_qty.or(fill_qty).unwrap())
        );
    }

    let expected = format!(
        "leave retained position {} unchanged: order {} cached order side BUY does not close LONG",
        closing_scenario_position_id(),
        closing_scenario_venue_order_id(),
    );
    let messages = MANAGER_LOG_CAPTURE.messages.lock().clone();
    assert!(
        messages.iter().any(|message| message.contains(&expected)),
        "expected warning containing {expected:?}, found {messages:?}"
    );
}

/// Bounded fills from a client without bulk position coverage stay on the order only, and a
/// warning names the retained position, when the closing order's report shows the opening side
/// and no fill is reported: a claimed order materializes on the report's side and infers its
/// fill on it, and a cached order whose report disagrees with its own side gives no evidence of
/// which way the venue moved the position.
#[rstest]
#[case::cached(ClosingOrder::Cached)]
#[case::claimed(ClosingOrder::Claimed)]
#[tokio::test]
async fn test_bounded_fills_for_an_opening_side_report_stay_order_only(
    #[case] closing: ClosingOrder,
) {
    let _log_guard = MANAGER_LOG_TEST_LOCK.lock().await;
    install_manager_log_capture();
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    cache_closing_scenario_open_long(&ctx, &test_instrument());
    let (_, reported_order_id) = prepare_closing_scenario_order(&mut ctx, closing);

    let mut order_report = closing_scenario_closing_order_report(reported_order_id);
    order_report.order_side = Some(OrderSide::Buy);
    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2 + 1_000_000),
        Some(UUID4::new()),
    );
    mass_status.set_report_window(Some(UnixNanos::from(CLOSING_SCENARIO_WINDOW_START)), true);
    mass_status.add_order_reports(vec![order_report]);
    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert_eq!(count_filled_events(&result.events), 1);
    assert!(position_events.is_empty(), "found {position_events:?}");
    {
        let cache = ctx.cache.borrow();
        assert_closing_scenario_position_unchanged(&cache);
        assert_eq!(
            closing_scenario_instrument_position_ids(&cache),
            vec![closing_scenario_position_id()]
        );
    }

    let expected = format!(
        "leave retained position {} unchanged: order {} report side BUY does not close LONG",
        closing_scenario_position_id(),
        closing_scenario_venue_order_id(),
    );
    let messages = MANAGER_LOG_CAPTURE.messages.lock().clone();
    assert!(
        messages.iter().any(|message| message.contains(&expected)),
        "expected warning containing {expected:?}, found {messages:?}"
    );
}

/// Bounded fills from a client without bulk position coverage stay on the order only, and a
/// warning names the retained position, when an uncached claimed order for it reports no side,
/// even beside a partial closing order that alone fits the open quantity and even when the
/// sideless order's fills show the closing side: reconciliation materializes no order from a
/// report without a side, so the venue's change to the position is unknown.
#[rstest]
#[case::report_only(false)]
#[case::report_with_closing_fill(true)]
#[tokio::test]
async fn test_bounded_partial_closing_fill_beside_unknown_side_order_stays_order_only(
    #[case] with_unknown_fill: bool,
) {
    let _log_guard = MANAGER_LOG_TEST_LOCK.lock().await;
    install_manager_log_capture();
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    cache_closing_scenario_open_long(&ctx, &test_instrument());
    let (closing_order_id, reported_order_id) =
        prepare_closing_scenario_order(&mut ctx, ClosingOrder::Cached);
    claim_closing_scenario_instrument(&mut ctx, ClosingOrder::Claimed);

    let fill_ts = UnixNanos::from(CLOSING_SCENARIO_FILL_TS_1);
    let closing_report = OrderStatusReport::new(
        test_account_id(),
        test_instrument_id(),
        reported_order_id,
        closing_scenario_venue_order_id(),
        OrderSide::Sell.into(),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::PartiallyFilled,
        Quantity::from("1.000"),
        Quantity::from("0.400"),
        UnixNanos::from(CLOSING_SCENARIO_ACCEPTED_TS),
        fill_ts,
        fill_ts,
        None,
    )
    .with_price(Price::from("3100.00"))
    .with_avg_px(dec!(3100.00));
    let closing_fill = FillReport::new(
        test_account_id(),
        test_instrument_id(),
        closing_scenario_venue_order_id(),
        TradeId::from("T-CLOSE-1"),
        OrderSide::Sell,
        Quantity::from("0.400"),
        Price::from("3100.00"),
        Money::from("0.25 USDT"),
        LiquiditySide::Taker,
        reported_order_id,
        None,
        fill_ts,
        fill_ts,
        None,
    );

    let unknown_venue_order_id = VenueOrderId::from("V-UNKNOWN");
    let (mut unknown_report, unknown_fill) = create_bounded_fill_lifecycle(
        test_instrument_id(),
        unknown_venue_order_id,
        TradeId::from("T-UNKNOWN"),
        OrderSide::Sell,
        "0.500",
        "3130.00",
        false,
        UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2),
    );
    unknown_report.order_side = None;

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2 + 1_000_000),
        Some(UUID4::new()),
    );
    mass_status.set_report_window(Some(UnixNanos::from(CLOSING_SCENARIO_WINDOW_START)), true);
    mass_status.add_order_reports(vec![closing_report, unknown_report]);
    let mut fill_reports = vec![closing_fill];

    if with_unknown_fill {
        fill_reports.push(unknown_fill);
    }

    mass_status.add_fill_reports(fill_reports);
    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert!(position_events.is_empty(), "found {position_events:?}");
    {
        let cache = ctx.cache.borrow();
        assert_closing_scenario_position_unchanged(&cache);
        let order = cache
            .order(&closing_order_id)
            .expect("closing order cached");
        assert_eq!(order.filled_qty(), Quantity::from("0.400"));
    }

    let expected = format!(
        "leave retained position {} unchanged: order {unknown_venue_order_id} side is unknown",
        closing_scenario_position_id(),
    );
    let messages = MANAGER_LOG_CAPTURE.messages.lock().clone();
    assert!(
        messages.iter().any(|message| message.contains(&expected)),
        "expected warning containing {expected:?}, found {messages:?}"
    );
}

fn external_closing_scenario_position_id() -> PositionId {
    PositionId::new(format!("{}-EXTERNAL", test_instrument_id()))
}

/// Caches the open `EXTERNAL` LONG 1.000 @ 3000.00 that the filled order `O-EXT-OPEN` (trade
/// `T-EXT-OPEN`) opened under NETTING, and the accepted `EXTERNAL` closing order `O-CLOSE`
/// (SELL 1.000 limit 3100.00, venue `V-CLOSE`) with no indexed position, as a prior session
/// leaves an external order it materialized while open.
fn cache_external_closing_scenario(ctx: &TestContext, instrument: &InstrumentAny) -> ClientOrderId {
    let position_id = external_closing_scenario_position_id();
    let mut opening_order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(ClientOrderId::from("O-EXT-OPEN"))
        .strategy_id(StrategyId::external())
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .price(Price::from("3000.00"))
        .build();
    apply_submitted_and_accepted(&mut opening_order, VenueOrderId::from("V-EXT-OPEN"));
    let filled = TestOrderEventStubs::filled(
        &opening_order,
        instrument,
        Some(TradeId::from("T-EXT-OPEN")),
        Some(position_id),
        Some(Price::from("3000.00")),
        Some(Quantity::from("1.000")),
        Some(LiquiditySide::Maker),
        Some(Money::from("0.60 USDT")),
        Some(UnixNanos::from(CLOSING_SCENARIO_OPENING_TS)),
        Some(test_account_id()),
    );
    opening_order.apply(filled.clone()).unwrap();
    let position = Position::new(instrument, filled.into());

    let closing_order_id = ClientOrderId::from("O-CLOSE");
    let mut closing_order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(closing_order_id)
        .strategy_id(StrategyId::external())
        .instrument_id(instrument.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from("1.000"))
        .price(Price::from("3100.00"))
        .build();
    apply_submitted_and_accepted(&mut closing_order, closing_scenario_venue_order_id());

    let mut cache = ctx.cache.borrow_mut();
    cache
        .add_order(
            opening_order,
            Some(position_id),
            Some(test_client_id()),
            false,
        )
        .unwrap();
    cache.add_position(&position, OmsType::Netting).unwrap();
    cache
        .add_order(closing_order, None, Some(test_client_id()), false)
        .unwrap();
    closing_order_id
}

/// A complete bounded status from a client without bulk position coverage leaves a retained
/// `EXTERNAL` position unchanged, with a warning naming it, when an order that is neither cached
/// nor claimed trades its account and instrument beside a cached `EXTERNAL` closing order. Event
/// processing applies none of the unclaimed order's fills to a position, yet they move the
/// venue's `EXTERNAL` inventory on either side, so the cached closing fills alone do not
/// establish what remains open. This holds whether the unclaimed order arrives with a report
/// and fills, fills only or a report only, and when only its fill names the position's account.
#[rstest]
#[case::extending(OrderSide::Buy, true, true, false)]
#[case::crossing(OrderSide::Sell, true, true, false)]
#[case::extending_fill_only(OrderSide::Buy, false, true, false)]
#[case::extending_report_only(OrderSide::Buy, true, false, false)]
#[case::report_of_another_account(OrderSide::Buy, true, true, true)]
#[tokio::test]
async fn test_bounded_external_closing_fills_beside_unclaimed_order_stay_order_only(
    #[case] unclaimed_side: OrderSide,
    #[case] with_report: bool,
    #[case] with_fill: bool,
    #[case] report_of_another_account: bool,
) {
    let _log_guard = MANAGER_LOG_TEST_LOCK.lock().await;
    install_manager_log_capture();
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    let closing_order_id = cache_external_closing_scenario(&ctx, &test_instrument());

    let unclaimed_venue_order_id = VenueOrderId::from("V-UNCLAIMED");
    let (mut unclaimed_report, unclaimed_fill) = create_bounded_fill_lifecycle(
        test_instrument_id(),
        unclaimed_venue_order_id,
        TradeId::from("T-UNCLAIMED"),
        unclaimed_side,
        "0.500",
        "3130.00",
        false,
        UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2 + 500_000),
    );

    if report_of_another_account {
        unclaimed_report.account_id = AccountId::from("BINANCE-002");
    }

    let mut mass_status = closing_scenario_mass_status(true, Some(closing_order_id));

    if with_report {
        mass_status.add_order_reports(vec![unclaimed_report]);
    }

    if with_fill {
        mass_status.add_fill_reports(vec![unclaimed_fill]);
    }

    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert!(position_events.is_empty(), "found {position_events:?}");
    {
        let cache = ctx.cache.borrow();
        assert_closing_scenario_order_filled(&cache, closing_order_id);
        let position = cache
            .position(&external_closing_scenario_position_id())
            .expect("external position cached");
        assert!(position.is_open(), "found {position:?}");
        assert_eq!(position.side, PositionSide::Long);
        assert_eq!(position.quantity, Quantity::from("1.000"));
        assert_eq!(
            position.trade_ids,
            [TradeId::from("T-EXT-OPEN")]
                .into_iter()
                .collect::<AHashSet<_>>()
        );
        drop(position);
        assert_eq!(
            closing_scenario_instrument_position_ids(&cache),
            vec![external_closing_scenario_position_id()]
        );
    }

    let expected = format!(
        "leave retained position {} unchanged: order {unclaimed_venue_order_id} is neither \
         cached nor claimed",
        external_closing_scenario_position_id(),
    );
    let messages = MANAGER_LOG_CAPTURE.messages.lock().clone();
    assert!(
        messages.iter().any(|message| message.contains(&expected)),
        "expected warning containing {expected:?}, found {messages:?}"
    );
}

/// A complete bounded status from a client without bulk position coverage closes a retained
/// `EXTERNAL` position with the fills of a cached `EXTERNAL` closing order when no order that
/// is neither cached nor claimed trades the position's account and instrument, including when
/// such an order trades the instrument for another account only.
#[rstest]
#[case::alone(false)]
#[case::beside_another_accounts_unclaimed_order(true)]
fn test_bounded_external_closing_fills_close_retained_external_position(
    #[case] with_other_account_order: bool,
) {
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    let closing_order_id = cache_external_closing_scenario(&ctx, &test_instrument());

    let mut mass_status = closing_scenario_mass_status(true, Some(closing_order_id));

    if with_other_account_order {
        let other_account_id = AccountId::from("BINANCE-002");
        let (mut unclaimed_report, mut unclaimed_fill) = create_bounded_fill_lifecycle(
            test_instrument_id(),
            VenueOrderId::from("V-UNCLAIMED"),
            TradeId::from("T-UNCLAIMED"),
            OrderSide::Buy,
            "0.500",
            "3130.00",
            false,
            UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2 + 500_000),
        );
        unclaimed_report.account_id = other_account_id;
        unclaimed_fill.account_id = other_account_id;
        mass_status.add_order_reports(vec![unclaimed_report]);
        mass_status.add_fill_reports(vec![unclaimed_fill]);
    }

    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert_eq!(
        count_filled_events(&result.events),
        2 + usize::from(with_other_account_order)
    );
    assert_closing_scenario_position_events(&position_events);
    let cache = ctx.cache.borrow();
    assert_closing_scenario_order_filled(&cache, closing_order_id);
    let position = cache
        .position(&external_closing_scenario_position_id())
        .expect("external position cached");
    assert!(position.is_closed(), "found {position:?}");
    assert_eq!(position.realized_pnl, Some(Money::from("110.78 USDT")));
    assert_eq!(
        position.trade_ids,
        [
            TradeId::from("T-EXT-OPEN"),
            TradeId::from("T-CLOSE-1"),
            TradeId::from("T-CLOSE-2"),
        ]
        .into_iter()
        .collect::<AHashSet<_>>()
    );
}

/// A complete bounded status from a client without bulk position coverage closes a strategy's
/// retained position with its cached closing order's fills although an order that is neither
/// cached nor claimed trades the same account and instrument: that order belongs to `EXTERNAL`,
/// so event processing applies none of its fills to the strategy's position.
#[rstest]
fn test_bounded_closing_fills_close_strategy_position_beside_unclaimed_order() {
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    cache_closing_scenario_open_long(&ctx, &test_instrument());
    let (closing_order_id, reported_order_id) =
        prepare_closing_scenario_order(&mut ctx, ClosingOrder::Cached);

    let (unclaimed_report, unclaimed_fill) = create_bounded_fill_lifecycle(
        test_instrument_id(),
        VenueOrderId::from("V-UNCLAIMED"),
        TradeId::from("T-UNCLAIMED"),
        OrderSide::Buy,
        "0.500",
        "3130.00",
        false,
        UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2 + 500_000),
    );
    let mut mass_status = closing_scenario_mass_status(true, reported_order_id);
    mass_status.add_order_reports(vec![unclaimed_report]);
    mass_status.add_fill_reports(vec![unclaimed_fill]);
    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert_eq!(result.external_orders.len(), 1);
    assert_eq!(count_filled_events(&result.events), 3);
    assert_closing_scenario_recorded_once(&ctx.cache.borrow(), closing_order_id);
    assert_closing_scenario_position_events(&position_events);
}

#[tokio::test]
async fn test_fill_before_retained_netting_lifecycle_projects_order_only() {
    let config = ExecutionManagerConfig {
        generate_missing_orders: false,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let strategy_id = StrategyId::from("STRATEGY-001");
    let old_order_id = ClientOrderId::from("O-PRIOR-LIFECYCLE");
    let old_venue_order_id = VenueOrderId::from("V-PRIOR-LIFECYCLE");
    let old_trade_id = TradeId::from("T-PRIOR-LIFECYCLE");
    let current_order_id = ClientOrderId::from("O-CURRENT-LIFECYCLE");
    let current_venue_order_id = VenueOrderId::from("V-CURRENT-LIFECYCLE");
    let current_trade_id = TradeId::from("T-CURRENT-LIFECYCLE");
    let position_id = PositionId::new(format!("{instrument_id}-{strategy_id}"));

    ctx.add_instrument(instrument.clone());
    ctx.manager
        .claim_external_orders(instrument_id, strategy_id)
        .unwrap();
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(strategy_id, OmsType::Netting);

    let mut current_order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(current_order_id)
        .strategy_id(strategy_id)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .price(Price::from("3200.00"))
        .build();
    apply_submitted_and_accepted(&mut current_order, current_venue_order_id);
    let current_fill = TestOrderEventStubs::filled(
        &current_order,
        &instrument,
        Some(current_trade_id),
        Some(position_id),
        Some(Price::from("3200.00")),
        Some(Quantity::from("1.000")),
        Some(LiquiditySide::Maker),
        Some(Money::from("0.50 USDT")),
        Some(UnixNanos::from(3_000_000)),
        Some(test_account_id()),
    );
    let position = Position::new(&instrument, current_fill.into());
    ctx.cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );
    let old_order_report = create_order_report(
        Some(old_order_id),
        old_venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("1.000"),
        Quantity::from("1.000"),
    )
    .with_avg_px(dec!(3000.0));
    let current_order_report = create_order_report(
        Some(current_order_id),
        current_venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("1.000"),
        Quantity::from("1.000"),
    )
    .with_avg_px(dec!(3200.0));

    let old_fill_report = FillReport::new(
        test_account_id(),
        instrument_id,
        old_venue_order_id,
        old_trade_id,
        OrderSide::Buy,
        Quantity::from("1.000"),
        Price::from("3000.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        Some(old_order_id),
        None,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );

    let current_fill_report = FillReport::new(
        test_account_id(),
        instrument_id,
        current_venue_order_id,
        current_trade_id,
        OrderSide::Buy,
        Quantity::from("1.000"),
        Price::from("3200.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        Some(current_order_id),
        None,
        UnixNanos::from(3_000_000),
        UnixNanos::from(3_000_000),
        None,
    );

    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("1.000"),
        UnixNanos::from(3_000_000),
        UnixNanos::from(3_000_000),
        None,
        None,
        Some(dec!(3200.00)),
    );
    mass_status.add_order_reports(vec![old_order_report, current_order_report]);
    mass_status.add_fill_reports(vec![old_fill_report, current_fill_report]);
    mass_status.add_position_reports(vec![position_report]);

    ctx.manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let cache = ctx.cache.borrow();
    let old_order = cache.order(&old_order_id).unwrap();
    let current_order = cache.order(&current_order_id).unwrap();
    let position = cache.position(&position_id).unwrap();

    assert_eq!(old_order.status(), OrderStatus::Filled);
    assert_eq!(current_order.status(), OrderStatus::Filled);
    assert_eq!(position.quantity, Quantity::from("1.000"));
    assert_eq!(position.realized_pnl, Some(Money::from("-0.50 USDT")));
    assert_eq!(position.commissions(), vec![Money::from("0.50 USDT")]);
    assert_eq!(position.trade_ids.len(), 1);
    assert!(position.trade_ids.contains(&current_trade_id));
    assert!(!position.trade_ids.contains(&old_trade_id));
}

#[tokio::test]
async fn test_filled_report_with_reduced_quantity_closes_partially_filled_order() {
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let client_order_id = ClientOrderId::from("O-REDUCED-FILLED");
    let venue_order_id = VenueOrderId::from("V-REDUCED-FILLED");

    ctx.add_instrument(instrument.clone());
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("10.000"))
        .price(Price::from("3000.00"))
        .build();
    apply_submitted_and_accepted(&mut order, venue_order_id);
    let fill = TestOrderEventStubs::filled(
        &order,
        &instrument,
        Some(TradeId::from("T-REDUCED-FILLED")),
        None,
        Some(Price::from("3000.00")),
        Some(Quantity::from("5.000")),
        Some(LiquiditySide::Maker),
        Some(Money::from("0.50 USDT")),
        Some(UnixNanos::from(1_000_000)),
        Some(test_account_id()),
    );
    order.apply(fill).unwrap();
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );
    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("5.000"),
        Quantity::from("5.000"),
    )
    .with_avg_px(dec!(3000.0));
    mass_status.add_order_reports(vec![report]);

    ctx.manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let order = ctx.get_order(&client_order_id).unwrap();
    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.quantity(), Quantity::from("5.000"));
    assert_eq!(order.filled_qty(), Quantity::from("5.000"));
}

#[tokio::test]
async fn test_reconcile_mass_status_skips_order_without_instrument() {
    let mut ctx = TestContext::new();
    // Don't add instrument to cache

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        None,
        VenueOrderId::from("V-EXT-001"),
        test_instrument_id(),
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert!(result.events.is_empty());
}

#[tokio::test]
async fn test_reconcile_mass_status_sorts_events_chronologically() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");

    ctx.add_instrument(test_instrument());
    let order = create_limit_order("O-001", instrument_id, OrderSide::Buy, "2.0", "3000.00");
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add fills in reverse chronological order
    let fill2 = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-002"),
        OrderSide::Buy,
        Quantity::from("0.5"),
        Price::from("3001.00"),
        Money::from("0.25 USDT"),
        LiquiditySide::Maker,
        Some(client_order_id),
        None,
        UnixNanos::from(2_000_000), // Later
        UnixNanos::from(2_000_000),
        None,
    );

    let fill1 = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-001"),
        OrderSide::Buy,
        Quantity::from("0.5"),
        Price::from("3000.00"),
        Money::from("0.25 USDT"),
        LiquiditySide::Maker,
        Some(client_order_id),
        None,
        UnixNanos::from(1_000_000), // Earlier
        UnixNanos::from(1_000_000),
        None,
    );
    mass_status.add_fill_reports(vec![fill2, fill1]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 2);

    // Verify chronological ordering
    assert!(result.events[0].ts_event() < result.events[1].ts_event());
}

#[rstest]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_inflight_order_generates_rejection_after_max_retries(
    #[values(false, true)] submission: bool,
) {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 1,
        submission_recovery_policy: SubmissionRecoveryPolicy::ResolveLocally,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");

    ctx.add_instrument(test_instrument());

    // Order must be submitted (have account_id) to generate rejection
    let order = create_submitted_order("O-001", instrument_id, OrderSide::Buy, "1.0", "3000.00");
    ctx.add_order(order.clone());

    if submission {
        ctx.manager
            .register_submission(order.init_event(), Some(test_client_id()));
    } else {
        ctx.manager.register_inflight(client_order_id);
    }
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await; // 200ms, past threshold

    let result = ctx.manager.check_inflight_orders();

    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Rejected(_)));

    if let OrderEventAny::Rejected(rejected) = &result.events[0] {
        assert_eq!(rejected.client_order_id, client_order_id);
        assert_eq!(rejected.reason, "INFLIGHT_TIMEOUT");
    }

    let diagnostics = ctx.manager.take_submission_recovery_exhaustions();
    let order = ctx.get_order(&client_order_id).unwrap();
    assert_eq!(diagnostics, Vec::new());
    assert!(
        ctx.manager
            .take_submission_recovery_exhaustions()
            .is_empty()
    );
    assert_eq!(order.status(), OrderStatus::Submitted);
}

#[rstest]
#[case(1)]
#[case(3)]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_submission_registry_preserves_identity_and_bounded_queries(#[case] budget: u32) {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: budget,
        submission_recovery_policy: SubmissionRecoveryPolicy::RetainUnresolved,
        ..Default::default()
    });
    ctx.add_instrument(test_instrument());
    let order = create_submitted_order(
        "O-REGISTRY",
        test_instrument_id(),
        OrderSide::Buy,
        "1.0",
        "3000.00",
    );
    let initialized = order.init_event().clone();
    let client_order_id = order.client_order_id();
    ctx.manager
        .register_submission(&initialized, Some(test_client_id()));
    ctx.add_order(order);

    let mut query_count = 0;

    for check in 1..=budget {
        ctx.advance_both(dst::time::Duration::from_millis(101))
            .await;
        let result = ctx.manager.check_inflight_orders();
        query_count += result.queries.len();

        if check < budget {
            assert!(result.events.is_empty());
            assert_eq!(result.queries.len(), 1);
            let TradingCommand::QueryOrder(query) = &result.queries[0] else {
                panic!("Recovery must only query an existing submission");
            };
            assert_eq!(query.trader_id, initialized.trader_id);
            assert_eq!(query.client_id, Some(test_client_id()));
            assert_eq!(query.strategy_id, initialized.strategy_id);
            assert_eq!(query.instrument_id, initialized.instrument_id);
            assert_eq!(query.client_order_id, client_order_id);
            assert_eq!(query.venue_order_id, None);
            assert_eq!(query.ts_init, ctx.clock.borrow().timestamp_ns());
            assert!(
                ctx.manager
                    .take_submission_recovery_exhaustions()
                    .is_empty()
            );

            let mut duplicate = initialized.clone();
            duplicate.trader_id = TraderId::from("OTHER-001");
            ctx.manager
                .register_submission(&duplicate, Some(ClientId::from("OTHER")));
        } else {
            assert!(result.queries.is_empty());
            assert!(result.events.is_empty());
            assert_eq!(
                ctx.get_order(&client_order_id).unwrap().status(),
                OrderStatus::Submitted
            );
            assert_eq!(
                ctx.manager.take_submission_recovery_exhaustions(),
                vec![SubmissionRecoveryExhausted {
                    trader_id: initialized.trader_id,
                    client_id: Some(test_client_id()),
                    strategy_id: initialized.strategy_id,
                    instrument_id: initialized.instrument_id,
                    client_order_id,
                    source: SubmissionRecoverySource::Inflight,
                    retry_count: budget,
                    ts_event: ctx.clock.borrow().timestamp_ns(),
                }],
            );
        }
    }

    ctx.advance_both(dst::time::Duration::from_millis(101))
        .await;
    let after_exhaustion = ctx.manager.check_inflight_orders();
    assert_eq!(query_count, usize::try_from(budget - 1).unwrap());
    assert!(after_exhaustion.events.is_empty());
    assert!(after_exhaustion.queries.is_empty());
    assert!(
        ctx.manager
            .take_submission_recovery_exhaustions()
            .is_empty()
    );
}

#[rstest]
#[case::unfiltered(false)]
#[case::filtered(true)]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_submission_registry_before_cache_insertion(#[case] filtered: bool) {
    let order = create_limit_order(
        "O-PRECACHE",
        test_instrument_id(),
        OrderSide::Buy,
        "1.0",
        "3000.00",
    );
    let client_order_id = order.client_order_id();
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 1,
        submission_recovery_policy: SubmissionRecoveryPolicy::RetainUnresolved,
        filtered_client_order_ids: if filtered {
            IndexSet::from([client_order_id])
        } else {
            IndexSet::new()
        },
        ..Default::default()
    });
    ctx.manager
        .register_submission(order.init_event(), Some(test_client_id()));
    ctx.advance_both(dst::time::Duration::from_millis(101))
        .await;

    let result = ctx.manager.check_inflight_orders();
    let diagnostics = ctx.manager.take_submission_recovery_exhaustions();

    assert!(result.queries.is_empty());
    assert!(result.events.is_empty());
    assert_eq!(diagnostics.len(), usize::from(!filtered));
    if !filtered {
        assert_eq!(
            diagnostics[0],
            SubmissionRecoveryExhausted {
                trader_id: order.trader_id(),
                client_id: Some(test_client_id()),
                strategy_id: order.strategy_id(),
                instrument_id: order.instrument_id(),
                client_order_id,
                source: SubmissionRecoverySource::Inflight,
                retry_count: 1,
                ts_event: ctx.clock.borrow().timestamp_ns(),
            },
        );
    }
    assert!(
        ctx.manager
            .take_submission_recovery_exhaustions()
            .is_empty()
    );
}

#[rstest]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_submission_registry_applied_acceptance_prevents_exhaustion() {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 1,
        submission_recovery_policy: SubmissionRecoveryPolicy::RetainUnresolved,
        ..Default::default()
    });
    ctx.add_instrument(test_instrument());
    let order = create_submitted_order(
        "O-DELAYED-ACCEPT",
        test_instrument_id(),
        OrderSide::Buy,
        "1.0",
        "3000.00",
    );
    let accepted = TestOrderEventStubs::accepted(
        &order,
        test_account_id(),
        VenueOrderId::from("V-DELAYED-ACCEPT"),
    );
    ctx.manager
        .register_submission(order.init_event(), Some(test_client_id()));
    ctx.add_order(order);
    ctx.cache.borrow_mut().update_order(&accepted).unwrap();
    ctx.advance_both(dst::time::Duration::from_millis(101))
        .await;

    let result = ctx.manager.check_inflight_orders();

    assert!(result.events.is_empty());
    assert!(result.queries.is_empty());
    assert!(ctx.manager.check_open_order_queries().is_empty());
    assert!(
        ctx.manager
            .take_submission_recovery_exhaustions()
            .is_empty()
    );
}

#[rstest]
#[case::cancel_rejected("cancel_rejected")]
#[case::modify_rejected("modify_rejected")]
#[case::updated("updated")]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_submission_registry_survives_unacknowledged_command_response(
    #[case] response: &str,
    #[values(
        SubmissionRecoveryPolicy::ResolveLocally,
        SubmissionRecoveryPolicy::RetainUnresolved
    )]
    policy: SubmissionRecoveryPolicy,
) {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 2,
        submission_recovery_policy: policy,
        ..Default::default()
    });
    let order = create_submitted_order(
        "O-REGISTRY-RESPONSE",
        test_instrument_id(),
        OrderSide::Buy,
        "1.0",
        "3000.00",
    );
    let initialized = order.init_event().clone();
    let client_order_id = order.client_order_id();
    ctx.manager
        .register_submission(&initialized, Some(test_client_id()));
    ctx.add_order(order.clone());
    ctx.advance_both(dst::time::Duration::from_millis(101))
        .await;
    let first = ctx.manager.check_inflight_orders();
    assert!(first.events.is_empty());
    assert_eq!(first.queries.len(), 1);

    let pending = if response == "cancel_rejected" {
        OrderEventAny::PendingCancel(
            OrderPendingCancelSpec::builder()
                .trader_id(order.trader_id())
                .strategy_id(order.strategy_id())
                .instrument_id(order.instrument_id())
                .client_order_id(client_order_id)
                .account_id(test_account_id())
                .build(),
        )
    } else {
        OrderEventAny::PendingUpdate(
            OrderPendingUpdateSpec::builder()
                .trader_id(order.trader_id())
                .strategy_id(order.strategy_id())
                .instrument_id(order.instrument_id())
                .client_order_id(client_order_id)
                .account_id(test_account_id())
                .build(),
        )
    };
    ctx.manager.observe_order_event(&pending);
    ctx.cache.borrow_mut().update_order(&pending).unwrap();
    ctx.manager.register_inflight(client_order_id);
    let event = match response {
        "cancel_rejected" => OrderEventAny::CancelRejected(
            OrderCancelRejectedSpec::builder()
                .trader_id(order.trader_id())
                .strategy_id(order.strategy_id())
                .instrument_id(order.instrument_id())
                .client_order_id(client_order_id)
                .account_id(test_account_id())
                .build(),
        ),
        "modify_rejected" => OrderEventAny::ModifyRejected(
            OrderModifyRejectedSpec::builder()
                .trader_id(order.trader_id())
                .strategy_id(order.strategy_id())
                .instrument_id(order.instrument_id())
                .client_order_id(client_order_id)
                .account_id(test_account_id())
                .build(),
        ),
        _ => OrderEventAny::Updated(
            OrderUpdatedSpec::builder()
                .trader_id(order.trader_id())
                .strategy_id(order.strategy_id())
                .instrument_id(order.instrument_id())
                .client_order_id(client_order_id)
                .account_id(test_account_id())
                .quantity(order.quantity())
                .price(Price::from("3000.00"))
                .build(),
        ),
    };
    ctx.manager.observe_order_event(&event);
    ctx.cache.borrow_mut().update_order(&event).unwrap();
    assert_eq!(
        ctx.get_order(&client_order_id).unwrap().status(),
        OrderStatus::Submitted
    );

    let mut duplicate = initialized.clone();
    duplicate.trader_id = TraderId::from("OTHER-001");
    ctx.manager
        .register_submission(&duplicate, Some(ClientId::from("OTHER")));
    let tracking = policy == SubmissionRecoveryPolicy::RetainUnresolved;
    assert_eq!(
        ctx.manager.recon_check_retry_count(&client_order_id),
        u32::from(tracking)
    );
    ctx.advance_both(dst::time::Duration::from_millis(101))
        .await;
    let mut result = ctx.manager.check_inflight_orders();
    if !tracking {
        assert_eq!(result.queries.len(), 1);
        assert!(result.events.is_empty());
        ctx.advance_both(dst::time::Duration::from_millis(101))
            .await;
        result = ctx.manager.check_inflight_orders();
    }

    assert!(result.queries.is_empty());

    if tracking {
        assert!(result.events.is_empty());
        assert_eq!(
            ctx.get_order(&client_order_id).unwrap().status(),
            OrderStatus::Submitted
        );
    } else {
        assert!(
            matches!(&result.events[..], [OrderEventAny::Rejected(event)]
        if event.client_order_id == client_order_id && event.reason == "INFLIGHT_TIMEOUT")
        );
    }

    let expected = if tracking {
        vec![SubmissionRecoveryExhausted {
            trader_id: initialized.trader_id,
            client_id: Some(test_client_id()),
            strategy_id: initialized.strategy_id,
            instrument_id: initialized.instrument_id,
            client_order_id,
            source: SubmissionRecoverySource::Inflight,
            retry_count: 2,
            ts_event: ctx.clock.borrow().timestamp_ns(),
        }]
    } else {
        Vec::new()
    };
    assert_eq!(ctx.manager.take_submission_recovery_exhaustions(), expected);
    assert!(
        ctx.manager
            .take_submission_recovery_exhaustions()
            .is_empty()
    );
}

#[rstest]
#[case::unacknowledged_cancel(false, OrderStatus::PendingCancel)]
#[case::unacknowledged_modify(false, OrderStatus::PendingUpdate)]
#[case::acknowledged_cancel(true, OrderStatus::PendingCancel)]
#[case::acknowledged_modify(true, OrderStatus::PendingUpdate)]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_submission_registry_interleaved_missing_checks_preserve_budget(
    #[case] acknowledged: bool,
    #[case] pending_status: OrderStatus,
    #[values(
        SubmissionRecoveryPolicy::ResolveLocally,
        SubmissionRecoveryPolicy::RetainUnresolved
    )]
    policy: SubmissionRecoveryPolicy,
) {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        inflight_threshold_ms: 200,
        inflight_max_retries: 2,
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 1,
        open_check_open_only: false,
        single_order_query_delay_ms: 0,
        submission_recovery_policy: policy,
        ..Default::default()
    });
    ctx.add_instrument(test_instrument());
    let order = create_limit_order(
        "O-REGISTRY-INTERLEAVED",
        test_instrument_id(),
        OrderSide::Buy,
        "1.0",
        "3000.00",
    );
    let client_order_id = order.client_order_id();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    ctx.add_order(order.clone());
    ctx.cache.borrow_mut().update_order(&submitted).unwrap();

    if acknowledged {
        let accepted = TestOrderEventStubs::accepted(
            &order,
            test_account_id(),
            VenueOrderId::from("V-INTERLEAVED"),
        );
        ctx.cache.borrow_mut().update_order(&accepted).unwrap();
    }
    ctx.manager
        .register_submission(order.init_event(), Some(test_client_id()));
    let pending = if pending_status == OrderStatus::PendingCancel {
        OrderEventAny::PendingCancel(
            OrderPendingCancelSpec::builder()
                .trader_id(order.trader_id())
                .strategy_id(order.strategy_id())
                .instrument_id(order.instrument_id())
                .client_order_id(client_order_id)
                .account_id(test_account_id())
                .build(),
        )
    } else {
        OrderEventAny::PendingUpdate(
            OrderPendingUpdateSpec::builder()
                .trader_id(order.trader_id())
                .strategy_id(order.strategy_id())
                .instrument_id(order.instrument_id())
                .client_order_id(client_order_id)
                .account_id(test_account_id())
                .build(),
        )
    };
    ctx.cache.borrow_mut().update_order(&pending).unwrap();
    ctx.manager.register_inflight(client_order_id);
    let client = MockExecutionClient::new(Vec::new());
    let tracked = policy == SubmissionRecoveryPolicy::RetainUnresolved && !acknowledged;
    let mut queries = 0;
    let mut resolutions = Vec::new();
    let mut diagnostics = Vec::new();

    for _ in 0..5 {
        ctx.advance_both(dst::time::Duration::from_millis(101))
            .await;
        assert!(ctx.manager.check_open_orders(&[&client]).await.is_empty());
        let result = ctx.manager.check_inflight_orders();
        queries += result.queries.len();

        for event in &result.events {
            ctx.cache.borrow_mut().update_order(event).unwrap();
        }
        resolutions.extend(result.events);
        diagnostics.extend(ctx.manager.take_submission_recovery_exhaustions());
    }

    assert_eq!(queries, usize::from(tracked));
    assert!(resolutions.is_empty());
    assert_eq!(
        client.order_report_query_count.get(),
        if tracked { 4 } else { 5 }
    );
    assert_eq!(
        ctx.get_order(&client_order_id).unwrap().status(),
        pending_status
    );
    assert_eq!(diagnostics.len(), usize::from(tracked));
    if tracked {
        assert_eq!(diagnostics[0].client_order_id, client_order_id);
        assert_eq!(diagnostics[0].source, SubmissionRecoverySource::Inflight);
        assert_eq!(diagnostics[0].retry_count, 2);
        assert_eq!(diagnostics[0].ts_event, UnixNanos::from(404_000_000));
    }
}

#[rstest]
#[case::submitted(false)]
#[case::pending_cancel(true)]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_retained_submission_survives_recovery_and_accepts_late_evidence(
    #[case] pending_cancel: bool,
    #[values(false, true)] client_retention: bool,
) {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 1,
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 1,
        open_check_open_only: false,
        submission_recovery_policy: if client_retention {
            SubmissionRecoveryPolicy::ResolveLocally
        } else {
            SubmissionRecoveryPolicy::RetainUnresolved
        },
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());
    let mut client = MockExecutionClient::new(vec![]);
    client.retain_unresolved_submissions = client_retention;
    assert!(ctx.manager.check_open_orders(&[&client]).await.is_empty());
    let client_order_id = ClientOrderId::from("O-RETAINED");
    let mut order = create_submitted_order(
        "O-RETAINED",
        test_instrument_id(),
        OrderSide::Buy,
        "1.0",
        "3000.00",
    );

    if pending_cancel {
        order
            .apply(OrderEventAny::PendingCancel(
                OrderPendingCancelSpec::builder()
                    .trader_id(order.trader_id())
                    .strategy_id(order.strategy_id())
                    .instrument_id(order.instrument_id())
                    .client_order_id(order.client_order_id())
                    .account_id(test_account_id())
                    .build(),
            ))
            .unwrap();
    }

    ctx.add_order(order.clone());
    ctx.manager
        .register_submission(order.init_event(), Some(test_client_id()));
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;

    let exhausted = ctx.manager.check_inflight_orders();
    assert_eq!(
        ctx.manager.take_submission_recovery_exhaustions(),
        vec![SubmissionRecoveryExhausted {
            trader_id: order.trader_id(),
            client_id: Some(test_client_id()),
            strategy_id: order.strategy_id(),
            instrument_id: order.instrument_id(),
            client_order_id,
            source: SubmissionRecoverySource::Inflight,
            retry_count: 1,
            ts_event: ctx.clock.borrow().timestamp_ns(),
        }]
    );
    let mut duplicate = order.init_event().clone();
    duplicate.strategy_id = StrategyId::from("OTHER-002");
    ctx.manager
        .register_submission(&duplicate, Some(ClientId::from("OTHER")));
    ctx.manager.register_inflight(client_order_id);
    ctx.manager.clear_recon_tracking(&client_order_id, true);
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;
    let missing = ctx.manager.check_open_orders(&[&client]).await;
    let repeated = ctx.manager.check_inflight_orders();
    let queries = ctx.manager.check_open_order_queries();
    let accepted =
        TestOrderEventStubs::accepted(&order, test_account_id(), VenueOrderId::from("V-LATE"));
    ctx.cache.borrow_mut().update_order(&accepted).unwrap();
    ctx.manager.confirm_submission_outcome(&client_order_id);

    assert!(exhausted.events.is_empty());
    assert!(exhausted.queries.is_empty());
    assert!(missing.is_empty());
    assert!(repeated.events.is_empty());
    assert!(repeated.queries.is_empty());
    assert!(queries.is_empty());
    assert!(
        ctx.manager
            .take_submission_recovery_exhaustions()
            .is_empty()
    );
    assert_eq!(client.order_report_query_count.get(), 0);
    assert_eq!(
        ctx.cache.borrow().order(&client_order_id).unwrap().status(),
        OrderStatus::Accepted
    );
}

#[rstest]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_submission_retention_is_scoped_to_client(
    #[values(false, true)] explicit_client: bool,
) {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 1,
        ..Default::default()
    });

    ctx.add_instrument(test_instrument());
    let mut protected = MockExecutionClient::new(Vec::new());
    protected.retain_unresolved_submissions = true;
    let ordinary_id = ClientId::from("ORDINARY");
    let ordinary = MockExecutionClient::for_venue(ordinary_id, test_venue(), Vec::new());
    assert!(
        ctx.manager
            .check_open_orders(&[&protected, &ordinary])
            .await
            .is_empty()
    );

    let protected_order = create_submitted_order(
        "O-PROTECTED",
        test_instrument_id(),
        OrderSide::Buy,
        "1.0",
        "3000.00",
    );
    let ordinary_order = create_submitted_order(
        "O-ORDINARY",
        test_instrument_id(),
        OrderSide::Buy,
        "2.0",
        "3001.00",
    );

    for (order, client_id) in [
        (&protected_order, test_client_id()),
        (&ordinary_order, ordinary_id),
    ] {
        ctx.add_order_with_client_id(order.clone(), client_id);
        ctx.manager
            .register_submission(order.init_event(), explicit_client.then_some(client_id));
    }

    ctx.advance_both(dst::time::Duration::from_millis(101))
        .await;
    let result = ctx.manager.check_inflight_orders();

    for event in &result.events {
        ctx.cache.borrow_mut().update_order(event).unwrap();
    }

    assert!(result.queries.is_empty());
    assert_eq!(result.events.len(), 1);

    let OrderEventAny::Rejected(rejected) = &result.events[0] else {
        panic!("Expected timeout rejection, was {:?}", result.events[0]);
    };

    assert_eq!(rejected.client_order_id, ordinary_order.client_order_id());
    assert_eq!(rejected.reason, "INFLIGHT_TIMEOUT");
    assert_eq!(
        ctx.get_order(&ordinary_order.client_order_id())
            .unwrap()
            .status(),
        OrderStatus::Rejected
    );
    assert_eq!(
        ctx.get_order(&protected_order.client_order_id())
            .unwrap()
            .status(),
        OrderStatus::Submitted
    );
    assert_eq!(
        ctx.manager.take_submission_recovery_exhaustions(),
        vec![SubmissionRecoveryExhausted {
            trader_id: protected_order.trader_id(),
            client_id: Some(test_client_id()),
            strategy_id: protected_order.strategy_id(),
            instrument_id: protected_order.instrument_id(),
            client_order_id: protected_order.client_order_id(),
            source: SubmissionRecoverySource::Inflight,
            retry_count: 1,
            ts_event: ctx.clock.borrow().timestamp_ns(),
        }]
    );
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_inflight_timeout_uses_monotonic_gate_and_domain_event_timestamp() {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 1,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-SPLIT");

    ctx.add_instrument(test_instrument());
    let order = create_submitted_order("O-SPLIT", instrument_id, OrderSide::Buy, "1.0", "3000.00");
    ctx.add_order(order);

    ctx.manager.register_inflight(client_order_id);
    let domain_ts = ctx.clock.borrow().timestamp_ns();
    advance_clock(dst::time::Duration::from_millis(200)).await;

    let result = ctx.manager.check_inflight_orders();

    assert_eq!(result.events.len(), 1);
    assert_eq!(result.events[0].ts_event(), domain_ts);
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_inflight_check_skips_filtered_order_ids() {
    let filtered_id = ClientOrderId::from("O-FILTERED");

    let mut config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 1,
        ..Default::default()
    };

    config.filtered_client_order_ids.insert(filtered_id);
    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();

    ctx.add_instrument(test_instrument());

    // Use submitted order (has account_id) to verify filtering, not missing account_id
    let order = create_submitted_order(
        "O-FILTERED",
        instrument_id,
        OrderSide::Buy,
        "1.0",
        "3000.00",
    );
    ctx.add_order(order);

    ctx.manager.register_inflight(filtered_id);
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;

    let result = ctx.manager.check_inflight_orders();

    // Filtered order should not generate rejection
    assert!(result.events.is_empty());
}

#[rstest]
fn test_config_default_values() {
    let config = ExecutionManagerConfig::default();

    assert_eq!(config.lookback_mins, Some(60));
    assert!(!config.filter_unclaimed_external);
    assert!(!config.filter_position_reports);
    assert!(config.generate_missing_orders);
    assert_eq!(config.inflight_threshold_ms, 5_000);
    assert_eq!(config.inflight_max_retries, 5);
    assert_eq!(
        config.submission_recovery_policy,
        SubmissionRecoveryPolicy::ResolveLocally,
    );
}

#[rstest]
fn test_config_with_trader_id() {
    let trader_id = TraderId::from("TRADER-001");
    let config = ExecutionManagerConfig::default().with_trader_id(trader_id);

    assert_eq!(config.trader_id, trader_id);
}

#[rstest]
fn test_purge_operations_do_nothing_when_disabled() {
    let config = ExecutionManagerConfig {
        purge_closed_orders_buffer_mins: None,
        purge_closed_positions_buffer_mins: None,
        purge_account_events_lookback_mins: None,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);

    ctx.manager.purge_closed_orders();
    ctx.manager.purge_closed_positions();
    ctx.manager.purge_account_events();
}

#[tokio::test]
async fn test_reconcile_mass_status_accepted_order_canceled_at_venue() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");

    ctx.add_instrument(test_instrument());

    // Create and accept order locally
    let mut order =
        create_submitted_order("O-001", instrument_id, OrderSide::Buy, "1.0", "3000.00");

    // Apply accepted event to put order in ACCEPTED state
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order);

    // Venue reports order was canceled
    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Canceled,
        Quantity::from("1.0"),
        Quantity::from("0"),
    )
    .with_cancel_reason("USER_REQUEST".to_string());
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Canceled(_)));

    if let OrderEventAny::Canceled(canceled) = &result.events[0] {
        assert_eq!(canceled.client_order_id, client_order_id);
        assert!(canceled.reconciliation); // Verify reconciliation flag is set
    }
}

#[tokio::test]
async fn test_reconcile_mass_status_accepted_order_expired_at_venue() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-002");
    let venue_order_id = VenueOrderId::from("V-002");

    ctx.add_instrument(test_instrument());

    // Create and accept order locally
    let mut order =
        create_submitted_order("O-002", instrument_id, OrderSide::Sell, "2.0", "3100.00");

    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order);

    // Venue reports order expired
    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Expired,
        Quantity::from("2.0"),
        Quantity::from("0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Expired(_)));
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_inflight_increments_retry_count_before_max() {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 3,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");

    ctx.add_instrument(test_instrument());
    let order = create_submitted_order("O-001", instrument_id, OrderSide::Buy, "1.0", "3000.00");
    ctx.add_order(order);

    ctx.manager.register_inflight(client_order_id);

    // First check - past threshold, retry count becomes 1, generates QueryOrder
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;
    let result1 = ctx.manager.check_inflight_orders();
    assert!(result1.events.is_empty()); // Not at max yet
    assert_eq!(result1.queries.len(), 1);

    // Second check - retry count becomes 2, generates QueryOrder
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;
    let result2 = ctx.manager.check_inflight_orders();
    assert!(result2.events.is_empty()); // Still not at max
    assert_eq!(result2.queries.len(), 1);

    // Third check - retry count becomes 3, equals max, generates rejection
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;
    let result3 = ctx.manager.check_inflight_orders();
    assert_eq!(result3.events.len(), 1);
    assert!(matches!(result3.events[0], OrderEventAny::Rejected(_)));
    assert!(result3.queries.is_empty());
}

#[rstest]
#[case::resolve_locally(SubmissionRecoveryPolicy::ResolveLocally)]
#[case::retain_unresolved(SubmissionRecoveryPolicy::RetainUnresolved)]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_inflight_pending_update_generates_canceled(#[case] policy: SubmissionRecoveryPolicy) {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 1,
        submission_recovery_policy: policy,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-PENDING-UPD");
    let venue_order_id = VenueOrderId::from("V-001");

    ctx.add_instrument(test_instrument());

    let order = create_pending_update_order(
        "O-PENDING-UPD",
        instrument_id,
        OrderSide::Buy,
        "1.0",
        "3000.00",
        venue_order_id,
    );
    ctx.add_order(order);

    ctx.manager.register_inflight(client_order_id);
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await; // 200ms, past threshold

    let result = ctx.manager.check_inflight_orders();

    assert!(
        ctx.manager
            .take_submission_recovery_exhaustions()
            .is_empty()
    );
    assert_eq!(result.events.len(), 1);
    assert!(
        matches!(result.events[0], OrderEventAny::Canceled(_)),
        "Expected Canceled for PendingUpdate, was {:?}",
        result.events[0]
    );

    if let OrderEventAny::Canceled(canceled) = &result.events[0] {
        assert_eq!(canceled.client_order_id, client_order_id);
    }
}

#[rstest]
#[case::resolve_locally(SubmissionRecoveryPolicy::ResolveLocally)]
#[case::retain_unresolved(SubmissionRecoveryPolicy::RetainUnresolved)]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_inflight_pending_cancel_generates_canceled(#[case] policy: SubmissionRecoveryPolicy) {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 1,
        submission_recovery_policy: policy,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-PENDING-CXL");
    let venue_order_id = VenueOrderId::from("V-002");

    ctx.add_instrument(test_instrument());

    let order = create_pending_cancel_order(
        "O-PENDING-CXL",
        instrument_id,
        OrderSide::Buy,
        "1.0",
        "3000.00",
        venue_order_id,
    );
    ctx.add_order(order);

    ctx.manager.register_inflight(client_order_id);
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;

    let result = ctx.manager.check_inflight_orders();

    assert!(
        ctx.manager
            .take_submission_recovery_exhaustions()
            .is_empty()
    );
    assert_eq!(result.events.len(), 1);
    assert!(
        matches!(result.events[0], OrderEventAny::Canceled(_)),
        "Expected Canceled for PendingCancel, was {:?}",
        result.events[0]
    );

    if let OrderEventAny::Canceled(canceled) = &result.events[0] {
        assert_eq!(canceled.client_order_id, client_order_id);
    }
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_inflight_generates_query_before_max_retries() {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 3,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-QUERY");

    ctx.add_instrument(test_instrument());
    let order = create_submitted_order("O-QUERY", instrument_id, OrderSide::Buy, "1.0", "3000.00");
    ctx.add_order(order);

    ctx.manager.register_inflight(client_order_id);

    // First check - past threshold, retry 1 < max 3 -> generates QueryOrder
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;
    let result = ctx.manager.check_inflight_orders();

    assert!(
        result.events.is_empty(),
        "Should not generate terminal events yet"
    );
    assert_eq!(result.queries.len(), 1, "Should generate one QueryOrder");

    if let TradingCommand::QueryOrder(query) = &result.queries[0] {
        assert_eq!(query.client_order_id, client_order_id);
    } else {
        panic!("Expected QueryOrder, was {:?}", result.queries[0]);
    }
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_inflight_no_query_at_max_retries() {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 2,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-MAX");

    ctx.add_instrument(test_instrument());
    let order = create_submitted_order("O-MAX", instrument_id, OrderSide::Buy, "1.0", "3000.00");
    ctx.add_order(order);

    ctx.manager.register_inflight(client_order_id);

    // First check - intermediate, generates query
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;
    let result1 = ctx.manager.check_inflight_orders();
    assert!(result1.events.is_empty());
    assert_eq!(result1.queries.len(), 1);

    // Second check - at max retries, generates terminal event only
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;
    let result2 = ctx.manager.check_inflight_orders();

    assert_eq!(
        result2.events.len(),
        1,
        "Should generate terminal event at max retries"
    );
    assert!(
        result2.queries.is_empty(),
        "Should not generate queries at max retries"
    );
    assert!(matches!(result2.events[0], OrderEventAny::Rejected(_)));
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_inflight_query_preserves_client_id_routing() {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 3,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-ROUTED");
    let client_id = ClientId::from("EXEC-002");

    ctx.add_instrument(test_instrument());
    let order = create_submitted_order("O-ROUTED", instrument_id, OrderSide::Buy, "1.0", "3000.00");
    ctx.add_order_with_client_id(order, client_id);

    ctx.manager.register_inflight(client_order_id);
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;

    let result = ctx.manager.check_inflight_orders();
    assert_eq!(result.queries.len(), 1);

    if let TradingCommand::QueryOrder(query) = &result.queries[0] {
        assert_eq!(
            query.client_id,
            Some(client_id),
            "QueryOrder should preserve the execution client routing"
        );
        assert_eq!(query.client_order_id, client_order_id);
    } else {
        panic!("Expected QueryOrder, was {:?}", result.queries[0]);
    }
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_inflight_query_throttled_within_threshold() {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 5,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-THROTTLE");

    ctx.add_instrument(test_instrument());
    let order = create_submitted_order(
        "O-THROTTLE",
        instrument_id,
        OrderSide::Buy,
        "1.0",
        "3000.00",
    );
    ctx.add_order(order);

    ctx.manager.register_inflight(client_order_id);

    // First check past threshold generates a query
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;
    let result1 = ctx.manager.check_inflight_orders();
    assert_eq!(result1.queries.len(), 1);

    // Immediate second check without advancing time is throttled
    let result2 = ctx.manager.check_inflight_orders();
    assert!(result2.queries.is_empty(), "Query should be throttled");
    assert!(result2.events.is_empty());

    // Advance past threshold again, query should fire
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;
    let result3 = ctx.manager.check_inflight_orders();
    assert_eq!(result3.queries.len(), 1);
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_inflight_accepted_order_at_max_retries_no_event() {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 1,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-ACCEPTED");
    let venue_order_id = VenueOrderId::from("V-001");

    ctx.add_instrument(test_instrument());
    let order = create_accepted_order(
        "O-ACCEPTED",
        instrument_id,
        OrderSide::Buy,
        "1.0",
        "3000.00",
        venue_order_id,
    );
    ctx.add_order(order);

    ctx.manager.register_inflight(client_order_id);
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;

    let result = ctx.manager.check_inflight_orders();

    // Accepted orders are already resolved, no terminal event generated
    assert!(result.events.is_empty());
    assert!(result.queries.is_empty());

    // Tracking should be cleared, subsequent check also empty
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;
    let result2 = ctx.manager.check_inflight_orders();
    assert!(result2.events.is_empty());
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_inflight_order_not_in_cache_at_max_retries_no_event() {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 1,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let client_order_id = ClientOrderId::from("O-MISSING");

    // Register inflight without adding order to cache
    ctx.manager.register_inflight(client_order_id);
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;

    let result = ctx.manager.check_inflight_orders();

    assert!(result.events.is_empty());
    assert!(result.queries.is_empty());

    // Tracking cleared, subsequent check also empty
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;
    let result2 = ctx.manager.check_inflight_orders();
    assert!(result2.events.is_empty());
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_inflight_terminal_event_clears_tracking() {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 1,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-TERM");

    ctx.add_instrument(test_instrument());
    let order = create_submitted_order("O-TERM", instrument_id, OrderSide::Buy, "1.0", "3000.00");
    ctx.add_order(order);

    ctx.manager.register_inflight(client_order_id);
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;

    // First check generates terminal rejection
    let result1 = ctx.manager.check_inflight_orders();
    assert_eq!(result1.events.len(), 1);
    assert!(matches!(result1.events[0], OrderEventAny::Rejected(_)));

    // Second check should return empty (tracking was cleared)
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;
    let result2 = ctx.manager.check_inflight_orders();
    assert!(result2.events.is_empty());
    assert!(result2.queries.is_empty());
}

#[rstest]
fn test_observe_fill_report_without_client_order_id_uses_cache_fallback() {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 5,
        open_check_threshold_ns: DurationNanos::from_secs(1),
        max_single_order_queries_per_cycle: 5,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let trade_id = TradeId::from("T-FALLBACK");

    ctx.add_instrument(test_instrument());
    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, test_client_id());

    // Register inflight so we can verify activity recording deferred the check
    ctx.manager.register_inflight(client_order_id);

    // Create fill report without client_order_id
    let fill_report = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        trade_id,
        OrderSide::Buy,
        Quantity::from("1.0"),
        Price::from("3000.00"),
        Money::new(0.0, Currency::USDT()),
        LiquiditySide::Maker,
        None, // No client_order_id: triggers cache fallback
        None,
        UnixNanos::from(2_000_000),
        UnixNanos::from(2_000_000),
        None,
    );
    let report = ExecutionReport::Fill(Box::new(fill_report));

    // observe should resolve client_order_id via cache and record activity
    ctx.manager.observe_execution_report(&report);

    let queries = ctx.manager.check_open_order_queries();
    assert!(queries.is_empty());
}

#[tokio::test]
async fn test_reconcile_mass_status_external_order_partially_filled() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        None, // External order
        VenueOrderId::from("V-EXT-PARTIAL"),
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("3.0"),
    )
    .with_avg_px(dec!(3000.50));
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // External orders get: Accepted + Filled (for the partial fill)
    assert_eq!(result.events.len(), 2);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));
    assert!(matches!(result.events[1], OrderEventAny::Filled(_)));

    if let OrderEventAny::Filled(filled) = &result.events[1] {
        assert_eq!(filled.last_qty, Quantity::from("3.0"));
        assert!(filled.reconciliation);
    }

    // Verify order was created in cache (status is Initialized since events haven't been applied)
    let client_order_id = ClientOrderId::from("V-EXT-PARTIAL");
    let order = ctx.get_order(&client_order_id);
    assert!(order.is_some());
}

#[tokio::test]
async fn test_reconcile_mass_status_order_already_in_sync() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-SYNC");
    let venue_order_id = VenueOrderId::from("V-SYNC");

    ctx.add_instrument(test_instrument());

    // Create accepted order locally
    let mut order =
        create_submitted_order("O-SYNC", instrument_id, OrderSide::Buy, "5.0", "3000.00");
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order);

    // Venue reports exact same state
    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("5.0"),
        Quantity::from("0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // No events needed - already in sync
    assert!(result.events.is_empty());
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_clear_recon_tracking_removes_inflight() {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 5,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let client_order_id = ClientOrderId::from("O-001");

    ctx.manager.register_inflight(client_order_id);

    // Simulate order being resolved externally (e.g., accepted by venue)
    ctx.manager.clear_recon_tracking(&client_order_id, true);

    // Advance time past threshold
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;

    // Check should not generate events since order was cleared
    let result = ctx.manager.check_inflight_orders();
    assert!(result.events.is_empty());
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

fn create_pending_cancel_order(
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
    let pending = OrderEventAny::PendingCancel(
        OrderPendingCancelSpec::builder()
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

#[tokio::test]
async fn test_inferred_fill_generated_when_venue_reports_filled() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-FILL-001");
    let venue_order_id = VenueOrderId::from("V-FILL-001");

    ctx.add_instrument(test_instrument());

    // Create accepted order with no fills yet
    let order = create_accepted_order(
        "O-FILL-001",
        instrument_id,
        OrderSide::Buy,
        "10.0",
        "3000.00",
        venue_order_id,
    );
    ctx.add_order(order);

    // Venue reports order as partially filled (no FillReport, just status)
    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("5.0"), // 5 filled
    )
    .with_avg_px(dec!(3001.50));
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should generate an inferred fill
    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Filled(_)));

    if let OrderEventAny::Filled(filled) = &result.events[0] {
        assert_eq!(filled.client_order_id, client_order_id);
        assert_eq!(filled.last_qty, Quantity::from("5.0"));
        assert!(filled.reconciliation);
        assert_eq!(filled.trade_id.as_str().len(), 36);
    }
}

#[rstest]
#[case::filled(OrderStatus::Filled, "10.0", 1)]
#[case::canceled(OrderStatus::Canceled, "4.5", 2)]
#[case::expired(OrderStatus::Expired, "4.5", 2)]
#[tokio::test]
async fn test_mass_status_retries_missing_fill_data(
    #[case] status: OrderStatus,
    #[case] filled_qty: Quantity,
    #[case] event_count: usize,
) {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-COMMISSION-MASS-001");
    let venue_order_id = VenueOrderId::from("V-COMMISSION-MASS-001");
    let commission = Money::from("1.25 USDT");
    let commission_failure = Rc::new(Cell::new(true));

    ctx.add_instrument(test_instrument());
    let order = create_accepted_order(
        "O-COMMISSION-MASS-001",
        instrument_id,
        OrderSide::Buy,
        "10.0",
        "3000.00",
        venue_order_id,
    );
    ctx.add_order_with_client_id(order, test_client_id());
    ctx.exec_engine
        .borrow_mut()
        .deregister_client(test_client_id())
        .expect("generic test execution client deregisters");
    ctx.exec_engine
        .borrow_mut()
        .register_client(Box::new(
            MockExecutionClient::new(Vec::new())
                .with_commission(commission, commission_failure.clone()),
        ))
        .expect("test execution client registers");

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        status,
        Quantity::from("10.0"),
        filled_qty,
    )
    .with_avg_px(dec!(3001.50));
    let failed_mass_status = create_mass_status(vec![report.clone()], Vec::new());

    let failed = ctx
        .manager
        .reconcile_execution_mass_status(&failed_mass_status, &ctx.exec_engine);

    assert!(failed.events.is_empty());
    let order = ctx
        .get_order(&client_order_id)
        .expect("cached order remains available for retry");
    assert_eq!(order.status(), OrderStatus::Accepted);
    assert_eq!(order.filled_qty(), Quantity::from("0.0"));
    assert_eq!(order.commissions().get(&Currency::USDT()), None);

    commission_failure.set(false);
    let trade_id = TradeId::from("T-TERMINAL-MASS-001");

    let fills = if status == OrderStatus::Filled {
        Vec::new()
    } else {
        let mut fill = create_fill_report(
            client_order_id,
            venue_order_id,
            instrument_id,
            trade_id,
            "4.5",
        );
        fill.last_px = Price::from("3001.50");
        fill.commission = commission;
        vec![fill]
    };

    let retry_mass_status = create_mass_status(vec![report.clone()], fills);
    let retry = ctx
        .manager
        .reconcile_execution_mass_status(&retry_mass_status, &ctx.exec_engine);

    assert_eq!(retry.events.len(), event_count);

    let OrderEventAny::Filled(fill) = &retry.events[0] else {
        panic!("expected fill on valid retry");
    };

    assert_eq!(fill.client_order_id, client_order_id);
    assert_eq!(fill.last_qty, filled_qty);
    assert_eq!(fill.last_px, Price::from("3001.50"));
    assert_eq!(fill.commission, Some(commission));
    assert!(fill.reconciliation);

    if status != OrderStatus::Filled {
        assert_eq!(fill.trade_id, trade_id);
    }

    let order = ctx
        .get_order(&client_order_id)
        .expect("retry updates the cached order");
    assert_eq!(order.status(), status);
    assert_eq!(order.filled_qty(), filled_qty);
    assert_eq!(
        order.commissions().get(&Currency::USDT()),
        Some(&commission)
    );

    let repeated = ctx.manager.reconcile_execution_mass_status(
        &create_mass_status(vec![report], Vec::new()),
        &ctx.exec_engine,
    );

    assert!(repeated.events.is_empty());
}

#[tokio::test]
async fn test_inferred_fill_uses_avg_px_for_first_fill() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-AVG-001");
    let venue_order_id = VenueOrderId::from("V-AVG-001");

    ctx.add_instrument(test_instrument());

    let order = create_accepted_order(
        "O-AVG-001",
        instrument_id,
        OrderSide::Buy,
        "10.0",
        "3000.00",
        venue_order_id,
    );
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("3.0"),
    )
    .with_avg_px(dec!(2999.75));
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 1);

    if let OrderEventAny::Filled(filled) = &result.events[0] {
        // First fill should use avg_px directly
        assert_eq!(filled.last_px.as_f64(), 2999.75);
    }
}

#[tokio::test]
async fn test_no_inferred_fill_when_already_in_sync() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-SYNC-001");
    let venue_order_id = VenueOrderId::from("V-SYNC-001");

    ctx.add_instrument(test_instrument());

    // Create an order that is already partially filled
    let mut order = create_accepted_order(
        "O-SYNC-001",
        instrument_id,
        OrderSide::Buy,
        "10.0",
        "3000.00",
        venue_order_id,
    );

    // Apply a fill to the order
    let fill = TestOrderEventStubs::filled(
        &order,
        &test_instrument(),
        None,                        // trade_id
        None,                        // position_id
        None,                        // last_px
        Some(Quantity::from("5.0")), // last_qty
        None,                        // liquidity_side
        None,                        // commission
        None,                        // ts_filled_ns
        None,                        // account_id
    );
    order.apply(fill).unwrap();
    ctx.add_order(order);

    // Venue reports same fill state
    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("5.0"), // Same as local
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // No events needed - already in sync
    assert!(result.events.is_empty());
}

#[rstest]
#[case(false)]
#[case(true)]
#[tokio::test]
async fn test_fill_qty_mismatch_venue_less_generates_fill_void(#[case] echo_cached_fill: bool) {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-MISMATCH");
    let venue_order_id = VenueOrderId::from("V-MISMATCH");
    let trade_id = TradeId::from("T-MISMATCH");

    ctx.add_instrument(test_instrument());

    // Create an order that is already partially filled with 5
    let mut order = create_accepted_order(
        "O-MISMATCH",
        instrument_id,
        OrderSide::Buy,
        "10.0",
        "3000.00",
        venue_order_id,
    );
    let fill = OrderFilledTestBuilder::new(&order, &test_instrument())
        .trade_id(trade_id)
        .account_id(test_account_id())
        .last_qty(Quantity::from("5.0"))
        .without_position_id()
        .build();
    order.apply(fill).unwrap();
    ctx.add_order(order);

    // Venue reports less filled than Nautilus has applied.
    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("3.0"), // Less than our 5
    );
    mass_status.add_order_reports(vec![report]);

    if echo_cached_fill {
        mass_status.add_fill_reports(vec![create_fill_report(
            client_order_id,
            venue_order_id,
            instrument_id,
            trade_id,
            "5.0",
        )]);
    }

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let order = ctx
        .cache
        .borrow()
        .order_owned(&client_order_id)
        .expect("order cached");

    let expected_events = if echo_cached_fill {
        // The echoed fill report prices the trade at 3000.00 while the cached fill recorded
        // 1.0, so reconciliation first restates the trade's price, then voids the decrease
        let OrderEventAny::Filled(restatement) = &result.events[0] else {
            panic!("expected price restatement fill event");
        };

        assert_eq!(restatement.trade_id, trade_id);
        assert_eq!(restatement.last_qty, Quantity::from("5.0"));
        assert_eq!(restatement.last_px, Price::from("3000.00"));
        2
    } else {
        1
    };

    assert_eq!(result.events.len(), expected_events);

    let OrderEventAny::FillVoided(voided) = &result.events[expected_events - 1] else {
        panic!("expected OrderFillVoided event");
    };

    assert_eq!(voided.client_order_id, client_order_id);
    assert_eq!(voided.trade_id, trade_id);
    assert_eq!(voided.voided_qty, Quantity::from("2.0"));
    assert!(voided.is_reopened);
    assert_eq!(order.status(), OrderStatus::PartiallyFilled);
    assert_eq!(order.filled_qty(), Quantity::from("3.0"));
    assert_eq!(order.voided_qty(), Quantity::from("2.0"));
    assert_eq!(
        order.avg_px(),
        if echo_cached_fill {
            Some(dec!(3000.00))
        } else {
            Some(dec!(1.0))
        }
    );
}

#[tokio::test]
async fn test_market_order_inferred_fill_is_taker() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-MKT-001");
    let venue_order_id = VenueOrderId::from("V-MKT-001");

    ctx.add_instrument(test_instrument());

    // Create a market order (submitted and accepted)
    let mut order = OrderTestBuilder::new(OrderType::Market)
        .client_order_id(ClientOrderId::from("O-MKT-001"))
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("10.0"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = OrderStatusReport::new(
        test_account_id(),
        instrument_id,
        Some(client_order_id),
        venue_order_id,
        OrderSide::Buy.into(),
        OrderType::Market,
        TimeInForce::Ioc,
        OrderStatus::Filled,
        Quantity::from("10.0"),
        Quantity::from("10.0"),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        Some(UUID4::new()),
    )
    .with_avg_px(dec!(3005.00));
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 1);

    if let OrderEventAny::Filled(filled) = &result.events[0] {
        assert_eq!(filled.liquidity_side, LiquiditySide::Taker);
    }
}

#[tokio::test]
async fn test_pending_cancel_status_no_event() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-PEND-001");
    let venue_order_id = VenueOrderId::from("V-PEND-001");

    ctx.add_instrument(test_instrument());

    let order = create_accepted_order(
        "O-PEND-001",
        instrument_id,
        OrderSide::Buy,
        "10.0",
        "3000.00",
        venue_order_id,
    );
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PendingCancel,
        Quantity::from("10.0"),
        Quantity::from("0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Pending states don't generate events
    assert!(result.events.is_empty());
}

#[tokio::test]
async fn test_incremental_fill_calculates_weighted_price() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-INCR-001");
    let venue_order_id = VenueOrderId::from("V-INCR-001");

    ctx.add_instrument(test_instrument());

    // Create an order that already has 5 filled at 3000.00
    let mut order = create_accepted_order(
        "O-INCR-001",
        instrument_id,
        OrderSide::Buy,
        "10.0",
        "3000.00",
        venue_order_id,
    );
    let fill = TestOrderEventStubs::filled(
        &order,
        &test_instrument(),
        None,                         // trade_id
        None,                         // position_id
        Some(Price::from("3000.00")), // last_px
        Some(Quantity::from("5.0")),  // last_qty
        None,                         // liquidity_side
        None,                         // commission
        None,                         // ts_filled_ns
        None,                         // account_id
    );
    order.apply(fill).unwrap();
    ctx.add_order(order);

    // Venue reports 8 filled total at avg_px 3002.50
    // Original: 5 @ 3000.00 = 15000
    // New avg: 8 @ 3002.50 = 24020
    // Incremental: 3 @ (24020 - 15000) / 3 = 3006.67
    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("8.0"),
    )
    .with_avg_px(dec!(3002.50));
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 1);

    if let OrderEventAny::Filled(filled) = &result.events[0] {
        assert_eq!(filled.last_qty, Quantity::from("3.0"));
        // (8 * 3002.50 - 5 * 3000.00) / 3 ≈ 3006.67
        let expected_px = (8.0 * 3002.50 - 5.0 * 3000.00) / 3.0;
        assert!((filled.last_px.as_f64() - expected_px).abs() < 0.01);
    }
}

#[rstest]
#[tokio::test]
async fn test_mass_status_skips_exact_duplicate_orders() {
    let mut ctx = TestContext::new();
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let instrument_id = test_instrument_id();

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .quantity(Quantity::from("1.0"))
        .price(Price::from("100.0"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );
    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0.0"),
    )
    .with_price(Price::from("100.0"));
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert!(result.events.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_mass_status_deduplicates_within_batch() {
    let mut ctx = TestContext::new();
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let instrument_id = test_instrument_id();

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .quantity(Quantity::from("1.0"))
        .price(Price::from("100.0"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report1 = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0.0"),
    );
    let report2 = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0.0"),
    );
    mass_status.add_order_reports(vec![report1, report2]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));
}

#[rstest]
#[tokio::test]
async fn test_mass_status_reconciles_when_status_differs() {
    let mut ctx = TestContext::new();
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let instrument_id = test_instrument_id();

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .quantity(Quantity::from("1.0"))
        .price(Price::from("100.0"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );
    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Canceled,
        Quantity::from("1.0"),
        Quantity::from("0.0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Canceled(_)));
}

#[rstest]
#[tokio::test]
async fn test_mass_status_reconciles_when_filled_qty_differs() {
    let mut ctx = TestContext::new();
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let instrument_id = test_instrument_id();

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .quantity(Quantity::from("10.0"))
        .price(Price::from("100.0"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );
    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("5.0"),
    )
    .with_avg_px(dec!(100.0));
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 1);

    if let OrderEventAny::Filled(filled) = &result.events[0] {
        assert_eq!(filled.last_qty, Quantity::from("5.0"));
    } else {
        panic!("Expected OrderFilled event");
    }
}

#[rstest]
#[tokio::test]
async fn test_mass_status_matches_order_by_venue_order_id() {
    let mut ctx = TestContext::new();
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let instrument_id = test_instrument_id();

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .quantity(Quantity::from("1.0"))
        .price(Price::from("100.0"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order);

    ctx.cache
        .borrow_mut()
        .add_venue_order_id(&client_order_id, &venue_order_id, false)
        .unwrap();

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );
    let report = create_order_report(
        None,
        venue_order_id,
        instrument_id,
        OrderStatus::Canceled,
        Quantity::from("1.0"),
        Quantity::from("0.0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Canceled(_)));

    if let OrderEventAny::Canceled(canceled) = &result.events[0] {
        assert_eq!(canceled.client_order_id, client_order_id);
    }
}

#[rstest]
#[tokio::test]
async fn test_mass_status_matches_order_by_venue_order_id_with_mismatched_client_id() {
    let mut ctx = TestContext::new();
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let instrument_id = test_instrument_id();

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .quantity(Quantity::from("1.0"))
        .price(Price::from("100.0"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order);

    ctx.cache
        .borrow_mut()
        .add_venue_order_id(&client_order_id, &venue_order_id, false)
        .unwrap();

    // Report has wrong client_order_id but correct venue_order_id
    let wrong_client_order_id = ClientOrderId::from("O-WRONG");

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );
    let report = create_order_report(
        Some(wrong_client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Canceled,
        Quantity::from("1.0"),
        Quantity::from("0.0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Canceled(_)));

    if let OrderEventAny::Canceled(canceled) = &result.events[0] {
        assert_eq!(canceled.client_order_id, client_order_id);
    }
}

#[tokio::test]
async fn test_reconcile_mass_status_indexes_venue_order_id_for_accepted_orders() {
    // Test that venue_order_id is properly indexed during reconciliation for orders
    // that are already in ACCEPTED state and don't generate new OrderAccepted events.
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let instrument = test_instrument();
    ctx.add_instrument(instrument);

    let client_order_id = ClientOrderId::from("O-TEST");
    let venue_order_id = VenueOrderId::from("V-123");

    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.0"))
        .client_order_id(client_order_id)
        .build();

    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();

    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order.clone());

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0.0"),
    );

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        Venue::from("SIM"),
        UnixNanos::default(),
        Some(UUID4::new()),
    );
    mass_status.add_order_reports(vec![report]);

    let _events = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(
        ctx.cache.borrow().client_order_id(&venue_order_id),
        Some(&client_order_id),
        "venue_order_id should be indexed after reconciliation"
    );
}

#[tokio::test]
async fn test_reconcile_mass_status_indexes_venue_order_id_for_external_orders() {
    // Test that venue_order_id is properly indexed for external orders discovered
    // during reconciliation.
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let instrument = test_instrument();
    ctx.add_instrument(instrument);

    let venue_order_id = VenueOrderId::from("V-EXT-001");

    let report = create_order_report(
        None,
        venue_order_id,
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0.0"),
    );

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        Venue::from("SIM"),
        UnixNanos::default(),
        Some(UUID4::new()),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert!(
        !result.events.is_empty(),
        "Should generate events for external order"
    );

    let cache_borrow = ctx.cache.borrow();
    let indexed_client_id = cache_borrow.client_order_id(&venue_order_id);
    assert!(
        indexed_client_id.is_some(),
        "venue_order_id should be indexed for external order"
    );
}

#[tokio::test]
async fn test_reconcile_mass_status_indexes_venue_order_id_for_filled_orders() {
    // Test that venue_order_id is properly indexed for orders that are already
    // FILLED and don't generate new OrderAccepted events.
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let instrument = test_instrument();
    ctx.add_instrument(instrument.clone());

    let client_order_id = ClientOrderId::from("O-FILLED");
    let venue_order_id = VenueOrderId::from("V-456");

    // Create order and process to FILLED state
    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.0"))
        .client_order_id(client_order_id)
        .build();

    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();

    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();

    let filled = TestOrderEventStubs::filled(
        &order,
        &instrument,
        Some(TradeId::from("T-1")),
        None,                          // position_id
        Some(Price::from("1.0")),      // last_px
        Some(Quantity::from("1.0")),   // last_qty
        Some(LiquiditySide::Taker),    // liquidity_side
        Some(Money::from("0.01 USD")), // commission
        None,                          // ts_filled_ns
        Some(test_account_id()),
    );
    order.apply(filled).unwrap();
    ctx.add_order(order.clone());

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("1.0"),
        Quantity::from("1.0"),
    );

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        Venue::from("SIM"),
        UnixNanos::default(),
        Some(UUID4::new()),
    );
    mass_status.add_order_reports(vec![report]);

    let _events = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let cache_borrow = ctx.cache.borrow();
    assert_eq!(
        cache_borrow.client_order_id(&venue_order_id),
        Some(&client_order_id),
        "venue_order_id should be indexed for filled order"
    );
}

#[tokio::test]
async fn test_reconcile_mass_status_skips_orders_without_loaded_instruments() {
    // Test that reconciliation properly skips orders for instruments that aren't loaded,
    // and these skipped orders don't cause validation warnings.
    let mut ctx = TestContext::new();
    let loaded_instrument_id = test_instrument_id();
    let loaded_instrument = test_instrument();
    ctx.add_instrument(loaded_instrument);

    let unloaded_instrument_id = InstrumentId::from("BTCUSDT.SIM");

    let loaded_venue_order_id = VenueOrderId::from("V-LOADED");
    let unloaded_venue_order_id = VenueOrderId::from("V-UNLOADED");

    let loaded_report = create_order_report(
        Some(ClientOrderId::from("O-LOADED")),
        loaded_venue_order_id,
        loaded_instrument_id,
        OrderStatus::Filled,
        Quantity::from("1.0"),
        Quantity::from("1.0"),
    );

    let unloaded_report = create_order_report(
        Some(ClientOrderId::from("O-UNLOADED")),
        unloaded_venue_order_id,
        unloaded_instrument_id,
        OrderStatus::Filled,
        Quantity::from("1.0"),
        Quantity::from("1.0"),
    );

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        Venue::from("SIM"),
        UnixNanos::default(),
        Some(UUID4::new()),
    );
    mass_status.add_order_reports(vec![loaded_report, unloaded_report]);

    let _events = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let cache_borrow = ctx.cache.borrow();
    let loaded_client_id = cache_borrow.client_order_id(&loaded_venue_order_id);
    assert!(
        loaded_client_id.is_some(),
        "Loaded instrument order should be indexed"
    );

    let unloaded_client_id = cache_borrow.client_order_id(&unloaded_venue_order_id);
    assert!(
        unloaded_client_id.is_none(),
        "Unloaded instrument order should not be indexed (skipped during reconciliation)"
    );
}

#[tokio::test]
async fn test_reconcile_mass_status_creates_position_from_position_report() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add a position report with no corresponding order reports
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3000.50)),
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should generate Accepted + Filled events to create the position
    assert_eq!(result.events.len(), 2);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));
    assert!(matches!(result.events[1], OrderEventAny::Filled(_)));

    if let OrderEventAny::Filled(filled) = &result.events[1] {
        assert_eq!(filled.last_qty, Quantity::from("5.0"));
        assert_eq!(filled.last_px.as_f64(), 3000.50);
        assert!(filled.reconciliation);
    }
}

#[rstest]
#[case::long(PositionSide::Long, Some(dec!(3000.50)), None, dec!(5), 0)]
#[case::short(PositionSide::Short, Some(dec!(3000.50)), None, dec!(-5), 0)]
#[case::missing_price(PositionSide::Long, None, None, Decimal::ZERO, 1)]
#[case::offset_legs(PositionSide::Long, Some(dec!(3000.50)), Some(Quantity::from("3.0")), dec!(2), 2)]
#[tokio::test]
async fn test_mass_status_netting_client_recovers_venue_position_id(
    #[case] side: PositionSide,
    #[case] avg_px: Option<Decimal>,
    #[case] opposite_qty: Option<Quantity>,
    #[case] expected_qty: Decimal,
    #[case] unresolved_count: usize,
) {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let venue_position_id = PositionId::from("P-VENUE");
    ctx.add_instrument(test_instrument());

    let mut client = MockExecutionClient::new(Vec::new());
    client.oms_type = OmsType::Netting;
    {
        let mut engine = ctx.exec_engine.borrow_mut();
        engine.deregister_client(test_client_id()).unwrap();
        engine.register_client(Box::new(client)).unwrap();
        engine.register_oms_type(StrategyId::external(), OmsType::Unspecified);
    }

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        None,
    );
    mass_status.add_position_reports(vec![PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        side,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(venue_position_id),
        avg_px,
    )]);

    if let Some(quantity) = opposite_qty {
        mass_status.add_position_reports(vec![PositionStatusReport::new(
            test_account_id(),
            instrument_id,
            PositionSide::Short,
            quantity,
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            None,
            Some(PositionId::from("P-VENUE-SHORT")),
            avg_px,
        )]);
    }

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);
    let cache = ctx.cache.borrow();
    let positions = cache.positions_open(
        None,
        Some(&instrument_id),
        None,
        Some(&test_account_id()),
        None,
    );

    assert!(cache.position(&venue_position_id).is_none());
    assert_eq!(
        positions
            .iter()
            .map(|p| p.signed_decimal_qty())
            .sum::<Decimal>(),
        expected_qty
    );

    if avg_px.is_some() {
        assert_eq!(positions.len(), 1);
        assert_eq!(cache.oms_type(&positions[0].id), Some(OmsType::Netting));
    }

    assert_eq!(result.unresolved_positions.len(), unresolved_count);
}

#[rstest]
#[case::zero_venue_price("3000.00", Decimal::ZERO, false)]
#[case::negative_venue_price("3000.00", dec!(-1), false)]
#[case::fractional_average("3000.01", dec!(3000.005), true)]
#[case::tolerance_boundary("3000.30", dec!(3000), true)]
#[case::outside_tolerance("3000.31", dec!(3000), false)]
#[tokio::test]
async fn test_mass_status_entry_price_tolerance(
    #[case] cached_price: &str,
    #[case] venue_price: Decimal,
    #[case] matches: bool,
    #[values(false, true)] hedging: bool,
) {
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let position_id = PositionId::from("P-ENTRY-PRICE");
    let position = create_test_position(
        &instrument,
        position_id,
        OrderSide::Buy,
        "5.000",
        cached_price,
    );
    ctx.add_instrument(instrument);
    ctx.add_position(&position);
    let mut mass_status = create_mass_status(vec![], vec![]);
    mass_status.add_position_reports(vec![PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.000"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        hedging.then_some(position_id),
        Some(venue_price),
    )]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert!(result.events.is_empty());
    assert_eq!(result.unresolved_positions.len(), usize::from(!matches));

    if !matches {
        assert_eq!(
            result.unresolved_positions[0],
            format!(
                "account={}, instrument={instrument_id}, venue_position_id={:?}, venue_quantity=5.000: position recovery did not restore the reported average entry price",
                test_account_id(),
                hedging.then_some(position_id),
            )
        );
    }

    assert_eq!(
        ctx.cache.borrow().position_owned(&position_id),
        Some(position)
    );
}

#[tokio::test]
async fn test_mass_status_netting_rejects_aggregate_mismatch() {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        generate_missing_orders: false,
        ..Default::default()
    });

    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let position = create_test_position(
        &instrument,
        PositionId::from("P-NET"),
        OrderSide::Buy,
        "5.000",
        "3000.00",
    );
    ctx.add_instrument(instrument);
    ctx.cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();
    let mut mass_status = create_mass_status(vec![], vec![]);
    mass_status.add_position_reports(vec![
        create_test_position_report(
            test_account_id(),
            instrument_id,
            PositionSide::Long,
            "5.000",
            "V-1",
            dec!(3000),
        ),
        create_test_position_report(
            test_account_id(),
            instrument_id,
            PositionSide::Long,
            "5.000",
            "V-2",
            dec!(3000),
        ),
    ]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert!(result.events.is_empty());
    assert_eq!(result.unresolved_positions.len(), 2);
    assert_eq!(
        ctx.cache.borrow().position_owned(&position.id),
        Some(position)
    );
}

#[rstest]
#[case::duplicates(0, 2, 0)]
#[case::distinct_snapshots(1, 0, 3)]
#[tokio::test]
async fn test_mass_status_netting_ignores_duplicate_position_reports(
    #[case] ts_last_step: u64,
    #[case] duplicate_count: usize,
    #[case] unresolved_count: usize,
) {
    let _log_guard = MANAGER_LOG_TEST_LOCK.lock().await;
    install_manager_log_capture();
    let mut ctx = TestContext::new();

    // Unique ID keeps other tests' duplicate warnings out of the exact count
    let mut instrument = crypto_perpetual_ethusdt();
    instrument.id = InstrumentId::from("ETHUSDT-DUPLICATE.BINANCE");
    let instrument_id = instrument.id;
    ctx.add_instrument(InstrumentAny::CryptoPerpetual(instrument));

    let mut client = MockExecutionClient::new(Vec::new());
    client.oms_type = OmsType::Netting;
    {
        let mut engine = ctx.exec_engine.borrow_mut();
        engine.deregister_client(test_client_id()).unwrap();
        engine.register_client(Box::new(client)).unwrap();
        engine.register_oms_type(StrategyId::external(), OmsType::Unspecified);
    }

    let mut mass_status = create_mass_status(vec![], vec![]);
    mass_status.add_position_reports(
        (0..3)
            .map(|i| {
                PositionStatusReport::new(
                    test_account_id(),
                    instrument_id,
                    PositionSide::Long,
                    Quantity::from("5.0"),
                    UnixNanos::from(1_000_000 + i * ts_last_step),
                    UnixNanos::from(2_000_000 + i),
                    None,
                    None,
                    Some(dec!(3000.50)),
                )
            })
            .collect(),
    );

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let messages = MANAGER_LOG_CAPTURE.messages.lock().clone();
    let duplicate_message = format!("Duplicate position report for {instrument_id}");
    let unresolved_message = format!(
        "account={}, instrument={instrument_id}, venue_position_id=None, venue_quantity=5.0: position recovery did not restore the reported quantity",
        test_account_id(),
    );
    let cache = ctx.cache.borrow();
    let positions = cache.positions_open(
        None,
        Some(&instrument_id),
        None,
        Some(&test_account_id()),
        None,
    );

    assert_eq!(result.events.len(), 2);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));

    let OrderEventAny::Filled(fill) = &result.events[1] else {
        panic!("Expected Filled event, was {:?}", result.events[1]);
    };

    assert_eq!(fill.order_side, OrderSide::Buy);
    assert_eq!(fill.last_qty, Quantity::from("5.0"));
    assert_eq!(fill.last_px, Price::from("3000.50"));
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].signed_decimal_qty(), dec!(5));
    assert_eq!(cache.oms_type(&positions[0].id), Some(OmsType::Netting));
    assert_eq!(
        result.unresolved_positions,
        vec![unresolved_message; unresolved_count]
    );
    assert_eq!(
        messages
            .iter()
            .filter(|message| **message == duplicate_message)
            .count(),
        duplicate_count
    );
}

#[tokio::test]
async fn test_bounded_filtered_position_preserves_order_history() {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        filter_position_reports: true,
        ..Default::default()
    });

    let instrument = test_instrument();
    let instrument_id = instrument.id();
    ctx.add_instrument(instrument);
    let mut orders = Vec::new();
    let mut fills = Vec::new();

    for (id, side, price, ts) in [
        ("1", OrderSide::Buy, "3000.00", 1_000_000),
        ("2", OrderSide::Sell, "3050.00", 2_000_000),
        ("3", OrderSide::Buy, "3100.00", 3_000_000),
    ] {
        let (order, fill) = create_bounded_fill_lifecycle(
            instrument_id,
            VenueOrderId::from(id),
            TradeId::from(id),
            side,
            "1.000",
            price,
            false,
            UnixNanos::from(ts),
        );
        orders.push(order);
        fills.push(fill);
    }

    let mut mass_status = create_mass_status(orders, fills);
    mass_status.set_report_window(Some(UnixNanos::from(500_000)), true);
    mass_status.add_position_reports(vec![PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("2.000"),
        UnixNanos::from(4_000_000),
        UnixNanos::from(4_000_000),
        None,
        None,
        Some(dec!(3200)),
    )]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 6);
    assert_eq!(result.external_orders.len(), 3);
    assert!(result.unresolved_positions.is_empty());
    let cache = ctx.cache.borrow();
    assert_eq!(cache.positions(None, None, None, None, None).len(), 0);
    assert_eq!(cache.orders(None, None, None, None, None).len(), 3);

    for id in ["1", "2", "3"] {
        let order = cache.order(&ClientOrderId::from(id)).unwrap();
        assert_eq!(order.venue_order_id(), Some(VenueOrderId::from(id)));
        assert_eq!(order.status(), OrderStatus::Filled);
        assert_eq!(order.filled_qty(), Quantity::from("1.000"));
        assert_eq!(order.trade_ids(), vec![&TradeId::from(id)]);
    }
}

#[rstest]
#[case::weighted_sides(Some(dec!(3200)), dec!(3300), true)]
#[case::long_mismatch(Some(dec!(3400)), dec!(3300), false)]
#[case::short_mismatch(Some(dec!(3200)), dec!(3500), false)]
#[case::missing_contributor(None, dec!(3300), false)]
#[case::zero_contributor(Some(Decimal::ZERO), dec!(3300), false)]
#[tokio::test]
async fn test_mass_status_entry_price_aggregates_each_side(
    #[case] second_long_price: Option<Decimal>,
    #[case] short_price: Decimal,
    #[case] matches: bool,
) {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        generate_missing_orders: false,
        ..Default::default()
    });

    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let positions = [
        create_test_position(
            &instrument,
            PositionId::from("P-LONG-1"),
            OrderSide::Buy,
            "2.000",
            "3000.00",
        ),
        create_test_position(
            &instrument,
            PositionId::from("P-LONG-2"),
            OrderSide::Buy,
            "6.000",
            "3200.00",
        ),
        create_test_position(
            &instrument,
            PositionId::from("P-SHORT"),
            OrderSide::Sell,
            "3.000",
            "3300.00",
        ),
    ];

    ctx.add_instrument(instrument);

    for position in &positions {
        ctx.cache
            .borrow_mut()
            .add_position(position, OmsType::Netting)
            .unwrap();
    }

    let mut reports = vec![
        create_test_position_report(
            test_account_id(),
            instrument_id,
            PositionSide::Long,
            "2.000",
            "V-LONG-1",
            dec!(3000),
        ),
        create_test_position_report(
            test_account_id(),
            instrument_id,
            PositionSide::Long,
            "6.000",
            "V-LONG-2",
            dec!(3200),
        ),
        create_test_position_report(
            test_account_id(),
            instrument_id,
            PositionSide::Short,
            "3.000",
            "V-SHORT",
            short_price,
        ),
    ];
    reports[1].avg_px_open = second_long_price;
    let mut mass_status = create_mass_status(vec![], vec![]);
    mass_status.add_position_reports(reports);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert!(result.events.is_empty());
    assert!(result.external_orders.is_empty());
    assert_eq!(
        result.unresolved_positions.len(),
        if matches { 0 } else { 3 }
    );

    for position in positions {
        assert_eq!(
            ctx.cache.borrow().position_owned(&position.id),
            Some(position)
        );
    }
}

#[rstest]
#[case::long(OrderSide::Buy, PositionSide::Long)]
#[case::short(OrderSide::Sell, PositionSide::Short)]
#[tokio::test]
async fn test_mass_status_reduction_uses_reported_average_and_validates_remaining_entry(
    #[case] opening_side: OrderSide,
    #[case] report_side: PositionSide,
) {
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let position_id = PositionId::from("P-REDUCTION");
    let position =
        create_test_position(&instrument, position_id, opening_side, "20.000", "3000.00");
    ctx.add_instrument(instrument);
    ctx.add_position(&position);
    let mut mass_status = create_mass_status(vec![], vec![]);
    mass_status.add_position_reports(vec![PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        report_side,
        Quantity::from("10.000"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(position_id),
        Some(dec!(3100)),
    )]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 2);

    let OrderEventAny::Filled(fill) = &result.events[1] else {
        panic!("Expected reduction fill")
    };

    assert_eq!(
        fill.order_side,
        if opening_side == OrderSide::Buy {
            OrderSide::Sell
        } else {
            OrderSide::Buy
        }
    );
    assert_eq!(fill.last_qty, Quantity::from("10.000"));
    assert_eq!(fill.last_px, Price::from("3100.00"));
    let position = ctx.cache.borrow().position_owned(&position_id).unwrap();
    assert_eq!(position.quantity, Quantity::from("10.000"));
    assert_eq!(position.avg_px_open, 3000.0);
    assert_eq!(
        result.unresolved_positions,
        vec![format!(
            "account={}, instrument={instrument_id}, venue_position_id=Some({position_id:?}), venue_quantity={}: position recovery did not restore the reported average entry price",
            test_account_id(),
            if opening_side == OrderSide::Buy {
                "10.000"
            } else {
                "-10.000"
            },
        )]
    );
}

#[rstest]
#[case::match_rejects(AvgPxReconciliation::Match, OrderSide::Buy, PositionSide::Long, true)]
#[case::opening_only_long(
    AvgPxReconciliation::OpeningOnly,
    OrderSide::Buy,
    PositionSide::Long,
    false
)]
#[case::opening_only_short(
    AvgPxReconciliation::OpeningOnly,
    OrderSide::Sell,
    PositionSide::Short,
    false
)]
#[tokio::test]
async fn test_mass_status_opening_only_average_skips_entry_price_check(
    #[case] avg_px_reconciliation: AvgPxReconciliation,
    #[case] entry_side: OrderSide,
    #[case] report_side: PositionSide,
    #[case] unresolved: bool,
) {
    let mut ctx = TestContext::new();
    let instrument = fifo_test_instrument();
    let instrument_id = instrument.id();
    let position = create_fifo_divergent_position(&instrument, entry_side);
    ctx.add_instrument(instrument);
    ctx.cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();
    let mut mass_status = create_mass_status(vec![], vec![]);
    mass_status.add_position_reports(vec![
        PositionStatusReport::new(
            test_account_id(),
            instrument_id,
            report_side,
            Quantity::from("1.000"),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            None,
            None,
            Some(dec!(60000)),
        )
        .with_avg_px_open_reconciliation(avg_px_reconciliation),
    ]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let expected_unresolved = if unresolved {
        vec![format!(
            "account={}, instrument={instrument_id}, venue_position_id=None, venue_quantity=1.000: position recovery did not restore the reported average entry price",
            test_account_id(),
        )]
    } else {
        Vec::new()
    };

    assert!(result.events.is_empty());
    assert_eq!(result.unresolved_positions, expected_unresolved);
    assert_eq!(position.avg_px_open, 55000.0);
    assert_eq!(
        ctx.cache.borrow().position_owned(&position.id),
        Some(position)
    );
}

#[tokio::test]
async fn test_mass_status_opening_only_average_recovers_empty_cache() {
    let mut ctx = TestContext::new();
    let instrument = fifo_test_instrument();
    let instrument_id = instrument.id();
    ctx.add_instrument(instrument);
    let mut mass_status = create_mass_status(vec![], vec![]);
    mass_status.add_position_reports(vec![
        PositionStatusReport::new(
            test_account_id(),
            instrument_id,
            PositionSide::Long,
            Quantity::from("1.000"),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            None,
            None,
            Some(dec!(60000)),
        )
        .with_avg_px_open_reconciliation(AvgPxReconciliation::OpeningOnly),
    ]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let fills = filled_events(&result.events);
    let cache = ctx.cache.borrow();
    let positions = cache.positions_open(
        None,
        Some(&instrument_id),
        None,
        Some(&test_account_id()),
        None,
    );
    assert_eq!(
        fills,
        vec![(
            OrderSide::Buy,
            Quantity::from("1.000"),
            Price::from("60000.00")
        )]
    );
    assert!(result.unresolved_positions.is_empty());
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].signed_decimal_qty(), dec!(1));
    assert_eq!(positions[0].avg_px_open, 60000.0);
}

#[rstest]
#[case::opening_only_increase(
    AvgPxReconciliation::OpeningOnly,
    PositionSide::Long,
    "2.000",
    dec!(65000),
    vec![(OrderSide::Buy, "1.000", "55000.00")]
)]
#[case::opening_only_reduction(
    AvgPxReconciliation::OpeningOnly,
    PositionSide::Long,
    "0.500",
    dec!(60000),
    vec![(OrderSide::Sell, "0.500", "55000.00")]
)]
#[case::opening_only_reversal(
    AvgPxReconciliation::OpeningOnly,
    PositionSide::Short,
    "1.000",
    dec!(70000),
    vec![(OrderSide::Sell, "1.000", "55000.00"), (OrderSide::Sell, "1.000", "70000.00")]
)]
#[case::match_increase_solves_average(
    AvgPxReconciliation::Match,
    PositionSide::Long,
    "2.000",
    dec!(65000),
    vec![(OrderSide::Buy, "1.000", "75000.00")]
)]
#[tokio::test]
async fn test_mass_status_opening_only_average_prices_cached_adjustments(
    #[case] avg_px_reconciliation: AvgPxReconciliation,
    #[case] report_side: PositionSide,
    #[case] report_qty: &str,
    #[case] report_avg_px: Decimal,
    #[case] expected_fills: Vec<(OrderSide, &str, &str)>,
) {
    let mut ctx = TestContext::new();
    let instrument = fifo_test_instrument();
    let instrument_id = instrument.id();
    let position = create_fifo_divergent_position(&instrument, OrderSide::Buy);
    ctx.add_instrument(instrument);
    ctx.cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();
    let mut mass_status = create_mass_status(vec![], vec![]);
    mass_status.add_position_reports(vec![
        PositionStatusReport::new(
            test_account_id(),
            instrument_id,
            report_side,
            Quantity::from(report_qty),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            None,
            None,
            Some(report_avg_px),
        )
        .with_avg_px_open_reconciliation(avg_px_reconciliation),
    ]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let expected_fills = expected_fills
        .into_iter()
        .map(|(side, qty, px)| (side, Quantity::from(qty), Price::from(px)))
        .collect::<Vec<_>>();
    assert_eq!(filled_events(&result.events), expected_fills);
    assert!(result.unresolved_positions.is_empty());
}

#[rstest]
#[case::opening_only_keeps_fills(AvgPxReconciliation::OpeningOnly, false)]
#[case::match_rejects_fifo_average(AvgPxReconciliation::Match, true)]
#[tokio::test]
async fn test_mass_status_opening_only_average_applies_full_history(
    #[case] avg_px_reconciliation: AvgPxReconciliation,
    #[case] unresolved: bool,
) {
    let mut ctx = TestContext::new();
    let instrument = fifo_test_instrument();
    let instrument_id = instrument.id();
    ctx.add_instrument(instrument);

    let mut client = MockExecutionClient::new(Vec::new());
    client.oms_type = OmsType::Netting;
    {
        let mut engine = ctx.exec_engine.borrow_mut();
        engine.deregister_client(test_client_id()).unwrap();
        engine.register_client(Box::new(client)).unwrap();
        engine.register_oms_type(StrategyId::external(), OmsType::Unspecified);
    }

    let mut orders = Vec::new();
    let mut fills = Vec::new();

    for (id, side, price, ts) in [
        ("1", OrderSide::Buy, "50000.00", 1_000_000),
        ("2", OrderSide::Buy, "60000.00", 2_000_000),
        ("3", OrderSide::Sell, "65000.00", 3_000_000),
    ] {
        let (order, fill) = create_bounded_fill_lifecycle(
            instrument_id,
            VenueOrderId::from(id),
            TradeId::from(id),
            side,
            "1.000",
            price,
            false,
            UnixNanos::from(ts),
        );
        orders.push(order);
        fills.push(fill);
    }

    let mut mass_status = create_mass_status(orders, fills);
    mass_status.add_position_reports(vec![
        PositionStatusReport::new(
            test_account_id(),
            instrument_id,
            PositionSide::Long,
            Quantity::from("1.000"),
            UnixNanos::from(4_000_000),
            UnixNanos::from(4_000_000),
            None,
            None,
            Some(dec!(60000)),
        )
        .with_avg_px_open_reconciliation(avg_px_reconciliation),
    ]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let expected_unresolved = if unresolved {
        vec![format!(
            "account={}, instrument={instrument_id}, venue_position_id=None, venue_quantity=1.000: position recovery did not restore the reported average entry price",
            test_account_id(),
        )]
    } else {
        Vec::new()
    };

    let cache = ctx.cache.borrow();
    let positions = cache.positions_open(
        None,
        Some(&instrument_id),
        None,
        Some(&test_account_id()),
        None,
    );
    assert_eq!(result.unresolved_positions, expected_unresolved);
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].signed_decimal_qty(), dec!(1));
    assert_eq!(positions[0].avg_px_open, 55000.0);
    assert_eq!(
        positions[0].trade_ids,
        AHashSet::from([TradeId::from("1"), TradeId::from("2"), TradeId::from("3")])
    );
}

#[rstest]
#[case::coarsest_precision(
    vec![(AvgPxReconciliation::Match, Some(4), dec!(0.5599)), (AvgPxReconciliation::Match, Some(6), dec!(0.5599))],
    false
)]
#[case::opening_only_contributor(
    vec![(AvgPxReconciliation::Match, None, dec!(0.56)), (AvgPxReconciliation::OpeningOnly, None, dec!(0.70))],
    false
)]
#[case::unmarked_coarse_average(
    vec![(AvgPxReconciliation::Match, None, dec!(0.5599)), (AvgPxReconciliation::Match, None, dec!(0.5599))],
    true
)]
#[tokio::test]
async fn test_mass_status_entry_price_combines_side_report_metadata(
    #[case] report_averages: Vec<(AvgPxReconciliation, Option<u8>, Decimal)>,
    #[case] unresolved: bool,
) {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        generate_missing_orders: false,
        ..Default::default()
    });

    let instrument = coarse_report_instrument();
    let instrument_id = instrument.id();

    let positions = ["P-SIDE-1", "P-SIDE-2"].map(|position_id| {
        create_test_position(
            &instrument,
            PositionId::from(position_id),
            OrderSide::Buy,
            "1.000000",
            "0.560",
        )
    });

    ctx.add_instrument(instrument);

    for position in &positions {
        ctx.cache
            .borrow_mut()
            .add_position(position, OmsType::Netting)
            .unwrap();
    }

    let reports = report_averages
        .into_iter()
        .zip(["V-SIDE-1", "V-SIDE-2"])
        .map(
            |((avg_px_reconciliation, avg_px_precision, avg_px), venue_position_id)| {
                let mut report = create_test_position_report(
                    test_account_id(),
                    instrument_id,
                    PositionSide::Long,
                    "1.000000",
                    venue_position_id,
                    avg_px,
                )
                .with_avg_px_open_reconciliation(avg_px_reconciliation);
                report.avg_px_open_precision = avg_px_precision;
                report
            },
        )
        .collect();

    let mut mass_status = create_mass_status(vec![], vec![]);
    mass_status.add_position_reports(reports);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert!(result.events.is_empty());
    assert_eq!(
        result.unresolved_positions.len(),
        if unresolved { 2 } else { 0 }
    );
}

#[rstest]
#[case::opening_only_keeps_cached_average(
    AvgPxReconciliation::OpeningOnly,
    None,
    dec!(65000),
    "55000.00"
)]
#[case::coarse_average_keeps_cached_average(AvgPxReconciliation::Match, Some(0), dec!(54994), "55000.00")]
#[case::match_solves_weighted_average(AvgPxReconciliation::Match, None, dec!(65000), "75000.00")]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_position_check_prices_increase_from_report_metadata(
    #[case] avg_px_reconciliation: AvgPxReconciliation,
    #[case] avg_px_precision: Option<u8>,
    #[case] report_avg_px: Decimal,
    #[case] expected_px: &str,
) {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = fifo_test_instrument();
    let instrument_id = instrument.id();
    let position = create_fifo_divergent_position(&instrument, OrderSide::Buy);
    let mut venue_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("2.000"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(report_avg_px),
    )
    .with_avg_px_open_reconciliation(avg_px_reconciliation);
    venue_report.avg_px_open_precision = avg_px_precision;
    ctx.add_instrument(instrument);
    ctx.add_position(&position);

    let mock_client = MockPositionExecutionClient::new(vec![], vec![venue_report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert_eq!(
        filled_events(&events),
        vec![(
            OrderSide::Buy,
            Quantity::from("1.000"),
            Price::from(expected_px)
        )]
    );
}

#[rstest]
#[case::marked(Some(4), false)]
#[case::unmarked(None, true)]
#[tokio::test]
async fn test_mass_status_coarse_average_keeps_retained_position(
    #[case] avg_px_open_precision: Option<u8>,
    #[case] unresolved: bool,
) {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = coarse_report_instrument();
    let instrument_id = instrument.id();
    let account_id = AccountId::from("POLYMARKET-001");

    // The Data API truncates the retained 8.928572 @ 0.56 position (cost basis 0.55999996)
    let position = create_test_position_for_account(
        &instrument,
        PositionId::from("P-COARSE-RETAINED"),
        OrderSide::Buy,
        "8.928572",
        "0.560",
        account_id,
    );

    let mut report = PositionStatusReport::new(
        account_id,
        instrument_id,
        PositionSide::Long,
        Quantity::from("8.928500"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(0.5599)),
    );
    report.avg_px_open_precision = avg_px_open_precision;
    ctx.add_instrument(instrument);
    ctx.add_margin_account(account_id);
    ctx.cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();

    let client = MockPositionExecutionClient::configured(
        ClientId::from("POLYMARKET"),
        account_id,
        Venue::from("POLYMARKET"),
        IndexSet::from([instrument_id.venue]),
        false,
    )
    .with_position_reports(vec![report.clone()])
    .with_position_reconciliation_tolerance(dec!(0.009999));
    let clients: Vec<&dyn ExecutionClient> = vec![&client];

    // Seeds the account tolerance, as the node builder does in production
    let seed_events = ctx.manager.check_positions_consistency(&clients).await;
    assert!(seed_events.is_empty());

    let mut mass_status = ExecutionMassStatus::new(
        client.client_id(),
        account_id,
        client.venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );
    mass_status.add_position_reports(vec![report]);
    ctx.exec_engine
        .borrow_mut()
        .register_client(Box::new(client))
        .unwrap();

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let expected_unresolved = if unresolved {
        vec![format!(
            "account={account_id}, instrument={instrument_id}, venue_position_id=None, venue_quantity=8.928500: position recovery did not restore the reported average entry price",
        )]
    } else {
        Vec::new()
    };

    assert!(result.events.is_empty());
    assert_eq!(result.unresolved_positions, expected_unresolved);
    assert_eq!(
        ctx.cache.borrow().position_owned(&position.id),
        Some(position)
    );
}

#[rstest]
#[case::marked_keeps_cached_average(Some(4), "0.560")]
#[case::unmarked_solves_weighted_average(None, "0.060")]
#[tokio::test]
async fn test_mass_status_coarse_average_prices_increase(
    #[case] avg_px_open_precision: Option<u8>,
    #[case] expected_px: &str,
) {
    let mut ctx = TestContext::new();
    let instrument = coarse_report_instrument();
    let instrument_id = instrument.id();
    let position = create_test_position(
        &instrument,
        PositionId::from("P-COARSE-INCREASE"),
        OrderSide::Buy,
        "100.000000",
        "0.560",
    );

    let mut report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("100.020000"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(0.5599)),
    );
    report.avg_px_open_precision = avg_px_open_precision;
    ctx.add_instrument(instrument);
    ctx.cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();
    let mut mass_status = create_mass_status(vec![], vec![]);
    mass_status.add_position_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(
        filled_events(&result.events),
        vec![(
            OrderSide::Buy,
            Quantity::from("0.020000"),
            Price::from(expected_px)
        )]
    );
    assert!(result.unresolved_positions.is_empty());
}

#[rstest]
#[case::missing_instrument(false, true, true, Some(dec!(3000.50)), "instrument missing from cache")]
#[case::missing_account(true, false, true, Some(dec!(3000.50)), "account missing from cache")]
#[case::disabled_generation(true, true, false, Some(dec!(3000.50)), "generate_missing_orders is disabled")]
#[case::missing_price(true, true, true, None, "missing avg_px_open for position recovery")]
#[tokio::test]
async fn test_mass_status_reports_unresolved_position_prerequisite(
    #[case] has_instrument: bool,
    #[case] has_account: bool,
    #[case] generate_missing_orders: bool,
    #[case] avg_px: Option<Decimal>,
    #[case] reason: &str,
    #[values(None, Some(PositionId::from("P-UNRECOVERED")))] position_id: Option<PositionId>,
) {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        generate_missing_orders,
        ..Default::default()
    });

    let instrument_id = test_instrument_id();
    let account_id = test_account_id();

    if !has_account {
        ctx.cache.borrow_mut().reset();
    }

    if has_instrument {
        ctx.add_instrument(test_instrument());
    }

    let report = PositionStatusReport::new(
        account_id,
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        position_id,
        avg_px,
    );

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        account_id,
        test_venue(),
        UnixNanos::default(),
        None,
    );
    mass_status.add_position_reports(vec![report]);
    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(
        result.unresolved_positions,
        vec![format!(
            "account={account_id}, instrument={instrument_id}, venue_position_id={position_id:?}, venue_quantity=5.0: {reason}"
        )],
    );
    assert_eq!(
        ctx.cache
            .borrow()
            .positions_open_count(None, None, None, None, None),
        0
    );
}

#[rstest]
#[case::netting(None)]
#[case::hedging(Some(PositionId::from("P-RECOVERED")))]
#[tokio::test]
async fn test_mass_status_synchronized_position_does_not_require_entry_price(
    #[case] position_id: Option<PositionId>,
) {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let mut report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        position_id,
        Some(dec!(3000.50)),
    );

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        None,
    );
    mass_status.add_position_reports(vec![report.clone()]);
    let recovered = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);
    assert!(recovered.unresolved_positions.is_empty());

    report.avg_px_open = None;

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        None,
    );
    mass_status.add_position_reports(vec![report]);
    let synchronized = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert!(synchronized.events.is_empty());
    assert!(synchronized.unresolved_positions.is_empty());
    let cache = ctx.cache.borrow();
    let positions = cache.positions_open(
        None,
        Some(&instrument_id),
        None,
        Some(&test_account_id()),
        None,
    );
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].signed_decimal_qty(), dec!(5));
}

#[tokio::test]
async fn test_reconcile_mass_status_skips_flat_position_report() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add a flat position report
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Flat,
        Quantity::from("0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        None,
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // No events should be generated for flat position
    assert!(result.events.is_empty());
}

#[tokio::test]
async fn test_reconcile_mass_status_skips_position_report_when_filtered() {
    let config = ExecutionManagerConfig {
        filter_position_reports: true,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3000.50)),
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Position reports should be filtered
    assert!(result.events.is_empty());
}

#[tokio::test]
async fn test_reconcile_mass_status_creates_short_position_from_report() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add a short position report
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Short,
        Quantity::from("3.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(2950.25)),
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 2);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));
    assert!(matches!(result.events[1], OrderEventAny::Filled(_)));

    if let OrderEventAny::Filled(filled) = &result.events[1] {
        assert_eq!(filled.last_qty, Quantity::from("3.0"));
        assert_eq!(filled.order_side, OrderSide::Sell);
    }
}

#[tokio::test]
async fn test_reconcile_mass_status_skips_position_report_when_fills_exist() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");

    ctx.add_instrument(test_instrument());
    let mut order = create_limit_order("O-001", instrument_id, OrderSide::Buy, "5.0", "3000.00");
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order);
    ctx.cache
        .borrow_mut()
        .add_venue_order_id(&client_order_id, &venue_order_id, false)
        .unwrap();

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add a fill report for 5.0 qty
    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-001"),
        OrderSide::Buy,
        Quantity::from("5.0"),
        Price::from("3000.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        Some(client_order_id),
        None,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );
    mass_status.add_fill_reports(vec![fill]);

    // Add a position report for the same instrument (would duplicate if not skipped)
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3000.00)),
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should only have 1 fill event from the fill report, not additional events
    // from the position report (which would double-count)
    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Filled(_)));
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

// A Polymarket-style outcome token with six-decimal share quantities
fn coarse_report_instrument() -> InstrumentAny {
    let mut instrument = binary_option();
    instrument.size_precision = 6;
    instrument.size_increment = Quantity::from("0.000001");
    InstrumentAny::BinaryOption(instrument)
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

fn close_test_long_position(
    mut position: Position,
    instrument: &InstrumentAny,
    qty: &str,
    trade_id: TradeId,
) -> Position {
    let close_order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from(qty))
        .build();
    let close_fill = TestOrderEventStubs::filled(
        &close_order,
        instrument,
        Some(trade_id),
        Some(position.id),
        Some(Price::from("3000.00")),
        Some(Quantity::from(qty)),
        None,
        None,
        None,
        Some(position.account_id),
    );
    let close_filled: OrderFilled = close_fill.into();
    position.apply(&close_filled);
    position
}

#[tokio::test]
async fn test_reconcile_mass_status_iterates_all_position_reports() {
    // Tests that we iterate ALL position reports, not just the first one
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add two position reports for the same instrument (hedge mode scenario)
    let position_report_long = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(PositionId::from("P-LONG-001")),
        Some(dec!(3000.50)),
    );

    let position_report_short = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Short,
        Quantity::from("3.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(PositionId::from("P-SHORT-001")),
        Some(dec!(3100.00)),
    );

    mass_status.add_position_reports(vec![position_report_long, position_report_short]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Both position reports should be processed, not just the first
    let fill_events: Vec<_> = result
        .events
        .iter()
        .filter_map(|e| {
            if let OrderEventAny::Filled(f) = e {
                Some(f)
            } else {
                None
            }
        })
        .collect();

    // Should have fills for both long and short positions
    let has_buy_fill = fill_events.iter().any(|f| f.order_side == OrderSide::Buy);
    let has_sell_fill = fill_events.iter().any(|f| f.order_side == OrderSide::Sell);
    assert!(has_buy_fill, "Should have BUY fill for long position");
    assert!(has_sell_fill, "Should have SELL fill for short position");

    // Verify both positions exist in cache
    let cache = ctx.cache.borrow();
    let positions = cache.positions(None, None, None, None, None);
    assert_eq!(positions.len(), 2, "Should have 2 positions in cache");
}

#[tokio::test]
async fn test_reconcile_mass_status_routes_to_hedging_with_venue_position_id() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Position report WITH venue_position_id = hedge mode
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(PositionId::from("P-HEDGE-001")),
        Some(dec!(3000.50)),
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should create position since position doesn't exist in cache
    assert!(!result.events.is_empty());
}

#[tokio::test]
async fn test_reconcile_mass_status_routes_to_netting_without_venue_position_id() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Position report WITHOUT venue_position_id = netting mode
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None, // No venue_position_id
        Some(dec!(3000.50)),
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should create position since no position exists for instrument
    assert!(!result.events.is_empty());
}

#[tokio::test]
async fn test_reconcile_mass_status_reconciles_partial_hedge_fill_to_position_report() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let venue_position_id = PositionId::from("P-HEDGE-001");

    ctx.add_instrument(test_instrument());
    let order = create_submitted_order("O-001", instrument_id, OrderSide::Buy, "5.0", "3000.00");
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-001"),
        OrderSide::Buy,
        Quantity::from("2.0"),
        Price::from("3000.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        Some(client_order_id),
        Some(venue_position_id),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );

    mass_status.add_fill_reports(vec![fill]);
    mass_status.set_report_window(Some(UnixNanos::from(1)), false);

    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(venue_position_id),
        Some(dec!(3000.00)),
    );

    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 3);

    assert_eq!(
        result
            .events
            .iter()
            .filter(|event| matches!(event, OrderEventAny::Accepted(_)))
            .count(),
        1,
    );

    assert_eq!(
        result
            .events
            .iter()
            .filter(|event| matches!(event, OrderEventAny::Filled(_)))
            .count(),
        2,
    );

    let position = ctx
        .cache
        .borrow()
        .position_owned(&venue_position_id)
        .unwrap();
    assert_eq!(position.quantity, Quantity::from("5.0"));
    assert_eq!(position.avg_px_open, 3000.0);
}

#[tokio::test]
async fn test_reconcile_mass_status_leaves_hedge_reversal_identity_unresolved() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let venue_position_id = PositionId::from("P-HEDGE-001");

    ctx.add_instrument(test_instrument());
    let order = create_submitted_order("O-001", instrument_id, OrderSide::Sell, "5.0", "3000.00");
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );
    mass_status.set_report_window(Some(UnixNanos::from(1)), false);

    mass_status.add_fill_reports(vec![FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-001"),
        OrderSide::Sell,
        Quantity::from("5.0"),
        Price::from("3100.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        Some(client_order_id),
        Some(venue_position_id),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    )]);

    mass_status.add_position_reports(vec![PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(venue_position_id),
        Some(dec!(3000.00)),
    )]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 3);

    let cache = ctx.cache.borrow();
    assert_eq!(cache.positions(None, None, None, None, None).len(), 2);
    let reported_position = cache.position(&venue_position_id).unwrap();
    assert!(reported_position.is_closed());
    assert_eq!(reported_position.quantity, Quantity::zero(3));
    let positions = cache.positions_open(
        None,
        Some(&instrument_id),
        None,
        Some(&test_account_id()),
        None,
    );
    assert_eq!(positions.len(), 1);
    assert_ne!(positions[0].id, venue_position_id);
    assert_eq!(
        result.unresolved_positions,
        vec![format!(
            "account={}, instrument={instrument_id}, venue_position_id=Some({venue_position_id:?}), venue_quantity=5.0: position recovery did not restore the reported quantity",
            test_account_id(),
        )]
    );
    assert_eq!(positions[0].side, PositionSide::Long);
    assert_eq!(positions[0].quantity, Quantity::from("5.0"));
    assert_eq!(positions[0].avg_px_open, 3000.0);
    assert_eq!(positions[0].realized_pnl, Some(Money::from("0 USDT")));
}

#[tokio::test]
async fn test_reconcile_mass_status_skips_only_position_with_fill_position_id_conflict() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let legacy_position_id = PositionId::from("P-LEGACY-LONG");
    let venue_position_id = PositionId::from("P-HEDGE-LONG");
    let other_position_id = PositionId::from("P-HEDGE-SHORT");

    ctx.add_instrument(test_instrument());
    let order = create_submitted_order("O-001", instrument_id, OrderSide::Buy, "5.0", "3000.00");
    ctx.cache
        .borrow_mut()
        .add_order(
            order,
            Some(legacy_position_id),
            Some(test_client_id()),
            false,
        )
        .unwrap();

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    mass_status.add_fill_reports(vec![FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-001"),
        OrderSide::Buy,
        Quantity::from("5.0"),
        Price::from("3000.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        Some(client_order_id),
        Some(venue_position_id),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    )]);

    mass_status.add_position_reports(vec![PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(venue_position_id),
        Some(dec!(3000.00)),
    )]);

    mass_status.add_position_reports(vec![PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Short,
        Quantity::from("3.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(other_position_id),
        Some(dec!(3100.00)),
    )]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let [
        OrderEventAny::Filled(conflicting_fill),
        OrderEventAny::Accepted(_),
        OrderEventAny::Filled(other_fill),
    ] = result.events.as_slice()
    else {
        panic!("expected the conflicting fill followed by the synthetic position events");
    };

    assert_eq!(conflicting_fill.position_id, Some(venue_position_id));
    assert_eq!(other_fill.position_id, Some(other_position_id));

    let order = ctx.get_order(&client_order_id).unwrap();
    assert_eq!(order.filled_qty(), Quantity::zero(3));
    assert!(ctx.cache.borrow().position(&legacy_position_id).is_none());
    assert!(ctx.cache.borrow().position(&venue_position_id).is_none());

    let other_position = ctx
        .cache
        .borrow()
        .position_owned(&other_position_id)
        .unwrap();
    assert_eq!(other_position.id, other_position_id);
    assert_eq!(other_position.instrument_id, instrument_id);
    assert_eq!(other_position.side, PositionSide::Short);
    assert_eq!(other_position.quantity, Quantity::from("3.0"));
    assert_eq!(other_position.avg_px_open, 3100.0);
}

#[tokio::test]
async fn test_reconcile_mass_status_does_not_duplicate_matching_hedge_filled_order() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let venue_order_id = VenueOrderId::from("V-001");
    let venue_position_id = PositionId::from("P-HEDGE-001");

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let order_report = OrderStatusReport::new(
        test_account_id(),
        instrument_id,
        None,
        venue_order_id,
        OrderSide::Buy.into(),
        OrderType::Market,
        TimeInForce::Gtc,
        OrderStatus::Filled,
        Quantity::from("5.0"),
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    )
    .with_avg_px(dec!(3000.0))
    .with_venue_position_id(venue_position_id);

    mass_status.add_order_reports(vec![order_report]);

    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(venue_position_id),
        Some(dec!(3000.00)),
    );

    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let filled_count = result
        .events
        .iter()
        .filter(|e| matches!(e, OrderEventAny::Filled(_)))
        .count();

    assert_eq!(filled_count, 1, "Expected exactly 1 fill event");
}

#[tokio::test]
async fn test_reconcile_mass_status_skips_hedge_position_when_fills_lack_position_id() {
    // Tests that hedge position reconciliation is skipped when fills exist for the
    // instrument but lack venue_position_id (common when venues only include IDs on
    // position reports)
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");

    ctx.add_instrument(test_instrument());
    let order = create_limit_order("O-001", instrument_id, OrderSide::Buy, "5.0", "3000.00");
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add a fill report WITHOUT venue_position_id
    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-001"),
        OrderSide::Buy,
        Quantity::from("5.0"),
        Price::from("3000.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        Some(client_order_id),
        None, // No venue_position_id on fill
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );
    mass_status.add_fill_reports(vec![fill]);

    // Add a hedge position report WITH venue_position_id
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(PositionId::from("P-HEDGE-001")), // Has venue_position_id
        Some(dec!(3000.00)),
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should only have fill event, position report skipped due to instrument-level fill
    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Filled(_)));
}

#[tokio::test]
async fn test_reconcile_hedge_does_not_skip_unrelated_positions() {
    // Tests that when fills have venue_position_id, only that specific position is skipped,
    // not other hedge positions on the same instrument
    let config = ExecutionManagerConfig {
        generate_missing_orders: true,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let position_id_1 = PositionId::from("P-HEDGE-001");
    let position_id_2 = PositionId::from("P-HEDGE-002");

    ctx.add_instrument(test_instrument());
    let order = create_limit_order("O-001", instrument_id, OrderSide::Buy, "5.0", "3000.00");
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add a fill report WITH venue_position_id for P-HEDGE-001
    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-001"),
        OrderSide::Buy,
        Quantity::from("5.0"),
        Price::from("3000.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        Some(client_order_id),
        Some(position_id_1), // Fill attributed to P-HEDGE-001
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );
    mass_status.add_fill_reports(vec![fill]);

    // Add position reports for BOTH positions
    let position_report_1 = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(position_id_1),
        Some(dec!(3000.00)),
    );

    let position_report_2 = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Short,
        Quantity::from("3.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(position_id_2), // Different position
        Some(dec!(3100.00)),
    );
    mass_status.add_position_reports(vec![position_report_1, position_report_2]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should have:
    // - 1 fill event for P-HEDGE-001 (from fill report)
    // - Events for P-HEDGE-002 (from position report, should NOT be skipped)
    let filled_count = result
        .events
        .iter()
        .filter(|e| matches!(e, OrderEventAny::Filled(_)))
        .count();

    // At least 2 fills: one from fill report, one from position report for P-HEDGE-002
    assert!(
        filled_count >= 2,
        "Expected at least 2 fill events, was {filled_count}"
    );
}

#[tokio::test]
async fn test_reconcile_hedge_position_matching_quantities() {
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = test_instrument_id();
    let position_id = PositionId::from("P-HEDGE-001");

    ctx.add_instrument(instrument.clone());

    // Add existing position to cache with 5.0 qty
    let position = create_test_position(&instrument, position_id, OrderSide::Buy, "5.0", "3000.00");
    ctx.add_position(&position);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Position report matches cached position exactly
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(position_id),
        Some(dec!(3000.00)),
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // No events needed since positions match
    assert!(
        result.events.is_empty(),
        "Expected no events when positions match, was {}",
        result.events.len()
    );
}

#[tokio::test]
async fn test_reconcile_hedge_position_discrepancy_generates_order() {
    let config = ExecutionManagerConfig {
        generate_missing_orders: true,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = test_instrument_id();
    let position_id = PositionId::from("P-HEDGE-001");

    ctx.add_instrument(instrument.clone());

    // Add existing position to cache with 5.0 qty
    let position = create_test_position(&instrument, position_id, OrderSide::Buy, "5.0", "3000.00");
    ctx.add_position(&position);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Position report shows 8.0 qty (discrepancy of 3.0)
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("8.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(position_id),
        Some(dec!(3000.00)),
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should generate reconciliation order to fix the discrepancy
    assert!(
        !result.events.is_empty(),
        "Expected events for position discrepancy reconciliation"
    );
}

#[tokio::test]
async fn test_reconcile_missing_hedge_position_generates_order() {
    let config = ExecutionManagerConfig {
        generate_missing_orders: true,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Position report for position that doesn't exist in cache
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(PositionId::from("P-MISSING-001")),
        Some(dec!(3000.50)),
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should generate order to create the missing position
    assert!(
        !result.events.is_empty(),
        "Expected events for missing position creation"
    );
}

#[tokio::test]
async fn test_reconcile_hedge_position_discrepancy_disabled() {
    let config = ExecutionManagerConfig {
        generate_missing_orders: false, // Disabled
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = test_instrument_id();
    let position_id = PositionId::from("P-HEDGE-001");

    ctx.add_instrument(instrument.clone());

    // Add existing position with different qty than venue reports
    let position = create_test_position(&instrument, position_id, OrderSide::Buy, "5.0", "3000.00");
    ctx.add_position(&position);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Position report shows 8.0 qty (discrepancy)
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("8.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(position_id),
        Some(dec!(3000.00)),
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // No events since generate_missing_orders is disabled
    assert!(
        result.events.is_empty(),
        "Expected no events when generate_missing_orders is disabled"
    );
}

#[tokio::test]
async fn test_reconcile_hedge_position_both_flat() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Position report with zero quantity (flat)
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Flat,
        Quantity::from("0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(PositionId::from("P-FLAT-001")),
        None,
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // No events needed - position doesn't exist in cache and report is flat
    assert!(result.events.is_empty());
}

#[tokio::test]
async fn test_reconcile_hedge_short_position() {
    let config = ExecutionManagerConfig {
        generate_missing_orders: true,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Short position report
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Short,
        Quantity::from("3.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(PositionId::from("P-SHORT-001")),
        Some(dec!(3100.00)),
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should create short position
    assert!(!result.events.is_empty());

    // Verify one of the events is a sell order (for short position)
    let has_sell = result.events.iter().any(|e| {
        if let OrderEventAny::Filled(filled) = e {
            filled.order_side == OrderSide::Sell
        } else {
            false
        }
    });

    assert!(has_sell, "Expected a sell order for short position");
}

#[tokio::test]
async fn test_reconcile_mass_status_deduplicates_netting_reports_same_instrument() {
    // Tests that multiple netting reports (no venue_position_id) for the same instrument
    // only create one position, not duplicates
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add two netting reports for the same instrument (both without venue_position_id)
    let position_report_1 = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None, // No venue_position_id = netting mode
        Some(dec!(3000.50)),
    );

    let position_report_2 = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_001),
        UnixNanos::from(1_000_001),
        None,
        None, // No venue_position_id = netting mode
        Some(dec!(3000.50)),
    );

    mass_status.add_position_reports(vec![position_report_1, position_report_2]);

    let _result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Deduplication: only ONE position should be created from duplicate netting reports
    let cache = ctx.cache.borrow();
    let positions = cache.positions(None, None, None, None, None);
    assert_eq!(
        positions.len(),
        1,
        "Should have exactly 1 position (duplicates skipped), was {}",
        positions.len()
    );
    assert_eq!(
        positions[0].quantity,
        Quantity::from("5.0"),
        "Position should have qty 5.0"
    );
}

#[tokio::test]
async fn test_reconcile_mass_status_deduplicates_hedge_reports_same_position_id() {
    // Tests that multiple hedge reports for the same venue_position_id only create
    // one position, not duplicates
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let venue_position_id = PositionId::from("P-HEDGE-001");
    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add two hedge reports for the same venue_position_id (duplicate snapshots)
    let position_report_1 = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(venue_position_id),
        Some(dec!(3000.50)),
    );

    let position_report_2 = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_001),
        UnixNanos::from(1_000_001),
        None,
        Some(venue_position_id), // Same venue_position_id
        Some(dec!(3000.50)),
    );

    mass_status.add_position_reports(vec![position_report_1, position_report_2]);

    let _result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Deduplication: only ONE position should be created from duplicate hedge reports
    let cache = ctx.cache.borrow();
    let positions = cache.positions(None, None, None, None, None);
    assert_eq!(
        positions.len(),
        1,
        "Should have exactly 1 position (duplicates skipped), was {}",
        positions.len()
    );
    assert_eq!(
        positions[0].id, venue_position_id,
        "Position should have correct ID"
    );
    assert_eq!(
        positions[0].quantity,
        Quantity::from("5.0"),
        "Position should have qty 5.0"
    );
}

#[tokio::test]
async fn test_adjust_fills_creates_synthetic_for_partial_window() {
    // Test that adjust_fills_for_partial_window creates synthetic fills when
    // historical fills don't fully explain the current position (partial window scenario).
    // This happens when lookback window started mid-position.
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Position report shows LONG 5.0 with avg_px 3000.00
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.000"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None, // Netting mode
        Some(dec!(3000.00)),
    );
    mass_status.add_position_reports(vec![position_report]);

    // Only have fills for 2.0 (partial window - missing opening fills)
    let venue_order_id = VenueOrderId::from("V-PARTIAL-001");
    let order_report = create_order_report(
        Some(ClientOrderId::from("O-PARTIAL-001")),
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("2.000"),
        Quantity::from("2.000"),
    )
    .with_avg_px(dec!(3100.00));
    mass_status.add_order_reports(vec![order_report]);

    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-PARTIAL-001"),
        OrderSide::Buy,
        Quantity::from("2.000"),
        Price::from("3100.00"),
        Money::from("1.00 USDT"),
        LiquiditySide::Taker,
        None,
        None,
        UnixNanos::from(1_000_001),
        UnixNanos::from(1_000_001),
        None,
    );
    mass_status.add_fill_reports(vec![fill]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // The adjustment should create a synthetic fill for the missing 3.0
    // (position=5.0, fills=2.0, so synthetic opening of 3.0 is needed)
    // Events: Synthetic order (Accepted + Filled) + Original order (Accepted + Filled) + Position events
    // The exact number depends on implementation but we should have more than just the original fill

    // Verify we have fills that sum to match the position
    let fill_events: Vec<_> = result
        .events
        .iter()
        .filter_map(|e| {
            if let OrderEventAny::Filled(f) = e {
                Some(f)
            } else {
                None
            }
        })
        .collect();

    // Should have at least 2 fills (synthetic opening + original)
    assert!(
        fill_events.len() >= 2,
        "Expected at least 2 fills (synthetic + original), was {}",
        fill_events.len()
    );

    // Total filled quantity should match position quantity (5.0)
    let total_qty: f64 = fill_events.iter().map(|f| f.last_qty.as_f64()).sum();
    assert!(
        (total_qty - 5.0).abs() < 0.001,
        "Total filled qty should be ~5.0 to match position, was {total_qty}"
    );
}

#[rstest]
#[case::netting(false)]
#[case::hedging(true)]
#[tokio::test]
async fn test_missing_orders_disabled_skips_synthetic_fill_recovery(#[case] hedging: bool) {
    let config = ExecutionManagerConfig {
        generate_missing_orders: false,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );
    let venue_position_id = hedging.then(|| PositionId::from("ETHUSDT-PERP.BINANCE-LONG"));
    mass_status.add_fill_reports(vec![FillReport::new(
        test_account_id(),
        instrument_id,
        VenueOrderId::from("V-PARTIAL-DISABLED"),
        TradeId::from("T-PARTIAL-DISABLED"),
        OrderSide::Buy,
        Quantity::from("2.000"),
        Price::from("3100.00"),
        Money::from("1.00 USDT"),
        LiquiditySide::Taker,
        None,
        venue_position_id,
        UnixNanos::from(1_000_001),
        UnixNanos::from(1_000_001),
        None,
    )]);
    mass_status.add_position_reports(vec![PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.000"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        venue_position_id,
        Some(dec!(3000.00)),
    )]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let cache = ctx.cache.borrow();

    assert!(result.events.is_empty());
    assert!(result.external_orders.is_empty());
    assert_eq!(cache.orders_total_count(None, None, None, None, None), 0);
    assert!(cache.positions(None, None, None, None, None).is_empty());
}

#[tokio::test]
async fn test_external_order_has_venue_tag() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let venue_order_id = VenueOrderId::from("V-EXT-001");

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // External order with no client_order_id
    let report = create_order_report(
        None,
        venue_order_id,
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0.0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert!(!result.events.is_empty());

    // Get the order created and verify it has the VENUE tag
    let client_order_id = ClientOrderId::from("V-EXT-001");
    let order = ctx.get_order(&client_order_id).expect("Order should exist");
    let tags = order.tags().expect("Order should have tags");
    assert!(
        tags.contains(&ustr::Ustr::from("VENUE")),
        "External order should have VENUE tag"
    );
}

#[rstest]
#[case::accepted_filled(
    OrderStatus::Accepted,
    OrderType::Market,
    "0.0",
    "10.0",
    OrderStatus::Filled
)]
#[case::accepted_partial(
    OrderStatus::Accepted,
    OrderType::Market,
    "0.0",
    "6.0",
    OrderStatus::PartiallyFilled
)]
#[case::accepted_priced_residual(
    OrderStatus::Accepted,
    OrderType::Limit,
    "6.0",
    "4.0",
    OrderStatus::PartiallyFilled
)]
#[case::triggered_filled(
    OrderStatus::Triggered,
    OrderType::StopMarket,
    "0.0",
    "10.0",
    OrderStatus::Filled
)]
#[case::triggered_partial(
    OrderStatus::Triggered,
    OrderType::StopMarket,
    "0.0",
    "6.0",
    OrderStatus::PartiallyFilled
)]
#[case::triggered_priced_residual(
    OrderStatus::Triggered,
    OrderType::StopLimit,
    "6.0",
    "4.0",
    OrderStatus::PartiallyFilled
)]
#[case::filled(
    OrderStatus::Filled,
    OrderType::Market,
    "10.0",
    "10.0",
    OrderStatus::Filled
)]
#[tokio::test]
async fn test_external_order_with_fills_but_no_avg_px_applies_real_fills_only(
    #[case] order_status: OrderStatus,
    #[case] order_type: OrderType,
    #[case] reported_filled_qty: &str,
    #[case] fill_qty: &str,
    #[case] expected_status: OrderStatus,
) {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let venue_order_id = VenueOrderId::from("V-FILLS-001");

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let mut report = OrderStatusReport::new(
        test_account_id(),
        instrument_id,
        None, // External order
        venue_order_id,
        OrderSide::Buy.into(),
        order_type,
        TimeInForce::Gtc,
        order_status,
        Quantity::from("10.0"),
        Quantity::from(reported_filled_qty),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        None,
    );

    if matches!(order_type, OrderType::Limit | OrderType::StopLimit) {
        report = report.with_price(Price::from("3000.00"));
    }

    if matches!(order_type, OrderType::StopMarket | OrderType::StopLimit) {
        report = report
            .with_trigger_price(Price::from("2900.00"))
            .with_trigger_type(TriggerType::MarkPrice);
    }

    // Real fill report with actual price
    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-REAL-001"),
        OrderSide::Buy,
        Quantity::from(fill_qty),
        Price::from("3000.00"),
        Money::from("1.00 USDT"),
        LiquiditySide::Taker,
        None,
        None,
        UnixNanos::from(1_000_001),
        UnixNanos::from(1_000_001),
        None,
    );

    mass_status.add_order_reports(vec![report]);
    mass_status.add_fill_reports(vec![fill]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should have events including Accepted and the real fill
    let accepted_count = result
        .events
        .iter()
        .filter(|e| matches!(e, OrderEventAny::Accepted(_)))
        .count();
    assert_eq!(accepted_count, 1, "Should have exactly 1 Accepted event");

    let fill_events: Vec<_> = result
        .events
        .iter()
        .filter_map(|e| {
            if let OrderEventAny::Filled(f) = e {
                Some(f)
            } else {
                None
            }
        })
        .collect();

    assert_eq!(
        fill_events.len(),
        1,
        "Should have exactly 1 fill (the real one)"
    );
    assert_eq!(
        fill_events[0].trade_id,
        TradeId::from("T-REAL-001"),
        "Fill should be the real fill, not an inferred one"
    );
    assert_eq!(result.events.len(), 2);
    assert_eq!(fill_events[0].last_qty, Quantity::from(fill_qty));
    assert_eq!(fill_events[0].last_px, Price::from("3000.00"));
    assert_eq!(fill_events[0].commission, Some(Money::from("1.00 USDT")));
    assert_eq!(fill_events[0].liquidity_side, LiquiditySide::Taker);
    assert_eq!(fill_events[0].ts_event, UnixNanos::from(1_000_001));
    assert!(fill_events[0].reconciliation);

    let client_order_id = ClientOrderId::from("V-FILLS-001");
    let order = ctx.get_order(&client_order_id).expect("Order should exist");
    assert_eq!(order.status(), expected_status);
    assert_eq!(order.quantity(), Quantity::from("10.0"));
    assert_eq!(order.filled_qty(), Quantity::from(fill_qty));
}

#[tokio::test]
async fn test_position_reconciliation_order_has_reconciliation_tag() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Position report with no corresponding orders - this triggers position reconciliation
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3000.50)),
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert!(!result.events.is_empty());

    // Get the synthetic order created and verify it has the RECONCILIATION tag
    if let OrderEventAny::Accepted(accepted) = &result.events[0] {
        let order = ctx
            .get_order(&accepted.client_order_id)
            .expect("Order should exist");
        let tags = order.tags().expect("Order should have tags");
        assert!(
            tags.contains(&ustr::Ustr::from("RECONCILIATION")),
            "Position reconciliation order should have RECONCILIATION tag, was {tags:?}",
        );
    } else {
        panic!("Expected Accepted event, was {:?}", result.events[0]);
    }
}

#[rstest]
#[case::unclaimed(None)]
#[case::claimed(Some(StrategyId::from("CLAIMER-001")))]
#[tokio::test]
async fn test_replayed_fill_does_not_reopen_reconciled_position(
    #[case] claimed_strategy: Option<StrategyId>,
) {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let strategy_id = claimed_strategy.unwrap_or_else(StrategyId::external);
    let replay_venue_order_id = VenueOrderId::from("V-REPLAY-001");
    let replay_trade_id = TradeId::from("T-REPLAY-001");
    ctx.add_instrument(test_instrument());
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(strategy_id, OmsType::Netting);

    if claimed_strategy.is_some() {
        ctx.manager
            .claim_external_orders(instrument_id, strategy_id)
            .unwrap();
    }

    ctx.advance_time(10_000_000);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );
    mass_status.add_position_reports(vec![PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3000.50)),
    )]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);
    let synthetic_client_order_id = result.events[0].client_order_id();

    // The venue resends the execution the position report already covers
    let replayed = FillReport::new(
        test_account_id(),
        instrument_id,
        replay_venue_order_id,
        replay_trade_id,
        OrderSide::Buy,
        Quantity::from("5.0"),
        Price::from("3000.50"),
        Money::from("0.50 USDT"),
        LiquiditySide::Taker,
        None,
        None,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );
    ctx.exec_engine
        .borrow_mut()
        .reconcile_fill_report(&replayed);

    let synthetic_order = ctx.get_order(&synthetic_client_order_id).unwrap();
    let cache = ctx.cache.borrow();
    let replay_client_order_id = cache
        .client_order_id(&replay_venue_order_id)
        .copied()
        .unwrap();
    let replay_order = cache.order(&replay_client_order_id).unwrap();
    let positions = cache.positions_open(
        None,
        Some(&instrument_id),
        None,
        Some(&test_account_id()),
        None,
    );

    assert_eq!(synthetic_order.strategy_id(), strategy_id);
    assert_eq!(
        synthetic_order.tags(),
        Some(&[ustr::Ustr::from("RECONCILIATION")][..])
    );
    assert_eq!(replay_order.status(), OrderStatus::Filled);
    assert_eq!(replay_order.filled_qty(), Quantity::from("5.0"));
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].opening_order_id, synthetic_client_order_id);
    assert_eq!(positions[0].signed_decimal_qty(), dec!(5.0));
    assert!(!positions[0].trade_ids.contains(&replay_trade_id));
}

#[tokio::test]
async fn test_closed_reconciliation_orders_skipped_on_restart() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-RECON-001");
    let venue_order_id = VenueOrderId::from("V-RECON-001");

    // Create a closed reconciliation order from a previous session
    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("5.0"))
        .client_order_id(client_order_id)
        .tags(vec![ustr::Ustr::from("RECONCILIATION")])
        .build();

    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    let filled = TestOrderEventStubs::filled(
        &order,
        &test_instrument(),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    );
    order.apply(filled).unwrap();

    assert!(order.is_closed());
    ctx.add_order(order);

    // Simulate restart with a mass status that contains this order
    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("5.0"),
        Quantity::from("5.0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should skip the closed reconciliation order - no new events generated
    assert!(
        result.events.is_empty(),
        "Should skip closed RECONCILIATION order, but got {} events",
        result.events.len()
    );
}

#[rstest]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_cross_zero_unbuildable_open_leg_has_no_side_effects() {
    let config = ExecutionManagerConfig {
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let position_id = PositionId::from("P-CROSS-ZERO-ATOMIC");
    let position = create_test_position(&instrument, position_id, OrderSide::Buy, "5.0", "3000.00");
    let strategy_id = StrategyId::from("CROSS-ZERO-ATOMIC");
    let venue_ts_last = UnixNanos::from(1_000_000);
    ctx.add_instrument(instrument.clone());
    ctx.add_position(&position);
    ctx.manager
        .claim_external_orders(instrument_id, strategy_id)
        .unwrap();

    let topic = switchboard::get_event_order_topic(strategy_id);
    let (handler, event_messages): (_, TypedMessageSavingHandler<OrderEventAny>) =
        get_typed_message_saving_handler(None);
    msgbus::subscribe_order_events(topic.into(), handler.clone(), None);

    let mut report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Short,
        Quantity::from("3.0"),
        venue_ts_last,
        venue_ts_last,
        None,
        None,
        Some(dec!(3100.00)),
    );
    report.signed_decimal_qty = -dec!(10000000000000000000);
    let client = MockPositionExecutionClient::new(vec![], vec![report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&client];

    let order_count_before = ctx
        .cache
        .borrow()
        .orders_total_count(None, None, None, None, None);
    let events = ctx.manager.check_positions_consistency(&clients).await;

    msgbus::unsubscribe_order_events(topic.into(), &handler);

    let close_venue_order_id = create_position_reconciliation_venue_order_id(
        test_account_id(),
        instrument_id,
        OrderSide::Sell,
        OrderType::Market,
        Quantity::from("5.0"),
        Price::from_decimal_dp(dec!(3000.00), instrument.price_precision()).ok(),
        None,
        Some("CLOSE"),
        venue_ts_last,
    );
    let close_client_order_id = ClientOrderId::from(close_venue_order_id.as_str());
    let cache = ctx.cache.borrow();
    let cached_qty = cache
        .position(&position_id)
        .expect("cached position should remain present")
        .signed_decimal_qty();

    assert!(events.is_empty());
    assert_eq!(
        cache.orders_total_count(None, None, None, None, None),
        order_count_before,
        "an unbuildable open leg must not cache the close order",
    );
    assert!(
        cache.client_order_id(&close_venue_order_id).is_none(),
        "an unbuildable open leg must not add the venue-to-client order index",
    );
    assert!(
        cache.venue_order_id(&close_client_order_id).is_none(),
        "an unbuildable open leg must not add the client-to-venue order index",
    );
    assert_eq!(cached_qty, dec!(5.0));
    assert!(
        event_messages.get_messages().is_empty(),
        "an unbuildable open leg must not publish any order event",
    );
}

#[rstest]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_cross_zero_unbuildable_open_leg_retries_without_poisoning_order_id() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let key = (instrument_id, test_account_id());
    let position = create_test_position(
        &instrument,
        PositionId::from("P-CROSS-ZERO-RETRY"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
    );
    let venue_ts_last = UnixNanos::from(1_000_000);
    ctx.add_instrument(instrument);
    ctx.add_position(&position);

    let mut unbuildable_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Short,
        Quantity::from("3.0"),
        venue_ts_last,
        venue_ts_last,
        None,
        None,
        Some(dec!(3100.00)),
    );
    unbuildable_report.signed_decimal_qty = -dec!(10000000000000000000);
    let unbuildable_client = MockPositionExecutionClient::new(vec![], vec![unbuildable_report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&unbuildable_client];

    let first_events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(first_events.is_empty());
    assert_eq!(ctx.manager.position_recon_retry_count(&key), 1);

    let valid_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Short,
        Quantity::from("3.0"),
        venue_ts_last,
        venue_ts_last,
        None,
        None,
        Some(dec!(3100.00)),
    );
    let valid_client = MockPositionExecutionClient::new(vec![], vec![valid_report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&valid_client];

    let second_events = ctx.manager.check_positions_consistency(&clients).await;

    let fills: Vec<_> = second_events
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => Some(fill),
            _ => None,
        })
        .collect();

    assert_eq!(ctx.manager.position_recon_retry_count(&key), 0);
    assert_eq!(
        fills.len(),
        2,
        "the retry must rebuild both cross-zero legs"
    );
    assert_eq!(fills[0].order_side, OrderSide::Sell);
    assert_eq!(fills[0].last_qty, Quantity::from("5.0"));
    assert_eq!(fills[1].order_side, OrderSide::Sell);
    assert_eq!(fills[1].last_qty, Quantity::from("3.0"));

    for event in &second_events {
        ctx.exec_engine.borrow_mut().process(event);
    }

    let cached_signed_qty = ctx
        .cache
        .borrow()
        .positions_open(
            None,
            Some(&instrument_id),
            None,
            Some(&test_account_id()),
            None,
        )
        .iter()
        .map(|position| position.signed_decimal_qty())
        .sum::<Decimal>();
    assert_eq!(cached_signed_qty, dec!(-3.0));
}

#[tokio::test]
async fn test_netting_position_cross_zero_long_to_short() {
    // Test: Cached position is long +5.0, venue reports short -3.0
    // Should generate: close fill (sell 5.0) + open fill (sell 3.0) = 2 fills
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(instrument.clone());

    // Add cached LONG position of 5.0
    let position = create_test_position(
        &instrument,
        PositionId::new("P-001"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
    );
    ctx.add_position(&position);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Venue reports SHORT position of -3.0
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Short,
        Quantity::from("3.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None, // Netting mode
        Some(dec!(3100.00)),
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should have 2 fills: close (sell 5.0) + open (sell 3.0)
    let fill_events: Vec<_> = result
        .events
        .iter()
        .filter_map(|e| {
            if let OrderEventAny::Filled(f) = e {
                Some(f)
            } else {
                None
            }
        })
        .collect();

    assert_eq!(
        fill_events.len(),
        2,
        "Cross-zero should generate 2 fills (close + open), was {}",
        fill_events.len()
    );

    // First fill should be SELL 5.0 (close long)
    assert_eq!(fill_events[0].order_side, OrderSide::Sell);
    assert_eq!(fill_events[0].last_qty, Quantity::from("5.0"));

    // Second fill should be SELL 3.0 (open short)
    assert_eq!(fill_events[1].order_side, OrderSide::Sell);
    assert_eq!(fill_events[1].last_qty, Quantity::from("3.0"));

    // Close and open legs must hash to distinct venue_order_ids so the engine
    // can tell them apart across reconciliation replays.
    assert_ne!(fill_events[0].venue_order_id, fill_events[1].venue_order_id);
}

#[tokio::test]
async fn test_netting_position_cross_zero_short_to_long() {
    // Test: Cached position is short -4.0, venue reports long +2.0
    // Should generate: close fill (buy 4.0) + open fill (buy 2.0) = 2 fills
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(instrument.clone());

    // Add cached SHORT position of 4.0
    let position = create_test_position(
        &instrument,
        PositionId::new("P-001"),
        OrderSide::Sell,
        "4.0",
        "3000.00",
    );
    ctx.add_position(&position);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Venue reports LONG position of +2.0
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("2.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None, // Netting mode
        Some(dec!(2900.00)),
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should have 2 fills: close (buy 4.0) + open (buy 2.0)
    let fill_events: Vec<_> = result
        .events
        .iter()
        .filter_map(|e| {
            if let OrderEventAny::Filled(f) = e {
                Some(f)
            } else {
                None
            }
        })
        .collect();

    assert_eq!(
        fill_events.len(),
        2,
        "Cross-zero should generate 2 fills (close + open), was {}",
        fill_events.len()
    );

    // First fill should be BUY 4.0 (close short)
    assert_eq!(fill_events[0].order_side, OrderSide::Buy);
    assert_eq!(fill_events[0].last_qty, Quantity::from("4.0"));

    // Second fill should be BUY 2.0 (open long)
    assert_eq!(fill_events[1].order_side, OrderSide::Buy);
    assert_eq!(fill_events[1].last_qty, Quantity::from("2.0"));

    // Close and open legs must hash to distinct venue_order_ids so the engine
    // can tell them apart across reconciliation replays.
    assert_ne!(fill_events[0].venue_order_id, fill_events[1].venue_order_id);
}

#[tokio::test]
async fn test_netting_position_flat_report_closes_cached_position() {
    // Test: Cached position is long +5.0, venue reports flat (0.0)
    // Should generate: close fill (sell 5.0)
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(instrument.clone());

    // Add cached LONG position of 5.0
    let position = create_test_position(
        &instrument,
        PositionId::new("P-001"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
    );
    ctx.add_position(&position);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Venue reports FLAT position (0.0)
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Flat,
        Quantity::from("0.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None, // Netting mode
        None, // No avg_px for flat
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should have 1 fill: close (sell 5.0)
    let fill_events: Vec<_> = result
        .events
        .iter()
        .filter_map(|e| {
            if let OrderEventAny::Filled(f) = e {
                Some(f)
            } else {
                None
            }
        })
        .collect();

    assert_eq!(
        fill_events.len(),
        1,
        "Flat report should generate 1 closing fill, was {}",
        fill_events.len()
    );

    // Fill should be SELL 5.0 (close long position)
    assert_eq!(fill_events[0].order_side, OrderSide::Sell);
    assert_eq!(fill_events[0].last_qty, Quantity::from("5.0"));
}

#[tokio::test]
async fn test_expired_order_applies_fills_before_terminal_event() {
    // Expired orders should apply fills before the expired event (same as canceled)
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let instrument = test_instrument();
    ctx.add_instrument(instrument);

    let client_order_id = ClientOrderId::from("O-EXPIRE-TEST");
    let venue_order_id = VenueOrderId::from("V-EXPIRE-001");

    // Create and submit an order
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("10.0"))
        .price(Price::from("100.0"))
        .client_order_id(client_order_id)
        .build();

    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order.clone());
    ctx.cache
        .borrow_mut()
        .add_venue_order_id(&client_order_id, &venue_order_id, false)
        .unwrap();

    // Report shows order EXPIRED with partial fills
    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Expired,
        Quantity::from("10.0"),
        Quantity::from("3.0"), // Partially filled
    );
    mass_status.add_order_reports(vec![report]);

    // Add fill report for the partial fill
    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-001"),
        OrderSide::Buy,
        Quantity::from("3.0"),
        Price::from("100.0"),
        Money::from("0.10 USDT"),
        LiquiditySide::Maker,
        Some(client_order_id),
        None,
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        None,
    );
    mass_status.add_fill_reports(vec![fill]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should have Fill event BEFORE Expired event
    let fill_count = result
        .events
        .iter()
        .filter(|e| matches!(e, OrderEventAny::Filled(_)))
        .count();
    let expired_count = result
        .events
        .iter()
        .filter(|e| matches!(e, OrderEventAny::Expired(_)))
        .count();

    assert_eq!(fill_count, 1, "Should have 1 fill event");
    assert_eq!(expired_count, 1, "Should have 1 expired event");

    // Verify fill comes before expired in the event list
    let fill_idx = result
        .events
        .iter()
        .position(|e| matches!(e, OrderEventAny::Filled(_)))
        .unwrap();
    let expired_idx = result
        .events
        .iter()
        .position(|e| matches!(e, OrderEventAny::Expired(_)))
        .unwrap();

    assert!(
        fill_idx < expired_idx,
        "Fill event should come before Expired event"
    );
}

#[tokio::test]
async fn test_partial_window_adjustment_skips_hedge_mode_instruments() {
    // Partial-window fill adjustment should skip hedge mode instruments
    // (those with venue_position_id set) to avoid corrupting fills
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let instrument = test_instrument();
    ctx.add_instrument(instrument);

    let venue_order_id = VenueOrderId::from("V-HEDGE-001");

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add a filled order report with fills
    let report = create_order_report(
        None,
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("5.0"),
        Quantity::from("5.0"),
    );
    mass_status.add_order_reports(vec![report]);

    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-HEDGE-001"),
        OrderSide::Buy,
        Quantity::from("5.0"),
        Price::from("100.0"),
        Money::from("0.10 USDT"),
        LiquiditySide::Taker,
        None,
        None,
        UnixNanos::from(1_000_001),
        UnixNanos::from(1_000_001),
        None,
    );
    mass_status.add_fill_reports(vec![fill]);

    // Add hedge mode position report (has venue_position_id)
    let hedge_position_id = PositionId::new("HEDGE-POS-001");

    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        None,                    // report_id
        Some(hedge_position_id), // Hedge mode - has venue_position_id
        Some(dec!(100.0)),       // avg_px_open
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // The fill should be preserved (not modified by partial-window adjustment)
    // and external order should be created
    let fill_events: Vec<_> = result
        .events
        .iter()
        .filter_map(|e| {
            if let OrderEventAny::Filled(f) = e {
                Some(f)
            } else {
                None
            }
        })
        .collect();

    assert!(
        !fill_events.is_empty(),
        "Fill events should be preserved for hedge mode instruments"
    );

    // Verify the fill quantity matches original (wasn't modified)
    assert_eq!(
        fill_events[0].last_qty,
        Quantity::from("5.0"),
        "Fill quantity should match original fill report"
    );
}

#[tokio::test]
async fn test_adjust_fills_multi_instrument_preserves_all_fills() {
    // Test that adjusting fills for one instrument doesn't affect fills for another.
    let mut ctx = TestContext::new();
    let instrument_id1 = test_instrument_id();
    let instrument_id2 = test_instrument_id2();

    ctx.add_instrument(test_instrument());
    ctx.add_instrument(test_instrument2());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Instrument 1 (ETHUSDT) - position of 2.0, fills sum to 2.0 (complete history)
    let venue_order_id1a = VenueOrderId::from("V-ETH-001");
    let venue_order_id1b = VenueOrderId::from("V-ETH-002");

    let order_report1a = create_order_report(
        Some(ClientOrderId::from("O-ETH-001")),
        venue_order_id1a,
        instrument_id1,
        OrderStatus::Filled,
        Quantity::from("1.000"),
        Quantity::from("1.000"),
    );
    let order_report1b = create_order_report(
        Some(ClientOrderId::from("O-ETH-002")),
        venue_order_id1b,
        instrument_id1,
        OrderStatus::Filled,
        Quantity::from("1.000"),
        Quantity::from("1.000"),
    );

    let fill1a = FillReport::new(
        test_account_id(),
        instrument_id1,
        venue_order_id1a,
        TradeId::from("T-ETH-001"),
        OrderSide::Buy,
        Quantity::from("1.000"),
        Price::from("3000.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Taker,
        None,
        None,
        UnixNanos::from(1_000_001),
        UnixNanos::from(1_000_001),
        None,
    );

    let fill1b = FillReport::new(
        test_account_id(),
        instrument_id1,
        venue_order_id1b,
        TradeId::from("T-ETH-002"),
        OrderSide::Buy,
        Quantity::from("1.000"),
        Price::from("3100.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Taker,
        None,
        None,
        UnixNanos::from(1_000_002),
        UnixNanos::from(1_000_002),
        None,
    );

    let position_report1 = PositionStatusReport::new(
        test_account_id(),
        instrument_id1,
        PositionSide::Long,
        Quantity::from("2.000"),
        UnixNanos::from(2_000),
        UnixNanos::from(2_000),
        None,
        None,
        Some(dec!(3050.00)),
    );

    // Instrument 2 (BTCUSD) - position of 100, fills sum to 100 (complete history)
    let venue_order_id2a = VenueOrderId::from("V-BTC-001");
    let venue_order_id2b = VenueOrderId::from("V-BTC-002");

    let order_report2a = create_order_report(
        Some(ClientOrderId::from("O-BTC-001")),
        venue_order_id2a,
        instrument_id2,
        OrderStatus::Filled,
        Quantity::from("50"),
        Quantity::from("50"),
    );
    let order_report2b = create_order_report(
        Some(ClientOrderId::from("O-BTC-002")),
        venue_order_id2b,
        instrument_id2,
        OrderStatus::Filled,
        Quantity::from("50"),
        Quantity::from("50"),
    );

    let fill2a = FillReport::new(
        test_account_id(),
        instrument_id2,
        venue_order_id2a,
        TradeId::from("T-BTC-001"),
        OrderSide::Buy,
        Quantity::from("50"),
        Price::from("50000.0"),
        Money::from("0.001 BTC"),
        LiquiditySide::Taker,
        None,
        None,
        UnixNanos::from(1_000_003),
        UnixNanos::from(1_000_003),
        None,
    );

    let fill2b = FillReport::new(
        test_account_id(),
        instrument_id2,
        venue_order_id2b,
        TradeId::from("T-BTC-002"),
        OrderSide::Buy,
        Quantity::from("50"),
        Price::from("51000.0"),
        Money::from("0.001 BTC"),
        LiquiditySide::Taker,
        None,
        None,
        UnixNanos::from(1_000_004),
        UnixNanos::from(1_000_004),
        None,
    );

    let position_report2 = PositionStatusReport::new(
        test_account_id(),
        instrument_id2,
        PositionSide::Long,
        Quantity::from("100"),
        UnixNanos::from(2_500),
        UnixNanos::from(2_500),
        None,
        None,
        Some(dec!(50500.0)),
    );

    mass_status.add_order_reports(vec![
        order_report1a,
        order_report1b,
        order_report2a,
        order_report2b,
    ]);
    mass_status.add_fill_reports(vec![fill1a, fill1b, fill2a, fill2b]);
    mass_status.add_position_reports(vec![position_report1, position_report2]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let fill_events: Vec<_> = result
        .events
        .iter()
        .filter_map(|e| {
            if let OrderEventAny::Filled(f) = e {
                Some(f)
            } else {
                None
            }
        })
        .collect();

    assert_eq!(
        fill_events.len(),
        4,
        "Expected 4 fill events (2 per instrument)"
    );

    let eth_fills: Vec<_> = fill_events
        .iter()
        .filter(|f| f.instrument_id == instrument_id1)
        .collect();
    assert_eq!(eth_fills.len(), 2, "Expected 2 fills for ETHUSDT");
    let eth_total_qty: f64 = eth_fills.iter().map(|f| f.last_qty.as_f64()).sum();
    assert!(
        (eth_total_qty - 2.0).abs() < 0.001,
        "ETHUSDT total qty should be 2.0, was {eth_total_qty}"
    );

    let btc_fills: Vec<_> = fill_events
        .iter()
        .filter(|f| f.instrument_id == instrument_id2)
        .collect();
    assert_eq!(btc_fills.len(), 2, "Expected 2 fills for BTCUSD");
    let btc_total_qty: f64 = btc_fills.iter().map(|f| f.last_qty.as_f64()).sum();
    assert!(
        (btc_total_qty - 100.0).abs() < 0.001,
        "BTCUSD total qty should be 100.0, was {btc_total_qty}"
    );
}

#[tokio::test]
async fn test_mass_status_preserves_symbol_scoped_trade_ids() {
    let mut ctx = TestContext::new();
    let instrument1 = test_instrument();
    let instrument2 = InstrumentAny::CurrencyPair(currency_pair_btcusdt());
    let instrument_id1 = instrument1.id();
    let instrument_id2 = instrument2.id();
    let client_order_id1 = ClientOrderId::from("O-SCOPED-ETH");
    let client_order_id2 = ClientOrderId::from("O-SCOPED-BTC");
    let venue_order_id1 = VenueOrderId::from("V-SCOPED-ETH");
    let venue_order_id2 = VenueOrderId::from("V-SCOPED-BTC");

    ctx.add_instrument(instrument1);
    ctx.add_instrument(instrument2);
    ctx.add_order(create_accepted_order(
        client_order_id1.as_str(),
        instrument_id1,
        OrderSide::Buy,
        "1.000",
        "3000.00",
        venue_order_id1,
    ));
    ctx.add_order(create_accepted_order(
        client_order_id2.as_str(),
        instrument_id2,
        OrderSide::Buy,
        "0.100000",
        "50000.00",
        venue_order_id2,
    ));

    let shared_trade_id = TradeId::from("12345678");

    let fill1 = FillReport::new(
        test_account_id(),
        instrument_id1,
        venue_order_id1,
        shared_trade_id,
        OrderSide::Buy,
        Quantity::from("1.000"),
        Price::from("3000.00"),
        Money::from("0.10 USDT"),
        LiquiditySide::Taker,
        Some(client_order_id1),
        None,
        UnixNanos::from(1_000_001),
        UnixNanos::from(1_000_001),
        None,
    );

    let fill2 = FillReport::new(
        test_account_id(),
        instrument_id2,
        venue_order_id2,
        shared_trade_id,
        OrderSide::Buy,
        Quantity::from("0.100000"),
        Price::from("50000.00"),
        Money::from("0.10 USDT"),
        LiquiditySide::Taker,
        Some(client_order_id2),
        None,
        UnixNanos::from(1_000_002),
        UnixNanos::from(1_000_002),
        None,
    );

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );
    mass_status.add_fill_reports(vec![fill1, fill2]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let fill_instruments: HashSet<_> = result
        .events
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => Some(fill.instrument_id),
            _ => None,
        })
        .collect();

    assert_eq!(
        fill_instruments,
        HashSet::from([instrument_id1, instrument_id2])
    );
}

#[tokio::test]
async fn test_adjust_fills_missing_order_reports_uses_fill_side() {
    // Test that fills without order reports still use fill.order_side correctly
    // for partial-window adjustment calculations.
    //
    // Scenario: When position qty > fills qty, partial-window adjustment calculates
    // the net effect of fills and creates a synthetic fill to match the position.
    // The fill.order_side from fills (even without order reports) is used to
    // determine the direction of the synthetic fill.
    //
    // Note: Fills without order reports contribute to calculations but don't
    // directly produce events - only the synthetic fill does.
    let instrument_id = test_instrument_id();
    let instrument = test_instrument();

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Fill without order report: 0.02 BUY
    let venue_order_id1 = VenueOrderId::from("V-NO-REPORT-001");

    let fill1 = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id1,
        TradeId::from("T-001"),
        OrderSide::Buy, // This is the key: fill.order_side is BUY
        Quantity::from("0.020"),
        Price::from("4000.00"),
        Money::from("0.00 USDT"),
        LiquiditySide::Taker,
        None,
        None,
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        None,
    );

    // Position report: 0.05 LONG (larger than our fill)
    // Synthetic fill of 0.03 BUY should be created to bridge the gap
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("0.050"),
        UnixNanos::from(2_000),
        UnixNanos::from(2_000),
        None,
        None,
        Some(dec!(3900.00)),
    );

    // No order reports - only fill1 with its fill.order_side
    mass_status.add_fill_reports(vec![fill1]);
    mass_status.add_position_reports(vec![position_report]);

    let result = process_mass_status_for_reconciliation(&mass_status, &instrument, None).unwrap();

    assert!(
        !result.orders.is_empty(),
        "Synthetic order should be created"
    );
    assert!(!result.fills.is_empty(), "Fills should be present");

    // Synthetic order direction should be inferred from fill.order_side
    for order in result.orders.values() {
        assert_eq!(
            order.order_side,
            OrderSide::Buy.into(),
            "Synthetic order side should be BUY (inferred from fill.order_side)"
        );
    }
}

#[tokio::test]
async fn test_adjust_fills_without_synthetic_reports_filters_to_current_lifecycle() {
    // Test FilterToCurrentLifecycle filters closed orders from previous lifecycles
    // while preserving working orders.
    //
    // Scenario:
    // - Position lifecycle: +100 (O1 BUY) -> FLAT (O2 SELL) -> +200 (O3 BUY current)
    // - O1 and O2 are FILLED (previous lifecycle, before zero-crossing)
    // - O3 is PARTIALLY_FILLED (working order in current lifecycle)
    // - Assert: O1 and O2 filtered out, O3 preserved
    let instrument_id = test_instrument_id();
    let instrument = test_instrument();
    let ts_now: u64 = 1_000_000_000_000;

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("200"),
        UnixNanos::from(ts_now),
        UnixNanos::from(ts_now),
        None,
        None,
        Some(dec!(1.1000)),
    );

    // O1: BUY 100 (previous lifecycle)
    let venue_order_id1 = VenueOrderId::from("V-001");
    let order_o1 = create_order_report(
        Some(ClientOrderId::from("C-001")),
        venue_order_id1,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("100"),
        Quantity::from("100"),
    );

    let fill_o1 = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id1,
        TradeId::from("T-001"),
        OrderSide::Buy,
        Quantity::from("100"),
        Price::from("1.0900"),
        Money::from("0.00 USDT"),
        LiquiditySide::Taker,
        None,
        None,
        UnixNanos::from(ts_now - 3_000_000_000),
        UnixNanos::from(ts_now - 3_000_000_000),
        None,
    );

    // O2: SELL 100 (zero-crossing to FLAT)
    let venue_order_id2 = VenueOrderId::from("V-002");

    let order_o2 = OrderStatusReport::new(
        test_account_id(),
        instrument_id,
        Some(ClientOrderId::from("C-002")),
        venue_order_id2,
        OrderSide::Sell.into(),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Filled,
        Quantity::from("100"),
        Quantity::from("100"),
        UnixNanos::from(ts_now - 2_000_000_000),
        UnixNanos::from(ts_now - 2_000_000_000),
        UnixNanos::from(ts_now - 2_000_000_000),
        None,
    );

    let fill_o2 = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id2,
        TradeId::from("T-002"),
        OrderSide::Sell,
        Quantity::from("100"),
        Price::from("1.0950"),
        Money::from("0.00 USDT"),
        LiquiditySide::Taker,
        None,
        None,
        UnixNanos::from(ts_now - 2_000_000_000), // Zero-crossing here
        UnixNanos::from(ts_now - 2_000_000_000),
        None,
    );

    // O3: BUY 200 (current lifecycle, PARTIALLY_FILLED working order)
    let venue_order_id3 = VenueOrderId::from("V-003");
    let order_o3 = create_order_report(
        Some(ClientOrderId::from("C-003")),
        venue_order_id3,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("300"),
        Quantity::from("200"),
    );

    let fill_o3 = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id3,
        TradeId::from("T-003"),
        OrderSide::Buy,
        Quantity::from("200"),
        Price::from("1.1000"),
        Money::from("0.00 USDT"),
        LiquiditySide::Taker,
        None,
        None,
        UnixNanos::from(ts_now),
        UnixNanos::from(ts_now),
        None,
    );

    mass_status.add_order_reports(vec![order_o1, order_o2, order_o3]);
    mass_status.add_fill_reports(vec![fill_o1, fill_o2, fill_o3]);
    mass_status.add_position_reports(vec![position_report]);

    let result = process_mass_status_for_reconciliation_without_synthetic_reports(
        &mass_status,
        &instrument,
        None,
    )
    .unwrap();

    // O1 and O2 should be filtered out (closed orders from previous lifecycle)
    assert!(
        !result.orders.contains_key(&venue_order_id1),
        "O1 should be filtered (closed order from previous lifecycle)"
    );
    assert!(
        !result.orders.contains_key(&venue_order_id2),
        "O2 should be filtered (closed order from previous lifecycle)"
    );

    // O3 should be preserved (working order in current lifecycle)
    assert!(
        result.orders.contains_key(&venue_order_id3),
        "O3 should be preserved (working order)"
    );
    assert_eq!(
        result.orders[&venue_order_id3].order_status,
        OrderStatus::PartiallyFilled
    );

    // Only O3 fill should be present
    assert!(
        result.fills.contains_key(&venue_order_id3),
        "O3 fills should be present"
    );
    assert_eq!(result.fills.len(), 1, "Only O3 fills should remain");
}

#[rstest]
#[tokio::test]
async fn test_replace_current_lifecycle_preserves_working_orders(
    #[values(false, true)] bounded: bool,
    #[values(false, true)] terminal_report: bool,
) {
    let mut ctx = TestContext::new();
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(StrategyId::external(), OmsType::Netting);
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());
    let ts_now: u64 = 1_000_000_000_000;

    let make_fill = |venue_order_id: &str, trade_id: &str, side: OrderSide, px: &str, ts: u64| {
        FillReport::new(
            test_account_id(),
            instrument_id,
            VenueOrderId::from(venue_order_id),
            TradeId::from(trade_id),
            side,
            Quantity::from("1.000"),
            Price::from(px),
            Money::from("0.00 USDT"),
            LiquiditySide::Taker,
            None,
            None,
            UnixNanos::from(ts),
            UnixNanos::from(ts),
            None,
        )
    };

    let mut mass_status = create_mass_status(
        vec![
            create_order_report(
                Some(ClientOrderId::from("C-004")),
                VenueOrderId::from("V-004"),
                instrument_id,
                OrderStatus::PartiallyFilled,
                Quantity::from("2.000"),
                Quantity::from("1.000"),
            ),
            create_order_report(
                Some(ClientOrderId::from("C-009")),
                VenueOrderId::from("V-009"),
                instrument_id,
                OrderStatus::Accepted,
                Quantity::from("1.000"),
                Quantity::from("0.000"),
            ),
        ],
        vec![
            make_fill(
                "V-001",
                "T-001",
                OrderSide::Buy,
                "3000.00",
                ts_now - 4_000_000_000,
            ),
            make_fill(
                "V-002",
                "T-002",
                OrderSide::Sell,
                "3050.00",
                ts_now - 3_000_000_000,
            ),
            make_fill(
                "V-003",
                "T-003",
                OrderSide::Buy,
                "3000.00",
                ts_now - 2_000_000_000,
            ),
            make_fill(
                "V-004",
                "T-004",
                OrderSide::Buy,
                "3100.00",
                ts_now - 1_000_000_000,
            ),
        ],
    );

    if terminal_report {
        mass_status.add_order_reports(vec![create_order_report(
            Some(ClientOrderId::from("C-003")),
            VenueOrderId::from("V-003"),
            instrument_id,
            OrderStatus::Filled,
            Quantity::from("1.000"),
            Quantity::from("1.000"),
        )]);
    }

    mass_status.add_position_reports(vec![PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("1.000"),
        UnixNanos::from(ts_now),
        UnixNanos::from(ts_now),
        None,
        None,
        Some(dec!(3142.04)),
    )]);

    if bounded {
        mass_status.set_report_window(Some(UnixNanos::from(ts_now - 5_000_000_000)), true);
    }

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let accepted: Vec<ClientOrderId> = result
        .events
        .iter()
        .filter_map(|e| match e {
            OrderEventAny::Accepted(accepted) => Some(accepted.client_order_id),
            _ => None,
        })
        .collect();

    assert!(
        accepted.contains(&ClientOrderId::from("C-009")),
        "working order not adopted, events: {:?}",
        result.events
    );
    assert!(
        accepted.contains(&ClientOrderId::from("C-004")),
        "partially filled working order not adopted, events: {:?}",
        result.events
    );

    let fills: Vec<&OrderFilled> = result
        .events
        .iter()
        .filter_map(|e| match e {
            OrderEventAny::Filled(fill) => Some(fill),
            _ => None,
        })
        .collect();

    assert_eq!(
        fills.len(),
        2 + usize::from(terminal_report),
        "fills: {fills:?}"
    );
    let synthetic = fills
        .iter()
        .find(|fill| fill.trade_id.as_str().starts_with("S-"))
        .unwrap();
    assert!(synthetic.venue_order_id.as_str().starts_with("S-"));
    assert_eq!(synthetic.last_qty, Quantity::from("1.000"));
    assert!(result.unresolved_positions.is_empty());
    let cache = ctx.cache.borrow();

    if terminal_report {
        let terminal = cache.order(&ClientOrderId::from("C-003")).unwrap();
        assert_eq!(terminal.status(), OrderStatus::Filled);
        assert_eq!(terminal.filled_qty(), Quantity::from("1.000"));
        assert_eq!(terminal.trade_ids(), vec![&TradeId::from("T-003")]);
    }

    let order = cache.order(&ClientOrderId::from("C-004")).unwrap();
    assert_eq!(order.venue_order_id(), Some(VenueOrderId::from("V-004")));
    assert_eq!(order.status(), OrderStatus::PartiallyFilled);
    assert_eq!(order.quantity(), Quantity::from("2.000"));
    assert_eq!(order.filled_qty(), Quantity::from("1.000"));
    assert_eq!(order.leaves_qty(), Quantity::from("1.000"));
    assert_eq!(order.trade_ids(), vec![&TradeId::from("T-004")]);
    assert_eq!(cache.orders_open(None, None, None, None, None).len(), 2);
    let positions = cache.positions_open(None, Some(&instrument_id), None, None, None);
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].signed_decimal_qty(), dec!(1));
    assert_eq!(positions[0].avg_px_open, 3142.04);
    drop(positions);
    drop(order);
    drop(cache);

    let replayed = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);
    assert!(
        replayed.unresolved_positions.is_empty(),
        "{:?}",
        replayed.unresolved_positions
    );
    let cache = ctx.cache.borrow();
    let positions = cache.positions_open(None, Some(&instrument_id), None, None, None);
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].signed_decimal_qty(), dec!(1));
    assert_eq!(positions[0].avg_px_open, 3142.04);
    drop(positions);
    drop(cache);

    let order = ctx.get_order(&ClientOrderId::from("C-004")).unwrap();
    let fill: OrderFilled = TestOrderEventStubs::filled(
        &order,
        &test_instrument(),
        Some(TradeId::from("T-005")),
        None,
        Some(Price::from("3200.00")),
        Some(Quantity::from("1.000")),
        None,
        None,
        Some(UnixNanos::from(ts_now + 1)),
        Some(test_account_id()),
    )
    .into();
    ctx.exec_engine
        .borrow_mut()
        .process(&OrderEventAny::Filled(fill));

    let order = ctx.get_order(&ClientOrderId::from("C-004")).unwrap();
    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.filled_qty(), Quantity::from("2.000"));
    let cache = ctx.cache.borrow();
    let positions = cache.positions_open(None, Some(&instrument_id), None, None, None);
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].signed_decimal_qty(), dec!(2));
    assert_eq!(positions[0].avg_px_open, 3171.02);
    assert_eq!(positions[0].trade_ids.len(), 2);
    assert!(positions[0].trade_ids.contains(&TradeId::from("T-005")));
    assert!(!positions[0].trade_ids.contains(&TradeId::from("T-004")));
}

#[tokio::test]
async fn test_cross_zero_with_missing_cached_avg_px_returns_none() {
    // When cached position has no avg_px, cross-zero cannot generate close fill
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(instrument.clone());

    let position = create_test_position(
        &instrument,
        PositionId::new("P-001"),
        OrderSide::Buy,
        "5.0",
        "0.00", // Zero price - will be treated as no avg_px in some paths
    );
    ctx.add_position(&position);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Short,
        Quantity::from("3.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3100.00)),
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // With zero cached price, cross-zero should still attempt reconciliation
    // but may produce different behavior - verify no panic at minimum
    assert!(
        result.events.len() <= 4,
        "Should not produce excessive events"
    );
}

#[tokio::test]
async fn test_cross_zero_with_missing_venue_avg_px_closes_only() {
    // When venue position has no avg_px, cross-zero can close but not open new position
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(instrument.clone());

    let position = create_test_position(
        &instrument,
        PositionId::new("P-001"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
    );
    ctx.add_position(&position);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Short,
        Quantity::from("3.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        None, // No avg_px - cannot open new position
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should generate close fill only (not open fill due to missing venue avg_px)
    let fill_events: Vec<_> = result
        .events
        .iter()
        .filter_map(|e| {
            if let OrderEventAny::Filled(f) = e {
                Some(f)
            } else {
                None
            }
        })
        .collect();

    assert_eq!(
        fill_events.len(),
        1,
        "Should generate only close fill when venue avg_px missing, was {}",
        fill_events.len()
    );
    assert_eq!(fill_events[0].order_side, OrderSide::Sell);
    assert_eq!(fill_events[0].last_qty, Quantity::from("5.0"));
}

#[tokio::test]
async fn test_hedge_mode_multiple_positions_same_instrument() {
    // Hedge mode venues can have multiple positions (long + short) for same instrument
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(instrument);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let long_position = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("10.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(PositionId::new("HEDGE-LONG-001")), // venue_position_id indicates hedge mode
        Some(dec!(3000.00)),
    );

    let short_position = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Short,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(PositionId::new("HEDGE-SHORT-001")),
        Some(dec!(3100.00)),
    );

    mass_status.add_position_reports(vec![long_position, short_position]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let fill_events: Vec<_> = result
        .events
        .iter()
        .filter_map(|e| {
            if let OrderEventAny::Filled(f) = e {
                Some(f)
            } else {
                None
            }
        })
        .collect();

    assert!(
        fill_events.len() >= 2,
        "Should process both hedge positions, was {} fills",
        fill_events.len()
    );
    let has_buy = fill_events.iter().any(|f| f.order_side == OrderSide::Buy);
    let has_sell = fill_events.iter().any(|f| f.order_side == OrderSide::Sell);
    assert!(has_buy, "Should have BUY fill for long position");
    assert!(has_sell, "Should have SELL fill for short position");
}

#[tokio::test]
async fn test_hedge_mode_with_filter_unclaimed_external_allows_synthetic() {
    // Synthetic orders should bypass filter_unclaimed_external
    let config = ExecutionManagerConfig {
        filter_unclaimed_external: true,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(instrument);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("10.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(PositionId::new("HEDGE-001")),
        Some(dec!(3000.00)),
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert!(
        !result.events.is_empty(),
        "Synthetic orders should bypass filter_unclaimed_external"
    );
}

#[tokio::test]
async fn test_duplicate_order_reports_reconciles_last_report() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let venue_order_id = VenueOrderId::from("V-DUP-001");
    let client_order_id = ClientOrderId::from("O-DUP-001");

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .quantity(Quantity::from("10.0"))
        .price(Price::from("3000.00"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order);
    ctx.cache
        .borrow_mut()
        .add_venue_order_id(&client_order_id, &venue_order_id, false)
        .unwrap();

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report_partial = OrderStatusReport::new(
        test_account_id(),
        instrument_id,
        Some(client_order_id),
        venue_order_id,
        OrderSide::Buy.into(),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("5.0"),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        None,
    );

    let report_filled = OrderStatusReport::new(
        test_account_id(),
        instrument_id,
        Some(client_order_id),
        venue_order_id,
        OrderSide::Buy.into(),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Filled,
        Quantity::from("10.0"),
        Quantity::from("10.0"),
        UnixNanos::from(2_000),
        UnixNanos::from(2_000),
        UnixNanos::from(2_000),
        None,
    )
    .with_avg_px(dec!(3000.0));

    mass_status.add_order_reports(vec![report_partial, report_filled]);

    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-001"),
        OrderSide::Buy,
        Quantity::from("10.0"),
        Price::from("3000.00"),
        Money::from("1.00 USDT"),
        LiquiditySide::Taker,
        None,
        None,
        UnixNanos::from(2_000),
        UnixNanos::from(2_000),
        None,
    );
    mass_status.add_fill_reports(vec![fill]);

    let _result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let order = ctx.get_order(&client_order_id).expect("Order should exist");
    assert_eq!(
        order.status(),
        OrderStatus::Filled,
        "Order should match the last report (Filled)"
    );
}

#[tokio::test]
async fn test_reconciliation_order_skipped_on_restart() {
    // Closed reconciliation orders (with RECONCILIATION tag) should be skipped on restart
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let venue_order_id = VenueOrderId::from("S-RECON-001");
    let client_order_id = ClientOrderId::from("S-RECON-001");

    let mut order = OrderTestBuilder::new(OrderType::Market)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .quantity(Quantity::from("5.0"))
        .tags(vec![ustr::Ustr::from("RECONCILIATION")])
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    let filled = TestOrderEventStubs::filled(
        &order,
        &test_instrument(),
        None,
        None,
        Some(Price::from("3000.00")),
        None,
        None,
        None,
        None,
        None,
    );
    order.apply(filled).unwrap();
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Report the same order again (simulating restart)
    let report = OrderStatusReport::new(
        test_account_id(),
        instrument_id,
        Some(client_order_id),
        venue_order_id,
        OrderSide::Buy.into(),
        OrderType::Market,
        TimeInForce::Gtc,
        OrderStatus::Filled,
        Quantity::from("5.0"),
        Quantity::from("5.0"),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        None,
    )
    .with_avg_px(dec!(3000.0));
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert!(
        result.events.is_empty(),
        "Closed reconciliation order should be skipped on restart"
    );
}

#[tokio::test]
async fn test_partially_filled_order_has_fills_applied() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let venue_order_id = VenueOrderId::from("V-PARTIAL-001");
    let client_order_id = ClientOrderId::from("O-PARTIAL-001");

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .quantity(Quantity::from("10.0"))
        .price(Price::from("3000.00"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order);
    ctx.cache
        .borrow_mut()
        .add_venue_order_id(&client_order_id, &venue_order_id, false)
        .unwrap();

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("5.0"),
    )
    .with_avg_px(dec!(3000.0));

    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-001"),
        OrderSide::Buy,
        Quantity::from("5.0"),
        Price::from("3000.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        None,
        None,
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        None,
    );

    mass_status.add_order_reports(vec![report]);
    mass_status.add_fill_reports(vec![fill]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let has_fills = result
        .events
        .iter()
        .any(|e| matches!(e, OrderEventAny::Filled(_)));
    assert!(has_fills, "Should generate fill events");

    let order = ctx.get_order(&client_order_id).expect("Order should exist");
    assert!(
        order.filled_qty() >= Quantity::from("5.0"),
        "Order should have at least 5.0 filled"
    );
}

#[rstest]
#[case(OrderStatus::PartiallyFilled, "7.0", true)]
#[case(OrderStatus::Filled, "10.0", true)]
#[case(OrderStatus::PartiallyFilled, "7.0", false)]
#[case(OrderStatus::Filled, "10.0", false)]
#[tokio::test]
async fn test_cached_fill_echo_preserves_mass_status_projection(
    #[case] status: OrderStatus,
    #[case] filled_qty: &str,
    #[case] has_new_fill: bool,
) {
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let client_order_id = ClientOrderId::from("O-CACHED-ECHO");
    let venue_order_id = VenueOrderId::from("V-CACHED-ECHO");
    let cached_trade_id = TradeId::from("T-CACHED-ECHO");
    let new_trade_id = TradeId::from("T-NEW-ECHO");
    ctx.add_instrument(instrument.clone());
    let mut order = create_accepted_order(
        client_order_id.as_str(),
        instrument_id,
        OrderSide::Buy,
        "10.0",
        "3000.00",
        venue_order_id,
    );
    let cached_fill = TestOrderEventStubs::filled(
        &order,
        &instrument,
        Some(cached_trade_id),
        None,
        Some(Price::from("3000.00")),
        Some(Quantity::from("3.0")),
        Some(LiquiditySide::Maker),
        None,
        None,
        Some(test_account_id()),
    );
    order.apply(cached_fill).unwrap();
    ctx.add_order(order);

    let filled_qty = Quantity::from(filled_qty);
    let remaining_qty = filled_qty - Quantity::from("3.0");
    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        status,
        Quantity::from("10.0"),
        filled_qty,
    )
    .with_avg_px(dec!(3000));
    let mut fills = vec![create_fill_report(
        client_order_id,
        venue_order_id,
        instrument_id,
        cached_trade_id,
        "3.0",
    )];

    if has_new_fill {
        let mut fill = create_fill_report(
            client_order_id,
            venue_order_id,
            instrument_id,
            new_trade_id,
            &remaining_qty.to_string(),
        );
        fill.ts_event = UnixNanos::from(2_000_000);
        fills.push(fill);
    }

    let result = ctx.manager.reconcile_execution_mass_status(
        &create_mass_status(vec![report], fills),
        &ctx.exec_engine,
    );

    let [OrderEventAny::Filled(fill)] = result.events.as_slice() else {
        panic!(
            "Expected one incremental fill, received {:?}",
            result.events
        );
    };

    assert_eq!(fill.client_order_id, client_order_id);
    assert_eq!(fill.venue_order_id, venue_order_id);
    assert_eq!(fill.account_id, test_account_id());
    assert_eq!(fill.instrument_id, instrument_id);
    assert_eq!(fill.last_qty, remaining_qty);
    assert_eq!(fill.last_px, Price::from("3000.00"));

    if has_new_fill {
        assert_eq!(fill.trade_id, new_trade_id);
    } else {
        assert_ne!(fill.trade_id, cached_trade_id);
    }

    let order = ctx.get_order(&client_order_id).unwrap();
    assert_eq!(order.status(), status);
    assert_eq!(order.filled_qty(), filled_qty);
    assert_eq!(order.trade_ids(), vec![&cached_trade_id, &fill.trade_id]);
}

#[tokio::test]
async fn test_working_order_with_new_fills_updates_correctly() {
    // Tests incremental fill reconciliation for a working order
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let venue_order_id = VenueOrderId::from("V-WORKING-001");
    let client_order_id = ClientOrderId::from("O-WORKING-001");

    // Start with order already partially filled (3 of 10)
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .quantity(Quantity::from("10.0"))
        .price(Price::from("3000.00"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    let first_fill = TestOrderEventStubs::filled(
        &order,
        &test_instrument(),
        Some(TradeId::from("T-PREV-001")),
        None,
        Some(Price::from("3000.00")),
        Some(Quantity::from("3.0")),
        None,
        None,
        None,
        None,
    );
    order.apply(first_fill).unwrap();
    ctx.add_order(order);
    ctx.cache
        .borrow_mut()
        .add_venue_order_id(&client_order_id, &venue_order_id, false)
        .unwrap();

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Venue reports 7 filled (was 3, now +4)
    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("7.0"),
    )
    .with_avg_px(dec!(3000.0));

    let new_fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-NEW-001"),
        OrderSide::Buy,
        Quantity::from("4.0"),
        Price::from("3000.00"),
        Money::from("0.40 USDT"),
        LiquiditySide::Maker,
        None,
        None,
        UnixNanos::from(2_000),
        UnixNanos::from(2_000),
        None,
    );

    mass_status.add_order_reports(vec![report]);
    mass_status.add_fill_reports(vec![new_fill]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let has_fills = result
        .events
        .iter()
        .any(|e| matches!(e, OrderEventAny::Filled(_)));
    assert!(has_fills, "Should generate fill events");

    let order = ctx.get_order(&client_order_id).expect("Order should exist");
    assert_eq!(
        order.filled_qty(),
        Quantity::from("7.0"),
        "Order should have updated filled qty"
    );
}

#[tokio::test]
async fn test_orphan_fills_without_order_reports_processed() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let orphan_venue_order_id = VenueOrderId::from("V-ORPHAN-001");
    let venue_position_id = PositionId::from("ETHUSDT-PERP.BINANCE-LONG");

    let orphan_fill = FillReport::new(
        test_account_id(),
        instrument_id,
        orphan_venue_order_id,
        TradeId::from("T-ORPHAN-001"),
        OrderSide::Buy,
        Quantity::from("0.75"),
        Price::from("3000.00"),
        Money::from("0.20 USDT"),
        LiquiditySide::Taker,
        None,
        Some(venue_position_id),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        None,
    );

    let second_fill = FillReport::new(
        test_account_id(),
        instrument_id,
        orphan_venue_order_id,
        TradeId::from("T-ORPHAN-002"),
        OrderSide::Buy,
        Quantity::from("1.25"),
        Price::from("3100.00"),
        Money::from("0.30 USDT"),
        LiquiditySide::Maker,
        None,
        Some(venue_position_id),
        UnixNanos::from(2_000),
        UnixNanos::from(2_000),
        None,
    );

    mass_status.add_fill_reports(vec![orphan_fill, second_fill]);

    let replay_status = mass_status.clone();

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 3);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));
    assert!(matches!(result.events[1], OrderEventAny::Filled(_)));
    assert!(matches!(result.events[2], OrderEventAny::Filled(_)));

    let position = ctx
        .cache
        .borrow()
        .position_owned(&venue_position_id)
        .unwrap();

    assert_eq!(position.id, venue_position_id);
    assert_eq!(position.instrument_id, instrument_id);
    assert_eq!(position.side, PositionSide::Long);
    assert_eq!(position.quantity, Quantity::from("2.0"));
    assert_eq!(position.avg_px_open, 3062.5);

    let replay = ctx
        .manager
        .reconcile_execution_mass_status(&replay_status, &ctx.exec_engine);

    assert!(replay.events.is_empty());
}

#[tokio::test]
async fn test_orphan_fill_group_validates_when_later_fill_has_position_id() {
    let _log_guard = MANAGER_LOG_TEST_LOCK.lock().await;
    install_manager_log_capture();

    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let venue_order_id = VenueOrderId::from("V-MIXED-POSITION-001");

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    mass_status.add_fill_reports(vec![
        FillReport::new(
            test_account_id(),
            instrument_id,
            venue_order_id,
            TradeId::from("T-MIXED-POSITION-001"),
            OrderSide::Buy,
            Quantity::from("0.75"),
            Price::from("3000.00"),
            Money::from("0.20 USDT"),
            LiquiditySide::Taker,
            None,
            None,
            UnixNanos::from(1_000),
            UnixNanos::from(1_000),
            None,
        ),
        FillReport::new(
            test_account_id(),
            instrument_id,
            venue_order_id,
            TradeId::from("T-MIXED-POSITION-002"),
            OrderSide::Buy,
            Quantity::from("1.25"),
            Price::from("3100.00"),
            Money::from("0.30 USDT"),
            LiquiditySide::Maker,
            None,
            Some(PositionId::from("ETHUSDT-PERP.BINANCE-LONG")),
            UnixNanos::from(2_000),
            UnixNanos::from(2_000),
            None,
        ),
    ]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let messages = MANAGER_LOG_CAPTURE.messages.lock().clone();
    let cache = ctx.cache.borrow();

    assert!(result.events.is_empty());
    assert!(result.external_orders.is_empty());
    assert_eq!(cache.orders_total_count(None, None, None, None, None), 0);
    assert!(cache.positions(None, None, None, None, None).is_empty());

    assert_eq!(
        messages,
        vec![
            "Cannot materialize orphan fills for venue order V-MIXED-POSITION-001: venue position ID is missing"
                .to_string(),
        ]
    );
}

#[tokio::test]
async fn test_orphan_fills_for_unknown_instrument_skipped() {
    let _log_guard = MANAGER_LOG_TEST_LOCK.lock().await;
    install_manager_log_capture();

    let mut ctx = TestContext::new();
    ctx.add_instrument(test_instrument());

    let unknown_instrument_id = InstrumentId::from("UNKNOWN-PERP.BINANCE");

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let orphan_fill = FillReport::new(
        test_account_id(),
        unknown_instrument_id,
        VenueOrderId::from("V-UNKNOWN-001"),
        TradeId::from("T-UNKNOWN-001"),
        OrderSide::Buy,
        Quantity::from("1.0"),
        Price::from("100.00"),
        Money::from("0.10 USDT"),
        LiquiditySide::Taker,
        None,
        Some(PositionId::from("UNKNOWN-PERP.BINANCE-LONG")),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        None,
    );

    mass_status.add_fill_reports(vec![orphan_fill]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let messages = MANAGER_LOG_CAPTURE.messages.lock().clone();
    let cache = ctx.cache.borrow();

    assert!(result.events.is_empty());
    assert!(result.external_orders.is_empty());
    assert_eq!(cache.orders_total_count(None, None, None, None, None), 0);
    assert!(cache.positions(None, None, None, None, None).is_empty());

    assert!(
        messages
            .iter()
            .any(|message| message == "1 orders skipped (instrument not in cache)")
    );
}

#[tokio::test]
async fn test_orphan_fills_without_position_id_dropped() {
    let _log_guard = MANAGER_LOG_TEST_LOCK.lock().await;
    install_manager_log_capture();

    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let orphan_fill = |venue_order_id: &str, trade_id: &str, ts: u64| {
        FillReport::new(
            test_account_id(),
            instrument_id,
            VenueOrderId::from(venue_order_id),
            TradeId::from(trade_id),
            OrderSide::Buy,
            Quantity::from("0.50"),
            Price::from("3000.00"),
            Money::from("0.10 USDT"),
            LiquiditySide::Taker,
            None,
            None,
            UnixNanos::from(ts),
            UnixNanos::from(ts),
            None,
        )
    };

    mass_status.add_fill_reports(vec![
        orphan_fill("V-NETTING-001", "T-NETTING-001", 1_000),
        orphan_fill("V-NETTING-001", "T-NETTING-002", 2_000),
        orphan_fill("V-NETTING-002", "T-NETTING-003", 3_000),
        orphan_fill("V-NETTING-003", "T-NETTING-004", 4_000),
        orphan_fill("V-NETTING-004", "T-NETTING-005", 5_000),
        orphan_fill("V-NETTING-005", "T-NETTING-006", 6_000),
        orphan_fill("V-NETTING-006", "T-NETTING-007", 7_000),
    ]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let messages = MANAGER_LOG_CAPTURE.messages.lock().clone();
    let cache = ctx.cache.borrow();

    assert!(result.events.is_empty());
    assert!(result.external_orders.is_empty());
    assert!(result.unresolved_positions.is_empty());
    assert_eq!(cache.orders_total_count(None, None, None, None, None), 0);
    assert!(cache.positions(None, None, None, None, None).is_empty());
    assert_eq!(
        messages,
        vec![
            "Dropped 7 fill(s) in 6 fill group(s) without an order report or cached order \
             (V-NETTING-001 (ETHUSDT-PERP.BINANCE), V-NETTING-002 (ETHUSDT-PERP.BINANCE), \
             V-NETTING-003 (ETHUSDT-PERP.BINANCE), V-NETTING-004 (ETHUSDT-PERP.BINANCE), \
             V-NETTING-005 (ETHUSDT-PERP.BINANCE))"
                .to_string(),
        ]
    );
}

#[tokio::test]
async fn test_filtered_client_order_ids_skips_matching_orders() {
    // Orders in filtered_client_order_ids should be skipped during reconciliation
    let filtered_id = ClientOrderId::from("O-FILTERED-001");

    let config = ExecutionManagerConfig {
        filtered_client_order_ids: IndexSet::from([filtered_id]),
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add an order report that should be filtered
    let report = create_order_report(
        Some(filtered_id),
        VenueOrderId::from("V-FILTERED-001"),
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("10.0"),
        Quantity::from("0.0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // No events should be generated for filtered order
    assert!(
        result.events.is_empty(),
        "Filtered order should not generate events"
    );

    // Order should not be in cache
    assert!(
        ctx.get_order(&filtered_id).is_none(),
        "Filtered order should not be added to cache"
    );
}

#[tokio::test]
async fn test_filtered_client_order_ids_skips_orphan_fills() {
    // Orphan fills (fills without order reports) should also be filtered
    let filtered_id = ClientOrderId::from("O-FILTERED-002");
    let venue_order_id = VenueOrderId::from("V-FILTERED-002");

    let config = ExecutionManagerConfig {
        filtered_client_order_ids: IndexSet::from([filtered_id]),
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    // Add the order to cache (simulating an order placed before filtering was enabled)
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(filtered_id)
        .instrument_id(instrument_id)
        .quantity(Quantity::from("10.0"))
        .price(Price::from("100.0"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add orphan fill (fill without order report) for the filtered order
    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-001"),
        OrderSide::Buy,
        Quantity::from("5.0"),
        Price::from("100.00"),
        Money::from("0.50 USD"),
        LiquiditySide::Taker,
        Some(filtered_id),
        None,
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        None,
    );
    mass_status.add_fill_reports(vec![fill]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // No fill events should be generated for filtered order
    assert!(
        !result
            .events
            .iter()
            .any(|e| matches!(e, OrderEventAny::Filled(_))),
        "Filtered order should not receive orphan fill events"
    );
}

#[tokio::test]
async fn test_filtered_client_order_ids_skips_orphan_fills_via_venue_order_id_lookup() {
    // Orphan fills looked up by venue_order_id should also be filtered
    let filtered_id = ClientOrderId::from("O-FILTERED-003");
    let venue_order_id = VenueOrderId::from("V-FILTERED-003");

    let config = ExecutionManagerConfig {
        filtered_client_order_ids: IndexSet::from([filtered_id]),
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    // Add the order to cache
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(filtered_id)
        .instrument_id(instrument_id)
        .quantity(Quantity::from("10.0"))
        .price(Price::from("100.0"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add orphan fill WITHOUT client_order_id (will be looked up by venue_order_id)
    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-002"),
        OrderSide::Buy,
        Quantity::from("5.0"),
        Price::from("100.00"),
        Money::from("0.50 USD"),
        LiquiditySide::Taker,
        None, // No client_order_id - will use venue_order_id lookup
        None,
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        None,
    );
    mass_status.add_fill_reports(vec![fill]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // No fill events should be generated for filtered order
    assert!(
        !result
            .events
            .iter()
            .any(|e| matches!(e, OrderEventAny::Filled(_))),
        "Filtered order should not receive orphan fill events via venue_order_id lookup"
    );
}

#[tokio::test]
async fn test_reconciliation_instrument_ids_filters_other_instruments() {
    // Only instruments in reconciliation_instrument_ids should be reconciled
    let included_instrument = test_instrument_id();

    let config = ExecutionManagerConfig {
        reconciliation_instrument_ids: IndexSet::from([included_instrument]),
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    // Add a second instrument that's NOT in the filter list
    let excluded_instrument = test_instrument2();
    let excluded_instrument_id = test_instrument_id2();
    ctx.add_instrument(excluded_instrument);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add order for included instrument
    let included_report = create_order_report(
        Some(ClientOrderId::from("O-INCLUDED-001")),
        VenueOrderId::from("V-INCLUDED-001"),
        included_instrument,
        OrderStatus::Accepted,
        Quantity::from("5.0"),
        Quantity::from("0.0"),
    );

    // Add order for excluded instrument
    let excluded_report = create_order_report(
        Some(ClientOrderId::from("O-EXCLUDED-001")),
        VenueOrderId::from("V-EXCLUDED-001"),
        excluded_instrument_id,
        OrderStatus::Accepted,
        Quantity::from("5.0"),
        Quantity::from("0.0"),
    );

    mass_status.add_order_reports(vec![included_report, excluded_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Only included instrument should have generated events
    let included_order = ctx.get_order(&ClientOrderId::from("O-INCLUDED-001"));
    let excluded_order = ctx.get_order(&ClientOrderId::from("O-EXCLUDED-001"));

    assert!(
        included_order.is_some(),
        "Included instrument order should be reconciled"
    );
    assert!(
        excluded_order.is_none(),
        "Excluded instrument order should NOT be reconciled"
    );

    // Events should only be for included instrument
    let has_excluded_events = result.events.iter().any(|e| match e {
        OrderEventAny::Initialized(init) => init.instrument_id == excluded_instrument_id,
        OrderEventAny::Accepted(acc) => acc.instrument_id == excluded_instrument_id,
        _ => false,
    });

    assert!(
        !has_excluded_events,
        "No events should be generated for excluded instrument"
    );
}

#[tokio::test]
async fn test_reconciliation_instrument_ids_filters_position_reports() {
    // Position reports for instruments NOT in reconciliation_instrument_ids should be skipped
    let included_instrument_id = test_instrument_id();

    let config = ExecutionManagerConfig {
        reconciliation_instrument_ids: IndexSet::from([included_instrument_id]),
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    // Add excluded instrument
    let excluded_instrument = test_instrument2();
    let excluded_instrument_id = test_instrument_id2();
    ctx.add_instrument(excluded_instrument);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Position report for excluded instrument
    let position_report = PositionStatusReport::new(
        test_account_id(),
        excluded_instrument_id,
        PositionSide::Long,
        Quantity::from("10.0"),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        None,
        None,
        Some(dec!(100.00)),
    );
    mass_status.add_position_reports(vec![position_report]);

    let _result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // No position should be created for excluded instrument
    let cache = ctx.cache.borrow();
    let positions = cache.positions(None, None, None, None, None);
    let has_excluded_position = positions
        .iter()
        .any(|p| p.instrument_id == excluded_instrument_id);
    assert!(
        !has_excluded_position,
        "No position should be created for excluded instrument"
    );
}

struct MockExecutionClient {
    retain_unresolved_submissions: bool,
    client_id: ClientId,
    account_id: AccountId,
    venue: Venue,
    oms_type: OmsType,
    handled_venues: Option<IndexSet<Venue>>,
    bulk_position_coverage: bool,
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
            retain_unresolved_submissions: false,
            client_id: test_client_id(),
            account_id: test_account_id(),
            venue: test_venue(),
            oms_type: OmsType::Hedging,
            handled_venues: None,
            bulk_position_coverage: true,
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
            retain_unresolved_submissions: false,
            client_id,
            account_id: test_account_id(),
            venue,
            oms_type: OmsType::Hedging,
            handled_venues: None,
            bulk_position_coverage: true,
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
            retain_unresolved_submissions: false,
            client_id,
            account_id: test_account_id(),
            venue,
            oms_type: OmsType::Hedging,
            handled_venues: None,
            bulk_position_coverage: true,
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

    fn with_account_id(mut self, account_id: AccountId) -> Self {
        self.account_id = account_id;
        self
    }

    fn with_handled_venues(mut self, handled_venues: IndexSet<Venue>) -> Self {
        self.handled_venues = Some(handled_venues);
        self
    }

    fn with_bulk_position_coverage(mut self, covered: bool) -> Self {
        self.bulk_position_coverage = covered;
        self
    }

    fn with_oms_type(mut self, oms_type: OmsType) -> Self {
        self.oms_type = oms_type;
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

    fn provides_bulk_position_coverage(&self, _instrument_id: InstrumentId) -> bool {
        self.bulk_position_coverage
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

#[rstest]
fn test_check_open_order_queries_builds_query_for_cached_open_order() {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        max_single_order_queries_per_cycle: 5,
        ..Default::default()
    });

    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-QUERY-001");
    let venue_order_id = VenueOrderId::from("V-QUERY-001");
    let client_id = test_client_id();

    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, client_id);

    let queries = ctx.manager.check_open_order_queries();

    assert_eq!(queries.len(), 1);

    match &queries[0] {
        TradingCommand::QueryOrder(query) => {
            assert_eq!(query.client_id, Some(client_id));
            assert_eq!(query.client_order_id, client_order_id);
            assert_eq!(query.venue_order_id, Some(venue_order_id));
        }
        command => panic!("Expected QueryOrder, was {command:?}"),
    }
}

#[rstest]
fn test_check_open_order_queries_dedupes_open_inflight_order() {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        max_single_order_queries_per_cycle: 5,
        single_order_query_delay_ms: 0,
        ..Default::default()
    });

    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-QUERY-002");
    let venue_order_id = VenueOrderId::from("V-QUERY-002");
    let client_id = test_client_id();
    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, client_id);
    let order = ctx
        .cache
        .borrow()
        .order(&client_order_id)
        .map(|order| order.clone())
        .unwrap();
    let pending = OrderEventAny::PendingCancel(
        OrderPendingCancelSpec::builder()
            .trader_id(order.trader_id())
            .strategy_id(order.strategy_id())
            .instrument_id(order.instrument_id())
            .client_order_id(order.client_order_id())
            .account_id(test_account_id())
            .venue_order_id(venue_order_id)
            .build(),
    );
    ctx.cache.borrow_mut().update_order(&pending).unwrap();

    let queries = ctx.manager.check_open_order_queries();

    assert_eq!(queries.len(), 1);

    match &queries[0] {
        TradingCommand::QueryOrder(query) => {
            assert_eq!(query.client_id, Some(client_id));
            assert_eq!(query.client_order_id, client_order_id);
            assert_eq!(query.venue_order_id, Some(venue_order_id));
        }
        command => panic!("Expected QueryOrder, was {command:?}"),
    }
}

#[rstest]
fn test_check_open_order_queries_respects_per_cycle_limit() {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        max_single_order_queries_per_cycle: 1,
        ..Default::default()
    });

    ctx.add_instrument(test_instrument());

    insert_accepted_limit_order(
        &ctx,
        ClientOrderId::from("O-QUERY-003"),
        VenueOrderId::from("V-QUERY-003"),
        test_client_id(),
    );
    insert_accepted_limit_order(
        &ctx,
        ClientOrderId::from("O-QUERY-004"),
        VenueOrderId::from("V-QUERY-004"),
        test_client_id(),
    );

    let queries = ctx.manager.check_open_order_queries();

    assert_eq!(queries.len(), 1);
}

#[rstest]
fn test_check_open_order_queries_rotates_after_open_report_response() {
    fn queried_client_order_id(queries: &[TradingCommand]) -> ClientOrderId {
        match &queries[0] {
            TradingCommand::QueryOrder(query) => query.client_order_id,
            command => panic!("Expected QueryOrder, was {command:?}"),
        }
    }

    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        max_single_order_queries_per_cycle: 1,
        open_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    });

    ctx.add_instrument(test_instrument());

    let first_id = ClientOrderId::from("O-QUERY-011");
    let second_id = ClientOrderId::from("O-QUERY-012");
    let third_id = ClientOrderId::from("O-QUERY-013");
    let first_venue_id = VenueOrderId::from("V-QUERY-011");
    let second_venue_id = VenueOrderId::from("V-QUERY-012");
    let third_venue_id = VenueOrderId::from("V-QUERY-013");

    insert_accepted_limit_order(&ctx, first_id, first_venue_id, test_client_id());
    insert_accepted_limit_order(&ctx, second_id, second_venue_id, test_client_id());
    insert_accepted_limit_order(&ctx, third_id, third_venue_id, test_client_id());

    let first_queries = ctx.manager.check_open_order_queries();
    let first_report = create_order_report(
        Some(first_id),
        first_venue_id,
        test_instrument_id(),
        OrderStatus::Accepted,
        Quantity::from("10.0"),
        Quantity::from("0.0"),
    );
    ctx.manager
        .observe_execution_report(&ExecutionReport::Order(Box::new(first_report)));

    let second_queries = ctx.manager.check_open_order_queries();
    let second_report = create_order_report(
        Some(second_id),
        second_venue_id,
        test_instrument_id(),
        OrderStatus::Accepted,
        Quantity::from("10.0"),
        Quantity::from("0.0"),
    );
    ctx.manager
        .observe_execution_report(&ExecutionReport::Order(Box::new(second_report)));

    let third_queries = ctx.manager.check_open_order_queries();

    assert_eq!(first_queries.len(), 1);
    assert_eq!(second_queries.len(), 1);
    assert_eq!(third_queries.len(), 1);
    assert_eq!(queried_client_order_id(&first_queries), first_id);
    assert_eq!(queried_client_order_id(&second_queries), second_id);
    assert_eq!(queried_client_order_id(&third_queries), third_id);
}

#[rstest]
fn test_check_open_order_queries_returns_empty_when_cycle_limit_is_zero() {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        max_single_order_queries_per_cycle: 0,
        ..Default::default()
    });

    ctx.add_instrument(test_instrument());

    insert_accepted_limit_order(
        &ctx,
        ClientOrderId::from("O-QUERY-005"),
        VenueOrderId::from("V-QUERY-005"),
        test_client_id(),
    );

    let queries = ctx.manager.check_open_order_queries();

    assert!(queries.is_empty());
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_check_open_order_queries_respects_query_delay() {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        max_single_order_queries_per_cycle: 5,
        single_order_query_delay_ms: 100,
        ..Default::default()
    });

    ctx.add_instrument(test_instrument());

    insert_accepted_limit_order(
        &ctx,
        ClientOrderId::from("O-QUERY-006"),
        VenueOrderId::from("V-QUERY-006"),
        test_client_id(),
    );

    let first_queries = ctx.manager.check_open_order_queries();
    let delayed_queries = ctx.manager.check_open_order_queries();
    ctx.advance_time(200_000_000);
    let domain_advanced_queries = ctx.manager.check_open_order_queries();
    advance_clock(dst::time::Duration::from_millis(100)).await;
    let monotonic_advanced_queries = ctx.manager.check_open_order_queries();

    assert_eq!(first_queries.len(), 1);
    assert!(delayed_queries.is_empty());
    assert!(domain_advanced_queries.is_empty());
    assert_eq!(monotonic_advanced_queries.len(), 1);
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_check_open_order_queries_defers_with_recent_local_activity() {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        max_single_order_queries_per_cycle: 5,
        open_check_threshold_ns: DurationNanos::from_secs(5),
        ..Default::default()
    });

    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-QUERY-007");
    insert_accepted_limit_order(
        &ctx,
        client_order_id,
        VenueOrderId::from("V-QUERY-007"),
        test_client_id(),
    );
    ctx.manager.record_local_activity(client_order_id);

    let queries = ctx.manager.check_open_order_queries();
    ctx.advance_time(6_000_000_000);
    let domain_advanced_queries = ctx.manager.check_open_order_queries();
    advance_clock(dst::time::Duration::from_secs(5)).await;
    let monotonic_advanced_queries = ctx.manager.check_open_order_queries();

    assert!(queries.is_empty());
    assert!(domain_advanced_queries.is_empty());
    assert_eq!(monotonic_advanced_queries.len(), 1);
}

#[rstest]
fn test_check_open_order_queries_skips_filtered_client_order_ids() {
    let filtered_id = ClientOrderId::from("O-QUERY-008");

    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        max_single_order_queries_per_cycle: 5,
        filtered_client_order_ids: IndexSet::from([filtered_id]),
        ..Default::default()
    });

    ctx.add_instrument(test_instrument());

    insert_accepted_limit_order(
        &ctx,
        filtered_id,
        VenueOrderId::from("V-QUERY-008"),
        test_client_id(),
    );

    let queries = ctx.manager.check_open_order_queries();

    assert!(queries.is_empty());
}

#[rstest]
fn test_check_open_order_queries_filters_reconciliation_instruments() {
    let included_id = ClientOrderId::from("O-QUERY-009");
    let excluded_id = ClientOrderId::from("O-QUERY-010");

    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        max_single_order_queries_per_cycle: 5,
        reconciliation_instrument_ids: IndexSet::from([test_instrument_id()]),
        ..Default::default()
    });

    ctx.add_instrument(test_instrument());
    ctx.add_instrument(test_instrument2());

    insert_accepted_limit_order(
        &ctx,
        included_id,
        VenueOrderId::from("V-QUERY-009"),
        test_client_id(),
    );
    insert_accepted_limit_order_for_instrument(
        &ctx,
        excluded_id,
        VenueOrderId::from("V-QUERY-010"),
        test_instrument_id2(),
        ClientId::from("BYBIT"),
    );

    let queries = ctx.manager.check_open_order_queries();

    assert_eq!(queries.len(), 1);

    match &queries[0] {
        TradingCommand::QueryOrder(query) => {
            assert_eq!(query.client_order_id, included_id);
            assert_eq!(query.instrument_id, test_instrument_id());
        }
        command => panic!("Expected QueryOrder, was {command:?}"),
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

#[rstest]
#[case::client_id(true)]
#[case::venue_id(false)]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_check_open_orders_defers_with_recent_local_activity(#[case] has_client_id: bool) {
    // Test that reconciliation is deferred when there's recent local activity
    // within the threshold, to avoid race conditions with in-flight fills.
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::from_millis(200), // 200ms threshold
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let instrument_id = test_instrument_id();

    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, test_client_id());

    ctx.manager.record_local_activity(client_order_id);

    let report = create_order_report(
        has_client_id.then_some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("5.0"),
    )
    .with_avg_px(dec!(100.0));

    let mock_client = MockExecutionClient::new(vec![report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_open_orders(&clients).await;

    assert!(
        events.is_empty(),
        "Reconciliation should be deferred with recent local activity"
    );
    let cached_order = ctx.get_order(&client_order_id).unwrap();
    assert_eq!(cached_order.status(), OrderStatus::Accepted);
    assert_eq!(cached_order.filled_qty(), Quantity::from("0.0"));
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_check_open_orders_proceeds_after_threshold_exceeded() {
    // Test that reconciliation proceeds when the local activity is older than
    // the configured threshold.
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::from_millis(200), // 200ms threshold
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let instrument_id = test_instrument_id();

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .quantity(Quantity::from("10.0"))
        .price(Price::from("100.0"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order.clone());

    ctx.manager.record_local_activity(client_order_id);
    ctx.advance_both(dst::time::Duration::from_millis(500))
        .await;

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("5.0"),
    )
    .with_avg_px(dec!(100.0));

    let mock_client = MockExecutionClient::new(vec![report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_open_orders(&clients).await;

    assert_eq!(
        events.len(),
        1,
        "Reconciliation should proceed when threshold exceeded"
    );

    if let OrderEventAny::Filled(filled) = &events[0] {
        assert_eq!(filled.last_qty, Quantity::from("5.0"));
    } else {
        panic!("Expected OrderFilled event, was {:?}", events[0]);
    }
}

#[rstest]
#[case::client_id(true)]
#[case::venue_id(false)]
#[tokio::test]
async fn test_check_open_orders_proceeds_without_local_activity(#[case] has_client_id: bool) {
    // Test that reconciliation proceeds normally when there's no recorded
    // local activity for the order.
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::from_millis(200), // 200ms threshold
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let instrument_id = test_instrument_id();

    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, test_client_id());

    let report = create_order_report(
        has_client_id.then_some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("5.0"),
    )
    .with_avg_px(dec!(100.0));

    let mock_client = MockExecutionClient::new(vec![report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_open_orders(&clients).await;

    assert_eq!(
        events.len(),
        1,
        "Reconciliation should proceed without local activity"
    );

    if let OrderEventAny::Filled(filled) = &events[0] {
        assert_eq!(filled.client_order_id, client_order_id);
        assert_eq!(filled.venue_order_id, venue_order_id);
        assert_eq!(filled.last_qty, Quantity::from("5.0"));
    } else {
        panic!("Expected OrderFilled event, was {:?}", events[0]);
    }
}

#[rstest]
#[case::bulk("bulk", false, "2.0")]
#[case::bulk_inflight("bulk", true, "2.0")]
#[case::bulk_stale("bulk", false, "1.0")]
#[case::targeted("targeted", false, "2.0")]
#[case::targeted_inflight("targeted", true, "2.0")]
#[case::targeted_stale("targeted", false, "1.0")]
#[case::startup("startup", false, "2.0")]
#[tokio::test]
async fn test_terminal_reconciliation_and_stream_fill_apply_once(
    #[case] route: &str,
    #[case] stream_inflight: bool,
    #[case] reported_qty: Quantity,
    #[values(OrderStatus::Canceled, OrderStatus::Expired)] status: OrderStatus,
    #[values(false, true)] stream_first: bool,
) {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        open_check_open_only: false,
        open_check_missing_retries: 1,
        open_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    });

    let instrument = test_instrument();
    ctx.add_instrument(instrument.clone());
    let client_order_id = ClientOrderId::from("O-TERMINAL-STREAM");
    let venue_order_id = VenueOrderId::from("V-TERMINAL-STREAM");
    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, test_client_id());
    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument.id(),
        status,
        Quantity::from("10.0"),
        reported_qty,
    )
    .with_price(Price::from("100.0"))
    .with_avg_px(dec!(3000.0));
    let fill = create_fill_report(
        client_order_id,
        venue_order_id,
        instrument.id(),
        TradeId::from("T-TERMINAL-STREAM"),
        "2.0",
    );
    let order = ctx.get_order(&client_order_id).unwrap();
    let streamed = OrderFilledTestBuilder::new(&order, &instrument)
        .trade_id(fill.trade_id)
        .last_qty(fill.last_qty)
        .last_px(fill.last_px)
        .liquidity_side(fill.liquidity_side)
        .commission(fill.commission)
        .ts_event(fill.ts_event)
        .without_position_id()
        .build();

    if stream_first {
        ctx.exec_engine.borrow_mut().process(&streamed);
    }

    let client = MockExecutionClient::new(if route == "bulk" {
        vec![report.clone()]
    } else {
        Vec::new()
    })
    .with_order_report(report.clone())
    .with_fill_reports(vec![fill.clone()]);

    if stream_inflight {
        let engine = ctx.exec_engine.clone();
        let fill = streamed.clone();
        *client.on_fill_reports_query.borrow_mut() = Some(Box::new(move || {
            engine.borrow_mut().process(&fill);
        }));
    }

    if route == "startup" {
        ctx.manager.reconcile_execution_mass_status(
            &create_mass_status(vec![report.clone()], vec![fill.clone()]),
            &ctx.exec_engine,
        );
    } else {
        let events = ctx.manager.check_open_orders(&[&client]).await;

        for event in events {
            ctx.exec_engine.borrow_mut().process(&event);
        }
    }

    ctx.exec_engine.borrow_mut().process(&streamed);
    ctx.exec_engine.borrow_mut().process(&streamed);
    let recovered = ctx.get_order(&client_order_id).unwrap();
    let repeated_client = MockExecutionClient::new(vec![report]).with_fill_reports(vec![fill]);
    let repeated = ctx.manager.check_open_orders(&[&repeated_client]).await;

    for event in &repeated {
        ctx.exec_engine.borrow_mut().process(event);
    }

    let reconciled = ctx.get_order(&client_order_id).unwrap();

    for _ in 0..2 {
        let events = ctx.manager.check_open_orders(&[&repeated_client]).await;

        for event in events {
            ctx.exec_engine.borrow_mut().process(&event);
        }
    }

    let after_repeated = ctx.get_order(&client_order_id).unwrap();
    assert_eq!(after_repeated.events(), reconciled.events());

    let additional = OrderFilledTestBuilder::new(&recovered, &instrument)
        .trade_id(TradeId::from("T-TERMINAL-ADDITIONAL"))
        .last_qty(Quantity::from("1.0"))
        .last_px(Price::from("3030.0"))
        .commission(Money::from("0.25 USDT"))
        .ts_event(UnixNanos::from(2_000_000))
        .without_position_id()
        .build();

    ctx.exec_engine.borrow_mut().process(&additional);

    let final_order = ctx.get_order(&client_order_id).unwrap();
    let cache = ctx.cache.borrow();
    let position_id = cache.position_id(&client_order_id).unwrap();
    let position = cache.position(position_id).unwrap();

    assert_eq!(recovered.status(), status);
    assert_eq!(recovered.filled_qty(), Quantity::from("2.0"));
    assert_eq!(
        recovered.trade_ids(),
        vec![&TradeId::from("T-TERMINAL-STREAM")]
    );
    assert_eq!(
        recovered.commissions().get(&Currency::USDT()),
        Some(&Money::from("0.50 USDT"))
    );
    assert_eq!(
        repeated.len(),
        usize::from(reported_qty < recovered.filled_qty()),
    );

    for event in &repeated {
        assert!(matches!(
            (status, event),
            (OrderStatus::Canceled, OrderEventAny::Canceled(_))
                | (OrderStatus::Expired, OrderEventAny::Expired(_))
        ));
    }

    assert_eq!(final_order.status(), status);

    let quantity = Quantity::from("3.0");
    let price = dec!(3010.0);
    let commission = Money::from("0.75 USDT");

    assert_eq!(final_order.events().len(), reconciled.events().len() + 1);
    assert_eq!(final_order.filled_qty(), quantity);
    assert_eq!(final_order.avg_px(), Some(price));
    assert_eq!(
        final_order.commissions().get(&Currency::USDT()),
        Some(&commission)
    );
    assert_eq!(position.quantity, quantity);
}

#[rstest]
#[case::bulk_empty("bulk", "empty")]
#[case::bulk_partial("bulk", "partial")]
#[case::bulk_failed("bulk", "failed")]
#[case::bulk_foreign("bulk", "foreign")]
#[case::bulk_account("bulk", "account")]
#[case::bulk_instrument("bulk", "instrument")]
#[case::bulk_side("bulk", "side")]
#[case::targeted_empty("targeted", "empty")]
#[case::targeted_zero("targeted", "zero")]
#[case::targeted_partial("targeted", "partial")]
#[case::targeted_failed("targeted", "failed")]
#[case::targeted_foreign("targeted", "foreign")]
#[case::targeted_account("targeted", "account")]
#[case::targeted_instrument("targeted", "instrument")]
#[case::targeted_side("targeted", "side")]
#[case::startup_empty("startup", "empty")]
#[case::startup_partial("startup", "partial")]
#[tokio::test]
async fn test_terminal_reconciliation_defers_unexplained_fill_gap(
    #[case] route: &str,
    #[case] response: &str,
) {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        open_check_open_only: false,
        open_check_missing_retries: 1,
        open_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    });

    ctx.add_instrument(test_instrument());
    let client_order_id = ClientOrderId::from("O-TERMINAL-GAP");
    let venue_order_id = VenueOrderId::from("V-TERMINAL-GAP");
    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, test_client_id());
    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        test_instrument_id(),
        OrderStatus::Canceled,
        Quantity::from("10.0"),
        Quantity::from("2.0"),
    )
    .with_price(Price::from("100.0"))
    .with_avg_px(dec!(3000.0));
    let first = create_fill_report(
        client_order_id,
        venue_order_id,
        test_instrument_id(),
        TradeId::from("T-GAP-1"),
        "1.0",
    );
    let second = create_fill_report(
        client_order_id,
        venue_order_id,
        test_instrument_id(),
        TradeId::from("T-GAP-2"),
        "1.0",
    );

    let initial_fills = match response {
        "zero" => {
            let mut zero = first.clone();
            zero.last_qty = Quantity::zero(1);
            zero.commission = Money::from("123.45 USDT");
            vec![zero]
        }
        "partial" => vec![first.clone()],
        "foreign" | "account" | "instrument" | "side" => {
            let mut foreign = first.clone();

            match response {
                "account" => foreign.account_id = AccountId::from("OTHER-001"),
                "instrument" => foreign.instrument_id = InstrumentId::from("BTCUSDT.BINANCE"),
                "side" => foreign.order_side = OrderSide::Sell,
                _ => foreign.venue_order_id = VenueOrderId::from("V-ANOTHER-ORDER"),
            }

            vec![foreign]
        }
        _ => Vec::new(),
    };

    let client = MockExecutionClient::new(if route == "bulk" {
        vec![report.clone()]
    } else {
        Vec::new()
    })
    .with_order_report(report.clone())
    .with_fill_reports(initial_fills.clone());

    client.fail_fill_reports.set(response == "failed");

    if route == "startup" {
        ctx.manager.reconcile_execution_mass_status(
            &create_mass_status(vec![report.clone()], initial_fills),
            &ctx.exec_engine,
        );
    } else {
        let events = ctx.manager.check_open_orders(&[&client]).await;

        if response == "zero" {
            assert!(events.is_empty());
        }

        for event in events {
            ctx.exec_engine.borrow_mut().process(&event);
        }
    }

    let deferred = ctx.get_order(&client_order_id).unwrap();

    let retry = MockExecutionClient::new(if route == "bulk" {
        vec![report.clone()]
    } else {
        Vec::new()
    })
    .with_order_report(report.clone())
    .with_fill_reports(vec![first.clone(), second.clone()]);

    if route == "startup" {
        ctx.manager.reconcile_execution_mass_status(
            &create_mass_status(vec![report], vec![first, second]),
            &ctx.exec_engine,
        );
    } else {
        let events = ctx.manager.check_open_orders(&[&retry]).await;

        for event in events {
            ctx.exec_engine.borrow_mut().process(&event);
        }
    }

    let recovered = ctx.get_order(&client_order_id).unwrap();
    let partial = response == "partial";

    assert_eq!(
        deferred.status(),
        if partial {
            OrderStatus::PartiallyFilled
        } else {
            OrderStatus::Accepted
        }
    );
    assert_eq!(
        deferred.filled_qty(),
        Quantity::from(if partial { "1.0" } else { "0.0" })
    );

    if response == "zero" {
        assert!(deferred.trade_ids().is_empty());
        assert!(deferred.commissions().is_empty());
    }

    assert_eq!(recovered.status(), OrderStatus::Canceled);
    assert_eq!(recovered.filled_qty(), Quantity::from("2.0"));
    assert_eq!(
        recovered.trade_ids(),
        vec![&TradeId::from("T-GAP-1"), &TradeId::from("T-GAP-2")]
    );
    assert_eq!(
        recovered.commissions().get(&Currency::USDT()),
        Some(&Money::from("1.00 USDT"))
    );

    if route != "startup" {
        let queries = client.fill_report_queries.borrow();
        assert_eq!(queries.len(), 1);
        assert_eq!(queries[0].instrument_id, Some(test_instrument_id()));
        assert_eq!(queries[0].venue_order_id, Some(venue_order_id));
        assert_eq!(queries[0].start, None);
        assert_eq!(queries[0].end, None);
        assert_eq!(retry.fill_report_queries.borrow().len(), 1);
    }
}

#[rstest]
#[case::bulk_canceled(false, OrderStatus::Canceled, "4.5", OrderStatus::Canceled, 2)]
#[case::bulk_expired(false, OrderStatus::Expired, "4.5", OrderStatus::Expired, 2)]
#[case::targeted_canceled(true, OrderStatus::Canceled, "4.5", OrderStatus::Canceled, 2)]
#[case::targeted_expired(true, OrderStatus::Expired, "4.5", OrderStatus::Expired, 2)]
#[case::bulk_canceled_full(false, OrderStatus::Canceled, "10.0", OrderStatus::Filled, 1)]
#[case::bulk_expired_full(false, OrderStatus::Expired, "10.0", OrderStatus::Filled, 1)]
#[tokio::test]
async fn test_check_open_orders_terminal_report_applies_fills(
    #[case] targeted: bool,
    #[case] report_status: OrderStatus,
    #[case] filled_qty: Quantity,
    #[case] expected_status: OrderStatus,
    #[case] event_count: usize,
) {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        open_check_open_only: false,
        open_check_missing_retries: 1,
        ..Default::default()
    });

    ctx.add_instrument(test_instrument());
    let client_order_id = ClientOrderId::from("O-TERMINAL-FILL");
    let venue_order_id = VenueOrderId::from("V-TERMINAL-FILL");
    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, test_client_id());
    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        test_instrument_id(),
        report_status,
        Quantity::from("10.0"),
        filled_qty,
    )
    .with_price(Price::from("100.0"))
    .with_avg_px(dec!(101.25));
    let commission = Money::from("1.23 USDT");
    let mut reported_fill = create_fill_report(
        client_order_id,
        venue_order_id,
        test_instrument_id(),
        TradeId::from("T-TERMINAL-REPORTED"),
        "1.0",
    );
    reported_fill.last_qty = filled_qty;
    reported_fill.last_px = Price::from("101.25");
    reported_fill.commission = commission;

    let client = if targeted {
        MockExecutionClient::new(Vec::new()).with_order_report(report.clone())
    } else {
        MockExecutionClient::new(vec![report.clone()])
    }
    .with_fill_reports(vec![reported_fill]);

    client.fail_fill_reports.set(true);

    let failed = ctx.manager.check_open_orders(&[&client]).await;

    assert!(failed.is_empty());
    assert_eq!(
        ctx.get_order(&client_order_id).unwrap().status(),
        OrderStatus::Accepted
    );

    client.fail_fill_reports.set(false);
    let events = ctx.manager.check_open_orders(&[&client]).await;
    for event in &events {
        ctx.exec_engine.borrow_mut().process(event);
    }

    let order = ctx.get_order(&client_order_id).unwrap();
    let repeated_client = MockExecutionClient::new(vec![report]);
    repeated_client.fail_fill_reports.set(true);
    let repeated = ctx.manager.check_open_orders(&[&repeated_client]).await;

    assert_eq!(events.len(), event_count);

    let OrderEventAny::Filled(fill) = &events[0] else {
        panic!("Expected fill before terminal event, was {:?}", events[0]);
    };

    assert_eq!(fill.trader_id, order.trader_id());
    assert_eq!(fill.strategy_id, order.strategy_id());
    assert_eq!(fill.instrument_id, test_instrument_id());
    assert_eq!(fill.client_order_id, client_order_id);
    assert_eq!(fill.venue_order_id, venue_order_id);
    assert_eq!(fill.account_id, test_account_id());
    assert_eq!(fill.order_side, OrderSide::Buy);
    assert_eq!(fill.order_type, OrderType::Limit);
    assert_eq!(fill.last_qty, filled_qty);
    assert_eq!(fill.last_px, Price::from("101.25"));
    assert_eq!(fill.commission, Some(commission));
    assert_eq!(fill.trade_id, TradeId::from("T-TERMINAL-REPORTED"));
    assert_eq!(order.status(), expected_status);
    assert_eq!(order.filled_qty(), filled_qty);
    assert_eq!(
        order.commissions().get(&Currency::USDT()),
        Some(&commission)
    );
    assert!(repeated.is_empty());
}

#[rstest]
#[case::client_id(true)]
#[case::venue_id(false)]
#[tokio::test]
async fn test_check_open_orders_skips_unknown_report_and_processes_next(
    #[case] has_client_id: bool,
) {
    let mut ctx = TestContext::new();
    ctx.add_instrument(test_instrument());
    let client_order_id = ClientOrderId::from("O-KNOWN");
    let venue_order_id = VenueOrderId::from("V-KNOWN");
    let unknown_client_order_id = ClientOrderId::from("O-UNKNOWN");
    let unknown_venue_order_id = VenueOrderId::from("V-UNKNOWN");
    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, test_client_id());
    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        test_instrument_id(),
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("4.5"),
    )
    .with_avg_px(dec!(101.25));
    let mut unknown_report = report.clone();
    unknown_report.client_order_id = has_client_id.then_some(unknown_client_order_id);
    unknown_report.venue_order_id = unknown_venue_order_id;
    let commission = Money::from("1.23 USDT");
    let client = MockExecutionClient::new(vec![unknown_report, report])
        .with_commission(commission, Rc::new(Cell::new(false)));

    let events = ctx.manager.check_open_orders(&[&client]).await;
    for event in &events {
        ctx.exec_engine.borrow_mut().process(event);
    }

    let order = ctx.get_order(&client_order_id).unwrap();
    let cache = ctx.cache.borrow();

    assert_eq!(events.len(), 1);

    let OrderEventAny::Filled(fill) = &events[0] else {
        panic!("Expected fill for known order, was {:?}", events[0]);
    };

    assert_eq!(fill.client_order_id, client_order_id);
    assert_eq!(fill.venue_order_id, venue_order_id);
    assert_eq!(fill.last_qty, Quantity::from("4.5"));
    assert_eq!(fill.last_px, Price::from("101.25"));
    assert_eq!(fill.commission, Some(commission));
    assert_eq!(order.status(), OrderStatus::PartiallyFilled);
    assert_eq!(order.filled_qty(), Quantity::from("4.5"));
    assert_eq!(order.avg_px(), Some(dec!(101.25)));
    assert_eq!(
        order.commissions().get(&Currency::USDT()),
        Some(&commission)
    );
    assert_eq!(cache.orders(None, None, None, None, None).len(), 1);
    assert_eq!(cache.client_order_id(&unknown_venue_order_id), None);
    assert!(cache.order(&unknown_client_order_id).is_none());
    assert_eq!(client.order_report_query_count.get(), 0);
    assert_eq!(
        ctx.manager
            .recon_check_retry_count(&unknown_client_order_id),
        0
    );
}

#[rstest]
#[case::client_id(true, true)]
#[case::venue_id(true, false)]
#[case::instrument(false, true)]
#[tokio::test]
async fn test_check_open_orders_skips_excluded_reports(
    #[case] exclude_client_order: bool,
    #[case] has_client_id: bool,
) {
    let client_order_id = ClientOrderId::from("O-EXCLUDED-REPORT");
    let venue_order_id = VenueOrderId::from("V-EXCLUDED-REPORT");

    let mut config = ExecutionManagerConfig {
        open_check_open_only: false,
        open_check_missing_retries: 1,
        ..Default::default()
    };

    if exclude_client_order {
        config.filtered_client_order_ids.insert(client_order_id);
    } else {
        config
            .reconciliation_instrument_ids
            .insert(test_instrument_id2());
    }

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());
    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, test_client_id());
    let report = create_order_report(
        has_client_id.then_some(client_order_id),
        venue_order_id,
        test_instrument_id(),
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("4.5"),
    )
    .with_avg_px(dec!(101.25));
    let client = MockExecutionClient::new(vec![report]);

    let events = ctx.manager.check_open_orders(&[&client]).await;
    let order = ctx.get_order(&client_order_id).unwrap();

    assert!(events.is_empty());
    assert_eq!(order.status(), OrderStatus::Accepted);
    assert_eq!(order.filled_qty(), Quantity::from("0.0"));
    assert_eq!(client.order_report_query_count.get(), 0);
    assert_eq!(ctx.manager.recon_check_retry_count(&client_order_id), 0);
}

#[tokio::test]
async fn test_check_open_orders_skips_excluded_missing_order() {
    let client_order_id = ClientOrderId::from("O-EXCLUDED-MISSING");
    let venue_order_id = VenueOrderId::from("V-EXCLUDED-MISSING");

    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        filtered_client_order_ids: IndexSet::from([client_order_id]),
        open_check_open_only: false,
        open_check_missing_retries: 1,
        ..Default::default()
    });

    ctx.add_instrument(test_instrument());
    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, test_client_id());
    let client = MockExecutionClient::new(Vec::new());

    let events = ctx.manager.check_open_orders(&[&client]).await;

    assert!(events.is_empty());
    assert_eq!(client.order_report_query_count.get(), 0);
    assert_eq!(ctx.manager.recon_check_retry_count(&client_order_id), 0);
    assert_eq!(
        ctx.get_order(&client_order_id).unwrap().status(),
        OrderStatus::Accepted
    );
}

#[rstest]
#[case::resolve_locally(SubmissionRecoveryPolicy::ResolveLocally, false, 1)]
#[case::retain_unresolved(SubmissionRecoveryPolicy::RetainUnresolved, false, 1)]
#[case::retain_unresolved_multiple_checks(SubmissionRecoveryPolicy::RetainUnresolved, false, 3)]
#[case::client_required(SubmissionRecoveryPolicy::ResolveLocally, true, 1)]
#[tokio::test]
async fn test_check_open_orders_submitted_missing_at_venue_obeys_recovery_policy(
    #[case] policy: SubmissionRecoveryPolicy,
    #[case] client_retention: bool,
    #[case] budget: u32,
) {
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: budget,
        open_check_open_only: false,
        submission_recovery_policy: policy,
        single_order_query_delay_ms: 0,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    let order = create_limit_order(
        "O-001",
        test_instrument_id(),
        OrderSide::Buy,
        "10.0",
        "100.0",
    );
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    ctx.add_order(order);
    ctx.cache.borrow_mut().update_order(&submitted).unwrap();

    // Neither bulk nor targeted reads establish the submission outcome
    let mut mock_client = MockExecutionClient::new(vec![]);
    mock_client.retain_unresolved_submissions = client_retention;
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    for _ in 1..budget {
        assert!(ctx.manager.check_open_orders(&clients).await.is_empty());
        assert!(
            ctx.manager
                .take_submission_recovery_exhaustions()
                .is_empty()
        );
        assert_eq!(mock_client.order_report_query_count.get(), 0);
    }
    let events = ctx.manager.check_open_orders(&clients).await;

    let client_order_id = ClientOrderId::from("O-001");
    let order = ctx.get_order(&client_order_id).unwrap();

    let expected = if policy == SubmissionRecoveryPolicy::RetainUnresolved || client_retention {
        vec![SubmissionRecoveryExhausted {
            trader_id: order.trader_id(),
            client_id: Some(test_client_id()),
            strategy_id: order.strategy_id(),
            instrument_id: order.instrument_id(),
            client_order_id,
            source: SubmissionRecoverySource::MissingOrder,
            retry_count: budget,
            ts_event: ctx.clock.borrow().timestamp_ns(),
        }]
    } else {
        Vec::new()
    };
    assert_eq!(ctx.manager.take_submission_recovery_exhaustions(), expected);
    assert!(
        ctx.manager
            .take_submission_recovery_exhaustions()
            .is_empty()
    );

    if policy == SubmissionRecoveryPolicy::RetainUnresolved || client_retention {
        let queries_before = mock_client.order_report_query_count.get();
        let repeated = ctx.manager.check_open_orders(&clients).await;
        assert!(events.is_empty());
        assert!(repeated.is_empty());
        assert_eq!(mock_client.order_report_query_count.get(), queries_before);
        assert_eq!(
            ctx.cache
                .borrow()
                .order(&ClientOrderId::from("O-001"))
                .unwrap()
                .status(),
            OrderStatus::Submitted
        );
        return;
    }

    assert_eq!(events.len(), 1);
    assert_eq!(mock_client.order_report_query_count.get(), 1);

    if let OrderEventAny::Rejected(rejected) = &events[0] {
        assert_eq!(rejected.client_order_id, ClientOrderId::from("O-001"));
        assert_eq!(rejected.reason, "NOT_FOUND_AT_VENUE");
    } else {
        panic!("Expected OrderRejected event, was {:?}", events[0]);
    }
}

#[rstest]
#[case::accepted("accepted", 1)]
#[case::targeted_error("targeted_error", 1)]
#[case::wrong_identity("wrong_identity", 1)]
#[case::bulk_error("bulk_error", 0)]
#[case::open_only("open_only", 0)]
#[case::query_cap("query_cap", 0)]
#[tokio::test]
async fn test_submission_registry_missing_checks_do_not_exhaust_without_absence(
    #[case] scenario: &str,
    #[case] query_count: usize,
) {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 1,
        open_check_open_only: scenario == "open_only",
        max_single_order_queries_per_cycle: u32::from(scenario != "query_cap"),
        single_order_query_delay_ms: 0,
        submission_recovery_policy: SubmissionRecoveryPolicy::RetainUnresolved,
        ..Default::default()
    });
    ctx.add_instrument(test_instrument());
    let order = create_limit_order(
        "O-REGISTRY-MISSING",
        test_instrument_id(),
        OrderSide::Buy,
        "10.0",
        "100.0",
    );
    let client_order_id = order.client_order_id();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    ctx.manager
        .register_submission(order.init_event(), Some(test_client_id()));
    ctx.add_order(order);
    ctx.cache.borrow_mut().update_order(&submitted).unwrap();
    let client = match scenario {
        "accepted" | "wrong_identity" => MockExecutionClient::new(Vec::new()).with_order_report(
            create_order_report(
                Some(if scenario == "accepted" {
                    client_order_id
                } else {
                    ClientOrderId::from("O-OTHER")
                }),
                VenueOrderId::from("V-REGISTRY-MISSING"),
                test_instrument_id(),
                OrderStatus::Accepted,
                Quantity::from("10.0"),
                Quantity::zero(1),
            )
            .with_price(Price::from("100.0")),
        ),
        "targeted_error" => MockExecutionClient::new(Vec::new()).with_failed_order_report(),
        "bulk_error" => MockExecutionClient::failing(test_client_id(), test_venue()),
        _ => MockExecutionClient::new(Vec::new()),
    };

    let events = ctx.manager.check_open_orders(&[&client]).await;

    assert_eq!(client.order_report_query_count.get(), query_count);
    if scenario == "accepted" {
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], OrderEventAny::Accepted(accepted)
            if accepted.client_order_id == client_order_id));
    } else {
        assert!(events.is_empty());
    }
    assert_eq!(
        ctx.get_order(&client_order_id).unwrap().status(),
        OrderStatus::Submitted
    );
    assert!(
        ctx.manager
            .take_submission_recovery_exhaustions()
            .is_empty()
    );
}

#[rstest]
#[tokio::test]
async fn test_check_open_orders_targeted_query_prevents_false_rejection() {
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 1,
        open_check_open_only: false,
        single_order_query_delay_ms: 0,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-TARGETED-FOUND");
    let venue_order_id = VenueOrderId::from("V-TARGETED-FOUND");
    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, test_client_id());

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        test_instrument_id(),
        OrderStatus::Accepted,
        Quantity::from("10.0"),
        Quantity::zero(1),
    )
    .with_price(Price::from("100.0"));
    let mock_client = MockExecutionClient::new(Vec::new()).with_order_report(report);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_open_orders(&clients).await;

    assert!(events.is_empty());
    assert_eq!(mock_client.order_report_query_count.get(), 1);
    assert_eq!(ctx.manager.recon_check_retry_count(&client_order_id), 0);
}

#[rstest]
#[tokio::test]
async fn test_check_open_orders_targeted_query_error_defers_resolution() {
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 1,
        open_check_open_only: false,
        single_order_query_delay_ms: 0,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-TARGETED-ERROR");
    let venue_order_id = VenueOrderId::from("V-TARGETED-ERROR");
    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, test_client_id());

    let mock_client = MockExecutionClient::new(Vec::new()).with_failed_order_report();
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_open_orders(&clients).await;

    assert!(events.is_empty());
    assert_eq!(mock_client.order_report_query_count.get(), 1);
    assert_eq!(ctx.manager.recon_check_retry_count(&client_order_id), 1);
}

#[rstest]
#[tokio::test]
async fn test_check_open_orders_mismatched_targeted_report_defers_resolution() {
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 1,
        open_check_open_only: false,
        single_order_query_delay_ms: 0,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-TARGETED-MISMATCH");
    let venue_order_id = VenueOrderId::from("V-TARGETED-MISMATCH");
    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, test_client_id());

    let report = create_order_report(
        Some(ClientOrderId::from("O-OTHER")),
        VenueOrderId::from("V-OTHER"),
        test_instrument_id(),
        OrderStatus::Accepted,
        Quantity::from("10.0"),
        Quantity::zero(1),
    )
    .with_price(Price::from("100.0"));
    let mock_client = MockExecutionClient::new(Vec::new()).with_order_report(report);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_open_orders(&clients).await;

    assert!(events.is_empty());
    assert_eq!(mock_client.order_report_query_count.get(), 1);
    assert_eq!(ctx.manager.recon_check_retry_count(&client_order_id), 1);
}

#[rstest]
#[tokio::test]
async fn test_check_open_orders_caps_targeted_queries_per_cycle() {
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 1,
        open_check_open_only: false,
        max_single_order_queries_per_cycle: 1,
        single_order_query_delay_ms: 0,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    let client_a = ClientId::from("CLIENT-A");
    let client_b = ClientId::from("CLIENT-B");
    insert_accepted_limit_order(
        &ctx,
        ClientOrderId::from("O-TARGETED-A"),
        VenueOrderId::from("V-TARGETED-A"),
        client_a,
    );
    insert_accepted_limit_order(
        &ctx,
        ClientOrderId::from("O-TARGETED-B"),
        VenueOrderId::from("V-TARGETED-B"),
        client_b,
    );

    let mock_a = MockExecutionClient::for_venue(client_a, test_venue(), Vec::new());
    let mock_b = MockExecutionClient::for_venue(client_b, test_venue(), Vec::new());
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_a, &mock_b];

    let events = ctx.manager.check_open_orders(&clients).await;

    assert_eq!(events.len(), 1);
    assert_eq!(mock_a.order_report_query_count.get(), 1);
    assert_eq!(mock_b.order_report_query_count.get(), 0);
}

#[rstest]
#[tokio::test]
async fn test_check_open_orders_queries_oversized_responsible_client_group() {
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 1,
        open_check_open_only: false,
        max_single_order_queries_per_cycle: 1,
        single_order_query_delay_ms: 0,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-TARGETED-MULTI");
    let venue_order_id = VenueOrderId::from("V-TARGETED-MULTI");
    let order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .instrument_id(test_instrument_id())
        .quantity(Quantity::from("10.0"))
        .price(Price::from("100.0"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    ctx.cache
        .borrow_mut()
        .add_order(order, None, None, false)
        .unwrap();
    let order = ctx.cache.borrow_mut().update_order(&submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    ctx.cache.borrow_mut().update_order(&accepted).unwrap();

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        test_instrument_id(),
        OrderStatus::Accepted,
        Quantity::from("10.0"),
        Quantity::zero(1),
    )
    .with_price(Price::from("100.0"));
    let mock_a =
        MockExecutionClient::for_venue(ClientId::from("CLIENT-A"), test_venue(), Vec::new());
    let mock_b =
        MockExecutionClient::for_venue(ClientId::from("CLIENT-B"), test_venue(), Vec::new())
            .with_order_report(report);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_a, &mock_b];

    let events = ctx.manager.check_open_orders(&clients).await;

    assert!(events.is_empty());
    assert_eq!(
        (
            mock_a.order_report_query_count.get(),
            mock_b.order_report_query_count.get(),
            ctx.manager.recon_check_retry_count(&client_order_id),
        ),
        (1, 1, 0),
    );
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_check_open_orders_spaces_targeted_queries() {
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 1,
        open_check_open_only: false,
        max_single_order_queries_per_cycle: 2,
        single_order_query_delay_ms: 100,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    let client_a = ClientId::from("CLIENT-A");
    let client_b = ClientId::from("CLIENT-B");
    insert_accepted_limit_order(
        &ctx,
        ClientOrderId::from("O-TARGETED-A"),
        VenueOrderId::from("V-TARGETED-A"),
        client_a,
    );
    insert_accepted_limit_order(
        &ctx,
        ClientOrderId::from("O-TARGETED-B"),
        VenueOrderId::from("V-TARGETED-B"),
        client_b,
    );

    let query_times = Rc::new(RefCell::new(Vec::new()));

    let mock_a = MockExecutionClient::for_venue(client_a, test_venue(), Vec::new())
        .with_on_order_report_query(Box::new({
            let query_times = query_times.clone();
            move || query_times.borrow_mut().push(dst::time::Instant::now())
        }));

    let mock_b = MockExecutionClient::for_venue(client_b, test_venue(), Vec::new())
        .with_on_order_report_query(Box::new({
            let query_times = query_times.clone();
            move || query_times.borrow_mut().push(dst::time::Instant::now())
        }));

    let clients: Vec<&dyn ExecutionClient> = vec![&mock_a, &mock_b];

    let events = ctx.manager.check_open_orders(&clients).await;
    let query_times = query_times.borrow();

    assert_eq!(events.len(), 2);
    assert_eq!(query_times.len(), 2);
    assert!(query_times[1].duration_since(query_times[0]) >= dst::time::Duration::from_millis(100));
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_check_open_orders_partially_filled_missing_at_venue_generates_canceled() {
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 1,
        open_check_open_only: false,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    ctx.add_instrument(instrument.clone());

    let client_order_id = ClientOrderId::from("O-PARTIAL-MISSING");
    let venue_order_id = VenueOrderId::from("V-PARTIAL-MISSING");
    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, test_client_id());
    let order = ctx.get_order(&client_order_id).unwrap();
    let fill = OrderFilledTestBuilder::new(&order, &instrument)
        .last_qty(Quantity::from("4.0"))
        .without_position_id()
        .build();
    ctx.cache.borrow_mut().update_order(&fill).unwrap();

    let mock_client = MockExecutionClient::new(vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_open_orders(&clients).await;

    assert_eq!(events.len(), 1);
    assert!(
        matches!(&events[0], OrderEventAny::Canceled(canceled) if canceled.client_order_id == client_order_id),
        "expected OrderCanceled event, was {:?}",
        events[0],
    );

    ctx.cache.borrow_mut().update_order(&events[0]).unwrap();
    let order = ctx.get_order(&client_order_id).unwrap();
    assert_eq!(order.status(), OrderStatus::Canceled);
    assert_eq!(order.filled_qty(), Quantity::from("4.0"));
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_check_open_orders_failed_client_does_not_advance_missing_retries() {
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 2,
        open_check_open_only: false,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());
    ctx.add_instrument(test_instrument2());

    let healthy_order_id = ClientOrderId::from("O-HEALTHY-MISSING");
    let failed_order_id = ClientOrderId::from("O-FAILED-VENUE");
    let healthy_client_id = test_client_id();
    let failed_client_id = ClientId::from("BYBIT");
    let failed_venue = Venue::from("BYBIT");

    insert_accepted_limit_order(
        &ctx,
        healthy_order_id,
        VenueOrderId::from("V-HEALTHY-MISSING"),
        healthy_client_id,
    );
    insert_accepted_limit_order_for_instrument(
        &ctx,
        failed_order_id,
        VenueOrderId::from("V-FAILED-VENUE"),
        test_instrument_id2(),
        failed_client_id,
    );

    let healthy_client = MockExecutionClient::for_venue(healthy_client_id, test_venue(), vec![]);
    let failed_client = MockExecutionClient::failing(failed_client_id, failed_venue);
    let clients: Vec<&dyn ExecutionClient> = vec![&healthy_client, &failed_client];

    let first_events = ctx.manager.check_open_orders(&clients).await;

    assert!(first_events.is_empty());

    advance_clock(dst::time::Duration::from_millis(1)).await;

    let recovered_client = MockExecutionClient::for_venue(failed_client_id, failed_venue, vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&healthy_client, &recovered_client];
    let second_events = ctx.manager.check_open_orders(&clients).await;

    assert_eq!(second_events.len(), 1);
    assert!(
        matches!(&second_events[0], OrderEventAny::Rejected(rejected) if rejected.client_order_id == healthy_order_id),
        "only the healthy venue order should exhaust its retry budget",
    );
    assert_eq!(
        ctx.get_order(&failed_order_id).unwrap().status(),
        OrderStatus::Accepted,
    );
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_check_open_orders_failed_routing_client_does_not_resolve_exchange_order() {
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 1,
        open_check_open_only: false,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let client_order_id = ClientOrderId::from("O-FAILED-ROUTING");
    let routing_client_id = ClientId::from("IB");
    ctx.add_instrument(instrument);
    insert_accepted_limit_order(
        &ctx,
        client_order_id,
        VenueOrderId::from("V-FAILED-ROUTING"),
        routing_client_id,
    );

    let routing_client = MockExecutionClient::failing(routing_client_id, Venue::from("IB"))
        .with_account_id(AccountId::from("IB-001"))
        .with_handled_venues(IndexSet::from([instrument_id.venue]));
    let clients: Vec<&dyn ExecutionClient> = vec![&routing_client];

    let events = ctx.manager.check_open_orders(&clients).await;

    assert!(events.is_empty());
    assert_eq!(
        ctx.get_order(&client_order_id).unwrap().status(),
        OrderStatus::Accepted,
    );
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_check_open_orders_failed_client_does_not_suppress_healthy_client_same_venue() {
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 1,
        open_check_open_only: false,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    let failed_order_id = ClientOrderId::from("O-FAILED-SHARED-VENUE");
    let healthy_order_id = ClientOrderId::from("O-HEALTHY-SHARED-VENUE");
    let failed_client_id = ClientId::from("SHARED-FAILED");
    let healthy_client_id = ClientId::from("SHARED-HEALTHY");
    insert_accepted_limit_order(
        &ctx,
        failed_order_id,
        VenueOrderId::from("V-FAILED-SHARED-VENUE"),
        failed_client_id,
    );
    insert_accepted_limit_order(
        &ctx,
        healthy_order_id,
        VenueOrderId::from("V-HEALTHY-SHARED-VENUE"),
        healthy_client_id,
    );

    let failed_client = MockExecutionClient::failing(failed_client_id, test_venue());
    let healthy_client = MockExecutionClient::for_venue(healthy_client_id, test_venue(), vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&failed_client, &healthy_client];

    let events = ctx.manager.check_open_orders(&clients).await;

    assert_eq!(events.len(), 1);
    assert!(
        matches!(&events[0], OrderEventAny::Rejected(rejected) if rejected.client_order_id == healthy_order_id),
        "only the healthy-client order should be rejected",
    );
    assert_eq!(
        ctx.get_order(&failed_order_id).unwrap().status(),
        OrderStatus::Accepted,
    );
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_check_open_orders_failed_routing_client_fallback_coverage_does_not_advance() {
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 1,
        open_check_open_only: false,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let client_order_id = ClientOrderId::from("O-FAILED-ROUTING-FALLBACK");
    ctx.add_instrument(instrument);

    let order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .quantity(Quantity::from("10.0"))
        .price(Price::from("100.0"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    ctx.add_order(order);
    let order = ctx.cache.borrow_mut().update_order(&submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(
        &order,
        test_account_id(),
        VenueOrderId::from("V-FAILED-ROUTING-FALLBACK"),
    );
    ctx.cache.borrow_mut().update_order(&accepted).unwrap();

    let routing_client = MockExecutionClient::failing(ClientId::from("IB"), Venue::from("IB"))
        .with_account_id(AccountId::from("IB-001"))
        .with_handled_venues(IndexSet::from([instrument_id.venue]));
    let clients: Vec<&dyn ExecutionClient> = vec![&routing_client];

    let events = ctx.manager.check_open_orders(&clients).await;

    assert!(events.is_empty());
    assert_eq!(
        ctx.get_order(&client_order_id).unwrap().status(),
        OrderStatus::Accepted,
    );
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_check_open_orders_positive_report_resets_missing_retry_ladder() {
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 2,
        open_check_open_only: false,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-RETRY-RESET");
    let venue_order_id = VenueOrderId::from("V-RETRY-RESET");
    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, test_client_id());

    let empty_client = MockExecutionClient::new(vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&empty_client];
    let first_events = ctx.manager.check_open_orders(&clients).await;

    assert!(first_events.is_empty());
    assert_eq!(ctx.manager.recon_check_retry_count(&client_order_id), 1);

    advance_clock(dst::time::Duration::from_millis(1)).await;

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        test_instrument_id(),
        OrderStatus::Accepted,
        Quantity::from("10.0"),
        Quantity::from("0.0"),
    );
    let reporting_client = MockExecutionClient::new(vec![report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&reporting_client];
    let second_events = ctx.manager.check_open_orders(&clients).await;

    assert!(
        !second_events
            .iter()
            .any(|e| matches!(e, OrderEventAny::Rejected(_) | OrderEventAny::Canceled(_))),
        "a positive matching report must not resolve the order",
    );
    assert_eq!(ctx.manager.recon_check_retry_count(&client_order_id), 0);

    advance_clock(dst::time::Duration::from_millis(1)).await;

    let empty_again = MockExecutionClient::new(vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&empty_again];
    let third_events = ctx.manager.check_open_orders(&clients).await;

    assert!(
        third_events.is_empty(),
        "non-consecutive misses must not exhaust the retry budget",
    );
    assert_eq!(ctx.manager.recon_check_retry_count(&client_order_id), 1);
    assert_eq!(
        ctx.get_order(&client_order_id).unwrap().status(),
        OrderStatus::Accepted,
    );
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_check_open_orders_venue_id_only_report_resets_missing_retry_ladder() {
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 2,
        open_check_open_only: false,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-VENUE-ID-ONLY");
    let venue_order_id = VenueOrderId::from("V-VENUE-ID-ONLY");
    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, test_client_id());

    let empty_client = MockExecutionClient::new(vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&empty_client];
    let first_events = ctx.manager.check_open_orders(&clients).await;

    assert!(first_events.is_empty());
    assert_eq!(ctx.manager.recon_check_retry_count(&client_order_id), 1);

    advance_clock(dst::time::Duration::from_millis(1)).await;

    // The report carries no client order ID; the manager must map it through
    // the cached venue-order-ID index and give it full positive-report
    // bookkeeping, not walk it as missing in the same pass.
    let report = create_order_report(
        None,
        venue_order_id,
        test_instrument_id(),
        OrderStatus::Accepted,
        Quantity::from("10.0"),
        Quantity::from("0.0"),
    );
    let reporting_client = MockExecutionClient::new(vec![report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&reporting_client];
    let second_events = ctx.manager.check_open_orders(&clients).await;

    assert!(
        !second_events
            .iter()
            .any(|e| matches!(e, OrderEventAny::Rejected(_) | OrderEventAny::Canceled(_))),
        "a venue-ID-only positive report must not resolve the order",
    );
    assert_eq!(ctx.manager.recon_check_retry_count(&client_order_id), 0);

    advance_clock(dst::time::Duration::from_millis(1)).await;

    let empty_again = MockExecutionClient::new(vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&empty_again];
    let third_events = ctx.manager.check_open_orders(&clients).await;

    assert!(
        third_events.is_empty(),
        "a single miss after a venue-ID-only positive report must not reject",
    );
    assert_eq!(ctx.manager.recon_check_retry_count(&client_order_id), 1);
    assert_eq!(
        ctx.get_order(&client_order_id).unwrap().status(),
        OrderStatus::Accepted,
    );
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_check_open_orders_order_closed_during_query_leaves_no_retry_state() {
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 2,
        open_check_open_only: false,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-CLOSED-DURING-QUERY");
    let venue_order_id = VenueOrderId::from("V-CLOSED-DURING-QUERY");
    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, test_client_id());

    // The callback fires inside the report query, after the prepare-time
    // snapshot captured the order as an open missing-candidate.
    let cache = ctx.cache.clone();

    let client =
        MockExecutionClient::new(vec![]).with_on_order_reports_query(Box::new(move || {
            let order = cache.borrow().order(&client_order_id).unwrap().clone();
            let canceled =
                TestOrderEventStubs::canceled(&order, test_account_id(), Some(venue_order_id));
            cache.borrow_mut().update_order(&canceled).unwrap();
        }));

    let clients: Vec<&dyn ExecutionClient> = vec![&client];

    let events = ctx.manager.check_open_orders(&clients).await;

    assert!(events.is_empty());
    assert_eq!(
        ctx.manager.recon_check_retry_count(&client_order_id),
        0,
        "an order that closed during the query must leave no retry state behind",
    );
    assert_eq!(
        ctx.get_order(&client_order_id).unwrap().status(),
        OrderStatus::Canceled,
    );
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_check_open_orders_deferred_pending_order_keeps_inflight_registration() {
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 1,
        open_check_open_only: false,
        inflight_threshold_ms: 100,
        inflight_max_retries: 2,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-PENDING-DEFER");
    let venue_order_id = VenueOrderId::from("V-PENDING-DEFER");
    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, test_client_id());
    let order = ctx.get_order(&client_order_id).unwrap();
    let pending = OrderEventAny::PendingCancel(
        OrderPendingCancelSpec::builder()
            .trader_id(order.trader_id())
            .strategy_id(order.strategy_id())
            .instrument_id(order.instrument_id())
            .client_order_id(client_order_id)
            .account_id(test_account_id())
            .venue_order_id(venue_order_id)
            .build(),
    );
    ctx.cache.borrow_mut().update_order(&pending).unwrap();
    assert_eq!(
        ctx.get_order(&client_order_id).unwrap().status(),
        OrderStatus::PendingCancel,
    );
    ctx.manager.register_inflight(client_order_id);

    // Prime one inflight retry so the retained check carries a nonzero count
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;
    let primed = ctx.manager.check_inflight_orders();
    assert!(primed.events.is_empty());
    assert_eq!(primed.queries.len(), 1);

    // Missing at venue at max retries: the inflight status defers resolution
    let mock_client = MockExecutionClient::new(vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];
    let events = ctx.manager.check_open_orders(&clients).await;
    assert!(events.is_empty());

    // The deferral must not unregister the order from inflight recovery, and
    // must reset the inflight retry ladder: past the threshold the checker
    // queries again from scratch instead of escalating to a stale-count
    // cancellation
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;
    let result = ctx.manager.check_inflight_orders();
    assert!(
        result.events.is_empty(),
        "deferral must reset the inflight retry ladder, was {:?}",
        result.events,
    );
    assert_eq!(
        result.queries.len(),
        1,
        "deferred pending order should remain registered for inflight recovery",
    );
}

#[rstest]
#[tokio::test]
async fn test_check_open_orders_open_only_missing_venue_order_does_not_reject() {
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 1,
        open_check_open_only: true,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    let order = create_limit_order(
        "O-OPEN-ONLY",
        test_instrument_id(),
        OrderSide::Buy,
        "10.0",
        "100.0",
    );
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    ctx.add_order(order);
    ctx.cache.borrow_mut().update_order(&submitted).unwrap();

    let mock_client = MockExecutionClient::new(vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_open_orders(&clients).await;

    assert!(events.is_empty());
    let cached_order = ctx.get_order(&ClientOrderId::from("O-OPEN-ONLY")).unwrap();
    assert_eq!(cached_order.status(), OrderStatus::Submitted);
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_check_open_orders_missing_gate_uses_local_activity_not_venue_ts_last() {
    // A corrupted far-future ts_last must not stall missing-order reconciliation
    // after the local activity grace expires.
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::from_millis(200),
        open_check_missing_retries: 1,
        open_check_open_only: false,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    let order = create_limit_order(
        "O-AHEAD",
        test_instrument_id(),
        OrderSide::Buy,
        "10.0",
        "100.0",
    );
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    ctx.add_order(order);
    let order = ctx.cache.borrow_mut().update_order(&submitted).unwrap();

    let future_ts = ctx
        .clock
        .borrow()
        .timestamp_ns()
        .saturating_add(DurationNanos::from_secs(10));
    let accepted = OrderEventAny::Accepted(
        OrderAcceptedSpec::builder()
            .trader_id(order.trader_id())
            .strategy_id(order.strategy_id())
            .instrument_id(order.instrument_id())
            .client_order_id(order.client_order_id())
            .venue_order_id(VenueOrderId::from("V-AHEAD"))
            .account_id(test_account_id())
            .ts_event(future_ts)
            .ts_init(future_ts)
            .build(),
    );
    let client_order_id = order.client_order_id();
    ctx.cache.borrow_mut().update_order(&accepted).unwrap();
    // Track the event exactly as the LiveNode dispatch path does: ack cleanup
    // plus the local-activity stamp, in that order.
    ctx.manager.observe_order_event(&accepted);

    let mock_client = MockExecutionClient::new(vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_open_orders(&clients).await;

    assert!(
        events.is_empty(),
        "recent local activity should defer reconciliation",
    );

    advance_clock(dst::time::Duration::from_millis(250)).await;

    let events = ctx.manager.check_open_orders(&clients).await;

    assert_eq!(events.len(), 1);
    assert!(
        matches!(&events[0], OrderEventAny::Rejected(rejected) if rejected.client_order_id == client_order_id),
        "far-future venue ts_last must not keep deferring reconciliation",
    );
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_check_open_orders_defers_for_just_accepted_order() {
    // Regression: the LiveNode dispatch path for Accepted performs ack cleanup
    // (clear_recon_tracking) and stamps local activity via observe_order_event.
    // The stamp must survive the cleanup so a just-accepted order that a
    // lagging venue report omits defers until the grace expires, rather than
    // being rejected as missing.
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::from_millis(200),
        open_check_missing_retries: 1,
        open_check_open_only: false,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    let order = create_limit_order(
        "O-ACCEPTED",
        test_instrument_id(),
        OrderSide::Buy,
        "10.0",
        "100.0",
    );
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    ctx.add_order(order);
    let order = ctx.cache.borrow_mut().update_order(&submitted).unwrap();
    let client_order_id = order.client_order_id();

    let accepted =
        TestOrderEventStubs::accepted(&order, test_account_id(), VenueOrderId::from("V-ACCEPTED"));
    ctx.cache.borrow_mut().update_order(&accepted).unwrap();
    ctx.manager.observe_order_event(&accepted);

    // Venue response lags and omits the just-accepted order
    let mock_client = MockExecutionClient::new(vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_open_orders(&clients).await;

    assert!(
        events.is_empty(),
        "a just-accepted order must defer missing-order reconciliation",
    );

    advance_clock(dst::time::Duration::from_millis(250)).await;

    let events = ctx.manager.check_open_orders(&clients).await;

    assert_eq!(events.len(), 1);
    assert!(
        matches!(&events[0], OrderEventAny::Rejected(rejected) if rejected.client_order_id == client_order_id),
        "reconciliation must proceed once the local-activity grace expires",
    );
}

#[tokio::test]
async fn test_position_check_reconciles_venue_only_nonflat_report() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();

    ctx.add_instrument(instrument);

    let venue_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("3.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3000.00)),
    );
    let mock_client = MockPositionExecutionClient::new(vec![], vec![venue_report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(
        events
            .iter()
            .any(|event| matches!(event, OrderEventAny::Filled(_))),
        "Expected a synthetic fill for a non-flat venue-only position report"
    );
}

#[rstest]
#[case::venue_only(false, PositionSide::Long)]
#[case::increase(true, PositionSide::Long)]
#[case::cross_zero(true, PositionSide::Short)]
#[tokio::test]
async fn test_position_check_respects_disabled_order_generation(
    #[case] has_position: bool,
    #[case] venue_side: PositionSide,
) {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        generate_missing_orders: false,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    });

    let instrument = test_instrument();
    let position_id = PositionId::from("P-GENERATION-DISABLED");
    let position = create_test_position(&instrument, position_id, OrderSide::Buy, "3.0", "3000.00");
    ctx.add_instrument(instrument);

    if has_position {
        ctx.add_position(&position);
    }

    let report = PositionStatusReport::new(
        test_account_id(),
        test_instrument_id(),
        venue_side,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3100.00)),
    );
    let client = MockPositionExecutionClient::new(Vec::new(), vec![report]);

    let events = ctx.manager.check_positions_consistency(&[&client]).await;
    let cache = ctx.cache.borrow();

    assert!(events.is_empty());
    assert!(cache.orders(None, None, None, None, None).is_empty());
    assert_eq!(
        cache.position(&position_id).as_deref(),
        has_position.then_some(&position),
    );
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(test_instrument_id(), test_account_id())),
        0
    );
}

#[tokio::test]
async fn test_position_check_updates_reported_hedge_position() {
    let config = ExecutionManagerConfig {
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let position_id = PositionId::from("P-CONTINUOUS-HEDGE-LONG");
    let position = create_test_position(&instrument, position_id, OrderSide::Buy, "5.0", "3000.00");

    let report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("7.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(position_id),
        Some(dec!(3100.00)),
    );
    ctx.add_instrument(instrument);
    ctx.add_position(&position);
    let client = MockPositionExecutionClient::new(vec![], vec![report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    let fill = events
        .iter()
        .find_map(|event| match event {
            OrderEventAny::Filled(fill) => Some(fill),
            _ => None,
        })
        .expect("reconciliation fill is emitted");

    assert_eq!(fill.position_id, Some(position_id));

    for event in &events {
        ctx.exec_engine.borrow_mut().process(event);
    }

    let cache = ctx.cache.borrow();
    let positions = cache.positions_open(
        None,
        Some(&instrument_id),
        None,
        Some(&test_account_id()),
        None,
    );
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].id, position_id);
    assert_eq!(positions[0].signed_decimal_qty(), dec!(7.0));
}

#[tokio::test]
async fn test_position_check_cross_zero_preserves_both_hedge_position_ids() {
    let config = ExecutionManagerConfig {
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let close_position_id = PositionId::from("P-CONTINUOUS-HEDGE-LONG");
    let open_position_id = PositionId::from("P-CONTINUOUS-HEDGE-SHORT");
    let position = create_test_position(
        &instrument,
        close_position_id,
        OrderSide::Buy,
        "5.0",
        "3000.00",
    );

    let report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Short,
        Quantity::from("3.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(open_position_id),
        Some(dec!(3100.00)),
    );
    ctx.add_instrument(instrument);
    ctx.add_position(&position);
    let client = MockPositionExecutionClient::new(vec![], vec![report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    let fills = events
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => Some(fill),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(fills.len(), 2);
    assert_eq!(fills[0].position_id, Some(close_position_id));
    assert_eq!(fills[1].position_id, Some(open_position_id));

    for event in &events {
        ctx.exec_engine.borrow_mut().process(event);
    }

    let cache = ctx.cache.borrow();
    assert!(
        cache
            .position(&close_position_id)
            .is_some_and(|position| position.is_closed())
    );
    assert_eq!(
        cache
            .position(&open_position_id)
            .expect("short hedge position is created")
            .signed_decimal_qty(),
        dec!(-3.0),
    );
}

#[rstest]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_position_check_rereads_position_closed_during_request() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let position = create_test_position(
        &instrument,
        PositionId::from("P-CLOSE-DURING-REQUEST"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
    );
    let closed_position = close_test_long_position(
        position.clone(),
        &instrument,
        "5.0",
        TradeId::from("T-CLOSE-DURING-REQUEST"),
    );
    ctx.add_instrument(instrument);
    ctx.add_position(&position);

    let mock_client = MockPositionExecutionClient::new(vec![], vec![])
        .with_position_request_mutation(PositionRequestMutation::Update {
            cache: ctx.cache.clone(),
            position: closed_position,
        });

    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;
    for event in &events {
        ctx.exec_engine.borrow_mut().process(event);
    }

    let open_positions = ctx
        .cache
        .borrow()
        .positions_open(None, Some(&test_instrument_id()), None, None, None)
        .len();
    assert!(events.is_empty());
    assert_eq!(
        open_positions, 0,
        "a synthetic sell from the stale long would open a real short",
    );
}

#[rstest]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_position_check_rereads_position_opened_during_request() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let position = create_test_position(
        &instrument,
        PositionId::from("P-OPEN-DURING-REQUEST"),
        OrderSide::Buy,
        "3.0",
        "3000.00",
    );

    let venue_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("3.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3000.00)),
    );
    ctx.add_instrument(instrument);

    let mock_client = MockPositionExecutionClient::new(vec![], vec![venue_report])
        .with_position_request_mutation(PositionRequestMutation::Add {
            cache: ctx.cache.clone(),
            position,
        });

    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(
        events.is_empty(),
        "the matching report must not recreate a position opened during the request",
    );
}

#[rstest]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_position_check_uses_current_avg_px_after_request() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let position_id = PositionId::from("P-AVG-PX-DURING-REQUEST");
    let stale_position =
        create_test_position(&instrument, position_id, OrderSide::Buy, "5.0", "3000.00");
    let current_position =
        create_test_position(&instrument, position_id, OrderSide::Buy, "3.0", "3100.00");

    let venue_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("6.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3200.00)),
    );
    ctx.add_instrument(instrument);
    ctx.add_position(&stale_position);

    let mock_client = MockPositionExecutionClient::new(vec![], vec![venue_report])
        .with_position_request_mutation(PositionRequestMutation::Update {
            cache: ctx.cache.clone(),
            position: current_position,
        });

    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    let fill = events
        .iter()
        .find_map(|event| match event {
            OrderEventAny::Filled(fill) => Some(fill),
            _ => None,
        })
        .expect("expected a reconciliation fill");

    assert_eq!(fill.last_qty, Quantity::from("3.0"));
    assert_eq!(fill.last_px, Price::from("3300.00"));
}

#[rstest]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_position_check_preserves_retry_for_uncovered_position_opened_during_request() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let key = (instrument_id, test_account_id());
    let old_position = create_test_position(
        &instrument,
        PositionId::from("P-RETRY-OLD"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
    );
    ctx.add_position(&old_position);

    let initial_client = MockPositionExecutionClient::new(vec![], vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&initial_client];
    assert!(
        ctx.manager
            .check_positions_consistency(&clients)
            .await
            .is_empty()
    );
    assert_eq!(ctx.manager.position_recon_retry_count(&key), 1);

    let closed_position = close_test_long_position(
        old_position,
        &instrument,
        "5.0",
        TradeId::from("T-RETRY-CLOSE"),
    );
    ctx.cache
        .borrow_mut()
        .update_position(&closed_position)
        .unwrap();
    ctx.add_instrument(instrument.clone());

    let new_position = create_test_position(
        &instrument,
        PositionId::from("P-RETRY-NEW"),
        OrderSide::Buy,
        "3.0",
        "3100.00",
    );

    let venue_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("4.0"),
        UnixNanos::from(2_000_000),
        UnixNanos::from(2_000_000),
        None,
        None,
        Some(dec!(3200.00)),
    );

    let request_client = MockPositionExecutionClient::new(vec![], vec![venue_report])
        .with_position_request_mutation(PositionRequestMutation::Add {
            cache: ctx.cache.clone(),
            position: new_position,
        });

    let clients: Vec<&dyn ExecutionClient> = vec![&request_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(events.is_empty());
    assert_eq!(
        ctx.manager.position_recon_retry_count(&key),
        1,
        "the uncovered key must be deferred and remain active for retry pruning",
    );
}

#[tokio::test]
async fn test_position_check_retries_stops_after_max() {
    // When instrument is not in cache, check_position_discrepancy returns None
    // (can't generate fills), so retries should increment until exhausted
    let config = ExecutionManagerConfig {
        position_check_retries: 2,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let position_id = PositionId::from("P-001");

    let position = create_test_position(&instrument, position_id, OrderSide::Buy, "5.0", "3000.00");

    // Add position to cache but NOT the instrument - forces reconciliation to
    // return None on the cache.instrument() lookup
    ctx.add_position(&position);

    let mock_client = MockExecutionClient::new(vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    // First attempt: detects discrepancy, can't reconcile, retry count -> 1
    let events = ctx.manager.check_positions_consistency(&clients).await;
    assert!(events.is_empty());

    // Second attempt: retry count -> 2 (= max), logs error
    let events = ctx.manager.check_positions_consistency(&clients).await;
    assert!(events.is_empty());

    // Third attempt: retries exhausted, silently skipped
    let events = ctx.manager.check_positions_consistency(&clients).await;
    assert!(events.is_empty());
}

#[tokio::test]
async fn test_position_check_retries_clears_when_discrepancy_resolves() {
    // When reconciliation succeeds (generates events), the retry counter resets
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let position_id = PositionId::from("P-001");

    let position = create_test_position(&instrument, position_id, OrderSide::Buy, "5.0", "3000.00");

    // First: add position without instrument to force a failed retry
    ctx.add_position(&position);
    let mock_client = MockExecutionClient::new(vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;
    assert!(events.is_empty()); // Failed, retry count = 1

    // Now add instrument - reconciliation can succeed and generate events
    ctx.add_instrument(instrument.clone());

    let events = ctx.manager.check_positions_consistency(&clients).await;
    assert!(
        !events.is_empty(),
        "Expected reconciliation events when instrument is available"
    );
}

#[tokio::test]
async fn test_position_check_stale_retries_pruned_when_position_closed() {
    // When a position is closed and no longer reported by venue, the maxed
    // retry counter should be pruned so future discrepancies aren't suppressed
    let config = ExecutionManagerConfig {
        position_check_retries: 1,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let position_id = PositionId::from("P-001");

    ctx.add_instrument(instrument.clone());
    let position = create_test_position(&instrument, position_id, OrderSide::Buy, "5.0", "3000.00");
    ctx.add_position(&position);

    let mock_client = MockExecutionClient::new(vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    // First call: reconciliation succeeds (generates events to match venue=flat)
    let events = ctx.manager.check_positions_consistency(&clients).await;
    assert!(!events.is_empty());

    // Simulate that reconciliation didn't actually fix it (position still open)
    // by not applying the events. Instead, directly max out the retry counter
    // by calling again - the position is still discrepant
    ctx.manager.check_positions_consistency(&clients).await;

    // Now close the position so it disappears from open positions
    let close_order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_id)
        .side(OrderSide::Sell)
        .quantity(Quantity::from("5.0"))
        .build();
    let close_fill = TestOrderEventStubs::filled(
        &close_order,
        &instrument,
        Some(TradeId::new("T-CLOSE-001")),
        Some(position_id),
        Some(Price::from("3000.00")),
        Some(Quantity::from("5.0")),
        None,
        None,
        None,
        Some(test_account_id()),
    );
    let close_filled: OrderFilled = close_fill.into();
    let mut pos = position;
    pos.apply(&close_filled);
    ctx.cache.borrow_mut().update_position(&pos).unwrap();

    // Run consistency check with no open positions - should prune stale counter
    ctx.manager.check_positions_consistency(&clients).await;

    // Create a new position for the same instrument
    let position2 = create_test_position(
        &instrument,
        PositionId::from("P-002"),
        OrderSide::Buy,
        "3.0",
        "3100.00",
    );
    ctx.add_position(&position2);

    // Should produce events (counter was pruned, not suppressed)
    let events = ctx.manager.check_positions_consistency(&clients).await;
    assert!(
        !events.is_empty(),
        "Expected events: stale retry counter should have been pruned"
    );
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

#[tokio::test]
async fn test_position_check_uses_client_tolerance_for_missing_dust_report() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let position = create_test_position(
        &instrument,
        PositionId::from("P-DUST-MISSING"),
        OrderSide::Buy,
        "0.008007",
        "3000.00",
    );
    ctx.add_instrument(instrument);
    ctx.add_position(&position);

    let mock_client = MockPositionExecutionClient::new(vec![], vec![])
        .with_position_reconciliation_tolerance(dec!(0.009999));
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(events.is_empty());
}

#[tokio::test]
async fn test_position_check_uses_client_tolerance_for_observed_smoke_difference() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let position = create_test_position(
        &instrument,
        PositionId::from("P-SMOKE-DIFFERENCE"),
        OrderSide::Buy,
        "5.202897",
        "3000.00",
    );

    // The venue report includes a pre-existing 0.005103-share balance in addition to this order.
    let venue_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.208000"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3000.00)),
    );
    ctx.add_instrument(instrument);
    ctx.add_position(&position);

    let mock_client = MockPositionExecutionClient::new(vec![], vec![venue_report])
        .with_position_reconciliation_tolerance(dec!(0.009999));
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(events.is_empty());
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_position_check_uses_routing_client_tolerance() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let account_id = AccountId::from("IB-ROUTING-CHECK");
    let position = create_test_position_for_account(
        &instrument,
        PositionId::from("P-ROUTING-TOLERANCE-CHECK"),
        OrderSide::Buy,
        "5.000000",
        "3000.00",
        account_id,
    );

    let venue_report = PositionStatusReport::new(
        account_id,
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.005000"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3000.00)),
    );
    ctx.add_instrument(instrument);
    ctx.add_margin_account(account_id);
    ctx.add_position(&position);

    let routing_client = MockPositionExecutionClient::configured(
        ClientId::from("IB-ROUTING-CHECK"),
        account_id,
        Venue::from("IB"),
        IndexSet::from([instrument_id.venue]),
        false,
    )
    .with_position_reports(vec![venue_report])
    .with_position_reconciliation_tolerance(dec!(0.010000));
    let clients: Vec<&dyn ExecutionClient> = vec![&routing_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(events.is_empty());
}

#[rstest]
#[case::magnitude("5.000000", PositionSide::Long, "5.005000")]
#[case::flat_dust("0.005000", PositionSide::Flat, "0.000000")]
#[case::opposite_dust("0.003000", PositionSide::Short, "0.003000")]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_mass_status_netting_uses_routing_client_tolerance(
    #[case] cached_qty: &str,
    #[case] report_side: PositionSide,
    #[case] report_qty: &str,
) {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let account_id = AccountId::from("IB-ROUTING-MASS-STATUS");
    let position = create_test_position_for_account(
        &instrument,
        PositionId::from("P-ROUTING-TOLERANCE-MASS-STATUS"),
        OrderSide::Buy,
        cached_qty,
        "3000.00",
        account_id,
    );

    let matching_report = PositionStatusReport::new(
        account_id,
        instrument_id,
        PositionSide::Long,
        Quantity::from(cached_qty),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3000.00)),
    );
    ctx.add_instrument(instrument);
    ctx.add_margin_account(account_id);
    ctx.add_position(&position);

    let routing_client = MockPositionExecutionClient::configured(
        ClientId::from("IB-ROUTING-MASS-STATUS"),
        account_id,
        Venue::from("IB"),
        IndexSet::from([instrument_id.venue]),
        false,
    )
    .with_position_reports(vec![matching_report])
    .with_position_reconciliation_tolerance(dec!(0.010000));
    let clients: Vec<&dyn ExecutionClient> = vec![&routing_client];
    // Seed the manager's per-account tolerance state (production seeds this in the builder);
    // the matching report must not itself generate reconciliation events.
    let seed_events = ctx.manager.check_positions_consistency(&clients).await;
    assert!(seed_events.is_empty());

    let mut mass_status = ExecutionMassStatus::new(
        routing_client.client_id(),
        account_id,
        routing_client.venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let drift_report = PositionStatusReport::new(
        account_id,
        instrument_id,
        report_side,
        Quantity::from(report_qty),
        UnixNanos::from(2_000_000),
        UnixNanos::from(2_000_000),
        None,
        None,
        Some(dec!(3000.00)),
    );
    mass_status.add_position_reports(vec![drift_report]);

    ctx.exec_engine
        .borrow_mut()
        .register_client(Box::new(routing_client))
        .unwrap();

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert!(result.events.is_empty());
    assert!(result.unresolved_positions.is_empty());
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_routing_clients_on_same_venue_use_account_tolerances_independently() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let tolerant_account_id = AccountId::from("IB-TOLERANT");
    let strict_account_id = AccountId::from("IB-STRICT");
    let tolerant_position = create_test_position_for_account(
        &instrument,
        PositionId::from("P-ROUTING-TOLERANT"),
        OrderSide::Buy,
        "5.000000",
        "3000.00",
        tolerant_account_id,
    );
    let strict_position = create_test_position_for_account(
        &instrument,
        PositionId::from("P-ROUTING-STRICT"),
        OrderSide::Buy,
        "5.000000",
        "3000.00",
        strict_account_id,
    );

    let tolerant_report = PositionStatusReport::new(
        tolerant_account_id,
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.005000"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3000.00)),
    );

    let strict_report = PositionStatusReport::new(
        strict_account_id,
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.005000"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3000.00)),
    );
    ctx.add_instrument(instrument);
    ctx.add_margin_account(tolerant_account_id);
    ctx.add_margin_account(strict_account_id);
    ctx.add_position(&tolerant_position);
    ctx.add_position(&strict_position);

    let tolerant_client = MockPositionExecutionClient::configured(
        ClientId::from("IB-TOLERANT"),
        tolerant_account_id,
        Venue::from("IB"),
        IndexSet::from([instrument_id.venue]),
        false,
    )
    .with_position_reports(vec![tolerant_report])
    .with_position_reconciliation_tolerance(dec!(0.010000));
    let strict_client = MockPositionExecutionClient::configured(
        ClientId::from("IB-STRICT"),
        strict_account_id,
        Venue::from("IB"),
        IndexSet::from([instrument_id.venue]),
        false,
    )
    .with_position_reports(vec![strict_report])
    .with_position_reconciliation_tolerance(dec!(0.001000));
    let clients: Vec<&dyn ExecutionClient> = vec![&tolerant_client, &strict_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    let fill_account_ids = events
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => Some(fill.account_id),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(fill_account_ids, vec![strict_account_id]);
}

#[tokio::test]
async fn test_position_check_reconciles_at_client_tolerance_boundary() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let position = create_test_position(
        &instrument,
        PositionId::from("P-TOLERANCE-BOUNDARY"),
        OrderSide::Buy,
        "5.200000",
        "3000.00",
    );

    let venue_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.210000"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3000.00)),
    );
    ctx.add_instrument(instrument);
    ctx.add_position(&position);

    let mock_client = MockPositionExecutionClient::new(vec![], vec![venue_report])
        .with_position_reconciliation_tolerance(dec!(0.009999));
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(events
        .iter()
        .any(|event| matches!(event, OrderEventAny::Filled(fill) if fill.last_qty == Quantity::from("0.010000"))));
}

#[tokio::test]
async fn test_position_check_failed_client_query_skips_cached_position() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let position = create_test_position(
        &instrument,
        PositionId::from("P-FAIL-SKIP"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
    );
    ctx.add_position(&position);

    let ok_client = MockPositionExecutionClient::new(vec![], vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&ok_client];
    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(events.is_empty());
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, test_account_id())),
        1
    );

    ctx.add_instrument(instrument);
    let failing_client = MockPositionExecutionClient::failing_position_reports();
    let clients: Vec<&dyn ExecutionClient> = vec![&failing_client];
    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(
        !events
            .iter()
            .any(|event| matches!(event, OrderEventAny::Filled(_))),
        "Expected no synthetic fill when the venue position query fails"
    );
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, test_account_id())),
        1
    );
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_position_check_failed_routing_client_leaves_exchange_retry_untouched() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let account_id = AccountId::from("IB-001");
    let position = create_test_position_for_account(
        &instrument,
        PositionId::from("P-FAILED-ROUTING"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
        account_id,
    );
    ctx.add_margin_account(account_id);
    ctx.add_position(&position);

    let routing_client = MockPositionExecutionClient::configured(
        ClientId::from("IB"),
        account_id,
        Venue::from("IB"),
        IndexSet::from([instrument_id.venue]),
        true,
    );
    let clients: Vec<&dyn ExecutionClient> = vec![&routing_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(events.is_empty());
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, account_id)),
        0,
    );
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_position_check_failed_client_does_not_suppress_healthy_account_same_venue() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let failed_account_id = AccountId::from("BINANCE-FAILED");
    let healthy_account_id = AccountId::from("BINANCE-HEALTHY");
    let failed_position = create_test_position_for_account(
        &instrument,
        PositionId::from("P-FAILED-SHARED-VENUE"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
        failed_account_id,
    );
    let healthy_position = create_test_position_for_account(
        &instrument,
        PositionId::from("P-HEALTHY-SHARED-VENUE"),
        OrderSide::Buy,
        "7.0",
        "3000.00",
        healthy_account_id,
    );
    ctx.add_margin_account(failed_account_id);
    ctx.add_margin_account(healthy_account_id);
    ctx.add_position(&failed_position);
    ctx.add_position(&healthy_position);

    let failed_client = MockPositionExecutionClient::configured(
        ClientId::from("SHARED-FAILED"),
        failed_account_id,
        test_venue(),
        IndexSet::from([instrument_id.venue]),
        true,
    );
    let healthy_client = MockPositionExecutionClient::configured(
        ClientId::from("SHARED-HEALTHY"),
        healthy_account_id,
        test_venue(),
        IndexSet::from([instrument_id.venue]),
        false,
    );
    let clients: Vec<&dyn ExecutionClient> = vec![&failed_client, &healthy_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(events.is_empty());
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, failed_account_id)),
        0,
    );
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, healthy_account_id)),
        1,
    );
}

#[tokio::test]
async fn test_position_check_aggregates_hedge_positions_before_comparing_report() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();

    ctx.add_instrument(instrument.clone());
    let pos_long = create_test_position(
        &instrument,
        PositionId::from("P-NET-LONG"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
    );
    let pos_short = create_test_position(
        &instrument,
        PositionId::from("P-NET-SHORT"),
        OrderSide::Sell,
        "3.0",
        "3100.00",
    );
    ctx.add_position(&pos_long);
    ctx.add_position(&pos_short);

    let venue_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("2.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3050.00)),
    );
    let mock_client = MockPositionExecutionClient::new(vec![], vec![venue_report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(
        !events
            .iter()
            .any(|event| matches!(event, OrderEventAny::Filled(_))),
        "Expected no synthetic fill when cached hedge positions net to the venue report"
    );
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, test_account_id())),
        0
    );
}

#[rstest]
#[case::duplicates(0, false)]
#[case::distinct_snapshots(1, true)]
#[tokio::test]
async fn test_position_check_ignores_duplicate_position_reports(
    #[case] ts_last_step: u64,
    #[case] discrepant: bool,
) {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let key = (instrument_id, test_account_id());

    ctx.add_instrument(instrument.clone());
    let position = create_test_position(
        &instrument,
        PositionId::from("P-NET"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
    );
    ctx.add_position(&position);

    let reports: Vec<PositionStatusReport> = (0..2)
        .map(|i| {
            PositionStatusReport::new(
                test_account_id(),
                instrument_id,
                PositionSide::Long,
                Quantity::from("5.0"),
                UnixNanos::from(1_000_000 + i * ts_last_step),
                UnixNanos::from(2_000_000 + i),
                None,
                None,
                Some(dec!(3000.00)),
            )
        })
        .collect();

    let mock_client = MockPositionExecutionClient::new(vec![], reports.clone());
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];
    let queried_clients = IndexSet::from([mock_client.client_id()]);
    let mut check = ctx
        .manager
        .prepare_position_report_check(UUID4::new(), &clients);

    let plan = ctx.manager.plan_position_fill_reports(
        &mut check,
        &reports,
        &queried_clients,
        &IndexSet::new(),
        &clients,
    );
    let events = ctx.manager.check_positions_consistency(&clients).await;

    let discrepancy_keys = if discrepant {
        IndexSet::from([key])
    } else {
        IndexSet::new()
    };

    assert_eq!(plan.discrepancy_keys, discrepancy_keys);
    assert!(events.is_empty());
    assert_eq!(
        ctx.manager.position_recon_retry_count(&key),
        u32::from(discrepant)
    );
}

#[tokio::test]
async fn test_position_check_skips_reports_outside_reconciliation_instruments() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        reconciliation_instrument_ids: IndexSet::from([test_instrument_id()]),
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let excluded_key = (test_instrument_id2(), test_account_id());

    let report = PositionStatusReport::new(
        test_account_id(),
        test_instrument_id2(),
        PositionSide::Long,
        Quantity::from("5"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(50000.0)),
    );
    let mock_client = MockPositionExecutionClient::new(vec![], vec![report.clone()]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];
    let queried_clients = IndexSet::from([mock_client.client_id()]);
    let mut check = ctx
        .manager
        .prepare_position_report_check(UUID4::new(), &clients);

    let plan = ctx.manager.plan_position_fill_reports(
        &mut check,
        &[report],
        &queried_clients,
        &IndexSet::new(),
        &clients,
    );
    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(plan.discrepancy_keys.is_empty());
    assert!(plan.queries.is_empty());
    assert!(events.is_empty());
    assert_eq!(ctx.manager.position_recon_retry_count(&excluded_key), 0);
}

#[rstest]
#[case(false)]
#[case(true)]
#[tokio::test]
async fn test_position_check_matching_hedge_reports_is_order_invariant(
    #[case] reverse_reports: bool,
) {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();

    ctx.add_instrument(instrument.clone());
    let pos_long = create_test_position(
        &instrument,
        PositionId::from("P-MATCH-LONG"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
    );
    let pos_short = create_test_position(
        &instrument,
        PositionId::from("P-MATCH-SHORT"),
        OrderSide::Sell,
        "2.0",
        "3100.00",
    );
    ctx.add_position(&pos_long);
    ctx.add_position(&pos_short);

    let report_long = create_test_position_report(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        "5.0",
        "P-MATCH-LONG",
        dec!(3000.00),
    );
    let report_short = create_test_position_report(
        test_account_id(),
        instrument_id,
        PositionSide::Short,
        "2.0",
        "P-MATCH-SHORT",
        dec!(3100.00),
    );
    let mut reports = vec![report_long, report_short];

    if reverse_reports {
        reports.reverse();
    }

    let mock_client = MockPositionExecutionClient::new(vec![], reports);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(events.is_empty());
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, test_account_id())),
        0,
    );
}

#[tokio::test]
async fn test_position_check_equal_net_with_mismatched_hedge_legs_is_discrepant() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let pos_long = create_test_position(
        &instrument,
        PositionId::from("P-NET-MISMATCH-LONG"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
    );
    let pos_short = create_test_position(
        &instrument,
        PositionId::from("P-NET-MISMATCH-SHORT"),
        OrderSide::Sell,
        "2.0",
        "3100.00",
    );
    let report_long = create_test_position_report(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        "4.0",
        "P-NET-MISMATCH-LONG",
        dec!(3000.00),
    );
    let report_short = create_test_position_report(
        test_account_id(),
        instrument_id,
        PositionSide::Short,
        "1.0",
        "P-NET-MISMATCH-SHORT",
        dec!(3100.00),
    );
    ctx.add_instrument(instrument);
    ctx.add_position(&pos_long);
    ctx.add_position(&pos_short);

    let mock_client = MockPositionExecutionClient::new(vec![], vec![report_long, report_short]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(events.is_empty());
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, test_account_id())),
        1,
    );
}

#[tokio::test]
async fn test_position_check_venue_only_offset_hedge_legs_are_discrepant() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let report_long = create_test_position_report(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        "2.0",
        "P-OFFSET-LONG",
        dec!(3000.00),
    );
    let report_short = create_test_position_report(
        test_account_id(),
        instrument_id,
        PositionSide::Short,
        "2.0",
        "P-OFFSET-SHORT",
        dec!(3100.00),
    );
    ctx.add_instrument(instrument);

    let mock_client = MockPositionExecutionClient::new(vec![], vec![report_long, report_short]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(events.is_empty());
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, test_account_id())),
        1,
    );
}

#[tokio::test]
async fn test_position_check_flat_and_nonflat_reports_use_nonflat_report() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let position = create_test_position(
        &instrument,
        PositionId::from("P-NONFLAT"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
    );
    let nonflat_report = create_test_position_report(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        "5.0",
        "P-NONFLAT",
        dec!(3000.00),
    );

    let flat_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Flat,
        Quantity::from("0"),
        UnixNanos::from(2_000_000),
        UnixNanos::from(2_000_000),
        None,
        None,
        None,
    );
    ctx.add_instrument(instrument);
    ctx.add_position(&position);

    let mock_client = MockPositionExecutionClient::new(vec![], vec![nonflat_report, flat_report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(events.is_empty());
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, test_account_id())),
        0,
    );
}

#[tokio::test]
async fn test_position_check_multi_leg_discrepancy_defers_reconciliation() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let pos_long = create_test_position(
        &instrument,
        PositionId::from("P-DEFER-LONG"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
    );
    let pos_short = create_test_position(
        &instrument,
        PositionId::from("P-DEFER-SHORT"),
        OrderSide::Sell,
        "2.0",
        "3100.00",
    );
    let report_long = create_test_position_report(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        "5.0",
        "P-DEFER-LONG",
        dec!(3000.00),
    );
    let report_short = create_test_position_report(
        test_account_id(),
        instrument_id,
        PositionSide::Short,
        "1.0",
        "P-DEFER-SHORT",
        dec!(3100.00),
    );
    ctx.add_instrument(instrument);
    ctx.add_position(&pos_long);
    ctx.add_position(&pos_short);

    let mock_client = MockPositionExecutionClient::new(vec![], vec![report_long, report_short]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(events.is_empty());
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, test_account_id())),
        1,
    );
}

#[tokio::test]
async fn test_position_check_single_leg_gets_fresh_budget_after_multi_leg_exhaustion() {
    let config = ExecutionManagerConfig {
        position_check_retries: 1,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let pos_long = create_test_position(
        &instrument,
        PositionId::from("P-SHAPE-LONG"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
    );
    let pos_short = create_test_position(
        &instrument,
        PositionId::from("P-SHAPE-SHORT"),
        OrderSide::Sell,
        "2.0",
        "3100.00",
    );
    ctx.add_instrument(instrument);
    ctx.add_position(&pos_long);
    ctx.add_position(&pos_short);

    let report_long = create_test_position_report(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        "4.0",
        "P-SHAPE-LONG",
        dec!(3000.00),
    );
    let report_short = create_test_position_report(
        test_account_id(),
        instrument_id,
        PositionSide::Short,
        "2.0",
        "P-SHAPE-SHORT",
        dec!(3100.00),
    );
    let multi_leg_client =
        MockPositionExecutionClient::new(vec![], vec![report_long, report_short]);
    let clients: Vec<&dyn ExecutionClient> = vec![&multi_leg_client];

    let first_events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(first_events.is_empty());
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, test_account_id())),
        1,
    );

    let single_report = create_test_position_report(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        "4.0",
        "P-SHAPE-NET",
        dec!(3200.00),
    );
    let single_leg_client = MockPositionExecutionClient::new(vec![], vec![single_report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&single_leg_client];

    let second_events = ctx.manager.check_positions_consistency(&clients).await;

    let fills = second_events
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => Some(fill),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(fills.len(), 1);
    assert_eq!(fills[0].account_id, test_account_id());
    assert_eq!(fills[0].order_side, OrderSide::Buy);
    assert_eq!(fills[0].last_qty, Quantity::from("1.0"));
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, test_account_id())),
        0,
    );
}

#[tokio::test]
async fn test_position_check_multi_leg_reports_remain_isolated_by_account() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let account_a = AccountId::from("BINANCE-HEDGE-A");
    let account_b = AccountId::from("BINANCE-HEDGE-B");
    ctx.add_instrument(instrument.clone());
    ctx.add_margin_account(account_a);
    ctx.add_margin_account(account_b);

    for (position_id, side, qty, price, account_id) in [
        ("P-A-LONG", OrderSide::Buy, "5.0", "3000.00", account_a),
        ("P-A-SHORT", OrderSide::Sell, "2.0", "3100.00", account_a),
        ("P-B-LONG", OrderSide::Buy, "7.0", "3200.00", account_b),
        ("P-B-SHORT", OrderSide::Sell, "3.0", "3300.00", account_b),
    ] {
        let position = create_test_position_for_account(
            &instrument,
            PositionId::from(position_id),
            side,
            qty,
            price,
            account_id,
        );
        ctx.add_position(&position);
    }

    let client_a = MockPositionExecutionClient::configured(
        ClientId::from("BINANCE-HEDGE-A"),
        account_a,
        test_venue(),
        IndexSet::from([instrument_id.venue]),
        false,
    )
    .with_position_reports(vec![
        create_test_position_report(
            account_a,
            instrument_id,
            PositionSide::Long,
            "5.0",
            "P-A-LONG",
            dec!(3000.00),
        ),
        create_test_position_report(
            account_a,
            instrument_id,
            PositionSide::Short,
            "2.0",
            "P-A-SHORT",
            dec!(3100.00),
        ),
    ]);
    let client_b = MockPositionExecutionClient::configured(
        ClientId::from("BINANCE-HEDGE-B"),
        account_b,
        test_venue(),
        IndexSet::from([instrument_id.venue]),
        false,
    )
    .with_position_reports(vec![
        create_test_position_report(
            account_b,
            instrument_id,
            PositionSide::Long,
            "7.0",
            "P-B-LONG",
            dec!(3200.00),
        ),
        create_test_position_report(
            account_b,
            instrument_id,
            PositionSide::Short,
            "2.0",
            "P-B-SHORT",
            dec!(3300.00),
        ),
    ]);
    let clients: Vec<&dyn ExecutionClient> = vec![&client_a, &client_b];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(events.is_empty());
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, account_a)),
        0,
    );
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, account_b)),
        1,
    );
}

#[tokio::test]
async fn test_position_check_single_report_reconciliation_is_unchanged() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let position = create_test_position(
        &instrument,
        PositionId::from("P-SINGLE"),
        OrderSide::Buy,
        "3.0",
        "3000.00",
    );
    let report = create_test_position_report(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        "5.0",
        "P-SINGLE",
        dec!(3100.00),
    );
    ctx.add_instrument(instrument);
    ctx.add_position(&position);

    let mock_client = MockPositionExecutionClient::new(vec![], vec![report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    let fills = events
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => Some(fill),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(fills.len(), 1);
    assert_eq!(fills[0].account_id, test_account_id());
    assert_eq!(fills[0].order_side, OrderSide::Buy);
    assert_eq!(fills[0].last_qty, Quantity::from("2.0"));
    assert_eq!(fills[0].last_px, Price::from("3250.00"));
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

#[tokio::test]
async fn test_position_check_dedup_skips_second_hedge_position_same_instrument() {
    // Two hedge positions should only consume one retry per cycle, not two
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();

    let pos_long = create_test_position(
        &instrument,
        PositionId::from("P-LONG"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
    );
    let pos_short = create_test_position(
        &instrument,
        PositionId::from("P-SHORT"),
        OrderSide::Sell,
        "3.0",
        "3100.00",
    );

    // Omit instrument from cache to force the retry path
    ctx.add_position(&pos_long);
    ctx.add_position(&pos_short);

    let mock_client = MockExecutionClient::new(vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    // Three cycles: each increments retry by 1 (not 2) thanks to dedup
    ctx.manager.check_positions_consistency(&clients).await;
    ctx.manager.check_positions_consistency(&clients).await;
    let events = ctx.manager.check_positions_consistency(&clients).await;
    assert!(events.is_empty());

    // With correct dedup retry count is 3 (= max), so adding the instrument
    // now should still be suppressed. If dedup were broken (2 per cycle),
    // retries would have hit 6 instead.
    ctx.add_instrument(instrument.clone());
    let events = ctx.manager.check_positions_consistency(&clients).await;
    assert!(
        events.is_empty(),
        "Expected retries to be exhausted after 3 cycles with dedup"
    );
}

#[tokio::test]
async fn test_position_check_flat_venue_report_does_not_protect_stale_counter() {
    // Flat (zero-qty) venue reports should not prevent pruning of stale
    // retry counters
    let config = ExecutionManagerConfig {
        position_check_retries: 1,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();

    ctx.add_instrument(instrument.clone());
    let position = create_test_position(
        &instrument,
        PositionId::from("P-001"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
    );
    ctx.add_position(&position);

    let flat_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Flat,
        Quantity::from("0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        None,
    );
    let mock_client = MockPositionExecutionClient::new(vec![], vec![flat_report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;
    assert!(!events.is_empty());

    // Don't apply events - simulate recon not fixing it, exhaust retries
    ctx.manager.check_positions_consistency(&clients).await;
    let close_order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_id)
        .side(OrderSide::Sell)
        .quantity(Quantity::from("5.0"))
        .build();
    let close_fill = TestOrderEventStubs::filled(
        &close_order,
        &instrument,
        Some(TradeId::new("T-CLOSE-001")),
        Some(PositionId::from("P-001")),
        Some(Price::from("3000.00")),
        Some(Quantity::from("5.0")),
        None,
        None,
        None,
        Some(test_account_id()),
    );
    let close_filled: OrderFilled = close_fill.into();
    let mut pos = position;
    pos.apply(&close_filled);
    ctx.cache.borrow_mut().update_position(&pos).unwrap();

    // Flat venue report should not protect the stale retry counter
    ctx.manager.check_positions_consistency(&clients).await;

    // Counter was pruned, so new position should trigger recon
    let position2 = create_test_position(
        &instrument,
        PositionId::from("P-002"),
        OrderSide::Buy,
        "3.0",
        "3100.00",
    );
    ctx.add_position(&position2);

    let events = ctx.manager.check_positions_consistency(&clients).await;
    assert!(
        !events.is_empty(),
        "Expected events: flat venue report should not protect stale retry counter"
    );
}

#[tokio::test]
async fn test_position_check_nonflat_venue_report_protects_counter() {
    // Non-flat venue report should protect the retry counter from pruning
    let config = ExecutionManagerConfig {
        position_check_retries: 1,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();

    ctx.add_instrument(instrument.clone());
    let position = create_test_position(
        &instrument,
        PositionId::from("P-001"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
    );
    ctx.add_position(&position);

    let venue_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("3.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3000.00)),
    );
    let mock_client = MockPositionExecutionClient::new(vec![], vec![venue_report.clone()]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;
    assert!(!events.is_empty());

    // Don't apply events - exhaust retries
    ctx.manager.check_positions_consistency(&clients).await;
    let close_order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_id)
        .side(OrderSide::Sell)
        .quantity(Quantity::from("5.0"))
        .build();
    let close_fill = TestOrderEventStubs::filled(
        &close_order,
        &instrument,
        Some(TradeId::new("T-CLOSE-002")),
        Some(PositionId::from("P-001")),
        Some(Price::from("3000.00")),
        Some(Quantity::from("5.0")),
        None,
        None,
        None,
        Some(test_account_id()),
    );
    let close_filled: OrderFilled = close_fill.into();
    let mut pos = position;
    pos.apply(&close_filled);
    ctx.cache.borrow_mut().update_position(&pos).unwrap();

    // Non-flat venue report should keep the counter alive
    ctx.manager.check_positions_consistency(&clients).await;
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, test_account_id())),
        1,
    );

    let position2 = create_test_position(
        &instrument,
        PositionId::from("P-002"),
        OrderSide::Buy,
        "3.0",
        "3100.00",
    );
    ctx.add_position(&position2);

    // Counter retained - retries still exhausted
    let events = ctx.manager.check_positions_consistency(&clients).await;
    assert!(
        events.is_empty(),
        "Expected no events: non-flat venue report should protect retry counter"
    );
}

#[tokio::test]
async fn test_position_check_retries_independent_per_account() {
    // Two accounts on the same instrument must track their reconciliation
    // retry counters independently. With instrument-only keying, account A's
    // increment would suppress account B's first attempt entirely.
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();

    let account_a = AccountId::from("BINANCE-A");
    let account_b = AccountId::from("BINANCE-B");

    ctx.add_margin_account(account_a);
    ctx.add_margin_account(account_b);
    // Instrument deliberately omitted from cache: forces the failed-retry
    // path so each iteration that reaches it bumps the per-key counter.

    let pos_a = create_test_position_for_account(
        &instrument,
        PositionId::from("P-RETRY-A"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
        account_a,
    );
    let pos_b = create_test_position_for_account(
        &instrument,
        PositionId::from("P-RETRY-B"),
        OrderSide::Buy,
        "3.0",
        "3100.00",
        account_b,
    );
    ctx.add_position(&pos_a);
    ctx.add_position(&pos_b);

    let mock_client = MockExecutionClient::new(vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    ctx.manager.check_positions_consistency(&clients).await;

    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, account_a)),
        1,
        "account A retry not incremented",
    );
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, account_b)),
        1,
        "account B retry not incremented (would be 0 if dedup collapsed by instrument)",
    );
}

#[tokio::test]
async fn test_position_check_activity_throttle_independent_per_account() {
    // Recent activity recorded for one account on an instrument must not
    // throttle reconciliation for another account on the same instrument.
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::from_mins(1), // 60s
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();

    let account_a = AccountId::from("BINANCE-A");
    let account_b = AccountId::from("BINANCE-B");

    ctx.add_margin_account(account_a);
    ctx.add_margin_account(account_b);
    ctx.add_instrument(instrument.clone());

    let pos_b = create_test_position_for_account(
        &instrument,
        PositionId::from("P-ACT-B"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
        account_b,
    );
    ctx.add_position(&pos_b);

    // Simulate recent activity on account A only: B must remain unthrottled.
    // Activity is stamped from the monotonic `dst::time` clock inside
    // `record_position_activity`, so no explicit timestamp is passed.
    ctx.manager
        .record_position_activity(instrument_id, account_a);

    // No venue report for B: treated as flat, so a discrepancy.
    let mock_client = MockPositionExecutionClient::new(vec![], vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    let mut accounts_with_filled: HashSet<AccountId> = HashSet::new();

    for event in &events {
        if let OrderEventAny::Filled(fill) = event {
            accounts_with_filled.insert(fill.account_id);
        }
    }

    assert!(
        accounts_with_filled.contains(&account_b),
        "B's reconciliation must not be throttled by activity recorded for A",
    );
}

#[tokio::test]
async fn test_position_check_grace_survives_accelerated_trading_clock() {
    // The position-reconciliation grace is measured on the monotonic clock, not
    // `self.clock` (see `record_position_activity`). Here we record local activity,
    // then jump `self.clock` ~954 days forward in a single step, standing in for a
    // trading clock that has raced ahead while only milliseconds of real time have
    // elapsed, and assert the grace still suppresses the discrepancy.
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::from_mins(1), // 60s of real cover
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let account = AccountId::from("BINANCE-A");

    ctx.add_instrument(instrument.clone());
    ctx.add_margin_account(account);

    // Observe a fill: records local activity on the monotonic clock. The venue
    // event timestamps do not feed the grace and are left arbitrary.
    let ts_event = UnixNanos::from(1_000_000_000);

    let fill_report = FillReport::new(
        account,
        instrument_id,
        VenueOrderId::from("V-1"),
        TradeId::from("T-1"),
        OrderSide::Buy,
        Quantity::from("0.06"),
        Price::from("3000.00"),
        Money::new(0.0, Currency::USDT()),
        LiquiditySide::Taker,
        Some(ClientOrderId::from("O-1")),
        None,     // venue_position_id
        ts_event, // ts_event
        ts_event, // ts_init
        None,     // report_id
    );
    ctx.manager
        .observe_execution_report(&ExecutionReport::Fill(Box::new(fill_report)));

    // Race the trading clock ~954 days ahead. A `self.clock`-based grace would now
    // read "activity was 954 days ago" and fire; the monotonic grace must not.
    let accelerated_jump_ns: u64 = 954 * 86_400 * 1_000_000_000; // 954 days in ns
    ctx.advance_time(accelerated_jump_ns);

    // Cache is flat but the venue reports a 0.06 long: a genuine discrepancy. The
    // only thing between it and a synthesized EXTERNAL position is the grace.
    let venue_report = PositionStatusReport::new(
        account,
        instrument_id,
        PositionSide::Long,
        Quantity::from("0.06"),
        UnixNanos::from(accelerated_jump_ns),
        UnixNanos::from(accelerated_jump_ns),
        None, // report_id
        None, // venue_position_id
        Some(dec!(3000.00)),
    );
    let mock_client = MockPositionExecutionClient::new(vec![], vec![venue_report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(
        events.is_empty(),
        "grace must survive an accelerated trading clock (it is measured on the \
         monotonic clock); was {} reconciliation event(s), the grace regressed \
         to self.clock",
        events.len(),
    );
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, account)),
        0,
        "grace path must return before the retry counter is touched",
    );
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_position_check_grace_expires_on_monotonic_clock() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::from_mins(1),
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let account = AccountId::from("BINANCE-A");

    ctx.add_instrument(instrument);
    ctx.add_margin_account(account);

    let ts_event = UnixNanos::from(1_000_000_000);

    let fill_report = FillReport::new(
        account,
        instrument_id,
        VenueOrderId::from("V-EXPIRY"),
        TradeId::from("T-EXPIRY"),
        OrderSide::Buy,
        Quantity::from("0.06"),
        Price::from("3000.00"),
        Money::new(0.0, Currency::USDT()),
        LiquiditySide::Taker,
        Some(ClientOrderId::from("O-EXPIRY")),
        None,
        ts_event,
        ts_event,
        None,
    );
    ctx.manager
        .observe_execution_report(&ExecutionReport::Fill(Box::new(fill_report)));

    advance_clock(dst::time::Duration::from_secs(61)).await;

    let venue_report = PositionStatusReport::new(
        account,
        instrument_id,
        PositionSide::Long,
        Quantity::from("0.06"),
        ts_event,
        ts_event,
        None,
        None,
        Some(dec!(3000.00)),
    );
    let mock_client = MockPositionExecutionClient::new(vec![], vec![venue_report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(
        events
            .iter()
            .any(|event| matches!(event, OrderEventAny::Filled(_))),
        "position discrepancy should fire after the monotonic grace expires",
    );
}

#[tokio::test]
async fn test_check_positions_consistency_processes_only_discrepant_account() {
    // With cache and venue agreeing on account A and disagreeing on account B,
    // only B should be reconciled. A must remain untouched.
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();

    let account_a = AccountId::from("BINANCE-A");
    let account_b = AccountId::from("BINANCE-B");

    ctx.add_margin_account(account_a);
    ctx.add_margin_account(account_b);
    ctx.add_instrument(instrument.clone());

    let pos_a = create_test_position_for_account(
        &instrument,
        PositionId::from("P-E2E-A"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
        account_a,
    );
    let pos_b = create_test_position_for_account(
        &instrument,
        PositionId::from("P-E2E-B"),
        OrderSide::Buy,
        "3.0",
        "3100.00",
        account_b,
    );
    ctx.add_position(&pos_a);
    ctx.add_position(&pos_b);

    // Venue agrees with A; B has no report, so B is discrepant against flat.
    let report_a = PositionStatusReport::new(
        account_a,
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3000.00)),
    );
    let mock_client = MockPositionExecutionClient::new(vec![], vec![report_a]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    let mut accounts_with_filled: HashSet<AccountId> = HashSet::new();

    for event in &events {
        if let OrderEventAny::Filled(fill) = event {
            accounts_with_filled.insert(fill.account_id);
        }
    }

    assert!(
        accounts_with_filled.contains(&account_b),
        "expected reconciliation events for the discrepant account B",
    );
    assert!(
        !accounts_with_filled.contains(&account_a),
        "account A is not discrepant and must not be reconciled",
    );
}

#[tokio::test]
async fn test_position_check_stale_retries_pruned_per_account() {
    // Mixed staleness across accounts on the same instrument: only the closed
    // account's retry counter should be pruned. The active account's counter
    // must be retained.
    let config = ExecutionManagerConfig {
        position_check_retries: 5,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();

    let account_a = AccountId::from("BINANCE-A");
    let account_b = AccountId::from("BINANCE-B");

    ctx.add_margin_account(account_a);
    ctx.add_margin_account(account_b);
    // Instrument deliberately omitted from cache: forces the failed-retry path.

    let pos_a = create_test_position_for_account(
        &instrument,
        PositionId::from("P-STALE-A"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
        account_a,
    );
    let pos_b = create_test_position_for_account(
        &instrument,
        PositionId::from("P-STALE-B"),
        OrderSide::Buy,
        "3.0",
        "3100.00",
        account_b,
    );
    ctx.add_position(&pos_a);
    ctx.add_position(&pos_b);

    let mock_client = MockExecutionClient::new(vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    // Cycle 1: both keys reach the failed-retry path, so counters land at 1 each.
    ctx.manager.check_positions_consistency(&clients).await;
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, account_a)),
        1,
    );
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, account_b)),
        1,
    );

    // Close account B's position so it disappears from open_positions.
    let close_order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_id)
        .side(OrderSide::Sell)
        .quantity(Quantity::from("3.0"))
        .build();
    let close_fill = TestOrderEventStubs::filled(
        &close_order,
        &instrument,
        Some(TradeId::new("T-CLOSE-B")),
        Some(PositionId::from("P-STALE-B")),
        Some(Price::from("3100.00")),
        Some(Quantity::from("3.0")),
        None,
        None,
        None,
        Some(account_b),
    );
    let close_filled: OrderFilled = close_fill.into();
    let mut pos_b = pos_b;
    pos_b.apply(&close_filled);
    ctx.cache.borrow_mut().update_position(&pos_b).unwrap();

    // Cycle 2: A still active and discrepant, so its counter increments;
    // B's position is closed and venue reports nothing, so its counter is pruned.
    ctx.manager.check_positions_consistency(&clients).await;

    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, account_a)),
        2,
        "account A's counter must be retained while it remains active",
    );
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, account_b)),
        0,
        "account B's counter must be pruned once its position is closed",
    );
}

#[tokio::test]
async fn test_reconcile_mass_status_publishes_raw_reports_for_capture() {
    // Live mass-status reconciliation bypasses the per-report engine entry
    // points, so the raw venue inputs must be published from this path or the
    // event store has no record of them for forensic replay.
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let order_topic = MessagingSwitchboard::reconciliation_raw_order_status_report_topic();
    let fill_topic = MessagingSwitchboard::reconciliation_raw_fill_report_topic();
    let position_topic = MessagingSwitchboard::reconciliation_raw_position_status_report_topic();

    let (order_handler, order_saver) = get_any_saving_handler::<OrderStatusReport>(None);
    let (fill_handler, fill_saver) = get_any_saving_handler::<FillReport>(None);
    let (position_handler, position_saver) = get_any_saving_handler::<PositionStatusReport>(None);

    let order_pattern: msgbus::MStr<msgbus::Pattern> = order_topic.into();
    let fill_pattern: msgbus::MStr<msgbus::Pattern> = fill_topic.into();
    let position_pattern: msgbus::MStr<msgbus::Pattern> = position_topic.into();
    msgbus::subscribe_any(order_pattern, order_handler.clone(), None);
    msgbus::subscribe_any(fill_pattern, fill_handler.clone(), None);
    msgbus::subscribe_any(position_pattern, position_handler.clone(), None);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );
    let order_report = create_order_report(
        None,
        VenueOrderId::from("V-RAW-CAPTURE"),
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0"),
    );
    mass_status.add_order_reports(vec![order_report.clone()]);

    let fill_report = FillReport::new(
        test_account_id(),
        instrument_id,
        VenueOrderId::from("V-RAW-CAPTURE"),
        TradeId::from("T-RAW-CAPTURE"),
        OrderSide::Buy,
        Quantity::from("1.0"),
        Price::from("3000.00"),
        Money::new(0.0, Currency::USDT()),
        LiquiditySide::Maker,
        None,
        None,
        UnixNanos::from(2_000_000),
        UnixNanos::from(2_000_000),
        None,
    );
    mass_status.add_fill_reports(vec![fill_report.clone()]);

    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("1.0"),
        UnixNanos::from(3_000_000),
        UnixNanos::from(3_000_000),
        None,
        None,
        Some(dec!(3000.0)),
    );
    mass_status.add_position_reports(vec![position_report.clone()]);

    let _ = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    msgbus::unsubscribe_any(order_pattern, &order_handler);
    msgbus::unsubscribe_any(fill_pattern, &fill_handler);
    msgbus::unsubscribe_any(position_pattern, &position_handler);

    let orders = order_saver.get_messages();
    assert_eq!(
        orders.len(),
        1,
        "raw OrderStatusReport must be published once"
    );
    assert_eq!(orders[0], order_report);

    let fills = fill_saver.get_messages();
    assert_eq!(fills.len(), 1, "raw FillReport must be published once");
    assert_eq!(fills[0], fill_report);

    let positions = position_saver.get_messages();
    assert_eq!(
        positions.len(),
        1,
        "raw PositionStatusReport must be published once",
    );
    assert_eq!(positions[0], position_report);
}

#[tokio::test]
async fn test_reconcile_mass_status_does_not_capture_synthetic_reports() {
    // The raw publish must happen BEFORE adjust_mass_status_fills, which can
    // synthesize replacement order/fill reports via
    // process_mass_status_for_reconciliation. Forensic replay must see only
    // the venue-supplied raw inputs; synthetic reports are an internal
    // reconstruction step and must never appear on `reconciliation.raw.*`.
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let order_topic = MessagingSwitchboard::reconciliation_raw_order_status_report_topic();
    let fill_topic = MessagingSwitchboard::reconciliation_raw_fill_report_topic();
    let order_pattern: msgbus::MStr<msgbus::Pattern> = order_topic.into();
    let fill_pattern: msgbus::MStr<msgbus::Pattern> = fill_topic.into();
    let (order_handler, order_saver) = get_any_saving_handler::<OrderStatusReport>(None);
    let (fill_handler, fill_saver) = get_any_saving_handler::<FillReport>(None);
    msgbus::subscribe_any(order_pattern, order_handler.clone(), None);
    msgbus::subscribe_any(fill_pattern, fill_handler.clone(), None);

    // Build a mass status that triggers AddSyntheticOpening: simulated qty
    // (0.4 from the single fill) does not match the venue position (Long 1.0),
    // so the adjustment step inserts a synthetic Buy 0.6 opening fill under a
    // new `S-...` venue_order_id.
    let venue_order_id = VenueOrderId::from("V-SYN-RAW");

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );
    let order_report = create_order_report(
        None,
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("1.0"),
        Quantity::from("0.4"),
    );
    mass_status.add_order_reports(vec![order_report.clone()]);

    let fill_report = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-SYN-RAW"),
        OrderSide::Buy,
        Quantity::from("0.4"),
        Price::from("3000.00"),
        Money::new(0.0, Currency::USDT()),
        LiquiditySide::Maker,
        None,
        None,
        UnixNanos::from(2_000_000),
        UnixNanos::from(2_000_000),
        None,
    );
    mass_status.add_fill_reports(vec![fill_report.clone()]);

    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("1.0"),
        UnixNanos::from(3_000_000),
        UnixNanos::from(3_000_000),
        None,
        None, // netting mode triggers adjust_mass_status_fills
        Some(dec!(3000.0)),
    );
    mass_status.add_position_reports(vec![position_report]);

    let _ = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    msgbus::unsubscribe_any(order_pattern, &order_handler);
    msgbus::unsubscribe_any(fill_pattern, &fill_handler);

    let orders = order_saver.get_messages();
    assert_eq!(
        orders.len(),
        1,
        "only the raw venue OrderStatusReport must be captured; \
         synthetic reports from adjust_mass_status_fills must not reach \
         the raw topic",
    );
    assert_eq!(orders[0], order_report);
    assert_eq!(
        orders[0].venue_order_id, venue_order_id,
        "captured venue_order_id must match the original raw input, not a synthetic `S-` id",
    );

    let fills = fill_saver.get_messages();
    assert_eq!(
        fills.len(),
        1,
        "only the raw venue FillReport must be captured; the synthetic \
         opening fill inserted by adjustment must not appear on the raw topic",
    );
    assert_eq!(fills[0], fill_report);
    assert_eq!(
        fills[0].trade_id,
        TradeId::from("T-SYN-RAW"),
        "captured trade_id must match the original raw input, not a synthetic `S-` id",
    );
}

#[rstest]
#[case::unbounded(false)]
#[case::bounded(true)]
#[tokio::test]
async fn test_mass_status_zero_quantity_fill_does_not_consume_trade_id(#[case] bounded: bool) {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let trade_id = TradeId::from("T-ZERO-RETRY");
    ctx.add_instrument(test_instrument());
    let order = create_limit_order("O-001", instrument_id, OrderSide::Buy, "2.0", "3000.00");
    ctx.add_order(order.clone());

    let valid = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        trade_id,
        OrderSide::Buy,
        Quantity::from("1.0"),
        Price::from("3000.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        Some(client_order_id),
        None,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );
    let mut zero = valid.clone();
    zero.last_qty = Quantity::zero(1);
    zero.commission = Money::from("123.45 USDT");

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        None,
    );

    if bounded {
        mass_status.set_report_window(Some(UnixNanos::from(1)), true);
    }

    mass_status.add_fill_reports(vec![zero]);

    let rejected = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert!(rejected.events.is_empty());
    assert_eq!(ctx.get_order(&client_order_id), Some(order));
    assert_eq!(
        ctx.cache
            .borrow()
            .positions_total_count(None, None, None, None, None),
        0
    );

    mass_status.add_fill_reports(vec![valid]);
    let accepted = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(accepted.events.len(), 1);

    let OrderEventAny::Filled(fill) = &accepted.events[0] else {
        panic!("Expected fill");
    };

    assert_eq!(fill.last_qty, Quantity::from("1.0"));
    assert_eq!(fill.commission, Some(Money::from("0.50 USDT")));
    assert_eq!(fill.trade_id, trade_id);
}

#[tokio::test]
async fn test_zero_quantity_fill_does_not_refresh_recency() {
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let trade_id = TradeId::from("T-ZERO-RECENCY");
    let mut order = create_accepted_order(
        "O-ZERO-RECENCY",
        instrument.id(),
        OrderSide::Buy,
        "2.0",
        "3000.00",
        VenueOrderId::from("V-ZERO-RECENCY"),
    );
    let event = TestOrderEventStubs::filled(
        &order,
        &instrument,
        Some(trade_id),
        None,
        Some(Price::from("3000.00")),
        Some(Quantity::from("1.0")),
        Some(LiquiditySide::Maker),
        None,
        None,
        Some(test_account_id()),
    );
    order.apply(event.clone()).unwrap();
    ctx.add_order(order);

    let OrderEventAny::Filled(fill) = event else {
        panic!("Expected fill");
    };

    let mut zero = fill.clone();
    zero.last_qty = Quantity::zero(1);

    ctx.manager.commit_recent_fill_if_applied(&zero);

    assert!(
        !ctx.manager
            .is_fill_recently_processed(test_account_id(), instrument.id(), trade_id)
    );

    ctx.manager.commit_recent_fill_if_applied(&fill);

    assert!(
        ctx.manager
            .is_fill_recently_processed(test_account_id(), instrument.id(), trade_id)
    );
}

#[tokio::test]
async fn test_zero_quantity_fill_does_not_suppress_hedge_position_report() {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        generate_missing_orders: true,
        ..Default::default()
    });

    let instrument_id = test_instrument_id();
    let position_id = PositionId::from("P-ZERO-HEDGE");
    ctx.add_instrument(test_instrument());
    let zero = create_fill_report(
        ClientOrderId::from("O-ZERO-HEDGE"),
        VenueOrderId::from("V-ZERO-HEDGE"),
        instrument_id,
        TradeId::from("T-ZERO-HEDGE"),
        "0.0",
    );
    let mut mass_status = create_mass_status(vec![], vec![zero]);
    mass_status.add_position_reports(vec![PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(position_id),
        Some(dec!(3000.00)),
    )]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let fills: Vec<_> = result
        .events
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => Some(fill),
            _ => None,
        })
        .collect();

    assert_eq!(fills.len(), 1);
    assert_eq!(fills[0].last_qty, Quantity::from("5.0"));
    assert_eq!(fills[0].position_id, Some(position_id));
    assert_ne!(fills[0].trade_id, TradeId::from("T-ZERO-HEDGE"));
    let cache = ctx.cache.borrow();
    assert!(!cache.order_exists(&ClientOrderId::from("O-ZERO-HEDGE")));
    assert_eq!(
        cache.position(&position_id).unwrap().quantity,
        Quantity::from("5.0")
    );
}
