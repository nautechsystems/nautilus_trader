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

use std::{cell::RefCell, rc::Rc};

use ahash::AHashSet;
use async_trait::async_trait;
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    clock::TestClock,
    messages::execution::{
        BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateOrderStatusReports, ModifyOrder,
        QueryAccount, QueryOrder, SubmitOrder, SubmitOrderList,
    },
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_execution::{
    engine::ExecutionEngine, reconciliation::process_mass_status_for_reconciliation,
};
use nautilus_live::manager::{ExecutionManager, ExecutionManagerConfig, ExecutionReport};
use nautilus_model::{
    accounts::{AccountAny, MarginAccount},
    enums::{
        AccountType, LiquiditySide, OmsType, OrderSide, OrderStatus, OrderType,
        PositionSideSpecified, TimeInForce,
    },
    events::{OrderEventAny, OrderFilled, account::state::AccountState},
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, PositionId, StrategyId, TradeId,
        TraderId, Venue, VenueOrderId,
    },
    instruments::{
        Instrument, InstrumentAny,
        stubs::{crypto_perpetual_ethusdt, xbtusd_bitmex},
    },
    orders::{Order, OrderAny, OrderTestBuilder, stubs::TestOrderEventStubs},
    position::Position,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, MarginBalance, Money, Price, Quantity},
};
use rstest::rstest;
use rust_decimal_macros::dec;

struct TestContext {
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    manager: ExecutionManager,
    exec_engine: Rc<RefCell<ExecutionEngine>>,
}

impl TestContext {
    fn new() -> Self {
        Self::with_config(ExecutionManagerConfig::default())
    }

    fn with_config(config: ExecutionManagerConfig) -> Self {
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));

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

        let manager = ExecutionManager::new(clock.clone(), cache.clone(), config);
        let mut engine = ExecutionEngine::new(clock.clone(), cache.clone(), None);

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

    fn add_instrument(&self, instrument: InstrumentAny) {
        self.cache.borrow_mut().add_instrument(instrument).unwrap();
    }

    fn add_order(&self, order: OrderAny) {
        self.cache
            .borrow_mut()
            .add_order(order, None, None, false)
            .unwrap();
    }

    fn add_position(&self, position: Position) {
        self.cache
            .borrow_mut()
            .add_position(position, OmsType::Hedging)
            .unwrap();
    }

    fn get_order(&self, client_order_id: &ClientOrderId) -> Option<OrderAny> {
        self.cache.borrow().order(client_order_id).cloned()
    }
}

fn test_instrument() -> InstrumentAny {
    InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt())
}

fn test_instrument_id() -> InstrumentId {
    crypto_perpetual_ethusdt().id()
}

fn test_instrument2() -> InstrumentAny {
    InstrumentAny::CryptoPerpetual(xbtusd_bitmex())
}

fn test_instrument_id2() -> InstrumentId {
    xbtusd_bitmex().id()
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

fn create_order_status_report(
    client_order_id: Option<ClientOrderId>,
    venue_order_id: VenueOrderId,
    instrument_id: InstrumentId,
    status: OrderStatus,
    quantity: Quantity,
    filled_qty: Quantity,
) -> OrderStatusReport {
    OrderStatusReport::new(
        test_account_id(),
        instrument_id,
        client_order_id,
        venue_order_id,
        OrderSide::Buy,
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

#[rstest]
fn test_fill_deduplication_new_fill_not_processed() {
    let ctx = TestContext::new();
    let trade_id = TradeId::from("T-001");

    assert!(!ctx.manager.is_fill_recently_processed(&trade_id));
}

#[rstest]
fn test_fill_deduplication_tracks_processed_fill() {
    let mut ctx = TestContext::new();
    let trade_id = TradeId::from("T-001");

    ctx.manager.mark_fill_processed(trade_id);

    assert!(ctx.manager.is_fill_recently_processed(&trade_id));
}

#[rstest]
fn test_fill_deduplication_prune_removes_expired() {
    let mut ctx = TestContext::new();
    let old_trade = TradeId::from("T-OLD");
    let new_trade = TradeId::from("T-NEW");

    ctx.manager.mark_fill_processed(old_trade);
    ctx.advance_time(120_000_000_000); // 120 seconds
    ctx.manager.mark_fill_processed(new_trade);

    ctx.manager.prune_recent_fills_cache(60.0); // 60 second TTL

    assert!(!ctx.manager.is_fill_recently_processed(&old_trade));
    assert!(ctx.manager.is_fill_recently_processed(&new_trade));
}

#[rstest]
fn test_reconcile_report_returns_empty_when_order_not_in_cache() {
    let mut ctx = TestContext::new();
    let client_order_id = ClientOrderId::from("O-MISSING");

    let report = ExecutionReport {
        client_order_id,
        venue_order_id: Some(VenueOrderId::from("V-001")),
        status: OrderStatus::Accepted,
        filled_qty: Quantity::from(0),
        avg_px: None,
        ts_event: UnixNanos::from(1_000_000),
    };

    let events = ctx.manager.reconcile_report(report).unwrap();

    assert!(events.is_empty());
}

#[rstest]
fn test_reconcile_report_handles_missing_venue_order_id() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();

    ctx.add_instrument(test_instrument());
    let order = create_limit_order("O-001", instrument_id, OrderSide::Buy, "1.0", "3000.00");
    ctx.add_order(order);

    let report = ExecutionReport {
        client_order_id: ClientOrderId::from("O-001"),
        venue_order_id: None, // Missing venue order ID
        status: OrderStatus::Accepted,
        filled_qty: Quantity::from(0),
        avg_px: None,
        ts_event: UnixNanos::from(1_000_000),
    };

    let events = ctx.manager.reconcile_report(report).unwrap();

    // Should return empty since venue_order_id is required
    assert!(events.is_empty());
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    assert!(result.events.is_empty());
}

#[tokio::test]
async fn test_reconcile_mass_status_creates_external_order_accepted() {
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

    let report = create_order_status_report(
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));

    // Verify order was added to cache
    let client_order_id = ClientOrderId::from("V-EXT-001");
    let order = ctx.get_order(&client_order_id);
    assert!(order.is_some());
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

    let report = create_order_status_report(
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
    let report = create_order_status_report(
        None,
        venue_order_id,
        instrument_id,
        OrderStatus::Canceled,
        Quantity::from("1.0"),
        Quantity::from("0.5"),
    )
    .with_avg_px(3000.00)
    .unwrap();
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
    let report = create_order_status_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Canceled,
        Quantity::from("2.0"),
        Quantity::from("1.0"),
    )
    .with_avg_px(3000.00)
    .unwrap();
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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

    // Create and cache an accepted order
    let mut order = create_submitted_order(
        "O-TRIG-CANCEL",
        instrument_id,
        OrderSide::Buy,
        "1.0",
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

    // Venue reports order was triggered then canceled
    let mut report = create_order_status_report(
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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

    let report = create_order_status_report(
        None,
        VenueOrderId::from("V-EXT-003"),
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("1.0"),
        Quantity::from("1.0"),
    )
    .with_avg_px(3000.50)
    .unwrap();
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
    let report = create_order_status_report(
        None,
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("2.0"),
        Quantity::from("2.0"),
    )
    .with_avg_px(3000.00)
    .unwrap();
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
    let cache = ctx.cache.borrow();
    let orders = cache.orders(None, None, None, None, None);
    assert_eq!(orders.len(), 1);
    let order = &orders[0];
    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.filled_qty(), Quantity::from("2.0"));
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
    let report = create_order_status_report(
        None,
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("3.000"),
        Quantity::from("3.000"),
    )
    .with_avg_px(3000.00)
    .unwrap();
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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

#[tokio::test]
async fn test_reconcile_mass_status_skips_external_when_filtered() {
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

    let report = create_order_status_report(
        None,
        VenueOrderId::from("V-EXT-001"),
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    assert!(result.events.is_empty());
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
    let report = create_order_status_report(
        None,
        VenueOrderId::from("S-abc123-def456"),
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("1.0"),
        Quantity::from("1.0"),
    )
    .with_avg_px(100.0)
    .unwrap();
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
}

#[tokio::test]
async fn test_reconcile_mass_status_uses_claimed_strategy() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let strategy_id = StrategyId::from("MY-STRATEGY");

    ctx.add_instrument(test_instrument());
    ctx.manager
        .claim_external_orders(instrument_id, strategy_id);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_status_report(
        None,
        VenueOrderId::from("V-EXT-001"),
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    assert_eq!(result.events.len(), 1);

    let client_order_id = ClientOrderId::from("V-EXT-001");
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // Only one fill should be processed
    assert_eq!(result.events.len(), 1);
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

    let report = create_order_status_report(
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    assert_eq!(result.events.len(), 2);

    // Verify chronological ordering
    assert!(result.events[0].ts_event() < result.events[1].ts_event());
}

#[rstest]
fn test_inflight_order_generates_rejection_after_max_retries() {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 1,
        ..Default::default()
    };
    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");

    ctx.add_instrument(test_instrument());

    // Order must be submitted (have account_id) to generate rejection
    let order = create_submitted_order("O-001", instrument_id, OrderSide::Buy, "1.0", "3000.00");
    ctx.add_order(order);

    ctx.manager.register_inflight(client_order_id);
    ctx.advance_time(200_000_000); // 200ms, past threshold

    let events = ctx.manager.check_inflight_orders();

    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], OrderEventAny::Rejected(_)));

    if let OrderEventAny::Rejected(rejected) = &events[0] {
        assert_eq!(rejected.client_order_id, client_order_id);
        assert_eq!(rejected.reason.as_str(), "INFLIGHT_TIMEOUT");
    }
}

#[rstest]
fn test_inflight_check_skips_filtered_order_ids() {
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
    ctx.advance_time(200_000_000);

    let events = ctx.manager.check_inflight_orders();

    // Filtered order should not generate rejection
    assert!(events.is_empty());
}

#[rstest]
fn test_config_default_values() {
    let config = ExecutionManagerConfig::default();

    assert!(config.reconciliation);
    assert_eq!(config.reconciliation_startup_delay_secs, 10.0);
    assert_eq!(config.lookback_mins, Some(60));
    assert!(!config.filter_unclaimed_external);
    assert!(!config.filter_position_reports);
    assert!(config.generate_missing_orders);
    assert_eq!(config.inflight_check_interval_ms, 2_000);
    assert_eq!(config.inflight_threshold_ms, 5_000);
    assert_eq!(config.inflight_max_retries, 5);
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

    let report = create_order_status_report(
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Canceled(_)));

    if let OrderEventAny::Canceled(canceled) = &result.events[0] {
        assert_eq!(canceled.client_order_id, client_order_id);
        assert!(canceled.reconciliation != 0); // Verify reconciliation flag is set
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

    let report = create_order_status_report(
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Expired(_)));
}

#[rstest]
fn test_inflight_increments_retry_count_before_max() {
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

    // First check - past threshold, retry count becomes 1
    ctx.advance_time(200_000_000);
    let events1 = ctx.manager.check_inflight_orders();
    assert!(events1.is_empty()); // Not at max yet

    // Second check - retry count becomes 2
    ctx.advance_time(200_000_000);
    let events2 = ctx.manager.check_inflight_orders();
    assert!(events2.is_empty()); // Still not at max

    // Third check - retry count becomes 3, equals max, generates rejection
    ctx.advance_time(200_000_000);
    let events3 = ctx.manager.check_inflight_orders();
    assert_eq!(events3.len(), 1);
    assert!(matches!(events3[0], OrderEventAny::Rejected(_)));
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

    let report = create_order_status_report(
        None, // External order
        VenueOrderId::from("V-EXT-PARTIAL"),
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("3.0"),
    )
    .with_avg_px(3000.50)
    .unwrap();
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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

    let report = create_order_status_report(
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // No events needed - already in sync
    assert!(result.events.is_empty());
}

#[rstest]
fn test_clear_recon_tracking_removes_inflight() {
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
    ctx.advance_time(200_000_000);

    // Check should not generate events since order was cleared
    let events = ctx.manager.check_inflight_orders();
    assert!(events.is_empty());
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
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
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

    let report = create_order_status_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("5.0"), // 5 filled
    )
    .with_avg_px(3001.50)
    .unwrap();
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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

    let report = create_order_status_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("3.0"),
    )
    .with_avg_px(2999.75)
    .unwrap();
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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

    let report = create_order_status_report(
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // No events needed - already in sync
    assert!(result.events.is_empty());
}

#[tokio::test]
async fn test_fill_qty_mismatch_venue_less_logs_error() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-MISMATCH");
    let venue_order_id = VenueOrderId::from("V-MISMATCH");

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

    // Venue reports LESS filled than we have (anomaly)
    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_status_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("3.0"), // Less than our 5
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // Should not generate events (error condition)
    assert!(result.events.is_empty());
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
        OrderSide::Buy,
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
    .with_avg_px(3005.00)
    .unwrap();
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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

    let report = create_order_status_report(
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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

    let report = create_order_status_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("8.0"),
    )
    .with_avg_px(3002.50)
    .unwrap();
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
    let report = create_order_status_report(
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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

    let report1 = create_order_status_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0.0"),
    );
    let report2 = create_order_status_report(
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
    let report = create_order_status_report(
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
    let report = create_order_status_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("5.0"),
    )
    .with_avg_px(100.0)
    .unwrap();
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
    let report = create_order_status_report(
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
    let report = create_order_status_report(
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
    ctx.add_instrument(instrument.clone());

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

    let report = create_order_status_report(
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
    ctx.add_instrument(instrument.clone());

    let venue_order_id = VenueOrderId::from("V-EXT-001");

    let report = create_order_status_report(
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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

    let report = create_order_status_report(
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
    ctx.add_instrument(loaded_instrument.clone());

    let unloaded_instrument_id = InstrumentId::from("BTCUSDT.SIM");

    let loaded_venue_order_id = VenueOrderId::from("V-LOADED");
    let unloaded_venue_order_id = VenueOrderId::from("V-UNLOADED");

    let loaded_report = create_order_status_report(
        Some(ClientOrderId::from("O-LOADED")),
        loaded_venue_order_id,
        loaded_instrument_id,
        OrderStatus::Filled,
        Quantity::from("1.0"),
        Quantity::from("1.0"),
    );

    let unloaded_report = create_order_status_report(
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
        PositionSideSpecified::Long,
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
        PositionSideSpecified::Flat,
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
        PositionSideSpecified::Long,
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
        PositionSideSpecified::Short,
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
    let order = create_limit_order("O-001", instrument_id, OrderSide::Buy, "5.0", "3000.00");
    ctx.add_order(order);

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
        PositionSideSpecified::Long,
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // Should only have 1 fill event from the fill report, not additional events
    // from the position report (which would double-count)
    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Filled(_)));
}

/// Helper to create a position from a fill for testing
fn create_test_position(
    instrument: &InstrumentAny,
    position_id: PositionId,
    side: OrderSide,
    qty: &str,
    price: &str,
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
        Some(test_account_id()),
    );

    let order_filled: OrderFilled = fill.into();
    Position::new(instrument, order_filled)
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
        PositionSideSpecified::Long,
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
        PositionSideSpecified::Short,
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
        PositionSideSpecified::Long,
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
        PositionSideSpecified::Long,
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // Should create position since no position exists for instrument
    assert!(!result.events.is_empty());
}

#[tokio::test]
async fn test_reconcile_mass_status_skips_hedge_position_when_fills_in_batch() {
    // Tests that hedge position reconciliation is skipped when fills for the same
    // venue_position_id exist in the batch (prevents duplicate synthetic orders)
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let venue_position_id = PositionId::from("P-HEDGE-001");

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

    // Add a fill report WITH venue_position_id
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
        Some(venue_position_id),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );
    mass_status.add_fill_reports(vec![fill]);

    // Add a hedge position report for the same venue_position_id
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSideSpecified::Long,
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // Should only have fill event, no synthetic order from position report
    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Filled(_)));
}

#[tokio::test]
async fn test_reconcile_mass_status_skips_hedge_position_when_filled_order_in_batch() {
    // Tests that hedge position reconciliation is skipped when a filled order report
    // with the same venue_position_id exists (even without explicit fill reports)
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

    // Add a filled order report WITH venue_position_id (no explicit fill report)
    let order_report = OrderStatusReport::new(
        test_account_id(),
        instrument_id,
        None,
        venue_order_id,
        OrderSide::Buy,
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
    .with_avg_px(3000.0)
    .unwrap()
    .with_venue_position_id(venue_position_id);

    mass_status.add_order_reports(vec![order_report]);

    // Add a hedge position report for the same venue_position_id
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSideSpecified::Long,
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // Should have events from the order report (initialized + filled), but no
    // additional synthetic order from position report
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
        PositionSideSpecified::Long,
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
        PositionSideSpecified::Long,
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
        PositionSideSpecified::Short,
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
    ctx.add_position(position);

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
        PositionSideSpecified::Long,
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
    ctx.add_position(position);

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
        PositionSideSpecified::Long,
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
        PositionSideSpecified::Long,
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
    ctx.add_position(position);

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
        PositionSideSpecified::Long,
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
        PositionSideSpecified::Flat,
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
        PositionSideSpecified::Short,
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
        PositionSideSpecified::Long,
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
        PositionSideSpecified::Long,
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
        PositionSideSpecified::Long,
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
        PositionSideSpecified::Long,
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
        PositionSideSpecified::Long,
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
    let order_report = create_order_status_report(
        Some(ClientOrderId::from("O-PARTIAL-001")),
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("2.000"),
        Quantity::from("2.000"),
    )
    .with_avg_px(3100.00)
    .unwrap();
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
        UnixNanos::from(500_000),
        UnixNanos::from(500_000),
        None,
    );
    mass_status.add_fill_reports(vec![fill]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
    let report = create_order_status_report(
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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

#[tokio::test]
async fn test_external_order_with_fills_but_no_avg_px_applies_real_fills_only() {
    // When an external order has real fill reports but order report lacks avg_px,
    // only the real fills should be applied (no inferred fill generated)
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

    // Order report shows Filled but WITHOUT avg_px (using Market order for this scenario)
    let report = OrderStatusReport::new(
        test_account_id(),
        instrument_id,
        None, // External order
        venue_order_id,
        OrderSide::Buy,
        OrderType::Market,
        TimeInForce::Gtc,
        OrderStatus::Filled,
        Quantity::from("10.0"),
        Quantity::from("10.0"),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        None, // No avg_px - cannot generate inferred fill
    );

    // Real fill report with actual price
    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-REAL-001"),
        OrderSide::Buy,
        Quantity::from("10.0"),
        Price::from("3000.00"),
        Money::from("1.00 USDT"),
        LiquiditySide::Taker,
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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

    // Only the real fill should be applied (no inferred fill due to missing avg_px)
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
    assert_eq!(fill_events[0].last_qty, Quantity::from("10.0"));

    // Order should exist and be in Filled state
    let client_order_id = ClientOrderId::from("V-FILLS-001");
    let order = ctx.get_order(&client_order_id).expect("Order should exist");
    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.filled_qty(), Quantity::from("10.0"));
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
        PositionSideSpecified::Long,
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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

    let report = create_order_status_report(
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // Should skip the closed reconciliation order - no new events generated
    assert!(
        result.events.is_empty(),
        "Should skip closed RECONCILIATION order, but got {} events",
        result.events.len()
    );
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
    ctx.add_position(position);

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
        PositionSideSpecified::Short,
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
    ctx.add_position(position);

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
        PositionSideSpecified::Long,
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
    ctx.add_position(position);

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
        PositionSideSpecified::Flat,
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
    ctx.add_instrument(instrument.clone());

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

    let report = create_order_status_report(
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
    ctx.add_instrument(instrument.clone());

    let venue_order_id = VenueOrderId::from("V-HEDGE-001");

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add a filled order report with fills
    let report = create_order_status_report(
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
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        None,
    );
    mass_status.add_fill_reports(vec![fill]);

    // Add hedge mode position report (has venue_position_id)
    let hedge_position_id = PositionId::new("HEDGE-POS-001");
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSideSpecified::Long,
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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

    let order_report1a = create_order_status_report(
        Some(ClientOrderId::from("O-ETH-001")),
        venue_order_id1a,
        instrument_id1,
        OrderStatus::Filled,
        Quantity::from("1.000"),
        Quantity::from("1.000"),
    );
    let order_report1b = create_order_status_report(
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
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
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
        UnixNanos::from(2_000),
        UnixNanos::from(2_000),
        None,
    );

    let position_report1 = PositionStatusReport::new(
        test_account_id(),
        instrument_id1,
        PositionSideSpecified::Long,
        Quantity::from("2.000"),
        UnixNanos::from(2_000),
        UnixNanos::from(2_000),
        None,
        None,
        Some(dec!(3050.00)),
    );

    // Instrument 2 (XBTUSD) - position of 100, fills sum to 100 (complete history)
    let venue_order_id2a = VenueOrderId::from("V-BTC-001");
    let venue_order_id2b = VenueOrderId::from("V-BTC-002");

    let order_report2a = create_order_status_report(
        Some(ClientOrderId::from("O-BTC-001")),
        venue_order_id2a,
        instrument_id2,
        OrderStatus::Filled,
        Quantity::from("50"),
        Quantity::from("50"),
    );
    let order_report2b = create_order_status_report(
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
        UnixNanos::from(1_500),
        UnixNanos::from(1_500),
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
        UnixNanos::from(2_500),
        UnixNanos::from(2_500),
        None,
    );

    let position_report2 = PositionStatusReport::new(
        test_account_id(),
        instrument_id2,
        PositionSideSpecified::Long,
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
    assert_eq!(btc_fills.len(), 2, "Expected 2 fills for XBTUSD");
    let btc_total_qty: f64 = btc_fills.iter().map(|f| f.last_qty.as_f64()).sum();
    assert!(
        (btc_total_qty - 100.0).abs() < 0.001,
        "XBTUSD total qty should be 100.0, was {btc_total_qty}"
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
        PositionSideSpecified::Long,
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
            OrderSide::Buy,
            "Synthetic order side should be BUY (inferred from fill.order_side)"
        );
    }
}

#[tokio::test]
async fn test_adjust_fills_filter_to_current_lifecycle_preserves_working_orders() {
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
        PositionSideSpecified::Long,
        Quantity::from("200"),
        UnixNanos::from(ts_now),
        UnixNanos::from(ts_now),
        None,
        None,
        Some(dec!(1.1000)),
    );

    // O1: BUY 100 (previous lifecycle)
    let venue_order_id1 = VenueOrderId::from("V-001");
    let order_o1 = create_order_status_report(
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
        OrderSide::Sell,
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
    let order_o3 = create_order_status_report(
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

    let result = process_mass_status_for_reconciliation(&mass_status, &instrument, None).unwrap();

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
    ctx.add_position(position);

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
        PositionSideSpecified::Short,
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
    ctx.add_position(position);

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
        PositionSideSpecified::Short,
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
    ctx.add_instrument(instrument.clone());

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
        PositionSideSpecified::Long,
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
        PositionSideSpecified::Short,
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
    ctx.add_instrument(instrument.clone());

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
        PositionSideSpecified::Long,
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    assert!(
        !result.events.is_empty(),
        "Synthetic orders should bypass filter_unclaimed_external"
    );
}

#[tokio::test]
async fn test_duplicate_order_reports_keeps_most_advanced_state() {
    // When multiple order reports exist for same venue_order_id, keep most advanced
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

    // Tests deduplication: PartiallyFilled and Filled reports, Filled should win
    let report_partial = OrderStatusReport::new(
        test_account_id(),
        instrument_id,
        Some(client_order_id),
        venue_order_id,
        OrderSide::Buy,
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
        OrderSide::Buy,
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
    .with_avg_px(3000.0)
    .unwrap();

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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    let order = ctx.get_order(&client_order_id).expect("Order should exist");
    assert_eq!(
        order.status(),
        OrderStatus::Filled,
        "Order should be in most advanced state (Filled)"
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
        OrderSide::Buy,
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
    .with_avg_px(3000.0)
    .unwrap();
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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

    let report = create_order_status_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("5.0"),
    )
    .with_avg_px(3000.0)
    .unwrap();

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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
    let report = create_order_status_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("7.0"),
    )
    .with_avg_px(3000.0)
    .unwrap();

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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
    // Fills without matching order reports should still be processed
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
    let orphan_fill = FillReport::new(
        test_account_id(),
        instrument_id,
        orphan_venue_order_id,
        TradeId::from("T-ORPHAN-001"),
        OrderSide::Buy,
        Quantity::from("2.0"),
        Price::from("3000.00"),
        Money::from("0.20 USDT"),
        LiquiditySide::Taker,
        None,
        None,
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        None,
    );

    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSideSpecified::Long,
        Quantity::from("2.0"),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        None,
        None,
        Some(dec!(3000.00)),
    );

    mass_status.add_fill_reports(vec![orphan_fill]);
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // Orphan fills processed via position reconciliation (should not panic)
    assert!(
        !result.events.is_empty()
            || !ctx
                .cache
                .borrow()
                .positions(None, None, None, None, None)
                .is_empty(),
        "Orphan fills should be processed in some way"
    );
}

#[tokio::test]
async fn test_orphan_fills_for_unknown_instrument_skipped() {
    // Fills for instruments not in cache should be skipped gracefully
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
        None,
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        None,
    );

    mass_status.add_fill_reports(vec![orphan_fill]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    let has_unknown_fills = result.events.iter().any(|e| {
        if let OrderEventAny::Filled(f) = e {
            f.instrument_id == unknown_instrument_id
        } else {
            false
        }
    });

    assert!(
        !has_unknown_fills,
        "Fills for unknown instruments should be skipped"
    );
}

#[tokio::test]
async fn test_filtered_client_order_ids_skips_matching_orders() {
    // Orders in filtered_client_order_ids should be skipped during reconciliation
    let filtered_id = ClientOrderId::from("O-FILTERED-001");
    let config = ExecutionManagerConfig {
        filtered_client_order_ids: AHashSet::from([filtered_id]),
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
    let report = create_order_status_report(
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
        filtered_client_order_ids: AHashSet::from([filtered_id]),
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
        filtered_client_order_ids: AHashSet::from([filtered_id]),
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
        reconciliation_instrument_ids: AHashSet::from([included_instrument]),
        ..Default::default()
    };
    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    // Add a second instrument that's NOT in the filter list
    let excluded_instrument = test_instrument2();
    let excluded_instrument_id = test_instrument_id2();
    ctx.add_instrument(excluded_instrument.clone());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add order for included instrument
    let included_report = create_order_status_report(
        Some(ClientOrderId::from("O-INCLUDED-001")),
        VenueOrderId::from("V-INCLUDED-001"),
        included_instrument,
        OrderStatus::Accepted,
        Quantity::from("5.0"),
        Quantity::from("0.0"),
    );

    // Add order for excluded instrument
    let excluded_report = create_order_status_report(
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
        reconciliation_instrument_ids: AHashSet::from([included_instrument_id]),
        ..Default::default()
    };
    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    // Add excluded instrument
    let excluded_instrument = test_instrument2();
    let excluded_instrument_id = test_instrument_id2();
    ctx.add_instrument(excluded_instrument.clone());

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
        PositionSideSpecified::Long,
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
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

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
    client_id: ClientId,
    account_id: AccountId,
    venue: Venue,
    order_reports: RefCell<Vec<OrderStatusReport>>,
}

impl MockExecutionClient {
    fn new(order_reports: Vec<OrderStatusReport>) -> Self {
        Self {
            client_id: test_client_id(),
            account_id: test_account_id(),
            venue: test_venue(),
            order_reports: RefCell::new(order_reports),
        }
    }
}

#[async_trait(?Send)]
impl ExecutionClient for MockExecutionClient {
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

    fn oms_type(&self) -> OmsType {
        OmsType::Hedging
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
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn start(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn submit_order(&self, _cmd: &SubmitOrder) -> anyhow::Result<()> {
        Ok(())
    }

    fn submit_order_list(&self, _cmd: &SubmitOrderList) -> anyhow::Result<()> {
        Ok(())
    }

    fn modify_order(&self, _cmd: &ModifyOrder) -> anyhow::Result<()> {
        Ok(())
    }

    fn cancel_order(&self, _cmd: &CancelOrder) -> anyhow::Result<()> {
        Ok(())
    }

    fn cancel_all_orders(&self, _cmd: &CancelAllOrders) -> anyhow::Result<()> {
        Ok(())
    }

    fn batch_cancel_orders(&self, _cmd: &BatchCancelOrders) -> anyhow::Result<()> {
        Ok(())
    }

    fn query_account(&self, _cmd: &QueryAccount) -> anyhow::Result<()> {
        Ok(())
    }

    fn query_order(&self, _cmd: &QueryOrder) -> anyhow::Result<()> {
        Ok(())
    }

    async fn generate_order_status_reports(
        &self,
        _cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        Ok(self.order_reports.borrow().clone())
    }
}

#[rstest]
#[tokio::test]
async fn test_check_open_orders_defers_with_recent_local_activity() {
    // Test that reconciliation is deferred when there's recent local activity
    // within the threshold, to avoid race conditions with in-flight fills.
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: 200_000_000, // 200ms threshold
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

    let report = create_order_status_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("5.0"),
    )
    .with_avg_px(100.0)
    .unwrap();

    let mock_client = Rc::new(MockExecutionClient::new(vec![report]));
    let clients: Vec<Rc<dyn ExecutionClient>> = vec![mock_client];

    let events = ctx.manager.check_open_orders(&clients).await;

    assert!(
        events.is_empty(),
        "Reconciliation should be deferred with recent local activity"
    );
    let cached_order = ctx.get_order(&client_order_id).unwrap();
    assert_eq!(cached_order.status(), OrderStatus::Accepted);
    assert_eq!(cached_order.filled_qty(), Quantity::from("0.0"));
}

#[rstest]
#[tokio::test]
async fn test_check_open_orders_proceeds_after_threshold_exceeded() {
    // Test that reconciliation proceeds when the local activity is older than
    // the configured threshold.
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: 200_000_000, // 200ms threshold
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
    ctx.advance_time(500_000_000);

    let report = create_order_status_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("5.0"),
    )
    .with_avg_px(100.0)
    .unwrap();

    let mock_client = Rc::new(MockExecutionClient::new(vec![report]));
    let clients: Vec<Rc<dyn ExecutionClient>> = vec![mock_client];

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
#[tokio::test]
async fn test_check_open_orders_proceeds_without_local_activity() {
    // Test that reconciliation proceeds normally when there's no recorded
    // local activity for the order.
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: 200_000_000, // 200ms threshold
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

    let report = create_order_status_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("5.0"),
    )
    .with_avg_px(100.0)
    .unwrap();

    let mock_client = Rc::new(MockExecutionClient::new(vec![report]));
    let clients: Vec<Rc<dyn ExecutionClient>> = vec![mock_client];

    let events = ctx.manager.check_open_orders(&clients).await;

    assert_eq!(
        events.len(),
        1,
        "Reconciliation should proceed without local activity"
    );
    if let OrderEventAny::Filled(filled) = &events[0] {
        assert_eq!(filled.last_qty, Quantity::from("5.0"));
    } else {
        panic!("Expected OrderFilled event, was {:?}", events[0]);
    }
}
