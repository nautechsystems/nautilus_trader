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

//! Tests for sandbox execution client.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    clock::{Clock, TestClock},
    live::set_exec_event_sender,
    messages::{
        ExecutionEvent,
        execution::{
            BatchCancelOrders, BatchModifyOrders, CancelAllOrders, CancelOrder, ModifyOrder,
            SubmitOrder, SubmitOrderList, TradingCommand,
        },
    },
    msgbus::{
        self, MessageBus, MessagingSwitchboard, TypedHandler,
        stubs::get_typed_into_message_saving_handler, typed_handler::TypedIntoHandler,
    },
    runner::OrderEventDispatchGuard,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_data::engine::DataEngine;
#[cfg(feature = "python")]
use nautilus_execution::python::fee::PythonFeeModel;
use nautilus_execution::{
    client::core::ExecutionClientCore,
    engine::ExecutionEngine,
    models::{
        fee::{FeeModelAny, ProbabilityPriceFeeModel},
        fill::{DefaultFillModel, FillModel, FillModelAny, FillModelHandle},
        latency::{LatencyModelAny, StaticLatencyModel},
    },
};
use nautilus_model::{
    accounts::AccountAny,
    data::{Bar, BarType, Data, InstrumentClose, InstrumentStatus, QuoteTick, TradeTick},
    enums::{
        AccountType, AggressorSide, BookType, InstrumentCloseType, MarketStatusAction, OmsType,
        OrderSide, OrderStatus, OrderType, PositionSide, TimeInForce,
    },
    events::{
        AccountState, OrderDenied, OrderEventAny, OrderFilled, OrderPendingCancel,
        OrderPendingUpdate, PositionClosed, PositionEvent,
    },
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, OrderListId, PositionId, StrategyId,
        TradeId, TraderId, Venue,
    },
    instruments::{
        CryptoPerpetual, Instrument, InstrumentAny,
        stubs::{binary_option, crypto_perpetual_ethusdt},
    },
    orders::{Order, OrderAny, OrderList, OrderTestBuilder, stubs::TestOrderEventStubs},
    position::Position,
    types::{Currency, Money, Price, Quantity},
};
use nautilus_sandbox::{SandboxExecutionClient, SandboxExecutionClientConfig};
#[cfg(feature = "python")]
use pyo3::{IntoPyObjectExt, Python, ffi::c_str, types::PyAnyMethods};
use rstest::{fixture, rstest};
use rust_decimal::Decimal;
use ustr::Ustr;

#[fixture]
fn trader_id() -> TraderId {
    TraderId::from("SANDBOX-001")
}

#[fixture]
fn account_id() -> AccountId {
    AccountId::from("SANDBOX-001")
}

#[fixture]
fn venue() -> Venue {
    Venue::new("SIM")
}

#[fixture]
fn client_id() -> ClientId {
    ClientId::new("SANDBOX")
}

#[fixture]
fn instrument(crypto_perpetual_ethusdt: CryptoPerpetual) -> InstrumentAny {
    InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt)
}

fn create_config(
    _trader_id: TraderId,
    account_id: AccountId,
    venue: Venue,
) -> SandboxExecutionClientConfig {
    let usd = Currency::USD();
    SandboxExecutionClientConfig {
        account_id,
        venue,
        starting_balances: vec![Money::new(100_000.0, usd)],
        base_currency: Some(usd),
        oms_type: OmsType::Netting,
        account_type: AccountType::Margin,
        default_leverage: Decimal::ONE,
        leverages: ahash::AHashMap::new(),
        book_type: BookType::L1_MBP,
        fee_model: None,
        fill_model: None,
        latency_model: None,
        frozen_account: false,
        bar_execution: false,
        trade_execution: false,
        reject_stop_orders: true,
        support_gtd_orders: true,
        support_contingent_orders: true,
        use_position_ids: true,
        use_random_ids: false,
        use_reduce_only: true,
        queue_position: false,
        liquidity_consumption: false,
        bar_adaptive_high_low_ordering: false,
        use_market_order_acks: false,
        oto_full_trigger: false,
        price_protection_points: 0,
    }
}

#[fixture]
fn config(
    trader_id: TraderId,
    account_id: AccountId,
    venue: Venue,
) -> SandboxExecutionClientConfig {
    create_config(trader_id, account_id, venue)
}

/// Test context bundling execution client with shared cache for tests that need both
struct TestContext {
    client: SandboxExecutionClient,
    cache: Rc<RefCell<Cache>>,
}

fn create_test_context(trader_id: TraderId, account_id: AccountId, venue: Venue) -> TestContext {
    create_test_context_with(trader_id, account_id, venue, |_| {})
}

fn create_test_context_with_trade_execution(
    trader_id: TraderId,
    account_id: AccountId,
    venue: Venue,
) -> TestContext {
    create_test_context_with(trader_id, account_id, venue, |config| {
        config.trade_execution = true;
    })
}

fn create_test_context_with(
    trader_id: TraderId,
    account_id: AccountId,
    venue: Venue,
    customize: impl FnOnce(&mut SandboxExecutionClientConfig),
) -> TestContext {
    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let mut config = create_config(trader_id, account_id, venue);
    customize(&mut config);

    let core = ExecutionClientCore::new(
        trader_id,
        ClientId::new("SANDBOX"),
        config.venue,
        config.oms_type,
        config.account_id,
        config.account_type,
        config.base_currency,
        cache.clone(),
    );

    let client = SandboxExecutionClient::new(core, config, clock, cache.clone());
    TestContext { client, cache }
}

#[fixture]
fn test_context(trader_id: TraderId, account_id: AccountId, venue: Venue) -> TestContext {
    create_test_context(trader_id, account_id, venue)
}

#[fixture]
fn execution_client(test_context: TestContext) -> SandboxExecutionClient {
    test_context.client
}

fn create_quote_tick_with_price_precision(
    instrument_id: InstrumentId,
    bid: f64,
    ask: f64,
    price_precision: u8,
) -> QuoteTick {
    QuoteTick::new(
        instrument_id,
        Price::new(bid, price_precision),
        Price::new(ask, price_precision),
        Quantity::new(100.0, 3),
        Quantity::new(100.0, 3),
        UnixNanos::default(),
        UnixNanos::default(),
    )
}

fn create_quote_tick(instrument_id: InstrumentId, bid: f64, ask: f64) -> QuoteTick {
    // Use price precision 2 to match crypto_perpetual_ethusdt fixture.
    create_quote_tick_with_price_precision(instrument_id, bid, ask, 2)
}

fn create_mismatched_quote_tick(instrument_id: InstrumentId, bid: f64, ask: f64) -> QuoteTick {
    // Uses price precision 3 (instrument fixture uses 2), should be rejected by sandbox guard.
    create_quote_tick_with_price_precision(instrument_id, bid, ask, 3)
}

fn create_trade_tick_with_precision(
    instrument_id: InstrumentId,
    price: f64,
    size: f64,
    price_precision: u8,
    size_precision: u8,
) -> TradeTick {
    TradeTick::new(
        instrument_id,
        Price::new(price, price_precision),
        Quantity::new(size, size_precision),
        AggressorSide::Buy,
        TradeId::new("1"),
        UnixNanos::default(),
        UnixNanos::default(),
    )
}

fn create_mismatched_trade_tick(instrument_id: InstrumentId) -> TradeTick {
    // Uses price precision 3 (instrument fixture uses 2), should be rejected by sandbox guard.
    create_trade_tick_with_precision(instrument_id, 1000.0, 1.0, 3, 3)
}

fn make_binary_option_instrument(
    condition_id: &str,
    token_id: &str,
    outcome: &str,
    expiration_ns: u64,
) -> InstrumentAny {
    let mut binary = binary_option();
    let raw_symbol = format!("{condition_id}-{token_id}");
    binary.raw_symbol = raw_symbol.as_str().into();
    binary.id = InstrumentId::from(format!("{raw_symbol}.POLYMARKET").as_str());
    binary.activation_ns = UnixNanos::from(1);
    binary.expiration_ns = UnixNanos::from(expiration_ns);
    binary.outcome = Some(Ustr::from(outcome));
    InstrumentAny::BinaryOption(binary)
}

fn create_binary_option_quote(instrument_id: InstrumentId) -> QuoteTick {
    QuoteTick::new(
        instrument_id,
        Price::new(0.40, 3),
        Price::new(0.41, 3),
        Quantity::new(100.0, 2),
        Quantity::new(100.0, 2),
        UnixNanos::default(),
        UnixNanos::default(),
    )
}

fn submit_open_position_and_seed_cache(
    client: &SandboxExecutionClient,
    cache: &Rc<RefCell<Cache>>,
    trader_id: TraderId,
    instrument: &InstrumentAny,
    client_order_id: &str,
    position_id: &str,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
) -> Position {
    submit_market_open_order(client, cache, trader_id, instrument, client_order_id, 10);

    let mut filled = None;

    for event in std::iter::from_fn(|| rx.try_recv().ok()) {
        let ExecutionEvent::Order(OrderEventAny::Filled(fill)) = event else {
            continue;
        };

        if fill.client_order_id.as_str() == client_order_id {
            filled = Some(fill);
            break;
        }
    }

    let fill_event =
        OrderEventAny::Filled(filled.expect("expected opening fill from sandbox market order"));
    cache.borrow_mut().update_order(&fill_event).unwrap();

    let OrderEventAny::Filled(mut filled) = fill_event else {
        unreachable!("constructed filled order event");
    };
    filled.position_id = Some(PositionId::new(position_id));
    Position::new(instrument, filled)
}

fn position_closed_event(position: &Position, account_id: AccountId) -> PositionEvent {
    PositionEvent::PositionClosed(PositionClosed {
        trader_id: position.trader_id,
        strategy_id: position.strategy_id,
        instrument_id: position.instrument_id,
        position_id: position.id,
        account_id,
        opening_order_id: position.opening_order_id,
        closing_order_id: position.closing_order_id,
        entry: position.entry,
        side: PositionSide::Flat,
        signed_qty: 0.0,
        quantity: Quantity::zero(position.size_precision),
        peak_quantity: position.peak_qty,
        last_qty: Quantity::zero(position.size_precision),
        last_px: Price::zero(position.price_precision),
        currency: position.quote_currency,
        avg_px_open: position.avg_px_open,
        avg_px_close: position.avg_px_close,
        realized_return: position.realized_return,
        realized_pnl: position.realized_pnl,
        unrealized_pnl: Money::zero(position.quote_currency),
        duration: 1,
        event_id: UUID4::new(),
        ts_opened: position.ts_opened,
        ts_closed: position.ts_closed.or(Some(position.ts_last)),
        ts_event: position.ts_last,
        ts_init: position.ts_last,
    })
}

fn settle_position_from_expiration_fill(
    cache: &Rc<RefCell<Cache>>,
    position: &Position,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
) -> Position {
    let mut expiration_fill = None;

    for event in std::iter::from_fn(|| rx.try_recv().ok()) {
        let ExecutionEvent::Order(OrderEventAny::Filled(fill)) = event else {
            continue;
        };

        if fill.client_order_id.as_str().starts_with("EXPIRATION-") {
            expiration_fill = Some(fill);
            break;
        }
    }

    let expiration_fill = expiration_fill.expect("expected expiration fill after InstrumentClose");

    let mut closed = position.clone();
    closed.apply(&expiration_fill);
    cache.borrow_mut().update_position(&closed).unwrap();
    closed
}

fn apply_order_events_from_channel(
    cache: &Rc<RefCell<Cache>>,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
) -> Vec<OrderEventAny> {
    let mut order_events = Vec::new();

    for event in std::iter::from_fn(|| rx.try_recv().ok()) {
        let ExecutionEvent::Order(order_event) = event else {
            continue;
        };

        let _ = cache.borrow_mut().update_order(&order_event);
        order_events.push(order_event);
    }

    order_events
}

fn create_submit_order_list(
    trader_id: TraderId,
    client_id: ClientId,
    instrument_id: InstrumentId,
    orders: &[OrderAny],
) -> SubmitOrderList {
    let strategy_id = orders
        .first()
        .expect("expected non-empty order list")
        .strategy_id();
    let order_list = OrderList::new(
        OrderListId::from("OL-SANDBOX-001"),
        instrument_id,
        strategy_id,
        orders.iter().map(OrderAny::client_order_id).collect(),
        UnixNanos::default(),
    );

    SubmitOrderList {
        trader_id,
        client_id: Some(client_id),
        strategy_id,
        instrument_id,
        order_list,
        order_inits: orders
            .iter()
            .map(|order| order.init_event().clone())
            .collect(),
        exec_algorithm_id: None,
        position_id: None,
        params: None,
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        correlation_id: None,
        causation_id: None,
    }
}

fn seed_binary_option_position_from_fill(
    cache: &Rc<RefCell<Cache>>,
    instrument: &InstrumentAny,
    fill: OrderFilled,
    position_id: &str,
) {
    let mut fill = fill;
    fill.position_id = Some(PositionId::new(position_id));
    let position = Position::new(instrument, fill);
    cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();
}

fn submit_market_open_order(
    client: &SandboxExecutionClient,
    cache: &Rc<RefCell<Cache>>,
    trader_id: TraderId,
    instrument: &InstrumentAny,
    client_order_id: &str,
    ts_init: u64,
) {
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.00"))
        .client_order_id(client_order_id.into())
        .ts_init(UnixNanos::from(ts_init))
        .submit(true)
        .build();
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    client
        .submit_order(SubmitOrder::from_order(
            &order,
            trader_id,
            Some(client.client_id()),
            None,
            UUID4::new(),
            UnixNanos::from(ts_init),
        ))
        .unwrap();
}

struct BinaryOptionLifecycleHarness {
    client: SandboxExecutionClient,
    cache: Rc<RefCell<Cache>>,
    test_clock: Rc<RefCell<TestClock>>,
    instrument: InstrumentAny,
    rx: tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
}

fn setup_binary_option_lifecycle_harness(
    trader_id: TraderId,
    account_id: AccountId,
    condition_id: &str,
    token_id: &str,
    outcome: &str,
    expiration_ns: u64,
) -> BinaryOptionLifecycleHarness {
    let instrument = make_binary_option_instrument(condition_id, token_id, outcome, expiration_ns);
    let venue = instrument.id().venue;
    let cache = Rc::new(RefCell::new(Cache::default()));
    let test_clock = Rc::new(RefCell::new(TestClock::new()));
    let clock: Rc<RefCell<dyn Clock>> = test_clock.clone();
    let config = create_config(trader_id, account_id, venue);
    let core = ExecutionClientCore::new(
        trader_id,
        ClientId::new("SANDBOX"),
        config.venue,
        config.oms_type,
        config.account_id,
        config.account_type,
        config.base_currency,
        cache.clone(),
    );
    let mut client = SandboxExecutionClient::new(core, config, clock, cache.clone());
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();

    set_exec_event_sender(tx);
    cache
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();
    client.start().unwrap();
    client
        .process_quote_tick(&create_binary_option_quote(instrument.id()))
        .unwrap();

    BinaryOptionLifecycleHarness {
        client,
        cache,
        test_clock,
        instrument,
        rx,
    }
}

fn publish_expired_close(
    test_clock: &Rc<RefCell<TestClock>>,
    instrument: &InstrumentAny,
    close_price: Price,
    ts_ns: u64,
) {
    let _ = test_clock
        .borrow_mut()
        .advance_time(UnixNanos::from(ts_ns), true);

    let close = InstrumentClose::new(
        instrument.id(),
        close_price,
        InstrumentCloseType::ContractExpired,
        UnixNanos::from(ts_ns),
        UnixNanos::from(ts_ns),
    );
    msgbus::publish_any(
        nautilus_common::msgbus::switchboard::get_instrument_close_topic(instrument.id()),
        &close,
    );
}

struct PendingResolutionHarness {
    context: TestContext,
    instrument: InstrumentAny,
    clock: Rc<RefCell<dyn Clock>>,
    rx: tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
}

fn setup_pending_resolution_harness(
    trader_id: TraderId,
    account_id: AccountId,
    client_order_suffix: &str,
) -> PendingResolutionHarness {
    let mut binary = binary_option();
    binary.activation_ns = UnixNanos::from(1);
    binary.expiration_ns = UnixNanos::from(100);
    let instrument = InstrumentAny::BinaryOption(binary);
    let venue = instrument.id().venue;
    let cache = Rc::new(RefCell::new(Cache::default()));
    let test_clock = Rc::new(RefCell::new(TestClock::new()));
    let clock: Rc<RefCell<dyn Clock>> = test_clock.clone();

    let mut config = create_config(trader_id, account_id, venue);
    config.base_currency = Some(Currency::USDC());
    config.starting_balances = vec![Money::new(100_000.0, Currency::USDC())];
    let core = ExecutionClientCore::new(
        trader_id,
        ClientId::new("SANDBOX"),
        config.venue,
        config.oms_type,
        config.account_id,
        config.account_type,
        config.base_currency,
        cache.clone(),
    );
    let mut client = SandboxExecutionClient::new(core, config, clock.clone(), cache.clone());

    cache
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();
    let _ = test_clock
        .borrow_mut()
        .advance_time(UnixNanos::from(50), true);

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
    nautilus_common::live::runner::replace_exec_event_sender(tx);
    client.start().unwrap();

    let quote = QuoteTick::new(
        instrument.id(),
        Price::new(0.40, 3),
        Price::new(0.41, 3),
        Quantity::new(100.0, 2),
        Quantity::new(100.0, 2),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    client.process_quote_tick(&quote).unwrap();

    let position = submit_open_position_and_seed_cache(
        &client,
        &cache,
        trader_id,
        &instrument,
        &format!("OPEN-{client_order_suffix}"),
        &format!("P-{client_order_suffix}"),
        &mut rx,
    );
    cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();

    let resting_order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .price(Price::from("0.050"))
        .quantity(Quantity::from("1.00"))
        .client_order_id(format!("REST-{client_order_suffix}").into())
        .ts_init(UnixNanos::from(20))
        .submit(true)
        .build();
    cache
        .borrow_mut()
        .add_order(resting_order.clone(), None, None, false)
        .unwrap();
    client
        .submit_order(SubmitOrder::from_order(
            &resting_order,
            trader_id,
            Some(client.client_id()),
            None,
            UUID4::new(),
            UnixNanos::from(20),
        ))
        .unwrap();

    let _ = test_clock
        .borrow_mut()
        .advance_time(UnixNanos::from(200), true);

    let probe_order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .price(Price::from("0.050"))
        .quantity(Quantity::from("1.00"))
        .client_order_id(format!("PROBE-{client_order_suffix}").into())
        .ts_init(UnixNanos::from(200))
        .submit(true)
        .build();
    cache
        .borrow_mut()
        .add_order(probe_order.clone(), None, None, false)
        .unwrap();
    client
        .submit_order(SubmitOrder::from_order(
            &probe_order,
            trader_id,
            Some(client.client_id()),
            None,
            UUID4::new(),
            UnixNanos::from(200),
        ))
        .unwrap();

    PendingResolutionHarness {
        context: TestContext { client, cache },
        instrument,
        clock,
        rx,
    }
}

fn assert_pending_resolution_transition(
    harness: &mut PendingResolutionHarness,
    resting_order_id: &str,
    probe_order_id: &str,
) {
    let mut seen_resting_canceled = false;
    let mut seen_probe_rejected = false;

    for event in std::iter::from_fn(|| harness.rx.try_recv().ok()) {
        if let ExecutionEvent::Order(order_event) = event {
            match order_event {
                OrderEventAny::Canceled(c) if c.client_order_id.as_str() == resting_order_id => {
                    seen_resting_canceled = true;
                }
                OrderEventAny::Rejected(r) if r.client_order_id.as_str() == probe_order_id => {
                    seen_probe_rejected = r.reason.as_str().contains("pending resolution");
                }
                _ => {}
            }
        }
    }

    assert!(
        seen_resting_canceled,
        "expected resting order cancellation at pending_resolution boundary"
    );
    assert!(
        seen_probe_rejected,
        "expected probe order rejection with pending resolution reason"
    );
}

fn updated_instrument_with_price_precision_3(instrument: InstrumentAny) -> InstrumentAny {
    match instrument {
        InstrumentAny::CryptoPerpetual(mut crypto_perp) => {
            crypto_perp.price_precision = 3;
            crypto_perp.price_increment = Price::from("0.001");
            InstrumentAny::CryptoPerpetual(crypto_perp)
        }
        _ => panic!("Test fixture expected CryptoPerpetual instrument"),
    }
}

fn setup_order_event_handler() {
    let (handler, _saving_handler) = get_typed_into_message_saving_handler::<OrderEventAny>(Some(
        Ustr::from("ExecEngine.process"),
    ));
    msgbus::register_order_event_endpoint(MessagingSwitchboard::exec_engine_process(), handler);
}

fn setup_account_state_handler(cache: Rc<RefCell<Cache>>) {
    let handler = TypedHandler::from(move |state: &AccountState| {
        cache.borrow_mut().update_account_state(state).unwrap();
    });
    msgbus::register_account_state_endpoint(
        MessagingSwitchboard::portfolio_update_account(),
        handler,
    );
}

/// Bundles a started sandbox client wired to a retained `TestClock` (standing in for the live
/// clock) with the shared cache and exec-event channel, for the inbound-latency tests.
struct LatencyHarness {
    client: SandboxExecutionClient,
    cache: Rc<RefCell<Cache>>,
    test_clock: Rc<RefCell<TestClock>>,
    rx: tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    /// Everything `ExecEngine.process` has received, in arrival order: events the inbound drain
    /// flushed at its barrier plus whatever `settle` has forwarded from the channel.
    events: Rc<RefCell<Vec<OrderEventAny>>>,
}

/// Builds a started sandbox client under the given `latency_model` with `instrument` seeded in the
/// cache.
fn setup_latency_harness(
    trader_id: TraderId,
    account_id: AccountId,
    venue: Venue,
    instrument: &InstrumentAny,
    latency_model: Option<LatencyModelAny>,
) -> LatencyHarness {
    *msgbus::get_message_bus().borrow_mut() = MessageBus::default();

    let cache = Rc::new(RefCell::new(Cache::default()));
    let events: Rc<RefCell<Vec<OrderEventAny>>> = Rc::new(RefCell::new(Vec::new()));

    // Stands in for `ExecutionEngine::process`: the cache the matching engine reads is updated
    // here, so a command applied after a barrier flush sees the events before it.
    {
        let cache = cache.clone();
        let events = events.clone();
        msgbus::register_order_event_endpoint(
            MessagingSwitchboard::exec_engine_process(),
            TypedIntoHandler::from(move |event: OrderEventAny| {
                let _ = cache.borrow_mut().update_order(&event);
                events.borrow_mut().push(event);
            }),
        );
    }

    let test_clock = Rc::new(RefCell::new(TestClock::new()));
    let clock: Rc<RefCell<dyn Clock>> = test_clock.clone();

    let mut config = create_config(trader_id, account_id, venue);
    config.latency_model = latency_model;

    let core = ExecutionClientCore::new(
        trader_id,
        ClientId::new("SANDBOX"),
        config.venue,
        config.oms_type,
        config.account_id,
        config.account_type,
        config.base_currency,
        cache.clone(),
    );
    let mut client = SandboxExecutionClient::new(core, config, clock, cache.clone());

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
    nautilus_common::live::runner::replace_exec_event_sender(tx);

    cache
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();
    client.start().unwrap();

    LatencyHarness {
        client,
        cache,
        test_clock,
        rx,
        events,
    }
}

/// A `StaticLatencyModel` with a zero base and the given per-leg latencies (nanoseconds).
fn static_latency_model(insert_ns: u64, update_ns: u64, delete_ns: u64) -> LatencyModelAny {
    LatencyModelAny::Static(StaticLatencyModel::new(
        UnixNanos::default(),
        UnixNanos::from(insert_ns),
        UnixNanos::from(update_ns),
        UnixNanos::from(delete_ns),
    ))
}

/// Advances the test clock to `to`, running any inbound-drain alerts that fire exactly as the live
/// runner would (`advance_time` → `match_handlers` → `handler.run()`), and returns the number of
/// alert handlers that ran.
fn advance_and_fire(test_clock: &Rc<RefCell<TestClock>>, to: UnixNanos) -> usize {
    let events = test_clock.borrow_mut().advance_time(to, true);
    let handlers = test_clock.borrow().match_handlers(events);
    let count = handlers.len();
    for handler in handlers {
        handler.run();
    }
    count
}

/// Drains all currently-queued order events from the exec channel without mutating the cached order
/// state (so a deferred drain re-reads the order exactly as it was submitted).
fn drain_order_events(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
) -> Vec<OrderEventAny> {
    std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(|event| match event {
            ExecutionEvent::Order(order_event) => Some(order_event),
            _ => None,
        })
        .collect()
}

/// Forwards everything the execution channel is holding to `ExecEngine.process`, then returns every
/// event that endpoint has received since the last call, in arrival order.
fn settle(harness: &mut LatencyHarness) -> Vec<OrderEventAny> {
    let endpoint = MessagingSwitchboard::exec_engine_process();

    for event in std::iter::from_fn(|| harness.rx.try_recv().ok()) {
        if let ExecutionEvent::Order(order_event) = event {
            msgbus::send_order_event(endpoint, order_event);
        }
    }

    harness.events.borrow_mut().drain(..).collect()
}

/// Returns the events that reached `ExecEngine.process` without the runner draining the execution
/// channel - the settlement barrier's own output.
fn flushed_at_barrier(harness: &LatencyHarness) -> Vec<OrderEventAny> {
    harness.events.borrow_mut().drain(..).collect()
}

/// Submits a resting buy limit order (far from any market, so it accepts rather than fills) through
/// the sandbox client, adding it to the cache first, and returns the built order.
fn submit_resting_limit(
    harness: &LatencyHarness,
    trader_id: TraderId,
    instrument: &InstrumentAny,
    client_order_id: &str,
    price: &str,
    ts: UnixNanos,
) -> OrderAny {
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .price(Price::from(price))
        .quantity(Quantity::from("1.000"))
        .client_order_id(client_order_id.into())
        .ts_init(ts)
        .build();
    // Not `.submit(true)`: that stub stamps `ACCOUNT-001`, while `process_cancel_all` filters the
    // cache by the client's own account.
    harness
        .cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    harness
        .client
        .submit_order(SubmitOrder::from_order(
            &order,
            trader_id,
            Some(harness.client.client_id()),
            None,
            UUID4::new(),
            ts,
        ))
        .unwrap();
    order
}

/// Builds a buy limit order whose instrument is deliberately absent from the harness cache, adds it
/// to the cache, and returns it, for the deferred no-engine-guard rejection tests.
fn build_uncached_instrument_order(
    harness: &LatencyHarness,
    client_order_id: &str,
    ts: UnixNanos,
) -> OrderAny {
    let uncached_instrument_id = InstrumentId::from("UNKNOWN-PERP.BINANCE");
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(uncached_instrument_id)
        .side(OrderSide::Buy)
        .price(Price::from("100.00"))
        .quantity(Quantity::from("1.000"))
        .client_order_id(client_order_id.into())
        .ts_init(ts)
        .submit(true)
        .build();
    harness
        .cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    order
}

/// Applies an `OrderPendingCancel` event to `order` in the cache, mirroring the state
/// `Strategy::cancel_order` / `cancel_orders` establish before their cancel command reaches the
/// execution client, so a synthesized `CancelRejected` later has a valid FSM transition.
fn mark_pending_cancel(
    cache: &Rc<RefCell<Cache>>,
    order: &OrderAny,
    trader_id: TraderId,
    ts: UnixNanos,
) {
    let event = OrderEventAny::PendingCancel(OrderPendingCancel::new(
        trader_id,
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        order.account_id(),
        UUID4::new(),
        ts,
        ts,
        false,
        order.venue_order_id(),
    ));
    cache.borrow_mut().update_order(&event).unwrap();
}

/// Applies an `OrderPendingUpdate` event to `order` in the cache, mirroring the state
/// `Strategy::modify_order` establishes before its modify command reaches the execution client, so
/// a synthesized `ModifyRejected` later has a valid FSM transition.
fn mark_pending_update(
    cache: &Rc<RefCell<Cache>>,
    order: &OrderAny,
    trader_id: TraderId,
    ts: UnixNanos,
) {
    let event = OrderEventAny::PendingUpdate(OrderPendingUpdate::new(
        trader_id,
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        order.account_id(),
        UUID4::new(),
        ts,
        ts,
        false,
        order.venue_order_id(),
    ));
    cache.borrow_mut().update_order(&event).unwrap();
}

/// The order-targeting commands that share the no-engine guard in `apply_trading_command`.
#[derive(Debug, Clone, Copy)]
enum DeferredCommand {
    Cancel,
    Modify,
    BatchCancel,
    BatchModify,
}

impl DeferredCommand {
    /// A latency model routing this command's own leg to `leg_ns` and the insert leg to
    /// `insert_ns`, so a submit can be held in flight behind it.
    fn latency_model(self, insert_ns: u64, leg_ns: u64) -> LatencyModelAny {
        match self {
            Self::Cancel | Self::BatchCancel => static_latency_model(insert_ns, 0, leg_ns),
            Self::Modify | Self::BatchModify => static_latency_model(insert_ns, leg_ns, 0),
        }
    }

    /// How many orders the command targets: one for the single variants, both for the batches.
    fn target_count(self) -> usize {
        match self {
            Self::Cancel | Self::Modify => 1,
            Self::BatchCancel | Self::BatchModify => 2,
        }
    }

    /// Marks each target with the pending status the `Strategy` establishes before sending this
    /// command, so a synthesized rejection has a valid FSM transition to make.
    fn mark_pending(
        self,
        cache: &Rc<RefCell<Cache>>,
        order: &OrderAny,
        trader: TraderId,
        ts: UnixNanos,
    ) {
        match self {
            Self::Cancel | Self::BatchCancel => mark_pending_cancel(cache, order, trader, ts),
            Self::Modify | Self::BatchModify => mark_pending_update(cache, order, trader, ts),
        }
    }

    /// Whether `events` carries the venue's response to this command for `client_order_id`.
    fn applied_to(self, events: &[OrderEventAny], client_order_id: ClientOrderId) -> bool {
        events.iter().any(|event| match (self, event) {
            (Self::Cancel | Self::BatchCancel, OrderEventAny::Canceled(canceled)) => {
                canceled.client_order_id == client_order_id
            }
            (Self::Modify | Self::BatchModify, OrderEventAny::Updated(updated)) => {
                updated.client_order_id == client_order_id
            }
            _ => false,
        })
    }

    /// Client order IDs this command's rejection event names, in dispatch order.
    fn rejected_ids(self, events: &[OrderEventAny]) -> Vec<ClientOrderId> {
        events
            .iter()
            .filter_map(|event| match (self, event) {
                (Self::Cancel | Self::BatchCancel, OrderEventAny::CancelRejected(rejected)) => {
                    Some(rejected.client_order_id)
                }
                (Self::Modify | Self::BatchModify, OrderEventAny::ModifyRejected(rejected)) => {
                    Some(rejected.client_order_id)
                }
                _ => None,
            })
            .collect()
    }
}

/// Builds a `ModifyOrder` amending `order` to a fixed price, as `Strategy::modify_order` would.
fn modify_for(
    harness: &LatencyHarness,
    trader_id: TraderId,
    order: &OrderAny,
    ts: UnixNanos,
) -> ModifyOrder {
    ModifyOrder::new(
        trader_id,
        Some(harness.client.client_id()),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        None,
        None,
        Some(Price::from("99.00")),
        None,
        UUID4::new(),
        ts,
        None,
        None,
    )
}

/// Builds a `CancelOrder` for `order`, as `Strategy::cancel_order` would.
fn cancel_for(
    harness: &LatencyHarness,
    trader_id: TraderId,
    order: &OrderAny,
    ts: UnixNanos,
) -> CancelOrder {
    CancelOrder::new(
        trader_id,
        Some(harness.client.client_id()),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        None,
        UUID4::new(),
        ts,
        None,
        None,
    )
}

/// Sends `kind` through the client as a single command targeting every order in `targets`.
fn send_deferred_command(
    harness: &LatencyHarness,
    trader_id: TraderId,
    kind: DeferredCommand,
    targets: &[OrderAny],
    ts: UnixNanos,
) {
    let instrument_id = targets[0].instrument_id();
    let strategy_id = targets[0].strategy_id();
    let client_id = Some(harness.client.client_id());

    match kind {
        DeferredCommand::Cancel => harness
            .client
            .cancel_order(cancel_for(harness, trader_id, &targets[0], ts))
            .unwrap(),
        DeferredCommand::Modify => harness
            .client
            .modify_order(modify_for(harness, trader_id, &targets[0], ts))
            .unwrap(),
        DeferredCommand::BatchCancel => harness
            .client
            .batch_cancel_orders(BatchCancelOrders::new(
                trader_id,
                client_id,
                strategy_id,
                instrument_id,
                targets
                    .iter()
                    .map(|order| cancel_for(harness, trader_id, order, ts))
                    .collect(),
                UUID4::new(),
                ts,
                None,
                None,
            ))
            .unwrap(),
        DeferredCommand::BatchModify => harness
            .client
            .batch_modify_orders(BatchModifyOrders::new(
                trader_id,
                client_id,
                strategy_id,
                instrument_id,
                targets
                    .iter()
                    .map(|order| modify_for(harness, trader_id, order, ts))
                    .collect(),
                UUID4::new(),
                ts,
                None,
                None,
            ))
            .unwrap(),
    }
}

#[rstest]
fn test_config_default() {
    let config = SandboxExecutionClientConfig::default();

    assert_eq!(config.account_id, AccountId::from("SANDBOX-001"));
    assert_eq!(config.venue, Venue::new("SANDBOX"));
    assert!(config.starting_balances.is_empty());
    assert!(config.base_currency.is_none());
    assert_eq!(config.oms_type, OmsType::Netting);
    assert_eq!(config.account_type, AccountType::Margin);
    assert_eq!(config.default_leverage, Decimal::ONE);
    assert_eq!(config.book_type, BookType::L1_MBP);
    assert!(config.fee_model.is_none());
    assert!(config.fill_model.is_none());
    assert!(!config.frozen_account);
    assert!(config.bar_execution);
    assert!(config.trade_execution);
    assert!(config.reject_stop_orders);
    assert!(config.support_gtd_orders);
    assert!(config.support_contingent_orders);
    assert!(config.use_position_ids);
    assert!(!config.use_random_ids);
    assert!(config.use_reduce_only);
    assert!(!config.queue_position);
    assert!(!config.liquidity_consumption);
    assert!(!config.bar_adaptive_high_low_ordering);
    assert!(!config.use_market_order_acks);
    assert!(!config.oto_full_trigger);
    assert_eq!(config.price_protection_points, 0);
}

#[rstest]
#[case::sports_p50("0.03", "0.500", "0.00750")]
#[case::sports_p30("0.03", "0.300", "0.00630")]
#[case::crypto_p97("0.072", "0.970", "0.00210")]
fn test_probability_price_fee_model_config_drives_sandbox_commission(
    #[case] taker_fee: &str,
    #[case] price: &str,
    #[case] expected: &str,
    trader_id: TraderId,
    account_id: AccountId,
) {
    assert_fee_model_config_drives_sandbox_commission(
        FeeModelAny::ProbabilityPrice(ProbabilityPriceFeeModel),
        taker_fee,
        price,
        expected,
        trader_id,
        account_id,
    );
}

#[cfg(feature = "python")]
#[rstest]
fn test_python_fee_model_config_drives_sandbox_commission(
    trader_id: TraderId,
    account_id: AccountId,
) {
    Python::initialize();

    let expected = "1.234567";
    let fee_model = Python::attach(|py| {
        let model = py
            .eval(
                c_str!(
                    "type('CustomFeeModel', (), {\
                        'get_commission': \
                            lambda self, order, fill_quantity, fill_px, instrument: self.commission\
                    })()"
                ),
                None,
                None,
            )
            .unwrap();
        model
            .setattr(
                "commission",
                Money::from(format!("{expected} USDC").as_str())
                    .into_py_any(py)
                    .unwrap(),
            )
            .unwrap();

        FeeModelAny::Python(PythonFeeModel::new(model.unbind()))
    });

    assert_fee_model_config_drives_sandbox_commission(
        fee_model, "0.03", "0.500", expected, trader_id, account_id,
    );
}

fn assert_fee_model_config_drives_sandbox_commission(
    fee_model: FeeModelAny,
    taker_fee: &str,
    price: &str,
    expected: &str,
    trader_id: TraderId,
    account_id: AccountId,
) {
    setup_order_event_handler();

    let mut binary = binary_option();
    binary.taker_fee = Decimal::from_str_exact(taker_fee).unwrap();
    let instrument = InstrumentAny::BinaryOption(binary);
    let venue = instrument.id().venue;
    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));

    let mut config = create_config(trader_id, account_id, venue);
    config.base_currency = Some(Currency::USDC());
    config.starting_balances = vec![Money::new(100_000.0, Currency::USDC())];
    config.fee_model = Some(fee_model);

    let core = ExecutionClientCore::new(
        trader_id,
        ClientId::new("SANDBOX"),
        config.venue,
        config.oms_type,
        config.account_id,
        config.account_type,
        config.base_currency,
        cache.clone(),
    );
    let mut client = SandboxExecutionClient::new(core, config, clock, cache.clone());

    cache
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
    set_exec_event_sender(tx);
    client.start().unwrap();

    let quote = QuoteTick::new(
        instrument.id(),
        Price::from(price),
        Price::from(price),
        Quantity::new(100.0, 2),
        Quantity::new(100.0, 2),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    client.process_quote_tick(&quote).unwrap();

    submit_market_open_order(&client, &cache, trader_id, &instrument, "OPEN-FEE", 10);

    let mut fill_commission = None;

    for event in std::iter::from_fn(|| rx.try_recv().ok()) {
        let ExecutionEvent::Order(OrderEventAny::Filled(fill)) = event else {
            continue;
        };

        if fill.client_order_id.as_str() == "OPEN-FEE" {
            fill_commission = fill.commission;
        }
    }

    assert_eq!(
        fill_commission,
        Some(Money::from(format!("{expected} USDC").as_str()))
    );
}

#[rstest]
fn test_config_builder(account_id: AccountId, venue: Venue) {
    let usd = Currency::USD();
    let starting_balances = vec![Money::new(50_000.0, usd)];

    let config = SandboxExecutionClientConfig::builder()
        .account_id(account_id)
        .venue(venue)
        .starting_balances(starting_balances)
        .build();

    assert_eq!(config.account_id, account_id);
    assert_eq!(config.venue, venue);
    assert_eq!(config.starting_balances.len(), 1);
    assert_eq!(config.starting_balances[0].as_f64(), 50_000.0);
}

#[rstest]
fn test_config_builder_with_overrides(account_id: AccountId, venue: Venue) {
    let usd = Currency::USD();
    let starting_balances = vec![Money::new(50_000.0, usd)];

    let config = SandboxExecutionClientConfig::builder()
        .account_id(account_id)
        .venue(venue)
        .starting_balances(starting_balances)
        .base_currency(usd)
        .oms_type(OmsType::Hedging)
        .account_type(AccountType::Cash)
        .default_leverage(Decimal::new(10, 0))
        .book_type(BookType::L2_MBP)
        .frozen_account(true)
        .bar_execution(false)
        .trade_execution(true)
        .build();

    assert_eq!(config.base_currency, Some(usd));
    assert_eq!(config.oms_type, OmsType::Hedging);
    assert_eq!(config.account_type, AccountType::Cash);
    assert_eq!(config.default_leverage, Decimal::new(10, 0));
    assert_eq!(config.book_type, BookType::L2_MBP);
    assert!(config.frozen_account);
    assert!(!config.bar_execution);
    assert!(config.trade_execution);
}

#[rstest]
fn test_config_to_matching_engine_config(config: SandboxExecutionClientConfig) {
    let engine_config = config.to_matching_engine_config();

    assert!(!engine_config.bar_execution);
    assert!(!engine_config.trade_execution);
    assert!(engine_config.reject_stop_orders);
    assert!(engine_config.support_gtd_orders);
    assert!(engine_config.support_contingent_orders);
    assert!(engine_config.use_position_ids);
    assert!(!engine_config.use_random_ids);
    assert!(engine_config.use_reduce_only);
    assert!(!engine_config.queue_position);
    assert!(!engine_config.liquidity_consumption);
    assert!(!engine_config.bar_adaptive_high_low_ordering);
    assert!(!engine_config.use_market_order_acks);
    assert!(!engine_config.oto_full_trigger);
    assert_eq!(engine_config.price_protection_points, None);
}

#[rstest]
#[case::queue_on(true, false)]
#[case::queue_off(false, true)]
fn test_queue_position_gates_trade_driven_limit_fill(
    #[case] queue_position: bool,
    #[case] expect_fill: bool,
    trader_id: TraderId,
    account_id: AccountId,
    instrument: InstrumentAny,
) {
    setup_order_event_handler();

    let venue = instrument.id().venue;
    let mut test_context = create_test_context_with(trader_id, account_id, venue, |config| {
        config.trade_execution = true;
        config.queue_position = queue_position;
    });
    let cache = test_context.cache.clone();
    cache
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
    set_exec_event_sender(tx);
    test_context.client.start().unwrap();

    let quote = create_quote_tick(instrument.id(), 1000.0, 1001.0);
    test_context.client.process_quote_tick(&quote).unwrap();

    let order = OrderTestBuilder::new(OrderType::Limit)
        .trader_id(trader_id)
        .instrument_id(instrument.id())
        .client_order_id(ClientOrderId::from("QUEUE-LIMIT-001"))
        .side(OrderSide::Buy)
        .price(Price::from("1000.00"))
        .quantity(Quantity::from("1.000"))
        .submit(true)
        .build();
    cache
        .borrow_mut()
        .add_order(order.clone(), None, Some(ClientId::new("SANDBOX")), false)
        .unwrap();
    test_context
        .client
        .submit_order(SubmitOrder::from_order(
            &order,
            trader_id,
            Some(test_context.client.client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
        ))
        .unwrap();

    let accepted_events = apply_order_events_from_channel(&cache, &mut rx);
    assert!(
        accepted_events
            .iter()
            .any(|event| matches!(event, OrderEventAny::Accepted(_))),
        "expected limit order acceptance before the queue-reducing trade",
    );
    assert!(
        accepted_events
            .iter()
            .all(|event| !matches!(event, OrderEventAny::Filled(_))),
        "limit order must rest behind displayed bid size",
    );

    let trade = TradeTick::new(
        instrument.id(),
        Price::from("1000.00"),
        Quantity::from("10.000"),
        AggressorSide::Sell,
        TradeId::new("T-QUEUE-1"),
        UnixNanos::from(1),
        UnixNanos::from(1),
    );
    test_context.client.process_trade_tick(&trade).unwrap();

    let trade_events = apply_order_events_from_channel(&cache, &mut rx);
    let filled = trade_events
        .iter()
        .any(|event| matches!(event, OrderEventAny::Filled(_)));

    assert_eq!(
        filled, expect_fill,
        "queue_position={queue_position}: a 10-unit sell into 100 displayed bid units \
         must fill the resting 1-unit buy only when queue tracking is off",
    );
}

#[rstest]
#[case::consumption_on(true, 1)]
#[case::consumption_off(false, 2)]
fn test_liquidity_consumption_shares_trade_size_across_limits(
    #[case] liquidity_consumption: bool,
    #[case] expected_fills: usize,
    trader_id: TraderId,
    account_id: AccountId,
    instrument: InstrumentAny,
) {
    setup_order_event_handler();

    let venue = instrument.id().venue;
    let mut test_context = create_test_context_with(trader_id, account_id, venue, |config| {
        config.trade_execution = true;
        config.liquidity_consumption = liquidity_consumption;
    });
    let cache = test_context.cache.clone();
    cache
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
    set_exec_event_sender(tx);
    test_context.client.start().unwrap();

    let quote = create_quote_tick(instrument.id(), 1000.0, 1001.0);
    test_context.client.process_quote_tick(&quote).unwrap();

    for (idx, client_order_id) in ["CONS-LIMIT-001", "CONS-LIMIT-002"].iter().enumerate() {
        let order = OrderTestBuilder::new(OrderType::Limit)
            .trader_id(trader_id)
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from(*client_order_id))
            .side(OrderSide::Buy)
            .price(Price::from("1000.00"))
            .quantity(Quantity::from("1.000"))
            .submit(true)
            .build();
        cache
            .borrow_mut()
            .add_order(order.clone(), None, Some(ClientId::new("SANDBOX")), false)
            .unwrap();
        test_context
            .client
            .submit_order(SubmitOrder::from_order(
                &order,
                trader_id,
                Some(test_context.client.client_id()),
                None,
                UUID4::new(),
                UnixNanos::from(idx as u64),
            ))
            .unwrap();
    }

    let accepted_events = apply_order_events_from_channel(&cache, &mut rx);
    assert_eq!(
        accepted_events
            .iter()
            .filter(|event| matches!(event, OrderEventAny::Accepted(_)))
            .count(),
        2,
        "expected both limit orders to rest before the shared trade",
    );

    let trade = TradeTick::new(
        instrument.id(),
        Price::from("1000.00"),
        Quantity::from("1.000"),
        AggressorSide::Sell,
        TradeId::new("T-CONS-1"),
        UnixNanos::from(10),
        UnixNanos::from(10),
    );
    test_context.client.process_trade_tick(&trade).unwrap();

    let fill_count = apply_order_events_from_channel(&cache, &mut rx)
        .iter()
        .filter(|event| matches!(event, OrderEventAny::Filled(_)))
        .count();

    assert_eq!(
        fill_count, expected_fills,
        "liquidity_consumption={liquidity_consumption}: a 1-unit sell must fill \
         one resting 1-unit buy when consumption is on, and both when it is off",
    );
}

#[rstest]
fn test_fill_model_can_block_limit_touch_fills(
    trader_id: TraderId,
    account_id: AccountId,
    instrument: InstrumentAny,
) {
    setup_order_event_handler();

    let venue = instrument.id().venue;
    let mut test_context = create_test_context_with(trader_id, account_id, venue, |config| {
        config.trade_execution = true;
        config.fill_model = Some(FillModelAny::Default(
            DefaultFillModel::new(0.0, 0.0, Some(1)).unwrap(),
        ));
    });
    let cache = test_context.cache.clone();
    cache
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
    set_exec_event_sender(tx);
    test_context.client.start().unwrap();

    let quote = create_quote_tick(instrument.id(), 1000.0, 1001.0);
    test_context.client.process_quote_tick(&quote).unwrap();

    let order = OrderTestBuilder::new(OrderType::Limit)
        .trader_id(trader_id)
        .instrument_id(instrument.id())
        .client_order_id(ClientOrderId::from("FILL-MODEL-LIMIT-001"))
        .side(OrderSide::Buy)
        .price(Price::from("1000.00"))
        .quantity(Quantity::from("1.000"))
        .submit(true)
        .build();
    cache
        .borrow_mut()
        .add_order(order.clone(), None, Some(ClientId::new("SANDBOX")), false)
        .unwrap();
    test_context
        .client
        .submit_order(SubmitOrder::from_order(
            &order,
            trader_id,
            Some(test_context.client.client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
        ))
        .unwrap();
    let _ = apply_order_events_from_channel(&cache, &mut rx);

    let trade = TradeTick::new(
        instrument.id(),
        Price::from("1000.00"),
        Quantity::from("1.000"),
        AggressorSide::Sell,
        TradeId::new("T-FILL-MODEL-1"),
        UnixNanos::from(1),
        UnixNanos::from(1),
    );
    test_context.client.process_trade_tick(&trade).unwrap();

    let trade_events = apply_order_events_from_channel(&cache, &mut rx);
    assert!(
        trade_events
            .iter()
            .all(|event| !matches!(event, OrderEventAny::Filled(_))),
        "prob_fill_on_limit=0 must not fill a limit order on touch",
    );
}

fn first_two_shared_limit_fills(seed: u64) -> [bool; 2] {
    let mut handle = FillModelHandle::from(FillModelAny::Default(
        DefaultFillModel::new(0.5, 0.0, Some(seed)).unwrap(),
    ));

    [
        handle.is_limit_filled().unwrap(),
        handle.is_limit_filled().unwrap(),
    ]
}

fn first_fills_from_independent_handles(seed: u64) -> [bool; 2] {
    let model = DefaultFillModel::new(0.5, 0.0, Some(seed)).unwrap();
    let mut first = FillModelHandle::from(FillModelAny::Default(model.clone()));
    let mut second = FillModelHandle::from(FillModelAny::Default(model));

    [
        first.is_limit_filled().unwrap(),
        second.is_limit_filled().unwrap(),
    ]
}

fn process_limit_touch_fill(
    test_context: &TestContext,
    trader_id: TraderId,
    instrument: &InstrumentAny,
    client_order_id: &str,
    trade_id: &str,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
) -> bool {
    let cache = test_context.cache.clone();
    cache
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();

    let quote = create_quote_tick(instrument.id(), 1000.0, 1001.0);
    test_context.client.process_quote_tick(&quote).unwrap();

    let order = OrderTestBuilder::new(OrderType::Limit)
        .trader_id(trader_id)
        .instrument_id(instrument.id())
        .client_order_id(ClientOrderId::from(client_order_id))
        .side(OrderSide::Buy)
        .price(Price::from("1000.00"))
        .quantity(Quantity::from("1.000"))
        .submit(true)
        .build();
    cache
        .borrow_mut()
        .add_order(order.clone(), None, Some(ClientId::new("SANDBOX")), false)
        .unwrap();
    test_context
        .client
        .submit_order(SubmitOrder::from_order(
            &order,
            trader_id,
            Some(test_context.client.client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
        ))
        .unwrap();
    let _ = apply_order_events_from_channel(&cache, rx);

    let trade = TradeTick::new(
        instrument.id(),
        Price::from("1000.00"),
        Quantity::from("1.000"),
        AggressorSide::Sell,
        TradeId::new(trade_id),
        UnixNanos::from(1),
        UnixNanos::from(1),
    );
    test_context.client.process_trade_tick(&trade).unwrap();

    apply_order_events_from_channel(&cache, rx)
        .iter()
        .any(|event| matches!(event, OrderEventAny::Filled(_)))
}

#[rstest]
fn test_seeded_fill_model_is_shared_across_instruments(
    trader_id: TraderId,
    account_id: AccountId,
    instrument: InstrumentAny,
) {
    setup_order_event_handler();

    // Seed 42's first two draws match; pick a seed where shared vs cloned From() diverge.
    let seed = (0u64..64)
        .find(|&seed| {
            first_two_shared_limit_fills(seed) != first_fills_from_independent_handles(seed)
        })
        .expect("a seed in 0..64 must discriminate shared vs cloned From() draws");
    let expected_shared = first_two_shared_limit_fills(seed);
    let expected_independent = first_fills_from_independent_handles(seed);

    assert_ne!(expected_shared, expected_independent);

    let venue = instrument.id().venue;
    let other = match instrument.clone() {
        InstrumentAny::CryptoPerpetual(mut perp) => {
            perp.id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
            InstrumentAny::CryptoPerpetual(perp)
        }
        other => panic!("expected crypto perpetual fixture, was {other:?}"),
    };

    let mut test_context = create_test_context_with(trader_id, account_id, venue, |config| {
        config.trade_execution = true;
        config.fill_model = Some(FillModelAny::Default(
            DefaultFillModel::new(0.5, 0.0, Some(seed)).unwrap(),
        ));
    });

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
    set_exec_event_sender(tx);
    test_context.client.start().unwrap();

    let first_filled = process_limit_touch_fill(
        &test_context,
        trader_id,
        &instrument,
        "SHARED-FILL-ETH-001",
        "T-SHARED-ETH-1",
        &mut rx,
    );
    let second_filled = process_limit_touch_fill(
        &test_context,
        trader_id,
        &other,
        "SHARED-FILL-BTC-001",
        "T-SHARED-BTC-1",
        &mut rx,
    );
    let observed = [first_filled, second_filled];

    assert_eq!(test_context.client.matching_engine_count(), 2);
    assert_eq!(observed, expected_shared);
    assert_ne!(observed, expected_independent);
}

#[rstest]
fn test_client_initial_state(execution_client: SandboxExecutionClient, venue: Venue) {
    assert!(!execution_client.is_connected());
    assert_eq!(execution_client.venue(), venue);
    assert_eq!(execution_client.oms_type(), OmsType::Netting);
    assert_eq!(execution_client.matching_engine_count(), 0);
}

#[rstest]
fn test_client_start(mut execution_client: SandboxExecutionClient) {
    setup_order_event_handler();

    let result = execution_client.start();

    assert!(result.is_ok());
    assert!(!execution_client.is_connected());
}

#[rstest]
fn test_client_start_idempotent(mut execution_client: SandboxExecutionClient) {
    setup_order_event_handler();

    execution_client.start().unwrap();
    let result = execution_client.start();

    assert!(result.is_ok());
}

#[rstest]
#[tokio::test]
async fn test_client_connect(mut execution_client: SandboxExecutionClient) {
    setup_order_event_handler();

    let result = execution_client.connect().await;

    assert!(result.is_ok());
    assert!(execution_client.is_connected());
}

#[rstest]
#[tokio::test]
async fn test_client_connect_syncs_cached_margin_account_config(
    trader_id: TraderId,
    account_id: AccountId,
    venue: Venue,
    instrument: InstrumentAny,
) {
    *msgbus::get_message_bus().borrow_mut() = MessageBus::default();

    let leverage = Decimal::from(5);
    let instrument_id = instrument.id();
    let context = create_test_context_with(trader_id, account_id, venue, |config| {
        config.default_leverage = leverage;
        config.leverages.insert(instrument_id, leverage);
    });
    setup_account_state_handler(context.cache.clone());

    let mut execution_client = context.client;
    execution_client.connect().await.unwrap();

    let cache = context.cache.borrow();
    let account = cache
        .account(&account_id)
        .expect("expected cached account after initial AccountState");

    let AccountAny::Margin(margin) = &*account else {
        panic!("expected margin account");
    };

    assert!(margin.base.calculate_account_state);
    assert_eq!(margin.default_leverage, leverage);
    assert_eq!(margin.get_leverage(&instrument_id), leverage);
}

#[rstest]
#[tokio::test]
async fn test_client_connect_respects_frozen_account_config(
    trader_id: TraderId,
    account_id: AccountId,
    venue: Venue,
) {
    *msgbus::get_message_bus().borrow_mut() = MessageBus::default();

    let context = create_test_context_with(trader_id, account_id, venue, |config| {
        config.frozen_account = true;
    });
    setup_account_state_handler(context.cache.clone());

    let mut execution_client = context.client;
    execution_client.connect().await.unwrap();

    let cache = context.cache.borrow();
    let account = cache
        .account(&account_id)
        .expect("expected cached account after initial AccountState");

    let AccountAny::Margin(margin) = &*account else {
        panic!("expected margin account");
    };

    assert!(!margin.base.calculate_account_state);
}

#[rstest]
#[tokio::test]
async fn test_client_connect_idempotent(mut execution_client: SandboxExecutionClient) {
    setup_order_event_handler();

    execution_client.connect().await.unwrap();
    let result = execution_client.connect().await;

    assert!(result.is_ok());
    assert!(execution_client.is_connected());
}

#[rstest]
#[tokio::test]
async fn test_client_disconnect(mut execution_client: SandboxExecutionClient) {
    setup_order_event_handler();

    execution_client.connect().await.unwrap();
    let result = execution_client.disconnect().await;

    assert!(result.is_ok());
    assert!(!execution_client.is_connected());
}

#[rstest]
#[tokio::test]
async fn test_client_disconnect_when_not_connected(mut execution_client: SandboxExecutionClient) {
    setup_order_event_handler();

    let result = execution_client.disconnect().await;

    assert!(result.is_ok());
    assert!(!execution_client.is_connected());
}

#[rstest]
#[tokio::test]
async fn test_client_stop(mut execution_client: SandboxExecutionClient) {
    setup_order_event_handler();

    execution_client.start().unwrap();
    execution_client.connect().await.unwrap();
    let result = execution_client.stop();

    assert!(result.is_ok());
    assert!(!execution_client.is_connected());
}

#[rstest]
fn test_client_stop_when_not_started(mut execution_client: SandboxExecutionClient) {
    setup_order_event_handler();

    let result = execution_client.stop();

    assert!(result.is_ok());
}

#[rstest]
fn test_paper_binary_option_pending_resolution_then_close_settlement(
    trader_id: TraderId,
    account_id: AccountId,
) {
    let mut harness = setup_pending_resolution_harness(trader_id, account_id, "BO-PAPER");
    assert_pending_resolution_transition(&mut harness, "REST-BO-PAPER", "PROBE-BO-PAPER");

    let close = InstrumentClose::new(
        harness.instrument.id(),
        Price::from("1.000"),
        InstrumentCloseType::ContractExpired,
        UnixNanos::from(300),
        UnixNanos::from(300),
    );
    msgbus::publish_any(
        nautilus_common::msgbus::switchboard::get_instrument_close_topic(harness.instrument.id()),
        &close,
    );

    let mut seen_expiration_fill = false;

    for event in std::iter::from_fn(|| harness.rx.try_recv().ok()) {
        if let ExecutionEvent::Order(OrderEventAny::Filled(fill)) = event
            && fill.client_order_id.as_str().starts_with("EXPIRATION-")
            && fill.last_px == Price::from("1.000")
        {
            seen_expiration_fill = true;
        }
    }
    assert!(
        seen_expiration_fill,
        "expected EXPIRATION fill after publishing InstrumentClose to sandbox paper lane"
    );
}

#[rstest]
fn test_paper_binary_option_pending_resolution_then_close_settlement_via_data_engine(
    trader_id: TraderId,
    account_id: AccountId,
) {
    let mut harness = setup_pending_resolution_harness(trader_id, account_id, "BO-DE");
    let cache = harness.context.cache.clone();
    let data_engine = Rc::new(RefCell::new(DataEngine::new(
        harness.clock.clone(),
        cache,
        None,
    )));
    DataEngine::register_msgbus_handlers(&data_engine);
    assert_pending_resolution_transition(&mut harness, "REST-BO-DE", "PROBE-BO-DE");

    let close = InstrumentClose::new(
        harness.instrument.id(),
        Price::from("1.000"),
        InstrumentCloseType::ContractExpired,
        UnixNanos::from(300),
        UnixNanos::from(300),
    );
    msgbus::send_data(
        MessagingSwitchboard::data_engine_process_data(),
        Data::InstrumentClose(close),
    );

    let mut seen_expiration_fill = false;

    for event in std::iter::from_fn(|| harness.rx.try_recv().ok()) {
        if let ExecutionEvent::Order(OrderEventAny::Filled(fill)) = event
            && fill.client_order_id.as_str().starts_with("EXPIRATION-")
            && fill.last_px == Price::from("1.000")
        {
            seen_expiration_fill = true;
        }
    }
    assert!(
        seen_expiration_fill,
        "expected EXPIRATION fill after sending InstrumentClose through DataEngine endpoint"
    );
}

#[rstest]
fn test_instrument_status_lazy_creates_but_close_requires_existing_engine(
    trader_id: TraderId,
    account_id: AccountId,
) {
    setup_order_event_handler();

    let instrument = make_binary_option_instrument("0xCOND", "0xYES", "Yes", 100);
    let mut test_context = create_test_context(trader_id, account_id, instrument.id().venue);
    test_context
        .cache
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();
    test_context.client.start().unwrap();

    let status = InstrumentStatus::new(
        instrument.id(),
        MarketStatusAction::Trading,
        UnixNanos::from(1),
        UnixNanos::from(1),
        None,
        None,
        Some(true),
        Some(true),
        None,
    );
    msgbus::publish_any(
        nautilus_common::msgbus::switchboard::get_instrument_status_topic(instrument.id()),
        &status,
    );
    assert_eq!(test_context.client.matching_engine_count(), 1);

    let second_instrument = make_binary_option_instrument("0xCOND2", "0xYES2", "Yes", 100);
    test_context
        .cache
        .borrow_mut()
        .add_instrument(second_instrument.clone())
        .unwrap();

    let close = InstrumentClose::new(
        second_instrument.id(),
        Price::from("1.000"),
        InstrumentCloseType::ContractExpired,
        UnixNanos::from(2),
        UnixNanos::from(2),
    );
    msgbus::publish_any(
        nautilus_common::msgbus::switchboard::get_instrument_close_topic(second_instrument.id()),
        &close,
    );

    assert_eq!(
        test_context.client.matching_engine_count(),
        1,
        "InstrumentClose should not lazy-create a matching engine from cache",
    );
}

#[rstest]
fn test_instrument_close_finalizes_expired_engine_without_open_state(
    trader_id: TraderId,
    account_id: AccountId,
) {
    *msgbus::get_message_bus().borrow_mut() = MessageBus::default();
    setup_order_event_handler();

    let mut harness = setup_binary_option_lifecycle_harness(
        trader_id,
        account_id,
        "0xFINALIZE",
        "0xYES",
        "Yes",
        100,
    );
    assert_eq!(harness.client.matching_engine_count(), 1);

    publish_expired_close(
        &harness.test_clock,
        &harness.instrument,
        Price::from("1.000"),
        200,
    );

    assert_eq!(harness.client.matching_engine_count(), 0);
    assert!(
        harness
            .cache
            .borrow()
            .instrument(&harness.instrument.id())
            .is_none()
    );

    harness.client.stop().unwrap();
}

#[rstest]
fn test_periodic_sweep_retires_expired_quote_only_engine(
    trader_id: TraderId,
    account_id: AccountId,
) {
    *msgbus::get_message_bus().borrow_mut() = MessageBus::default();
    setup_order_event_handler();

    // Quote-only lifecycle: the matching engine is lazily created from a quote, with no order,
    // position, or InstrumentClose to drive the event-driven cleanup paths.
    let mut harness = setup_binary_option_lifecycle_harness(
        trader_id, account_id, "0xSWEEP", "0xYES", "Yes", 100,
    );
    assert_eq!(harness.client.matching_engine_count(), 1);
    assert!(
        harness
            .cache
            .borrow()
            .instrument(&harness.instrument.id())
            .is_some()
    );

    // Expiry alone, before the sweep interval elapses, does not release the engine.
    let pre_sweep = harness
        .test_clock
        .borrow_mut()
        .advance_time(UnixNanos::from(200), true);
    assert!(
        pre_sweep.is_empty(),
        "sweep timer must not fire before its interval",
    );
    assert_eq!(harness.client.matching_engine_count(), 1);
    assert!(
        harness
            .cache
            .borrow()
            .instrument(&harness.instrument.id())
            .is_some()
    );

    // Advance past one sweep interval and fire the timer (60s matches
    // EXPIRED_ENGINE_SWEEP_INTERVAL_NS in `execution.rs`).
    let events = harness
        .test_clock
        .borrow_mut()
        .advance_time(UnixNanos::from(60_000_000_001), true);
    let handlers = harness.test_clock.borrow().match_handlers(events);
    for handler in handlers {
        handler.run();
    }

    assert_eq!(
        harness.client.matching_engine_count(),
        0,
        "periodic sweep should retire the expired quote-only matching engine",
    );
    assert!(
        harness
            .cache
            .borrow()
            .instrument(&harness.instrument.id())
            .is_none(),
        "periodic sweep should purge the expired instrument from the cache",
    );

    harness.client.stop().unwrap();
}

#[rstest]
fn test_periodic_sweep_retains_expired_engine_with_open_position(
    trader_id: TraderId,
    account_id: AccountId,
) {
    *msgbus::get_message_bus().borrow_mut() = MessageBus::default();
    setup_order_event_handler();

    let mut harness = setup_binary_option_lifecycle_harness(
        trader_id,
        account_id,
        "0xSWEEPOPEN",
        "0xYES",
        "Yes",
        100,
    );

    let position = submit_open_position_and_seed_cache(
        &harness.client,
        &harness.cache,
        trader_id,
        &harness.instrument,
        "OPEN-SWEEP",
        "P-OPEN-SWEEP",
        &mut harness.rx,
    );
    let venue = harness.instrument.id().venue;
    harness
        .cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();

    // Isolate any event produced by the sweep from the opening fill emitted during setup.
    while harness.rx.try_recv().is_ok() {}

    // Fire the sweep past expiry: settlement safety must retain the engine while a position is open.
    let events = harness
        .test_clock
        .borrow_mut()
        .advance_time(UnixNanos::from(60_000_000_001), true);
    let handlers = harness.test_clock.borrow().match_handlers(events);
    for handler in handlers {
        handler.run();
    }

    // The sweep performs no settlement: no fill or position-close event, and the position stays open.
    let sweep_events: Vec<ExecutionEvent> =
        std::iter::from_fn(|| harness.rx.try_recv().ok()).collect();
    assert!(
        sweep_events.is_empty(),
        "periodic sweep must not emit execution events for an open position, was {sweep_events:?}",
    );
    assert!(
        harness.cache.borrow().has_positions_open(
            Some(&venue),
            Some(&harness.instrument.id()),
            None,
            None,
            None,
        ),
        "periodic sweep must leave the open position open",
    );
    assert_eq!(
        harness.client.matching_engine_count(),
        1,
        "periodic sweep must not retire an expired engine with an open position",
    );
    assert!(
        harness
            .cache
            .borrow()
            .instrument(&harness.instrument.id())
            .is_some(),
        "periodic sweep must not purge an instrument with an open position",
    );

    harness.client.stop().unwrap();
}

#[rstest]
fn test_periodic_sweep_retains_expired_engine_with_open_order(
    trader_id: TraderId,
    account_id: AccountId,
) {
    *msgbus::get_message_bus().borrow_mut() = MessageBus::default();
    setup_order_event_handler();

    let mut harness = setup_binary_option_lifecycle_harness(
        trader_id,
        account_id,
        "0xSWEEPORDER",
        "0xYES",
        "Yes",
        100,
    );

    // A limit order well below the bid rests unfilled, so the instrument expires carrying a
    // non-terminal order and no position.
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(harness.instrument.id())
        .side(OrderSide::Buy)
        .price(Price::new(0.10, 3))
        .quantity(Quantity::from("1.00"))
        .client_order_id("RESTING-SWEEP".into())
        .ts_init(UnixNanos::from(10))
        .submit(true)
        .build();
    harness
        .cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    harness
        .client
        .submit_order(SubmitOrder::from_order(
            &order,
            trader_id,
            Some(harness.client.client_id()),
            None,
            UUID4::new(),
            UnixNanos::from(10),
        ))
        .unwrap();

    // Apply the acceptance so the order registers in the cache open-order index.
    let accepted = std::iter::from_fn(|| harness.rx.try_recv().ok())
        .find_map(|event| match event {
            ExecutionEvent::Order(order_event @ OrderEventAny::Accepted(_)) => Some(order_event),
            _ => None,
        })
        .expect("expected acceptance for the resting limit order");
    harness.cache.borrow_mut().update_order(&accepted).unwrap();

    let venue = harness.instrument.id().venue;
    let instrument_id = harness.instrument.id();
    assert!(
        harness.cache.borrow().has_orders_open(
            Some(&venue),
            Some(&instrument_id),
            None,
            None,
            None,
        ),
        "resting limit order should be open before the sweep",
    );
    assert!(
        !harness.cache.borrow().has_positions_open(
            Some(&venue),
            Some(&instrument_id),
            None,
            None,
            None,
        ),
        "resting limit order should not have opened a position",
    );

    let events = harness
        .test_clock
        .borrow_mut()
        .advance_time(UnixNanos::from(60_000_000_001), true);
    let handlers = harness.test_clock.borrow().match_handlers(events);
    for handler in handlers {
        handler.run();
    }

    // `purge_instrument_skip_order_guard` requires callers to have already terminalized order
    // state, which this sweep has not done, so engine and instrument must both be retained.
    assert_eq!(
        harness.client.matching_engine_count(),
        1,
        "periodic sweep must not retire an expired engine with an open order",
    );
    assert!(
        harness.cache.borrow().instrument(&instrument_id).is_some(),
        "periodic sweep must not purge an instrument with an open order",
    );
    assert!(
        harness.cache.borrow().has_orders_open(
            Some(&venue),
            Some(&instrument_id),
            None,
            None,
            None,
        ),
        "periodic sweep must leave the resting order open and reachable",
    );

    harness.client.stop().unwrap();
}

#[rstest]
fn test_instrument_close_keeps_engine_until_position_closed(
    trader_id: TraderId,
    account_id: AccountId,
) {
    *msgbus::get_message_bus().borrow_mut() = MessageBus::default();
    setup_order_event_handler();

    let mut harness = setup_binary_option_lifecycle_harness(
        trader_id, account_id, "0xSETTLE", "0xYES", "Yes", 100,
    );
    let venue = harness.instrument.id().venue;
    assert_eq!(harness.client.matching_engine_count(), 1);

    let position = submit_open_position_and_seed_cache(
        &harness.client,
        &harness.cache,
        trader_id,
        &harness.instrument,
        "OPEN-POSITION",
        "P-OPEN-POSITION",
        &mut harness.rx,
    );
    harness
        .cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();

    publish_expired_close(
        &harness.test_clock,
        &harness.instrument,
        Price::from("1.000"),
        200,
    );

    assert_eq!(harness.client.matching_engine_count(), 1);
    assert!(
        harness
            .cache
            .borrow()
            .instrument(&harness.instrument.id())
            .is_some()
    );

    let closed = settle_position_from_expiration_fill(&harness.cache, &position, &mut harness.rx);
    assert!(!harness.cache.borrow().has_orders_open(
        Some(&venue),
        Some(&harness.instrument.id()),
        None,
        None,
        None,
    ));
    assert!(!harness.cache.borrow().has_positions_open(
        Some(&venue),
        Some(&harness.instrument.id()),
        None,
        None,
        None,
    ));
    msgbus::publish_position_event(
        "events.position.TEST".into(),
        &position_closed_event(&closed, account_id),
    );

    assert_eq!(harness.client.matching_engine_count(), 0);

    harness.client.stop().unwrap();
}

#[rstest]
fn test_position_closed_finalize_ignores_other_account(trader_id: TraderId, account_id: AccountId) {
    *msgbus::get_message_bus().borrow_mut() = MessageBus::default();
    setup_order_event_handler();

    let mut harness = setup_binary_option_lifecycle_harness(
        trader_id,
        account_id,
        "0xACCOUNT",
        "0xYES",
        "Yes",
        100,
    );

    let position = submit_open_position_and_seed_cache(
        &harness.client,
        &harness.cache,
        trader_id,
        &harness.instrument,
        "OPEN-ACCOUNT",
        "P-OPEN-ACCOUNT",
        &mut harness.rx,
    );
    harness
        .cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();

    publish_expired_close(
        &harness.test_clock,
        &harness.instrument,
        Price::from("1.000"),
        200,
    );

    let closed = settle_position_from_expiration_fill(&harness.cache, &position, &mut harness.rx);
    msgbus::publish_position_event(
        "events.position.TEST".into(),
        &position_closed_event(&closed, AccountId::from("OTHER-001")),
    );

    assert_eq!(harness.client.matching_engine_count(), 1);
    assert!(
        harness
            .cache
            .borrow()
            .instrument(&harness.instrument.id())
            .is_some()
    );

    harness.client.stop().unwrap();
}

#[rstest]
fn test_position_closed_does_not_purge_non_expired_instrument(
    trader_id: TraderId,
    account_id: AccountId,
) {
    *msgbus::get_message_bus().borrow_mut() = MessageBus::default();
    setup_order_event_handler();

    let mut harness = setup_binary_option_lifecycle_harness(
        trader_id, account_id, "0xACTIVE", "0xYES", "Yes", 1_000,
    );

    let position = submit_open_position_and_seed_cache(
        &harness.client,
        &harness.cache,
        trader_id,
        &harness.instrument,
        "OPEN-ACTIVE",
        "P-ACTIVE",
        &mut harness.rx,
    );
    harness
        .cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();

    let mut closed = position;
    closed.side = PositionSide::Flat;
    closed.ts_closed = Some(closed.ts_last);
    harness.cache.borrow_mut().update_position(&closed).unwrap();

    msgbus::publish_position_event(
        "events.position.TEST".into(),
        &position_closed_event(&closed, account_id),
    );

    assert_eq!(
        harness.client.matching_engine_count(),
        1,
        "non-expired position close should not release sandbox matching engines",
    );
    assert!(
        harness
            .cache
            .borrow()
            .instrument(&harness.instrument.id())
            .is_some()
    );

    harness.client.stop().unwrap();
}

#[rstest]
fn test_instrument_close_removes_resting_order_only_engine_before_cancel_event_applies(
    trader_id: TraderId,
    account_id: AccountId,
) {
    *msgbus::get_message_bus().borrow_mut() = MessageBus::default();
    setup_order_event_handler();

    let mut harness = setup_binary_option_lifecycle_harness(
        trader_id,
        account_id,
        "0xRESTING",
        "0xYES",
        "Yes",
        100,
    );
    let venue = harness.instrument.id().venue;
    assert_eq!(harness.client.matching_engine_count(), 1);

    let resting_order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(harness.instrument.id())
        .side(OrderSide::Buy)
        .price(Price::from("0.050"))
        .quantity(Quantity::from("1.00"))
        .client_order_id("REST-CLOSE-ONLY".into())
        .ts_init(UnixNanos::from(20))
        .submit(true)
        .build();
    harness
        .cache
        .borrow_mut()
        .add_order(resting_order.clone(), None, None, false)
        .unwrap();
    harness
        .client
        .submit_order(SubmitOrder::from_order(
            &resting_order,
            trader_id,
            Some(harness.client.client_id()),
            None,
            UUID4::new(),
            UnixNanos::from(20),
        ))
        .unwrap();

    let order_events = apply_order_events_from_channel(&harness.cache, &mut harness.rx);
    assert!(
        order_events
            .iter()
            .any(|event| matches!(event, OrderEventAny::Accepted(accepted)
                if accepted.client_order_id.as_str() == "REST-CLOSE-ONLY")),
        "expected resting order acceptance before expiration",
    );
    assert!(harness.cache.borrow().has_orders_open(
        Some(&venue),
        Some(&harness.instrument.id()),
        None,
        None,
        None,
    ));

    publish_expired_close(
        &harness.test_clock,
        &harness.instrument,
        Price::from("1.000"),
        200,
    );

    assert_eq!(
        harness.client.matching_engine_count(),
        0,
        "order-only expired instruments should release their matching engine immediately",
    );
    assert!(
        harness
            .cache
            .borrow()
            .instrument(&harness.instrument.id())
            .is_none()
    );

    let order_events = apply_order_events_from_channel(&harness.cache, &mut harness.rx);
    assert!(
        order_events
            .iter()
            .any(|event| matches!(event, OrderEventAny::Canceled(canceled)
                if canceled.client_order_id.as_str() == "REST-CLOSE-ONLY")),
        "expected expiration to cancel the resting order",
    );
    assert!(!harness.cache.borrow().has_orders_open(
        Some(&venue),
        Some(&harness.instrument.id()),
        None,
        None,
        None,
    ));
    assert_eq!(
        harness.client.matching_engine_count(),
        0,
        "cancellation replay should not recreate engine retention after close",
    );
    assert!(
        harness
            .cache
            .borrow()
            .instrument(&harness.instrument.id())
            .is_none()
    );

    harness.client.stop().unwrap();
}

#[rstest]
fn test_submit_order_list_keeps_processing_all_expired_legs_before_cleanup(
    trader_id: TraderId,
    account_id: AccountId,
) {
    *msgbus::get_message_bus().borrow_mut() = MessageBus::default();
    let mut harness = setup_binary_option_lifecycle_harness(
        trader_id,
        account_id,
        "0xORDER-LIST",
        "0xYES",
        "Yes",
        100,
    );
    let client_id = harness.client.client_id();

    let _ = harness
        .test_clock
        .borrow_mut()
        .advance_time(UnixNanos::from(200), true);

    let first = OrderTestBuilder::new(OrderType::Limit)
        .trader_id(trader_id)
        .instrument_id(harness.instrument.id())
        .client_order_id(ClientOrderId::from("O-EXPIRED-LIST-001"))
        .side(OrderSide::Buy)
        .price(Price::from("0.400"))
        .quantity(Quantity::from("1"))
        .build();
    let second = OrderTestBuilder::new(OrderType::Limit)
        .trader_id(trader_id)
        .instrument_id(harness.instrument.id())
        .client_order_id(ClientOrderId::from("O-EXPIRED-LIST-002"))
        .side(OrderSide::Buy)
        .price(Price::from("0.450"))
        .quantity(Quantity::from("1"))
        .build();
    let orders = vec![first, second];

    for order in &orders {
        harness
            .cache
            .borrow_mut()
            .add_order(order.clone(), None, Some(client_id), false)
            .unwrap();
    }

    harness
        .client
        .submit_order_list(create_submit_order_list(
            trader_id,
            client_id,
            harness.instrument.id(),
            &orders,
        ))
        .unwrap();

    let order_events = apply_order_events_from_channel(&harness.cache, &mut harness.rx);

    for order in &orders {
        assert!(
            order_events.iter().any(|event| {
                event.client_order_id() == order.client_order_id()
                    && matches!(event, OrderEventAny::Rejected(_))
            }),
            "expired order-list leg should emit a terminal rejection, not stay SUBMITTED",
        );
    }

    assert_eq!(harness.client.matching_engine_count(), 0);
    assert!(
        harness
            .cache
            .borrow()
            .instrument(&harness.instrument.id())
            .is_none()
    );

    harness.client.stop().unwrap();
}

#[rstest]
fn test_instrument_close_sync_cleanup_handles_synchronous_position_closed_reentry(
    trader_id: TraderId,
) {
    std::thread::spawn(move || {
        *msgbus::get_message_bus().borrow_mut() = MessageBus::default();

        let venue = Venue::new("BINANCE");
        let account_id = AccountId::from("BINANCE-001");
        let client_id = ClientId::new("SANDBOX");
        let cache = Rc::new(RefCell::new(Cache::default()));
        let test_clock = Rc::new(RefCell::new(TestClock::new()));
        let clock: Rc<RefCell<dyn Clock>> = test_clock.clone();

        let mut binary = binary_option();
        binary.id = InstrumentId::from("YES.BINANCE");
        binary.raw_symbol = "YES".into();
        binary.activation_ns = UnixNanos::from(1);
        binary.expiration_ns = UnixNanos::from(100);
        let instrument = InstrumentAny::BinaryOption(binary);

        cache
            .borrow_mut()
            .add_instrument(instrument.clone())
            .unwrap();
        cache
            .borrow_mut()
            .add_quote(create_binary_option_quote(instrument.id()))
            .unwrap();

        let cache_for_handler = cache.clone();
        let order_events = Rc::new(RefCell::new(Vec::<OrderEventAny>::new()));
        let order_events_for_handler = order_events.clone();
        let opening_fill = Rc::new(RefCell::new(None::<OrderFilled>));
        let opening_fill_for_handler = opening_fill.clone();
        let instrument_for_position = instrument.clone();
        let instrument_for_handler = instrument.clone();
        let order_handler = TypedIntoHandler::from(move |event: OrderEventAny| {
            order_events_for_handler.borrow_mut().push(event.clone());
            let _ = cache_for_handler.borrow_mut().update_order(&event);

            let OrderEventAny::Filled(mut fill) = event else {
                return;
            };

            if fill.client_order_id.as_str().starts_with("EXPIRATION-") {
                let position = cache_for_handler
                    .borrow()
                    .positions_open(
                        Some(&venue),
                        Some(&instrument_for_handler.id()),
                        None,
                        Some(&account_id),
                        None,
                    )
                    .into_iter()
                    .next()
                    .expect("expected open position before expiration fill")
                    .clone();
                fill.position_id = Some(position.id);

                let mut closed = position;
                closed.apply(&fill);
                cache_for_handler
                    .borrow_mut()
                    .update_position(&closed)
                    .unwrap();

                let position_closed =
                    PositionClosed::create(&closed, &fill, UUID4::new(), fill.ts_event);
                msgbus::publish_position_event(
                    "events.position.TEST".into(),
                    &PositionEvent::PositionClosed(position_closed),
                );
            } else {
                *opening_fill_for_handler.borrow_mut() = Some(fill);
            }
        });
        msgbus::register_order_event_endpoint(
            MessagingSwitchboard::exec_engine_process(),
            order_handler,
        );

        let usd = Currency::USD();
        let config = SandboxExecutionClientConfig {
            account_id,
            venue,
            starting_balances: vec![Money::new(100_000.0, usd)],
            base_currency: Some(usd),
            oms_type: OmsType::Netting,
            account_type: AccountType::Margin,
            default_leverage: Decimal::ONE,
            leverages: ahash::AHashMap::new(),
            book_type: BookType::L1_MBP,
            fee_model: None,
            fill_model: None,
            latency_model: None,
            frozen_account: false,
            bar_execution: false,
            trade_execution: false,
            reject_stop_orders: true,
            support_gtd_orders: true,
            support_contingent_orders: true,
            use_position_ids: true,
            use_random_ids: false,
            use_reduce_only: true,
            queue_position: false,
            liquidity_consumption: false,
            bar_adaptive_high_low_ordering: false,
            use_market_order_acks: false,
            oto_full_trigger: false,
            price_protection_points: 0,
        };
        let core = ExecutionClientCore::new(
            trader_id,
            client_id,
            venue,
            config.oms_type,
            config.account_id,
            config.account_type,
            config.base_currency,
            cache.clone(),
        );
        let mut client = SandboxExecutionClient::new(core, config, clock, cache.clone());
        client.start().unwrap();

        let order = OrderTestBuilder::new(OrderType::Market)
            .trader_id(trader_id)
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-SYNC-REENTRY-001"))
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.00"))
            .build();
        cache
            .borrow_mut()
            .add_order(order.clone(), None, Some(client_id), false)
            .unwrap();

        let ts = test_clock.borrow().timestamp_ns();
        client
            .submit_order(SubmitOrder::from_order(
                &order,
                trader_id,
                Some(client_id),
                None,
                UUID4::new(),
                ts,
            ))
            .unwrap();

        assert!(
            order_events
                .borrow()
                .iter()
                .any(|event| matches!(event, OrderEventAny::Filled(_))),
            "expected opening fill event, found {:?}",
            order_events.borrow(),
        );

        let mut opening_fill = opening_fill
            .borrow_mut()
            .take()
            .expect("expected opening fill before expiration");
        opening_fill.position_id = Some(PositionId::new("P-SYNC-REENTRY"));
        let position = Position::new(&instrument_for_position, opening_fill);
        cache
            .borrow_mut()
            .add_position(&position, OmsType::Netting)
            .unwrap();

        assert!(cache.borrow().has_positions_open(
            Some(&venue),
            Some(&instrument.id()),
            None,
            Some(&account_id),
            None,
        ));

        publish_expired_close(&test_clock, &instrument, Price::from("1.000"), 200);

        assert_eq!(client.matching_engine_count(), 0);
        assert!(cache.borrow().instrument(&instrument.id()).is_none());
        client.stop().unwrap();
    })
    .join()
    .unwrap();
}

#[rstest]
fn test_local_expiry_removes_resting_order_only_engine_before_cancel_event_applies(
    trader_id: TraderId,
    account_id: AccountId,
) {
    *msgbus::get_message_bus().borrow_mut() = MessageBus::default();
    setup_order_event_handler();

    let mut harness = setup_binary_option_lifecycle_harness(
        trader_id,
        account_id,
        "0xLOCAL-REST",
        "0xYES",
        "Yes",
        100,
    );
    let venue = harness.instrument.id().venue;
    assert_eq!(harness.client.matching_engine_count(), 1);

    let resting_order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(harness.instrument.id())
        .side(OrderSide::Buy)
        .price(Price::from("0.050"))
        .quantity(Quantity::from("1.00"))
        .client_order_id("REST-LOCAL-ONLY".into())
        .ts_init(UnixNanos::from(20))
        .submit(true)
        .build();
    harness
        .cache
        .borrow_mut()
        .add_order(resting_order.clone(), None, None, false)
        .unwrap();
    harness
        .client
        .submit_order(SubmitOrder::from_order(
            &resting_order,
            trader_id,
            Some(harness.client.client_id()),
            None,
            UUID4::new(),
            UnixNanos::from(20),
        ))
        .unwrap();

    let order_events = apply_order_events_from_channel(&harness.cache, &mut harness.rx);
    assert!(
        order_events
            .iter()
            .any(|event| matches!(event, OrderEventAny::Accepted(accepted)
                if accepted.client_order_id.as_str() == "REST-LOCAL-ONLY")),
        "expected resting order acceptance before local expiry",
    );
    assert!(harness.cache.borrow().has_orders_open(
        Some(&venue),
        Some(&harness.instrument.id()),
        None,
        None,
        None,
    ));

    let _ = harness
        .test_clock
        .borrow_mut()
        .advance_time(UnixNanos::from(200), true);

    let probe_order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(harness.instrument.id())
        .side(OrderSide::Buy)
        .price(Price::from("0.050"))
        .quantity(Quantity::from("1.00"))
        .client_order_id("PROBE-LOCAL-ONLY".into())
        .ts_init(UnixNanos::from(200))
        .submit(true)
        .build();
    harness
        .cache
        .borrow_mut()
        .add_order(probe_order.clone(), None, None, false)
        .unwrap();
    harness
        .client
        .submit_order(SubmitOrder::from_order(
            &probe_order,
            trader_id,
            Some(harness.client.client_id()),
            None,
            UUID4::new(),
            UnixNanos::from(200),
        ))
        .unwrap();

    assert_eq!(
        harness.client.matching_engine_count(),
        0,
        "local expiry should release the engine before cancel/reject events apply",
    );
    assert!(
        harness
            .cache
            .borrow()
            .instrument(&harness.instrument.id())
            .is_none()
    );

    let order_events = apply_order_events_from_channel(&harness.cache, &mut harness.rx);
    assert!(
        order_events
            .iter()
            .any(|event| matches!(event, OrderEventAny::Canceled(canceled)
                if canceled.client_order_id.as_str() == "REST-LOCAL-ONLY")),
        "expected local expiry to cancel the resting order",
    );
    assert!(
        order_events
            .iter()
            .any(|event| matches!(event, OrderEventAny::Rejected(rejected)
                if rejected.client_order_id.as_str() == "PROBE-LOCAL-ONLY"
                    && rejected.reason.as_str().contains("pending resolution"))),
        "expected local expiry to reject new orders while pending resolution",
    );
    assert!(!harness.cache.borrow().has_orders_open(
        Some(&venue),
        Some(&harness.instrument.id()),
        None,
        None,
        None,
    ));
    assert_eq!(harness.client.matching_engine_count(), 0);

    harness.client.stop().unwrap();
}

#[rstest]
fn test_paper_binary_option_multiple_instruments_close_settlement_via_data_engine(
    trader_id: TraderId,
    account_id: AccountId,
) {
    let instruments = vec![
        (
            make_binary_option_instrument("0xCOND-BTC", "0xBTC-YES", "Yes", 100),
            Price::from("1.000"),
            "OPEN-BTC-YES",
            "P-BTC-YES",
        ),
        (
            make_binary_option_instrument("0xCOND-BTC", "0xBTC-NO", "No", 100),
            Price::from("0.000"),
            "OPEN-BTC-NO",
            "P-BTC-NO",
        ),
        (
            make_binary_option_instrument("0xCOND-ETH", "0xETH-YES", "Yes", 100),
            Price::from("0.000"),
            "OPEN-ETH-YES",
            "P-ETH-YES",
        ),
        (
            make_binary_option_instrument("0xCOND-ETH", "0xETH-NO", "No", 100),
            Price::from("1.000"),
            "OPEN-ETH-NO",
            "P-ETH-NO",
        ),
    ];
    let venue = instruments[0].0.id().venue;
    let cache = Rc::new(RefCell::new(Cache::default()));
    let test_clock = Rc::new(RefCell::new(TestClock::new()));
    let clock: Rc<RefCell<dyn Clock>> = test_clock.clone();

    let mut config = create_config(trader_id, account_id, venue);
    config.base_currency = Some(Currency::USDC());
    config.starting_balances = vec![Money::new(100_000.0, Currency::USDC())];
    let core = ExecutionClientCore::new(
        trader_id,
        ClientId::new("SANDBOX"),
        config.venue,
        config.oms_type,
        config.account_id,
        config.account_type,
        config.base_currency,
        cache.clone(),
    );
    let mut client = SandboxExecutionClient::new(core, config, clock.clone(), cache.clone());

    let data_engine = Rc::new(RefCell::new(DataEngine::new(clock, cache.clone(), None)));
    DataEngine::register_msgbus_handlers(&data_engine);

    for (instrument, _, _, _) in &instruments {
        cache
            .borrow_mut()
            .add_instrument(instrument.clone())
            .unwrap();
    }
    let _ = test_clock
        .borrow_mut()
        .advance_time(UnixNanos::from(50), true);

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
    nautilus_common::live::runner::replace_exec_event_sender(tx);
    client.start().unwrap();

    for (instrument, _, _, _) in &instruments {
        let quote = QuoteTick::new(
            instrument.id(),
            Price::new(0.40, 3),
            Price::new(0.41, 3),
            Quantity::new(100.0, 2),
            Quantity::new(100.0, 2),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        client.process_quote_tick(&quote).unwrap();
    }

    for (idx, (instrument, _, client_order_id, _)) in instruments.iter().enumerate() {
        submit_market_open_order(
            &client,
            &cache,
            trader_id,
            instrument,
            client_order_id,
            10 + idx as u64,
        );
    }

    let mut seeded_positions = ahash::AHashSet::new();

    for event in std::iter::from_fn(|| rx.try_recv().ok()) {
        let ExecutionEvent::Order(OrderEventAny::Filled(fill)) = event else {
            continue;
        };

        if let Some((instrument, _, client_order_id, position_id)) =
            instruments
                .iter()
                .find(|(_, _, expected_client_order_id, _)| {
                    fill.client_order_id.as_str() == *expected_client_order_id
                })
        {
            seed_binary_option_position_from_fill(&cache, instrument, fill, position_id);
            seeded_positions.insert(*client_order_id);
        }
    }

    assert_eq!(
        seeded_positions.len(),
        instruments.len(),
        "expected one opened position per instrument before settlement"
    );

    let _ = test_clock
        .borrow_mut()
        .advance_time(UnixNanos::from(200), true);

    for (idx, (instrument, close_price, _, _)) in instruments.iter().enumerate() {
        let close = InstrumentClose::new(
            instrument.id(),
            *close_price,
            InstrumentCloseType::ContractExpired,
            UnixNanos::from(300 + idx as u64),
            UnixNanos::from(300 + idx as u64),
        );
        msgbus::send_data(
            MessagingSwitchboard::data_engine_process_data(),
            Data::InstrumentClose(close),
        );
    }

    let mut expiration_fills = ahash::AHashMap::new();

    for event in std::iter::from_fn(|| rx.try_recv().ok()) {
        let ExecutionEvent::Order(OrderEventAny::Filled(fill)) = event else {
            continue;
        };

        if fill.client_order_id.as_str().starts_with("EXPIRATION-") {
            expiration_fills.insert(fill.instrument_id, fill.last_px);
        }
    }

    assert_eq!(
        expiration_fills.len(),
        instruments.len(),
        "expected one settlement fill per open instrument"
    );

    for (instrument, close_price, _, _) in &instruments {
        assert_eq!(
            expiration_fills.get(&instrument.id()),
            Some(close_price),
            "expected settlement price to match InstrumentClose for {}",
            instrument.id()
        );
    }
}

#[rstest]
fn test_process_quote_tick_creates_matching_engine(
    test_context: TestContext,
    instrument: InstrumentAny,
) {
    setup_order_event_handler();

    test_context
        .cache
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();

    let quote = create_quote_tick(instrument.id(), 1000.0, 1001.0);
    let result = test_context.client.process_quote_tick(&quote);

    assert!(result.is_ok());
    assert_eq!(test_context.client.matching_engine_count(), 1);
}

#[rstest]
fn test_process_quote_tick_reuses_matching_engine(
    test_context: TestContext,
    instrument: InstrumentAny,
) {
    setup_order_event_handler();

    test_context
        .cache
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();

    let quote1 = create_quote_tick(instrument.id(), 1000.0, 1001.0);
    let quote2 = create_quote_tick(instrument.id(), 1002.0, 1003.0);

    test_context.client.process_quote_tick(&quote1).unwrap();
    test_context.client.process_quote_tick(&quote2).unwrap();

    assert_eq!(test_context.client.matching_engine_count(), 1);
}

#[rstest]
fn test_process_quote_tick_drops_precision_mismatch(
    test_context: TestContext,
    instrument: InstrumentAny,
) {
    setup_order_event_handler();

    test_context
        .cache
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();

    let quote = create_mismatched_quote_tick(instrument.id(), 1000.0, 1001.0);
    let result = test_context.client.process_quote_tick(&quote);

    assert!(result.is_ok());
    assert_eq!(test_context.client.matching_engine_count(), 0);
}

#[rstest]
fn test_on_instrument_updates_engine_precision(
    mut test_context: TestContext,
    instrument: InstrumentAny,
) {
    setup_order_event_handler();

    test_context
        .cache
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();

    let quote_before = create_quote_tick(instrument.id(), 1000.0, 1001.0);
    test_context
        .client
        .process_quote_tick(&quote_before)
        .unwrap();
    assert_eq!(test_context.client.matching_engine_count(), 1);

    let updated_instrument = updated_instrument_with_price_precision_3(instrument);
    test_context
        .cache
        .borrow_mut()
        .add_instrument(updated_instrument.clone())
        .unwrap();
    test_context
        .client
        .on_instrument(updated_instrument.clone());

    let stale_quote = create_quote_tick(updated_instrument.id(), 1000.0, 1001.0);
    let stale_result = test_context.client.process_quote_tick(&stale_quote);
    assert!(stale_result.is_ok());

    let updated_quote =
        create_quote_tick_with_price_precision(updated_instrument.id(), 1000.0, 1001.0, 3);
    let updated_result = test_context.client.process_quote_tick(&updated_quote);
    assert!(updated_result.is_ok());
    assert_eq!(test_context.client.matching_engine_count(), 1);
}

#[rstest]
fn test_process_quote_tick_instrument_not_found(execution_client: SandboxExecutionClient) {
    setup_order_event_handler();

    let quote = create_quote_tick(InstrumentId::from("UNKNOWN.SIM"), 1000.0, 1001.0);
    let result = execution_client.process_quote_tick(&quote);

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found"));
}

#[rstest]
fn test_process_trade_tick_disabled(test_context: TestContext, instrument: InstrumentAny) {
    setup_order_event_handler();

    test_context
        .cache
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();

    // Config has trade_execution = false, so this should be a no-op
    let trade = TradeTick::new(
        instrument.id(),
        Price::from("1000.0"),
        Quantity::from("1.0"),
        AggressorSide::Buy,
        TradeId::new("1"),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let result = test_context.client.process_trade_tick(&trade);

    assert!(result.is_ok());
    // No matching engine created because trade_execution is disabled
    assert_eq!(test_context.client.matching_engine_count(), 0);
}

#[rstest]
fn test_process_trade_tick_drops_precision_mismatch(
    trader_id: TraderId,
    account_id: AccountId,
    instrument: InstrumentAny,
) {
    setup_order_event_handler();

    let venue = instrument.id().venue;
    let mut test_context = create_test_context_with_trade_execution(trader_id, account_id, venue);
    test_context.client.start().unwrap();
    test_context
        .cache
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();

    let trade = create_mismatched_trade_tick(instrument.id());
    let result = test_context.client.process_trade_tick(&trade);

    assert!(result.is_ok());
    assert_eq!(test_context.client.matching_engine_count(), 0);
}

#[rstest]
fn test_message_handler_drops_precision_mismatched_trade(
    trader_id: TraderId,
    account_id: AccountId,
    instrument: InstrumentAny,
) {
    setup_order_event_handler();

    let venue = instrument.id().venue;
    let mut test_context = create_test_context_with_trade_execution(trader_id, account_id, venue);
    test_context
        .cache
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();
    test_context.client.start().unwrap();

    let trade = create_mismatched_trade_tick(instrument.id());
    msgbus::publish_trade(
        format!("data.trades.{}.{}", instrument.id().venue, instrument.id()).into(),
        &trade,
    );

    assert_eq!(test_context.client.matching_engine_count(), 0);
    test_context.client.stop().unwrap();
}

#[rstest]
fn test_process_bar_disabled(test_context: TestContext, instrument: InstrumentAny) {
    use nautilus_model::data::{Bar, BarType};

    setup_order_event_handler();

    test_context
        .cache
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();

    // Config has bar_execution = false, so this should be a no-op
    let bar_type = BarType::from(format!("{}-1-MINUTE-LAST-INTERNAL", instrument.id()));
    let bar = Bar::new(
        bar_type,
        Price::from("1000.0"),
        Price::from("1001.0"),
        Price::from("999.0"),
        Price::from("1000.5"),
        Quantity::from("100.0"),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let result = test_context.client.process_bar(&bar);

    assert!(result.is_ok());
    // No matching engine created because bar_execution is disabled
    assert_eq!(test_context.client.matching_engine_count(), 0);
}

#[rstest]
fn test_process_bar_drops_precision_mismatch(
    trader_id: TraderId,
    account_id: AccountId,
    venue: Venue,
    instrument: InstrumentAny,
) {
    setup_order_event_handler();

    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let mut config = create_config(trader_id, account_id, venue);
    config.bar_execution = true;

    let core = ExecutionClientCore::new(
        trader_id,
        ClientId::new("SANDBOX"),
        config.venue,
        config.oms_type,
        config.account_id,
        config.account_type,
        config.base_currency,
        cache.clone(),
    );
    let client = SandboxExecutionClient::new(core, config, clock, cache.clone());

    cache
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();

    let bar_type = BarType::from(format!("{}-1-MINUTE-LAST-EXTERNAL", instrument.id()));
    let bar = Bar::new(
        bar_type,
        Price::new(1000.0, 3),
        Price::new(1001.0, 3),
        Price::new(999.0, 3),
        Price::new(1000.5, 3),
        Quantity::new(100.0, 3),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let result = client.process_bar(&bar);

    assert!(result.is_ok());
    assert_eq!(client.matching_engine_count(), 0);
}

#[rstest]
fn test_message_handler_drops_precision_mismatched_bar(
    trader_id: TraderId,
    account_id: AccountId,
    instrument: InstrumentAny,
) {
    setup_order_event_handler();

    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let mut config = create_config(trader_id, account_id, instrument.id().venue);
    config.bar_execution = true;

    let core = ExecutionClientCore::new(
        trader_id,
        ClientId::new("SANDBOX"),
        config.venue,
        config.oms_type,
        config.account_id,
        config.account_type,
        config.base_currency,
        cache.clone(),
    );
    let mut client = SandboxExecutionClient::new(core, config, clock, cache.clone());

    cache
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();
    client.start().unwrap();

    let bar_type = BarType::from(format!("{}-1-MINUTE-LAST-EXTERNAL", instrument.id()));
    let bar = Bar::new(
        bar_type,
        Price::new(1000.0, 3),
        Price::new(1001.0, 3),
        Price::new(999.0, 3),
        Price::new(1000.5, 3),
        Quantity::new(100.0, 3),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    msgbus::publish_bar(format!("data.bars.{bar_type}").into(), &bar);

    assert_eq!(client.matching_engine_count(), 0);
    client.stop().unwrap();
}

#[rstest]
fn test_reset_with_no_engines(execution_client: SandboxExecutionClient) {
    setup_order_event_handler();

    assert_eq!(execution_client.matching_engine_count(), 0);

    // Reset should work even with no engines
    execution_client.reset();

    assert_eq!(execution_client.matching_engine_count(), 0);
}

#[rstest]
fn test_client_id(execution_client: SandboxExecutionClient, client_id: ClientId) {
    assert_eq!(execution_client.client_id(), client_id);
}

#[rstest]
fn test_account_id(execution_client: SandboxExecutionClient, account_id: AccountId) {
    assert_eq!(execution_client.account_id(), account_id);
}

#[rstest]
#[tokio::test]
async fn test_generate_mass_status_returns_empty_report(
    execution_client: SandboxExecutionClient,
    client_id: ClientId,
    account_id: AccountId,
    venue: Venue,
) {
    let mass_status = execution_client
        .generate_mass_status(None)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(mass_status.client_id, client_id);
    assert_eq!(mass_status.account_id, account_id);
    assert_eq!(mass_status.venue, venue);
    assert!(mass_status.order_reports().is_empty());
    assert!(mass_status.fill_reports().is_empty());
    assert!(mass_status.position_reports().is_empty());
}

#[rstest]
fn test_config_accessor(execution_client: SandboxExecutionClient, venue: Venue) {
    let config = execution_client.config();

    assert_eq!(config.venue, venue);
    assert_eq!(config.oms_type, OmsType::Netting);
    assert_eq!(config.account_type, AccountType::Margin);
}

#[rstest]
fn test_get_account_when_none(execution_client: SandboxExecutionClient) {
    // No account in cache yet
    assert!(execution_client.get_account().is_none());
}

#[rstest]
fn test_initialized_ioc_market_order_cancels_remainder_through_live_runner(
    trader_id: TraderId,
    account_id: AccountId,
    instrument: InstrumentAny,
) {
    *msgbus::get_message_bus().borrow_mut() = MessageBus::default();

    let instrument_id = instrument.id();
    let mut context = create_test_context(trader_id, account_id, instrument_id.venue);
    context
        .cache
        .borrow_mut()
        .add_instrument(instrument)
        .unwrap();

    let quote = QuoteTick::new(
        instrument_id,
        Price::from("1000.00"),
        Price::from("1010.00"),
        Quantity::from("0.500"),
        Quantity::from("0.500"),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    context.cache.borrow_mut().add_quote(quote).unwrap();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
    set_exec_event_sender(tx);
    context.client.start().unwrap();
    context.client.process_quote_tick(&quote).unwrap();

    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.500"))
        .time_in_force(TimeInForce::Ioc)
        .client_order_id(ClientOrderId::from("O-IOC-INITIALIZED"))
        .build();
    assert_eq!(order.status(), OrderStatus::Initialized);
    context
        .cache
        .borrow_mut()
        .add_order(order.clone(), None, Some(context.client.client_id()), false)
        .unwrap();

    context
        .client
        .submit_order(SubmitOrder::from_order(
            &order,
            trader_id,
            Some(context.client.client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
        ))
        .unwrap();

    let events: Vec<OrderEventAny> = std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(|event| match event {
            ExecutionEvent::Order(order_event) => Some(order_event),
            _ => None,
        })
        .collect();
    assert_eq!(events.len(), 3);
    let OrderEventAny::Submitted(submitted) = &events[0] else {
        panic!("Expected OrderSubmitted, was {:?}", events[0]);
    };
    assert_eq!(submitted.client_order_id, order.client_order_id());
    let OrderEventAny::Filled(fill) = &events[1] else {
        panic!("Expected OrderFilled, was {:?}", events[1]);
    };
    assert_eq!(fill.client_order_id, order.client_order_id());
    assert_eq!(fill.last_px, Price::from("1010.00"));
    assert_eq!(fill.last_qty, Quantity::from("0.500"));
    let OrderEventAny::Canceled(canceled) = &events[2] else {
        panic!("Expected OrderCanceled, was {:?}", events[2]);
    };
    assert_eq!(canceled.client_order_id, order.client_order_id());

    for event in &events {
        context.cache.borrow_mut().update_order(event).unwrap();
    }

    let cached_order = context
        .cache
        .borrow()
        .order(&order.client_order_id())
        .unwrap()
        .clone();
    assert_eq!(cached_order.status(), OrderStatus::Canceled);
    assert_eq!(cached_order.filled_qty(), Quantity::from("0.500"));
    assert_eq!(cached_order.leaves_qty(), Quantity::from("1.000"));

    context.client.stop().unwrap();
}

#[rstest]
#[case(None, OrderSide::NoOrderSide)]
#[case(Some("SANDBOX-A"), OrderSide::Buy)]
#[case(Some("SANDBOX-B"), OrderSide::Sell)]
fn test_cancel_all_orders_routes_by_client_account_and_side(
    trader_id: TraderId,
    instrument: InstrumentAny,
    #[case] selected_client: Option<&str>,
    #[case] selected_side: OrderSide,
) {
    struct SeededOrder {
        client_order_id: ClientOrderId,
        client_id: ClientId,
        account_id: AccountId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        side: OrderSide,
        status: OrderStatus,
    }

    *msgbus::get_message_bus().borrow_mut() = MessageBus::default();

    let InstrumentAny::CryptoPerpetual(mut other) = instrument.clone() else {
        panic!("Expected crypto perpetual fixture");
    };
    other.id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    other.raw_symbol = "BTCUSDT".into();
    let other = InstrumentAny::CryptoPerpetual(other);
    let instrument_id = instrument.id();
    let other_instrument_id = other.id();
    let venue = instrument_id.venue;
    let client_a_id = ClientId::new("SANDBOX-A");
    let client_b_id = ClientId::new("SANDBOX-B");
    let account_a_id = AccountId::from("BINANCE-001");
    let account_b_id = AccountId::from("BINANCE-002");
    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));

    {
        let mut cache = cache.borrow_mut();
        cache.add_instrument(instrument).unwrap();
        cache.add_instrument(other).unwrap();
        cache
            .add_quote(create_quote_tick(instrument_id, 1000.0, 1001.0))
            .unwrap();
        cache
            .add_quote(create_quote_tick(other_instrument_id, 2000.0, 2001.0))
            .unwrap();
    }

    let create_client = |client_id: ClientId, account_id: AccountId| {
        let config = create_config(trader_id, account_id, venue);
        let core = ExecutionClientCore::new(
            trader_id,
            client_id,
            venue,
            config.oms_type,
            config.account_id,
            config.account_type,
            config.base_currency,
            cache.clone(),
        );
        SandboxExecutionClient::new(core, config, clock.clone(), cache.clone())
    };
    let mut client_a = create_client(client_a_id, account_a_id);
    let mut client_b = create_client(client_b_id, account_b_id);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
    set_exec_event_sender(tx);
    client_a.start().unwrap();
    client_b.start().unwrap();

    let mut orders = Vec::new();
    let resting_orders = [
        (
            &client_a,
            client_a_id,
            account_a_id,
            StrategyId::from("STRATEGY-A-001"),
            instrument_id,
            OrderSide::Buy,
            ClientOrderId::from("O-A-BUY-ACCEPTED"),
        ),
        (
            &client_a,
            client_a_id,
            account_a_id,
            StrategyId::from("STRATEGY-A-002"),
            instrument_id,
            OrderSide::Sell,
            ClientOrderId::from("O-A-SELL-ACCEPTED"),
        ),
        (
            &client_a,
            client_a_id,
            account_a_id,
            StrategyId::from("STRATEGY-A-001"),
            other_instrument_id,
            OrderSide::Buy,
            ClientOrderId::from("O-A-OTHER-BUY-ACCEPTED"),
        ),
        (
            &client_b,
            client_b_id,
            account_b_id,
            StrategyId::from("STRATEGY-B-001"),
            instrument_id,
            OrderSide::Buy,
            ClientOrderId::from("O-B-BUY-ACCEPTED"),
        ),
        (
            &client_b,
            client_b_id,
            account_b_id,
            StrategyId::from("STRATEGY-B-002"),
            instrument_id,
            OrderSide::Sell,
            ClientOrderId::from("O-B-SELL-ACCEPTED"),
        ),
    ];

    for (client, client_id, account_id, strategy_id, order_instrument_id, side, order_id) in
        resting_orders
    {
        let price = match side {
            OrderSide::Buy => Price::from("900.00"),
            OrderSide::Sell => Price::from("1100.00"),
            _ => unreachable!(),
        };
        let order = OrderTestBuilder::new(OrderType::Limit)
            .trader_id(trader_id)
            .strategy_id(strategy_id)
            .instrument_id(order_instrument_id)
            .client_order_id(order_id)
            .side(side)
            .price(price)
            .quantity(Quantity::from("1.000"))
            .build();
        cache
            .borrow_mut()
            .add_order(order.clone(), None, Some(client_id), false)
            .unwrap();
        client
            .submit_order(SubmitOrder::from_order(
                &order,
                trader_id,
                Some(client_id),
                None,
                UUID4::new(),
                UnixNanos::default(),
            ))
            .unwrap();
        let events = apply_order_events_from_channel(&cache, &mut rx);
        assert_eq!(events.len(), 2);
        assert!(matches!(events[0], OrderEventAny::Submitted(_)));
        assert!(matches!(events[1], OrderEventAny::Accepted(_)));
        orders.push(SeededOrder {
            client_order_id: order_id,
            client_id,
            account_id,
            strategy_id,
            instrument_id: order_instrument_id,
            side,
            status: OrderStatus::Accepted,
        });
    }

    for (client_id, account_id, strategy_id, side, order_id) in [
        (
            client_a_id,
            account_a_id,
            StrategyId::from("STRATEGY-A-002"),
            OrderSide::Buy,
            ClientOrderId::from("O-A-BUY-SUBMITTED"),
        ),
        (
            client_b_id,
            account_b_id,
            StrategyId::from("STRATEGY-B-001"),
            OrderSide::Sell,
            ClientOrderId::from("O-B-SELL-SUBMITTED"),
        ),
    ] {
        let order = OrderTestBuilder::new(OrderType::Limit)
            .trader_id(trader_id)
            .strategy_id(strategy_id)
            .instrument_id(instrument_id)
            .client_order_id(order_id)
            .side(side)
            .price(Price::from("950.00"))
            .quantity(Quantity::from("2.000"))
            .build();
        let submitted = TestOrderEventStubs::submitted(&order, account_id);
        let mut cache = cache.borrow_mut();
        cache
            .add_order(order, None, Some(client_id), false)
            .unwrap();
        cache.update_order(&submitted).unwrap();
        orders.push(SeededOrder {
            client_order_id: order_id,
            client_id,
            account_id,
            strategy_id,
            instrument_id,
            side,
            status: OrderStatus::Submitted,
        });
    }

    let mut engine = ExecutionEngine::new(clock, cache.clone(), None);
    engine.register_client(Box::new(client_a)).unwrap();
    engine.register_default_client(Box::new(client_b));
    let command_client = selected_client.map(ClientId::new);
    let routed_client = command_client.unwrap_or(client_a_id);
    let routed_account = if routed_client == client_a_id {
        account_a_id
    } else {
        account_b_id
    };
    let command = CancelAllOrders::new(
        trader_id,
        command_client,
        StrategyId::from("CANCEL-CALLER-001"),
        instrument_id,
        selected_side,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    engine.execute(TradingCommand::CancelAllOrders(command));

    let events = apply_order_events_from_channel(&cache, &mut rx);
    let canceled: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Canceled(canceled) => Some(canceled),
            _ => None,
        })
        .collect();
    let expected_ids: ahash::AHashSet<_> = orders
        .iter()
        .filter(|order| {
            order.client_id == routed_client
                && order.account_id == routed_account
                && order.instrument_id == instrument_id
                && (selected_side == OrderSide::NoOrderSide || order.side == selected_side)
        })
        .map(|order| order.client_order_id)
        .collect();
    let actual_ids: ahash::AHashSet<_> =
        canceled.iter().map(|event| event.client_order_id).collect();

    assert_eq!(actual_ids, expected_ids);

    for event in canceled {
        let order = orders
            .iter()
            .find(|order| order.client_order_id == event.client_order_id)
            .unwrap();
        assert_eq!(event.strategy_id, order.strategy_id);
        assert_eq!(event.account_id, Some(routed_account));
    }
    let cache = cache.borrow();
    for order in &orders {
        let cached = cache.order(&order.client_order_id).unwrap();
        let expected_status = if expected_ids.contains(&order.client_order_id) {
            OrderStatus::Canceled
        } else {
            order.status
        };
        assert_eq!(cached.status(), expected_status);
        assert_eq!(cached.strategy_id(), order.strategy_id);
        assert_eq!(cached.account_id(), Some(order.account_id));
        assert_eq!(
            cache.client_id(&order.client_order_id),
            Some(&order.client_id)
        );
    }
    drop(cache);
    engine.stop();
}

// Regression test for https://github.com/nautechsystems/nautilus_trader/issues/3732
//
// The exec_engine_execute handler holds an immutable borrow on the ExecutionEngine.
// Without the fix, the sandbox client and matching engine synchronously dispatch order
// events back through msgbus to exec_engine_process, which tries borrow_mut() on the
// same RefCell and panics with "RefCell already borrowed".
//
// The fix routes sandbox events through the async runner channel so they are processed
// in the next iteration, after the borrow is released.
#[rstest]
fn test_submit_order_through_exec_engine_no_reentrant_panic(
    trader_id: TraderId,
    instrument: InstrumentAny,
) {
    let venue = Venue::new("BINANCE");
    let account_id = AccountId::from("BINANCE-001");
    let client_id = ClientId::new("SANDBOX");

    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));

    cache
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();

    let instrument_id = instrument.id();
    let quote = create_quote_tick(instrument_id, 1000.0, 1001.0);
    cache.borrow_mut().add_quote(quote).unwrap();

    // Wire up exec engine with registered msgbus handlers
    let engine = Rc::new(RefCell::new(ExecutionEngine::new(
        clock.clone(),
        cache.clone(),
        None,
    )));
    ExecutionEngine::register_msgbus_handlers(&engine);

    // Initialize the exec event sender (simulates the async runner)
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
    set_exec_event_sender(tx);

    // Create and register the sandbox client (venue must match the instrument)
    let usd = Currency::USD();
    let config = SandboxExecutionClientConfig {
        account_id,
        venue,
        starting_balances: vec![Money::new(100_000.0, usd)],
        base_currency: Some(usd),
        oms_type: OmsType::Netting,
        account_type: AccountType::Margin,
        default_leverage: Decimal::ONE,
        leverages: ahash::AHashMap::new(),
        book_type: BookType::L1_MBP,
        fee_model: None,
        fill_model: None,
        latency_model: None,
        frozen_account: false,
        bar_execution: false,
        trade_execution: false,
        reject_stop_orders: true,
        support_gtd_orders: true,
        support_contingent_orders: true,
        use_position_ids: true,
        use_random_ids: false,
        use_reduce_only: true,
        queue_position: false,
        liquidity_consumption: false,
        bar_adaptive_high_low_ordering: false,
        use_market_order_acks: false,
        oto_full_trigger: false,
        price_protection_points: 0,
    };
    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        venue,
        config.oms_type,
        config.account_id,
        config.account_type,
        config.base_currency,
        cache.clone(),
    );
    let mut sandbox_client =
        SandboxExecutionClient::new(core, config, clock.clone(), cache.clone());
    sandbox_client.start().unwrap();
    engine
        .borrow_mut()
        .register_client(Box::new(sandbox_client))
        .unwrap();

    // Build and cache the order
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("0.001"))
        .build();
    cache
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id), false)
        .unwrap();

    // Submit through the exec engine endpoint (this panicked before the fix)
    let ts = clock.borrow().timestamp_ns();
    let submit =
        SubmitOrder::from_order(&order, trader_id, Some(client_id), None, UUID4::new(), ts);
    let endpoint = MessagingSwitchboard::exec_engine_execute();
    msgbus::send_trading_command(endpoint, TradingCommand::SubmitOrder(submit));

    // Verify events arrived through the channel instead of re-entering the engine
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }
    assert!(
        !events.is_empty(),
        "Expected order events through the exec event channel"
    );
}

/// The inbound-latency alert must drive order acceptance even with no market data after submission:
/// a `TestClock` advance past the due time exercises the drain path directly.
#[rstest]
fn test_inbound_latency_alert_drives_acceptance_with_no_market_data(
    trader_id: TraderId,
    account_id: AccountId,
    venue: Venue,
    instrument: InstrumentAny,
) {
    const INSERT_LATENCY_NS: u64 = 1_000_000_000; // 1s of inbound latency

    let mut harness = setup_latency_harness(
        trader_id,
        account_id,
        venue,
        &instrument,
        Some(static_latency_model(INSERT_LATENCY_NS, 0, 0)),
    );

    let submit_time = harness.test_clock.borrow().timestamp_ns();
    let order = submit_resting_limit(
        &harness,
        trader_id,
        &instrument,
        "O-LATENCY-1",
        "100.00",
        submit_time,
    );

    let before = flushed_at_barrier(&harness);
    assert!(
        before
            .iter()
            .any(|event| matches!(event, OrderEventAny::Submitted(submitted)
            if submitted.client_order_id == order.client_order_id())),
        "expected immediate OrderSubmitted",
    );
    assert!(
        !before
            .iter()
            .any(|event| matches!(event, OrderEventAny::Accepted(_))),
        "order must not be accepted before its inbound latency elapses",
    );

    let due = UnixNanos::from(*submit_time + INSERT_LATENCY_NS);
    assert_eq!(
        advance_and_fire(&harness.test_clock, due),
        1,
        "expected exactly one inbound-drain alert to fire",
    );

    let after = settle(&mut harness);
    assert!(
        after
            .iter()
            .any(|event| matches!(event, OrderEventAny::Accepted(accepted)
            if accepted.client_order_id == order.client_order_id())),
        "expected OrderAccepted once the inbound latency elapsed and the alert drained",
    );

    harness.client.stop().unwrap();
}

/// With no latency model configured, the submit path runs inline with no deferral, locking in zero
/// behavior change for existing users (the default `latency_model = None`).
#[rstest]
fn test_no_latency_model_accepts_order_immediately(
    trader_id: TraderId,
    account_id: AccountId,
    venue: Venue,
    instrument: InstrumentAny,
) {
    let mut harness = setup_latency_harness(trader_id, account_id, venue, &instrument, None);

    let submit_time = harness.test_clock.borrow().timestamp_ns();
    let _order = submit_resting_limit(
        &harness,
        trader_id,
        &instrument,
        "O-NO-LATENCY-1",
        "100.00",
        submit_time,
    );

    let events = drain_order_events(&mut harness.rx);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, OrderEventAny::Submitted(submitted)
            if submitted.client_order_id.as_str() == "O-NO-LATENCY-1")),
        "expected immediate OrderSubmitted",
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, OrderEventAny::Accepted(accepted)
            if accepted.client_order_id.as_str() == "O-NO-LATENCY-1")),
        "with no latency model the order must be accepted immediately (no deferral)",
    );

    let fired = advance_and_fire(
        &harness.test_clock,
        UnixNanos::from(*submit_time + 10_000_000_000),
    );
    assert_eq!(
        fired, 0,
        "no inbound alert should be armed without a latency model",
    );

    harness.client.stop().unwrap();
}

/// `reset` must clear the inbound queue and cancel its alert, so a command enqueued before the
/// reset is never later applied against a freshly-reset engine (mirrors backtest
/// `SimulatedExchange::reset`).
#[rstest]
fn test_reset_clears_inbound_queue_and_cancels_alert(
    trader_id: TraderId,
    account_id: AccountId,
    venue: Venue,
    instrument: InstrumentAny,
) {
    const INSERT_LATENCY_NS: u64 = 1_000_000_000; // 1s of inbound latency

    let mut harness = setup_latency_harness(
        trader_id,
        account_id,
        venue,
        &instrument,
        Some(static_latency_model(INSERT_LATENCY_NS, 0, 0)),
    );

    let submit_time = harness.test_clock.borrow().timestamp_ns();
    submit_resting_limit(
        &harness,
        trader_id,
        &instrument,
        "O-RESET-1",
        "100.00",
        submit_time,
    );

    let before = flushed_at_barrier(&harness);
    assert!(
        !before
            .iter()
            .any(|event| matches!(event, OrderEventAny::Accepted(_))),
        "order must not be accepted before its inbound latency elapses",
    );

    harness.client.reset();

    let alert_name = format!("SANDBOX-INBOUND-{}", harness.client.client_id());
    assert_eq!(
        harness.test_clock.borrow().next_time_ns(&alert_name),
        None,
        "reset must cancel the inbound alert",
    );

    let due = UnixNanos::from(*submit_time + INSERT_LATENCY_NS);
    let fired = advance_and_fire(&harness.test_clock, due);
    assert_eq!(fired, 0, "reset must leave no alert to fire");

    let after = flushed_at_barrier(&harness);
    assert!(
        after.is_empty() && drain_order_events(&mut harness.rx).is_empty(),
        "reset must discard the deferred command so it is never applied",
    );

    harness.client.stop().unwrap();
}

/// Commands sharing a due timestamp drain in submission order (FIFO), enforced by the `inbound_seq`
/// tie-break.
#[rstest]
fn test_inbound_latency_fifo_ordering_for_shared_due_timestamp(
    trader_id: TraderId,
    account_id: AccountId,
    venue: Venue,
    instrument: InstrumentAny,
) {
    const INSERT_LATENCY_NS: u64 = 1_000_000_000; // 1s
    const ORDER_COUNT: usize = 4;

    let mut harness = setup_latency_harness(
        trader_id,
        account_id,
        venue,
        &instrument,
        Some(static_latency_model(INSERT_LATENCY_NS, 0, 0)),
    );

    let submit_time = harness.test_clock.borrow().timestamp_ns();
    let expected_ids: Vec<String> = (0..ORDER_COUNT).map(|i| format!("O-FIFO-{i}")).collect();

    // Distinct resting prices below (buy) so every order accepts independently
    for (i, client_order_id) in expected_ids.iter().enumerate() {
        submit_resting_limit(
            &harness,
            trader_id,
            &instrument,
            client_order_id,
            &format!("{}.00", 100 - i),
            submit_time,
        );
    }

    let before = flushed_at_barrier(&harness);
    assert!(
        !before
            .iter()
            .any(|event| matches!(event, OrderEventAny::Accepted(_))),
        "no order may be accepted before its inbound latency elapses",
    );

    let due = UnixNanos::from(*submit_time + INSERT_LATENCY_NS);
    assert_eq!(
        advance_and_fire(&harness.test_clock, due),
        1,
        "orders sharing a due timestamp must arm exactly one inbound alert",
    );

    let accepted_ids: Vec<String> = settle(&mut harness)
        .into_iter()
        .filter_map(|event| match event {
            OrderEventAny::Accepted(accepted) => Some(accepted.client_order_id.to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(
        accepted_ids, expected_ids,
        "commands sharing a due timestamp must drain in submission (FIFO) order",
    );

    harness.client.stop().unwrap();
}

/// The barrier invariant: a command released from the inbound queue has its events processed into
/// the cache before the next released command is applied.
#[rstest]
fn test_inbound_latency_barrier_settles_each_command_before_the_next(
    trader_id: TraderId,
    account_id: AccountId,
    venue: Venue,
    instrument: InstrumentAny,
) {
    const LEG_LATENCY_NS: u64 = 1_000_000_000; // 1s on the insert and update legs alike

    let mut harness = setup_latency_harness(
        trader_id,
        account_id,
        venue,
        &instrument,
        Some(static_latency_model(LEG_LATENCY_NS, LEG_LATENCY_NS, 0)),
    );

    let submit_time = harness.test_clock.borrow().timestamp_ns();
    let order = submit_resting_limit(
        &harness,
        trader_id,
        &instrument,
        "O-BARRIER-1",
        "100.00",
        submit_time,
    );

    // The runner settles the "sent to venue" record during the latency window, as it would live
    let _ = settle(&mut harness);

    harness
        .client
        .modify_order(ModifyOrder::new(
            trader_id,
            Some(harness.client.client_id()),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            None,
            None,
            Some(Price::from("99.00")),
            None,
            UUID4::new(),
            submit_time,
            None,
            None,
        ))
        .unwrap();

    // Both commands fall due in one pass, the modify one sequence behind the submit
    let due = UnixNanos::from(*submit_time + LEG_LATENCY_NS);
    assert_eq!(advance_and_fire(&harness.test_clock, due), 1);

    // Read without settling the channel: whatever is here was flushed at the barrier
    let flushed = flushed_at_barrier(&harness);
    let accepted_at = flushed
        .iter()
        .position(|event| {
            matches!(event, OrderEventAny::Accepted(accepted)
            if accepted.client_order_id == order.client_order_id())
        })
        .expect("the submit's acceptance must be flushed at the barrier, not left on the channel");
    let updated_at = flushed
        .iter()
        .position(|event| {
            matches!(event, OrderEventAny::Updated(updated)
            if updated.client_order_id == order.client_order_id()
                && updated.price == Some(Price::from("99.00")))
        })
        .expect("the modify must be applied in the same pass, against the accepted order");
    assert!(
        accepted_at < updated_at,
        "the submit's events must settle before the modify behind it is applied",
    );

    assert!(
        drain_order_events(&mut harness.rx).is_empty(),
        "a released command's events go straight to the execution engine, not the channel",
    );

    let cache = harness.cache.borrow();
    let cached = cache.order(&order.client_order_id()).unwrap();
    assert_eq!(
        cached.price(),
        Some(Price::from("99.00")),
        "the cache the venue reads must carry both commands' effects",
    );
}

/// Commands sharing one due time apply in enqueue order within a single pass: the monotonic
/// `inbound_seq` tie-break means a later command can never overtake an earlier one.
#[rstest]
fn test_inbound_latency_commands_sharing_a_due_time_apply_in_enqueue_order(
    trader_id: TraderId,
    account_id: AccountId,
    venue: Venue,
    instrument: InstrumentAny,
) {
    const LEG_LATENCY_NS: u64 = 2_000_000_000; // 2s on every leg, so one shared due time

    let mut harness = setup_latency_harness(
        trader_id,
        account_id,
        venue,
        &instrument,
        Some(static_latency_model(
            LEG_LATENCY_NS,
            LEG_LATENCY_NS,
            LEG_LATENCY_NS,
        )),
    );

    let submit_time = harness.test_clock.borrow().timestamp_ns();
    let order = submit_resting_limit(
        &harness,
        trader_id,
        &instrument,
        "O-SHARED-DUE-1",
        "1000.00",
        submit_time,
    );
    let _ = settle(&mut harness);

    harness
        .client
        .modify_order(ModifyOrder::new(
            trader_id,
            Some(harness.client.client_id()),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            None,
            None,
            Some(Price::from("3000.00")),
            None,
            UUID4::new(),
            submit_time,
            None,
            None,
        ))
        .unwrap();
    harness
        .client
        .cancel_order(cancel_for(&harness, trader_id, &order, submit_time))
        .unwrap();

    let due = UnixNanos::from(*submit_time + LEG_LATENCY_NS);
    assert_eq!(advance_and_fire(&harness.test_clock, due), 1);

    let flushed = flushed_at_barrier(&harness);
    let kinds: Vec<&str> = flushed
        .iter()
        .filter(|event| event.client_order_id() == order.client_order_id())
        .filter_map(|event| match event {
            OrderEventAny::Accepted(_) => Some("accepted"),
            OrderEventAny::Updated(_) => Some("updated"),
            OrderEventAny::Canceled(_) => Some("canceled"),
            _ => None,
        })
        .collect();
    assert_eq!(
        kinds,
        vec!["accepted", "updated", "canceled"],
        "commands sharing a due time must apply in enqueue order, each settled before the next",
    );
}

/// A drain flush that re-enters the client must not apply the next command from inside the previous
/// command's flush.
#[rstest]
fn test_inbound_latency_drain_reentered_during_flush_does_not_split_a_command(
    trader_id: TraderId,
    account_id: AccountId,
    venue: Venue,
    instrument: InstrumentAny,
) {
    const INSERT_LATENCY_NS: u64 = 1_000_000_000; // 1s, shared by both commands below

    let mut harness = setup_latency_harness(
        trader_id,
        account_id,
        venue,
        &instrument,
        Some(static_latency_model(INSERT_LATENCY_NS, 0, 0)),
    );

    // Replaces the harness stand-in with one that does everything it did, plus re-enters the client
    // once from inside the flush.
    let reentered = Rc::new(Cell::new(false));
    {
        let cache = harness.cache.clone();
        let events = harness.events.clone();
        let reentered = reentered.clone();
        let reentry_id = InstrumentId::from("REENTRY-PERP.SIM");
        msgbus::register_order_event_endpoint(
            MessagingSwitchboard::exec_engine_process(),
            TypedIntoHandler::from(move |event: OrderEventAny| {
                let _ = cache.borrow_mut().update_order(&event);
                let is_accepted = matches!(event, OrderEventAny::Accepted(_));
                events.borrow_mut().push(event);

                // Exactly once, on the first acceptance the flush emits
                if is_accepted && !reentered.replace(true) {
                    let quote = create_quote_tick(reentry_id, 2000.00, 2010.00);
                    msgbus::publish_quote(
                        format!("data.quotes.{}.{}", reentry_id.venue, reentry_id).into(),
                        &quote,
                    );
                }
            }),
        );
    }

    let submit_time = harness.test_clock.borrow().timestamp_ns();

    // One command emitting two acceptances, so a split is observable
    let legs: Vec<OrderAny> = [("O-REENTRY-L1", "100.00"), ("O-REENTRY-L2", "99.00")]
        .iter()
        .map(|(client_order_id, price)| {
            let order = OrderTestBuilder::new(OrderType::Limit)
                .instrument_id(instrument.id())
                .side(OrderSide::Buy)
                .price(Price::from(*price))
                .quantity(Quantity::from("1.000"))
                .client_order_id((*client_order_id).into())
                .ts_init(submit_time)
                .submit(true)
                .build();
            harness
                .cache
                .borrow_mut()
                .add_order(order.clone(), None, None, false)
                .unwrap();
            order
        })
        .collect();

    harness
        .client
        .submit_order_list(create_submit_order_list(
            trader_id,
            harness.client.client_id(),
            instrument.id(),
            &legs,
        ))
        .unwrap();

    // Enqueued behind the list on the same insert leg, so both fall due in one pass
    submit_resting_limit(
        &harness,
        trader_id,
        &instrument,
        "O-REENTRY-S1",
        "98.00",
        submit_time,
    );

    // The runner settles the "sent to venue" records during the latency window, as it would live
    let _ = settle(&mut harness);

    let due = UnixNanos::from(*submit_time + INSERT_LATENCY_NS);
    assert_eq!(advance_and_fire(&harness.test_clock, due), 1);

    let accepted: Vec<String> = flushed_at_barrier(&harness)
        .into_iter()
        .filter_map(|event| match event {
            OrderEventAny::Accepted(accepted) => Some(accepted.client_order_id.to_string()),
            _ => None,
        })
        .collect();

    assert!(
        reentered.get(),
        "the flush must have re-entered the client, or this test proves nothing",
    );

    let position = |client_order_id: &str| {
        accepted
            .iter()
            .position(|got| got == client_order_id)
            .unwrap_or_else(|| panic!("missing acceptance for {client_order_id} in {accepted:?}"))
    };
    let leg_1 = position("O-REENTRY-L1");
    let leg_2 = position("O-REENTRY-L2");
    let standalone = position("O-REENTRY-S1");

    assert_eq!(
        leg_1.abs_diff(leg_2),
        1,
        "both acceptances come from one command, so nothing may be applied between them: \
         {accepted:?}",
    );
    assert!(
        standalone > leg_1 && standalone > leg_2,
        "the command queued behind the list must be applied only once the list has fully \
         settled: {accepted:?}",
    );

    harness.client.stop().unwrap();
}

/// The re-entrancy claim must hold across sandbox clients, not only within one.
#[rstest]
fn test_inbound_latency_drain_claim_holds_across_sandbox_clients(
    trader_id: TraderId,
    account_id: AccountId,
    venue: Venue,
    instrument: InstrumentAny,
) {
    const INSERT_LATENCY_NS: u64 = 1_000_000_000; // 1s, shared by both clients below

    let mut harness = setup_latency_harness(
        trader_id,
        account_id,
        venue,
        &instrument,
        Some(static_latency_model(INSERT_LATENCY_NS, 0, 0)),
    );

    // A second sandbox client on its own venue, sharing this thread, cache, clock, execution
    // channel and `ExecEngine.process` endpoint with the harness client
    let venue_b = Venue::new("SIM2");
    let client_id_b = ClientId::new("SANDBOX-2");
    let mut config_b = create_config(trader_id, AccountId::from("SANDBOX-002"), venue_b);
    config_b.latency_model = Some(static_latency_model(INSERT_LATENCY_NS, 0, 0));
    let core_b = ExecutionClientCore::new(
        trader_id,
        client_id_b,
        venue_b,
        config_b.oms_type,
        config_b.account_id,
        config_b.account_type,
        config_b.base_currency,
        harness.cache.clone(),
    );
    let clock_b: Rc<RefCell<dyn Clock>> = harness.test_clock.clone();
    let mut client_b =
        SandboxExecutionClient::new(core_b, config_b, clock_b, harness.cache.clone());
    client_b.start().unwrap();

    // Replaces the harness stand-in with one that also models the borrow `ExecutionEngine::process`
    // holds across its dispatch, and re-enters the second client once from inside the flush.
    let reentered = Rc::new(Cell::new(false));
    {
        let cache = harness.cache.clone();
        let events = harness.events.clone();
        let reentered = reentered.clone();
        let processing = Cell::new(false);
        let reentry_id = InstrumentId::from("REENTRY-PERP.SIM2");
        msgbus::register_order_event_endpoint(
            MessagingSwitchboard::exec_engine_process(),
            TypedIntoHandler::from(move |event: OrderEventAny| {
                assert!(
                    !processing.replace(true),
                    "a sandbox client flushed into the execution engine while it was already \
                     processing an event, which re-enters `ExecutionEngine::process`'s own borrow",
                );

                let _ = cache.borrow_mut().update_order(&event);
                let is_accepted = matches!(event, OrderEventAny::Accepted(_));
                events.borrow_mut().push(event);

                // Exactly once, on the first acceptance the flush emits
                if is_accepted && !reentered.replace(true) {
                    let quote = create_quote_tick(reentry_id, 2000.00, 2010.00);
                    msgbus::publish_quote(
                        format!("data.quotes.{}.{}", reentry_id.venue, reentry_id).into(),
                        &quote,
                    );
                }

                processing.set(false);
            }),
        );
    }

    let submit_time = harness.test_clock.borrow().timestamp_ns();

    // One command queued on each client, both falling due at the same time
    let _order_a = submit_resting_limit(
        &harness,
        trader_id,
        &instrument,
        "O-CROSS-A",
        "100.00",
        submit_time,
    );

    let order_b = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .price(Price::from("99.00"))
        .quantity(Quantity::from("1.000"))
        .client_order_id("O-CROSS-B".into())
        .ts_init(submit_time)
        .build();
    harness
        .cache
        .borrow_mut()
        .add_order(order_b.clone(), None, None, false)
        .unwrap();
    client_b
        .submit_order(SubmitOrder::from_order(
            &order_b,
            trader_id,
            Some(client_id_b),
            None,
            UUID4::new(),
            submit_time,
        ))
        .unwrap();

    // The runner settles the "sent to venue" records during the latency window, as it would live
    let _ = settle(&mut harness);

    // Both queues fall due.
    let due = UnixNanos::from(*submit_time + INSERT_LATENCY_NS);
    let _ = harness.test_clock.borrow_mut().advance_time(due, true);

    let accepted_ids = |harness: &LatencyHarness| -> Vec<String> {
        flushed_at_barrier(harness)
            .into_iter()
            .filter_map(|event| match event {
                OrderEventAny::Accepted(accepted) => Some(accepted.client_order_id.to_string()),
                _ => None,
            })
            .collect()
    };

    // Releases the first client's queue; its flush then reaches the second client, whose own
    // command is already due
    let release_id = InstrumentId::from("RELEASE-PERP.SIM");
    let quote = create_quote_tick(release_id, 2000.00, 2010.00);
    msgbus::publish_quote(
        format!("data.quotes.{}.{}", release_id.venue, release_id).into(),
        &quote,
    );

    assert!(
        reentered.get(),
        "the flush must have reached the second client, or this test proves nothing",
    );
    assert_eq!(
        accepted_ids(&harness),
        vec!["O-CROSS-A".to_string()],
        "the second client must not release its queue from inside the first client's flush",
    );

    // Nothing is lost by skipping: the next tick on that client's own venue releases it
    let quote = create_quote_tick(InstrumentId::from("REENTRY-PERP.SIM2"), 2000.00, 2010.00);
    msgbus::publish_quote("data.quotes.SIM2.REENTRY-PERP.SIM2".into(), &quote);
    assert_eq!(
        accepted_ids(&harness),
        vec!["O-CROSS-B".to_string()],
        "the skipped queue must be released by the next market-data tick on its own venue",
    );

    client_b.stop().unwrap();
    harness.client.stop().unwrap();
}

/// `SubmitOrderList` travels the same enqueue → drain path as a lone `SubmitOrder`, deferred by the
/// insert leg; an interior checkpoint confirms `command_leg_latency` is not misrouting it.
#[rstest]
fn test_inbound_latency_submit_order_list_through_deferred_path(
    trader_id: TraderId,
    account_id: AccountId,
    venue: Venue,
    instrument: InstrumentAny,
) {
    const INSERT_LATENCY_NS: u64 = 2_000_000_000; // 2s
    const UPDATE_LATENCY_NS: u64 = 5_000_000_000; // 5s (deliberately longer than insert)
    const DELETE_LATENCY_NS: u64 = 7_000_000_000; // 7s (deliberately longer than insert or update)

    let mut harness = setup_latency_harness(
        trader_id,
        account_id,
        venue,
        &instrument,
        Some(static_latency_model(
            INSERT_LATENCY_NS,
            UPDATE_LATENCY_NS,
            DELETE_LATENCY_NS,
        )),
    );

    let submit_time = harness.test_clock.borrow().timestamp_ns();
    let orders: Vec<OrderAny> = [("O-LIST-1", "100.00"), ("O-LIST-2", "99.00")]
        .iter()
        .map(|(client_order_id, price)| {
            let order = OrderTestBuilder::new(OrderType::Limit)
                .instrument_id(instrument.id())
                .side(OrderSide::Buy)
                .price(Price::from(*price))
                .quantity(Quantity::from("1.000"))
                .client_order_id((*client_order_id).into())
                .ts_init(submit_time)
                .submit(true)
                .build();
            harness
                .cache
                .borrow_mut()
                .add_order(order.clone(), None, None, false)
                .unwrap();
            order
        })
        .collect();

    harness
        .client
        .submit_order_list(create_submit_order_list(
            trader_id,
            harness.client.client_id(),
            instrument.id(),
            &orders,
        ))
        .unwrap();

    // Both legs' immediate "sent to venue" records arrive; neither reaches the engine yet
    let before = flushed_at_barrier(&harness);
    assert_eq!(
        before
            .iter()
            .filter(|event| matches!(event, OrderEventAny::Submitted(_)))
            .count(),
        2,
        "expected an immediate OrderSubmitted for each leg",
    );
    assert!(
        !before
            .iter()
            .any(|event| matches!(event, OrderEventAny::Accepted(_))),
        "no leg may be accepted before the insert leg elapses",
    );

    // Interior checkpoint, partway through the insert leg: if the command were misrouted to a leg
    // shorter than (or equal to) this point - the query default of zero, for instance - the alert
    // would already have fired and this advance would apply it, so it must fire nothing.
    let midpoint = UnixNanos::from(*submit_time + INSERT_LATENCY_NS / 2);
    assert_eq!(
        advance_and_fire(&harness.test_clock, midpoint),
        0,
        "the list must not apply before the insert leg elapses",
    );
    let mid = settle(&mut harness);
    assert!(
        !mid.iter()
            .any(|event| matches!(event, OrderEventAny::Accepted(_))),
        "no leg may be accepted before the insert leg elapses",
    );

    let due = UnixNanos::from(*submit_time + INSERT_LATENCY_NS);
    assert_eq!(advance_and_fire(&harness.test_clock, due), 1);

    let events = settle(&mut harness);

    for order in &orders {
        assert!(
            events
                .iter()
                .any(|event| matches!(event, OrderEventAny::Accepted(accepted)
            if accepted.client_order_id == order.client_order_id())),
            "expected OrderAccepted for {} once the insert leg elapsed",
            order.client_order_id(),
        );
        let status = harness
            .cache
            .borrow()
            .order(&order.client_order_id())
            .unwrap()
            .status();
        assert_eq!(
            status,
            OrderStatus::Accepted,
            "the list leg's OrderAccepted must be a valid FSM transition, not merely emitted",
        );
    }
    assert_eq!(
        harness.client.matching_engine_count(),
        1,
        "both legs share one instrument, so exactly one matching engine must exist",
    );

    harness.client.stop().unwrap();
}

/// Each deferred command is held by its own leg of the model, distinct from the insert leg its
/// submit used: advancing only by the (shorter) insert leg must not settle it.
#[rstest]
#[case::cancel_uses_delete_leg(DeferredCommand::Cancel)]
#[case::modify_uses_update_leg(DeferredCommand::Modify)]
fn test_inbound_latency_command_is_deferred_by_its_own_leg(
    trader_id: TraderId,
    account_id: AccountId,
    venue: Venue,
    instrument: InstrumentAny,
    #[case] kind: DeferredCommand,
) {
    const INSERT_LATENCY_NS: u64 = 1_000_000_000; // 1s
    const LEG_LATENCY_NS: u64 = 3_000_000_000; // 3s (deliberately longer than the insert leg)

    let mut harness = setup_latency_harness(
        trader_id,
        account_id,
        venue,
        &instrument,
        Some(kind.latency_model(INSERT_LATENCY_NS, LEG_LATENCY_NS)),
    );

    let submit_time = harness.test_clock.borrow().timestamp_ns();
    let order = submit_resting_limit(
        &harness,
        trader_id,
        &instrument,
        "O-LEG-1",
        "100.00",
        submit_time,
    );

    // In flight behind the insert leg: submitted immediately, not yet accepted
    let before = flushed_at_barrier(&harness);
    assert!(
        before
            .iter()
            .any(|event| matches!(event, OrderEventAny::Submitted(submitted)
            if submitted.client_order_id == order.client_order_id())),
        "expected immediate OrderSubmitted",
    );
    assert!(
        !before
            .iter()
            .any(|event| matches!(event, OrderEventAny::Accepted(_))),
        "submit must not reach the engine before the insert leg elapses",
    );

    // The submit drains exactly at t0 + insert.
    let accept_due = UnixNanos::from(*submit_time + INSERT_LATENCY_NS);
    assert_eq!(advance_and_fire(&harness.test_clock, accept_due), 1);
    let accepted = settle(&mut harness);
    assert!(
        accepted
            .iter()
            .any(|event| matches!(event, OrderEventAny::Accepted(accepted)
            if accepted.client_order_id == order.client_order_id())),
        "expected OrderAccepted at t0 + insert latency",
    );

    // Issued at the current clock (accept_due), so it falls due at accept_due + its own leg
    send_deferred_command(
        &harness,
        trader_id,
        kind,
        std::slice::from_ref(&order),
        accept_due,
    );

    // Advancing only by the insert leg must not settle it, since its own leg is longer
    assert_eq!(
        advance_and_fire(
            &harness.test_clock,
            UnixNanos::from(*accept_due + INSERT_LATENCY_NS),
        ),
        0,
        "the command must be deferred by its own leg, not the (shorter) insert leg",
    );
    let mid = settle(&mut harness);
    assert!(
        !kind.applied_to(&mid, order.client_order_id()),
        "the command must not settle before its own leg elapses",
    );

    // It reaches the venue once its own leg elapses
    let due = UnixNanos::from(*accept_due + LEG_LATENCY_NS);
    assert_eq!(advance_and_fire(&harness.test_clock, due), 1);
    let after = settle(&mut harness);
    assert!(
        kind.applied_to(&after, order.client_order_id()),
        "the command must settle at the time it was issued plus its own leg",
    );

    harness.client.stop().unwrap();
}

/// A `StaticLatencyModel` with only `insert_latency_nanos` set leaves the update and delete legs at
/// zero.
#[rstest]
fn test_inbound_latency_zero_leg_command_is_applied_inline(
    trader_id: TraderId,
    account_id: AccountId,
    venue: Venue,
    instrument: InstrumentAny,
) {
    // The client id `setup_latency_harness` builds, as `inbound_alert_name` formats it
    const ALERT: &str = "SANDBOX-INBOUND-SANDBOX";
    const INSERT_LATENCY_NS: u64 = 1_000_000_000; // 1s; update and delete legs default to zero

    let mut harness = setup_latency_harness(
        trader_id,
        account_id,
        venue,
        &instrument,
        Some(static_latency_model(INSERT_LATENCY_NS, 0, 0)),
    );

    let submit_time = harness.test_clock.borrow().timestamp_ns();
    let order = submit_resting_limit(
        &harness,
        trader_id,
        &instrument,
        "O-ZERO-LEG-1",
        "100.00",
        submit_time,
    );

    let accept_due = UnixNanos::from(*submit_time + INSERT_LATENCY_NS);
    assert_eq!(
        harness.test_clock.borrow().next_time_ns(ALERT),
        Some(accept_due),
        "a non-zero leg arms the alert for exactly its due time",
    );
    assert_eq!(advance_and_fire(&harness.test_clock, accept_due), 1);
    let accepted = settle(&mut harness);
    assert!(
        accepted
            .iter()
            .any(|event| matches!(event, OrderEventAny::Accepted(accepted)
            if accepted.client_order_id == order.client_order_id())),
        "expected OrderAccepted at t0 + insert latency",
    );

    // Zero update leg, empty queue: applied inline, with no clock advance to reach it
    harness
        .client
        .modify_order(modify_for(&harness, trader_id, &order, accept_due))
        .unwrap();

    assert!(
        harness.test_clock.borrow().next_time_ns(ALERT).is_none(),
        "a zero-latency command must not arm an alert for a time already passed",
    );
    let modified = settle(&mut harness);
    assert!(
        modified
            .iter()
            .any(|event| matches!(event, OrderEventAny::Updated(updated)
            if updated.client_order_id == order.client_order_id()
                && updated.price == Some(Price::from("99.00")))),
        "the zero-latency modify must be applied inline, not left queued",
    );

    // Same for the zero delete leg
    harness
        .client
        .cancel_order(cancel_for(&harness, trader_id, &order, accept_due))
        .unwrap();

    assert!(
        harness.test_clock.borrow().next_time_ns(ALERT).is_none(),
        "a zero-latency cancel must not arm an alert either",
    );
    let canceled = settle(&mut harness);
    assert!(
        canceled
            .iter()
            .any(|event| matches!(event, OrderEventAny::Canceled(canceled)
            if canceled.client_order_id == order.client_order_id())),
        "the zero-latency cancel must be applied inline, not left queued",
    );

    harness.client.stop().unwrap();
}

/// Without a latency model a command cannot overtake anything, so no engine is created for a cancel
/// with no order; creating one would consume a raw engine ID it should not need.
#[rstest]
fn test_no_latency_model_cancel_without_engine_creates_no_engine(
    trader_id: TraderId,
    account_id: AccountId,
    venue: Venue,
    instrument: InstrumentAny,
) {
    let mut harness = setup_latency_harness(trader_id, account_id, venue, &instrument, None);

    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .price(Price::from("100.00"))
        .quantity(Quantity::from("1.000"))
        .client_order_id("O-NO-ENGINE-1".into())
        .build();
    harness
        .cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    harness
        .client
        .cancel_order(CancelOrder::new(
            trader_id,
            Some(harness.client.client_id()),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            None,
            UUID4::new(),
            harness.test_clock.borrow().timestamp_ns(),
            None,
            None,
        ))
        .unwrap();

    assert_eq!(
        harness.client.matching_engine_count(),
        0,
        "the immediate path must not create a matching engine for an unknown order",
    );
    assert!(
        drain_order_events(&mut harness.rx).is_empty(),
        "the immediate path must keep its pre-feature silent no-op",
    );

    harness.client.stop().unwrap();
}

/// Inbound latency lets an order-targeting command overtake the submit that would have created the
/// matching engine.
#[rstest]
#[case::cancel(DeferredCommand::Cancel)]
#[case::modify(DeferredCommand::Modify)]
#[case::batch_cancel(DeferredCommand::BatchCancel)]
#[case::batch_modify(DeferredCommand::BatchModify)]
fn test_inbound_latency_deferred_command_overtaking_its_submit_is_rejected(
    trader_id: TraderId,
    account_id: AccountId,
    venue: Venue,
    instrument: InstrumentAny,
    #[case] kind: DeferredCommand,
) {
    const INSERT_LATENCY_NS: u64 = 3_000_000_000; // 3s
    const LEG_LATENCY_NS: u64 = 1_000_000_000; // 1s (shorter, so the command overtakes)

    let mut harness = setup_latency_harness(
        trader_id,
        account_id,
        venue,
        &instrument,
        Some(kind.latency_model(INSERT_LATENCY_NS, LEG_LATENCY_NS)),
    );

    let submit_time = harness.test_clock.borrow().timestamp_ns();
    let orders: Vec<OrderAny> = ["O-OVERTAKE-1", "O-OVERTAKE-2"]
        .iter()
        .map(|client_order_id| {
            submit_resting_limit(
                &harness,
                trader_id,
                &instrument,
                client_order_id,
                "100.00",
                submit_time,
            )
        })
        .collect();
    let targets = &orders[..kind.target_count()];

    // Precondition: the submits are still in flight, so no engine exists for the instrument
    assert_eq!(
        harness.client.matching_engine_count(),
        0,
        "no market data and deferred submits must leave the instrument without an engine",
    );

    send_deferred_command(&harness, trader_id, kind, targets, submit_time);
    let _ = drain_order_events(&mut harness.rx);

    // The command drains at t0 + its own leg, well before the submits' t0 + insert
    let due = UnixNanos::from(*submit_time + LEG_LATENCY_NS);
    assert_eq!(advance_and_fire(&harness.test_clock, due), 1);

    let events = settle(&mut harness);
    let expected: Vec<ClientOrderId> = targets
        .iter()
        .map(|order| order.client_order_id())
        .collect();
    assert_eq!(
        kind.rejected_ids(&events),
        expected,
        "every target of a command overtaking its submit must be rejected, not dropped",
    );

    harness.client.stop().unwrap();
}

/// When the instrument itself is absent from the cache, `ensure_engine_for` cannot build an engine
/// and none of the apply helpers can raise a rejection without one.
#[rstest]
#[case::cancel(DeferredCommand::Cancel)]
#[case::modify(DeferredCommand::Modify)]
#[case::batch_cancel(DeferredCommand::BatchCancel)]
#[case::batch_modify(DeferredCommand::BatchModify)]
fn test_inbound_latency_deferred_command_for_uncached_instrument_is_rejected(
    trader_id: TraderId,
    account_id: AccountId,
    venue: Venue,
    instrument: InstrumentAny,
    #[case] kind: DeferredCommand,
) {
    const LEG_LATENCY_NS: u64 = 1_000_000_000; // 1s

    let mut harness = setup_latency_harness(
        trader_id,
        account_id,
        venue,
        &instrument,
        Some(kind.latency_model(0, LEG_LATENCY_NS)),
    );

    let ts = harness.test_clock.borrow().timestamp_ns();
    let orders: Vec<OrderAny> = ["O-UNCACHED-1", "O-UNCACHED-2"]
        .iter()
        .map(|client_order_id| {
            let order = build_uncached_instrument_order(&harness, client_order_id, ts);
            kind.mark_pending(&harness.cache, &order, trader_id, ts);
            order
        })
        .collect();
    let targets = &orders[..kind.target_count()];

    send_deferred_command(&harness, trader_id, kind, targets, ts);
    let _ = drain_order_events(&mut harness.rx);

    let due = UnixNanos::from(*ts + LEG_LATENCY_NS);
    assert_eq!(advance_and_fire(&harness.test_clock, due), 1);

    let events = settle(&mut harness);
    let expected: Vec<ClientOrderId> = targets
        .iter()
        .map(|order| order.client_order_id())
        .collect();
    assert_eq!(
        kind.rejected_ids(&events),
        expected,
        "every target must be rejected when no engine can be built for its instrument",
    );

    for order in targets {
        let cache = harness.cache.borrow();
        let cached = cache.order(&order.client_order_id()).unwrap();
        assert!(
            !cached.is_pending_cancel() && !cached.is_pending_update(),
            "order must not be stranded pending when its instrument is absent from the cache",
        );
    }
    assert_eq!(
        harness.client.matching_engine_count(),
        0,
        "no engine can be built without the instrument, so none must exist",
    );

    harness.client.stop().unwrap();
}

/// Market data fed through the public processing API must drain commands that have fallen due
/// before matching, exactly as the registered message handlers do (`drain_inbound`).
#[rstest]
fn test_inbound_latency_public_quote_processing_drains_due_commands(
    trader_id: TraderId,
    account_id: AccountId,
    venue: Venue,
    instrument: InstrumentAny,
) {
    const INSERT_LATENCY_NS: u64 = 1_000_000_000; // 1s

    let mut harness = setup_latency_harness(
        trader_id,
        account_id,
        venue,
        &instrument,
        Some(static_latency_model(INSERT_LATENCY_NS, 0, 0)),
    );

    let submit_time = harness.test_clock.borrow().timestamp_ns();
    let order = submit_resting_limit(
        &harness,
        trader_id,
        &instrument,
        "O-DRAIN-ON-DATA-1",
        "3000.00",
        submit_time,
    );
    let _ = drain_order_events(&mut harness.rx);

    // Reach the due time without delivering the drain alert, as jitter can
    let _ = harness
        .test_clock
        .borrow_mut()
        .advance_time(UnixNanos::from(*submit_time + INSERT_LATENCY_NS), true);

    let quote = create_quote_tick(instrument.id(), 2000.00, 2010.00);
    harness.client.process_quote_tick(&quote).unwrap();

    let events = settle(&mut harness);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, OrderEventAny::Accepted(accepted)
            if accepted.client_order_id == order.client_order_id())),
        "the due submit must reach the venue before the quote is matched",
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, OrderEventAny::Filled(fill)
            if fill.client_order_id == order.client_order_id())),
        "the order must fill against the quote that arrived after its latency elapsed",
    );

    harness.client.stop().unwrap();
}

/// Commands discarded by `stop()` never reached the venue, so their orders must be terminalized
/// here: the sandbox emits no status reports to resolve them later.
#[rstest]
fn test_stop_terminalizes_commands_still_in_flight(
    trader_id: TraderId,
    account_id: AccountId,
    venue: Venue,
    instrument: InstrumentAny,
) {
    const INSERT_LATENCY_NS: u64 = 1_000_000_000; // 1s

    let mut harness = setup_latency_harness(
        trader_id,
        account_id,
        venue,
        &instrument,
        Some(static_latency_model(
            INSERT_LATENCY_NS,
            0,
            INSERT_LATENCY_NS,
        )),
    );

    let submit_time = harness.test_clock.borrow().timestamp_ns();
    let resting = submit_resting_limit(
        &harness,
        trader_id,
        &instrument,
        "O-STOP-CANCEL-1",
        "99.00",
        submit_time,
    );
    advance_and_fire(
        &harness.test_clock,
        UnixNanos::from(*submit_time + INSERT_LATENCY_NS),
    );
    let _ = settle(&mut harness);

    // Both of these are still in flight when the client stops
    let cancel_time = harness.test_clock.borrow().timestamp_ns();
    let stranded_submit = submit_resting_limit(
        &harness,
        trader_id,
        &instrument,
        "O-STOP-SUBMIT-1",
        "100.00",
        cancel_time,
    );

    mark_pending_cancel(&harness.cache, &resting, trader_id, cancel_time);
    harness
        .client
        .cancel_order(CancelOrder::new(
            trader_id,
            Some(harness.client.client_id()),
            resting.strategy_id(),
            resting.instrument_id(),
            resting.client_order_id(),
            None,
            UUID4::new(),
            cancel_time,
            None,
            None,
        ))
        .unwrap();
    let _ = drain_order_events(&mut harness.rx);

    harness.client.stop().unwrap();

    // Every event emitted at stop must be applicable to the cached order, not merely emitted
    let events = settle(&mut harness);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, OrderEventAny::Rejected(rejected)
            if rejected.client_order_id == stranded_submit.client_order_id())),
        "a submit discarded at stop must reject the order it never delivered",
    );
    assert!(
        events.iter().any(
            |event| matches!(event, OrderEventAny::CancelRejected(rejected)
            if rejected.client_order_id == resting.client_order_id())
        ),
        "a cancel discarded at stop must release the order from PENDING_CANCEL",
    );

    let stranded_submit_status = harness
        .cache
        .borrow()
        .order(&stranded_submit.client_order_id())
        .unwrap()
        .status();
    assert_eq!(
        stranded_submit_status,
        OrderStatus::Rejected,
        "the FSM must accept the synthesized rejection for the never-delivered submit",
    );

    let resting_status = harness
        .cache
        .borrow()
        .order(&resting.client_order_id())
        .unwrap()
        .status();
    assert_eq!(
        resting_status,
        OrderStatus::Accepted,
        "the FSM must accept the synthesized cancel-rejected, restoring the pre-cancel status",
    );

    // Discarded means gone, not merely rejected: a restart must not resurrect the queue and apply
    // the stranded commands later, on the first enqueue that re-arms the drain.
    harness.client.start().unwrap();
    let restart_time = harness.test_clock.borrow().timestamp_ns();
    submit_resting_limit(
        &harness,
        trader_id,
        &instrument,
        "O-FRESH-1",
        "100.00",
        restart_time,
    );
    let _ = drain_order_events(&mut harness.rx);

    advance_and_fire(
        &harness.test_clock,
        UnixNanos::from(*restart_time + INSERT_LATENCY_NS),
    );

    let events = settle(&mut harness);
    let accepted: Vec<&str> = events
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Accepted(accepted) => Some(accepted.client_order_id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        accepted,
        vec!["O-FRESH-1"],
        "a command stranded by stop() must be discarded, not applied after restart",
    );

    harness.client.stop().unwrap();
}

/// `stop()` unwinds the queue last-issued-first, so several commands pending on one order are
/// rejected in an order the FSM accepts: the modify's rejection restores the order's previous
/// status, and only then does the submit's rejection terminalize it.
#[rstest]
#[case::equal_legs(1_000_000_000, 1_000_000_000)] // insert and update legs both 1s: due_ns ties
#[case::shorter_update_leg(2_000_000_000, 500_000_000)] // update leg shorter: modify is due first
fn test_stop_unwinds_multiple_pending_commands_for_one_order_in_fsm_order(
    #[case] insert_latency_ns: u64,
    #[case] update_latency_ns: u64,
    trader_id: TraderId,
    account_id: AccountId,
    venue: Venue,
    instrument: InstrumentAny,
) {
    let mut harness = setup_latency_harness(
        trader_id,
        account_id,
        venue,
        &instrument,
        Some(static_latency_model(
            insert_latency_ns,
            update_latency_ns,
            0,
        )),
    );

    let submit_time = harness.test_clock.borrow().timestamp_ns();
    let order = submit_resting_limit(
        &harness,
        trader_id,
        &instrument,
        "O-UNWIND-1",
        "100.00",
        submit_time,
    );

    // The runner settles the "sent to venue" record, leaving the order SUBMITTED
    let _ = settle(&mut harness);

    // `Strategy::modify_order` establishes PENDING_UPDATE before its command reaches the client
    mark_pending_update(&harness.cache, &order, trader_id, submit_time);
    harness
        .client
        .modify_order(modify_for(&harness, trader_id, &order, submit_time))
        .unwrap();

    // Both commands are still in flight
    harness.client.stop().unwrap();

    let events = settle(&mut harness);
    let modify_rejected_at = events
        .iter()
        .position(|event| {
            matches!(event, OrderEventAny::ModifyRejected(rejected)
            if rejected.client_order_id == order.client_order_id())
        })
        .expect("the in-flight modify must be rejected at stop");
    let rejected_at = events
        .iter()
        .position(|event| {
            matches!(event, OrderEventAny::Rejected(rejected)
            if rejected.client_order_id == order.client_order_id())
        })
        .expect("the in-flight submit must be rejected at stop");
    assert!(
        modify_rejected_at < rejected_at,
        "the modify must unwind before the submit it was stacked on, or the FSM refuses it",
    );

    let cache = harness.cache.borrow();
    let cached = cache.order(&order.client_order_id()).unwrap();
    assert_eq!(
        cached.status(),
        OrderStatus::Rejected,
        "both rejections must apply, leaving the order terminal rather than stranded",
    );
    assert!(
        !cached.is_pending_update(),
        "the order must not be left pending an update that never reached the venue",
    );
}

/// `CancelAllOrders` travels the same enqueue → drain path as `CancelOrder`, deferred by the delete
/// leg; every open order on the instrument reaches `Canceled` directly from `Accepted`.
#[rstest]
fn test_inbound_latency_cancel_all_orders_through_deferred_path(
    trader_id: TraderId,
    account_id: AccountId,
    venue: Venue,
    instrument: InstrumentAny,
) {
    const INSERT_LATENCY_NS: u64 = 1_000_000_000; // 1s
    const DELETE_LATENCY_NS: u64 = 2_000_000_000; // 2s (deliberately != insert)

    let mut harness = setup_latency_harness(
        trader_id,
        account_id,
        venue,
        &instrument,
        Some(static_latency_model(
            INSERT_LATENCY_NS,
            0,
            DELETE_LATENCY_NS,
        )),
    );

    let submit_time = harness.test_clock.borrow().timestamp_ns();
    let first = submit_resting_limit(
        &harness,
        trader_id,
        &instrument,
        "O-CANCEL-ALL-DEFERRED-1",
        "100.00",
        submit_time,
    );
    let second = submit_resting_limit(
        &harness,
        trader_id,
        &instrument,
        "O-CANCEL-ALL-DEFERRED-2",
        "99.00",
        submit_time,
    );

    let accept_due = UnixNanos::from(*submit_time + INSERT_LATENCY_NS);
    assert_eq!(advance_and_fire(&harness.test_clock, accept_due), 1);
    let accepted = settle(&mut harness);
    assert_eq!(
        accepted
            .iter()
            .filter(|event| matches!(event, OrderEventAny::Accepted(_)))
            .count(),
        2,
        "both resting orders must be accepted before the cancel-all is issued",
    );

    harness
        .client
        .cancel_all_orders(CancelAllOrders::new(
            trader_id,
            Some(harness.client.client_id()),
            first.strategy_id(),
            instrument.id(),
            OrderSide::NoOrderSide,
            UUID4::new(),
            accept_due,
            None,
            None,
        ))
        .unwrap();

    // Advancing only by the insert leg must not settle the cancel-all: the delete leg is longer
    assert_eq!(
        advance_and_fire(
            &harness.test_clock,
            UnixNanos::from(*accept_due + INSERT_LATENCY_NS),
        ),
        0,
        "cancel-all must use the delete leg, not the (shorter) insert leg",
    );
    let mid = settle(&mut harness);
    assert!(
        !mid.iter()
            .any(|event| matches!(event, OrderEventAny::Canceled(_))),
        "cancel-all must not settle before its delete leg elapses",
    );

    // The cancel-all drains once the delete leg elapses (accept_due + delete)
    let cancel_due = UnixNanos::from(*accept_due + DELETE_LATENCY_NS);
    assert_eq!(advance_and_fire(&harness.test_clock, cancel_due), 1);

    let events = settle(&mut harness);

    for order in [&first, &second] {
        assert!(
            events
                .iter()
                .any(|event| matches!(event, OrderEventAny::Canceled(canceled)
            if canceled.client_order_id == order.client_order_id())),
            "expected OrderCanceled for {} once the delete leg elapsed",
            order.client_order_id(),
        );
        let status = harness
            .cache
            .borrow()
            .order(&order.client_order_id())
            .unwrap()
            .status();
        assert_eq!(
            status,
            OrderStatus::Canceled,
            "the cancel-all's OrderCanceled must be a valid FSM transition from Accepted, not \
             merely emitted",
        );
    }

    harness.client.stop().unwrap();
}

/// Unlike `cancel_order` / `cancel_orders`, the strategy marks no target `PENDING_CANCEL` for a
/// cancel-all, so discarding one at stop must be a no-op: the FSM has no rejection to fall back on.
#[rstest]
fn test_stop_terminalizes_cancel_all_without_invalid_transitions(
    trader_id: TraderId,
    account_id: AccountId,
    venue: Venue,
    instrument: InstrumentAny,
) {
    const INSERT_LATENCY_NS: u64 = 1_000_000_000; // 1s

    let mut harness = setup_latency_harness(
        trader_id,
        account_id,
        venue,
        &instrument,
        Some(static_latency_model(
            INSERT_LATENCY_NS,
            0,
            INSERT_LATENCY_NS,
        )),
    );

    let submit_time = harness.test_clock.borrow().timestamp_ns();
    let resting = submit_resting_limit(
        &harness,
        trader_id,
        &instrument,
        "O-CANCEL-ALL-1",
        "99.00",
        submit_time,
    );
    advance_and_fire(
        &harness.test_clock,
        UnixNanos::from(*submit_time + INSERT_LATENCY_NS),
    );
    let _ = settle(&mut harness);

    // Still in flight when the client stops: the strategy marks no order pending-cancel for a
    // cancel-all, so `resting` stays `Accepted` in the cache throughout.
    let cancel_time = harness.test_clock.borrow().timestamp_ns();
    harness
        .client
        .cancel_all_orders(CancelAllOrders::new(
            trader_id,
            Some(harness.client.client_id()),
            resting.strategy_id(),
            resting.instrument_id(),
            OrderSide::NoOrderSide,
            UUID4::new(),
            cancel_time,
            None,
            None,
        ))
        .unwrap();
    let _ = drain_order_events(&mut harness.rx);

    harness.client.stop().unwrap();

    // A silently-dropped invalid transition would leave `resting` looking untouched too, so assert
    // on the emitted events themselves rather than only on the post-apply status.
    let events = settle(&mut harness);
    assert!(
        events.is_empty(),
        "a discarded CancelAllOrders must emit nothing: the strategy marked no order pending \
         cancel, so there is nothing for the FSM to release",
    );

    let resting_status = harness
        .cache
        .borrow()
        .order(&resting.client_order_id())
        .unwrap()
        .status();
    assert_eq!(
        resting_status,
        OrderStatus::Accepted,
        "a discarded CancelAllOrders must not perturb an order the strategy never marked \
         pending cancel",
    );
}

/// A leg already closed when `submit_order_list` first ran never received an `OrderSubmitted`, so
/// discarding the still-deferred list at stop must reject only the leg still in flight.
#[rstest]
fn test_stop_terminalizes_only_in_flight_legs_of_a_submit_order_list(
    trader_id: TraderId,
    account_id: AccountId,
    venue: Venue,
    instrument: InstrumentAny,
) {
    const INSERT_LATENCY_NS: u64 = 1_000_000_000; // 1s

    let mut harness = setup_latency_harness(
        trader_id,
        account_id,
        venue,
        &instrument,
        Some(static_latency_model(INSERT_LATENCY_NS, 0, 0)),
    );

    let ts = harness.test_clock.borrow().timestamp_ns();
    let open_leg = OrderTestBuilder::new(OrderType::Limit)
        .trader_id(trader_id)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .price(Price::from("99.00"))
        .quantity(Quantity::from("1.000"))
        .client_order_id("O-LIST-OPEN-1".into())
        .ts_init(ts)
        .submit(true)
        .build();
    // Not `.submit(true)`: stays `Initialized`, the only status the FSM accepts the `OrderDenied`
    // below from.
    let closed_leg = OrderTestBuilder::new(OrderType::Limit)
        .trader_id(trader_id)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .price(Price::from("98.00"))
        .quantity(Quantity::from("1.000"))
        .client_order_id("O-LIST-CLOSED-1".into())
        .ts_init(ts)
        .build();
    harness
        .cache
        .borrow_mut()
        .add_order(open_leg.clone(), None, None, false)
        .unwrap();
    harness
        .cache
        .borrow_mut()
        .add_order(closed_leg.clone(), None, None, false)
        .unwrap();

    // Already closed before the list is even submitted, e.g. denied by a pre-trade risk check
    let denied = OrderEventAny::Denied(OrderDenied::new(
        closed_leg.trader_id(),
        closed_leg.strategy_id(),
        closed_leg.instrument_id(),
        closed_leg.client_order_id(),
        Ustr::from("test denial"),
        UUID4::new(),
        ts,
        ts,
    ));
    harness.cache.borrow_mut().update_order(&denied).unwrap();

    harness
        .client
        .submit_order_list(create_submit_order_list(
            trader_id,
            harness.client.client_id(),
            instrument.id(),
            &[open_leg.clone(), closed_leg.clone()],
        ))
        .unwrap();
    let _ = drain_order_events(&mut harness.rx);

    harness.client.stop().unwrap();

    let events = settle(&mut harness);
    let rejected_ids: Vec<ClientOrderId> = events
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Rejected(rejected) => Some(rejected.client_order_id),
            _ => None,
        })
        .collect();
    assert_eq!(
        rejected_ids,
        vec![open_leg.client_order_id()],
        "only the leg still in flight must be rejected when the list is discarded at stop",
    );

    let open_status = harness
        .cache
        .borrow()
        .order(&open_leg.client_order_id())
        .unwrap()
        .status();
    assert_eq!(
        open_status,
        OrderStatus::Rejected,
        "the FSM must accept the synthesized rejection for the in-flight leg",
    );

    let closed_status = harness
        .cache
        .borrow()
        .order(&closed_leg.client_order_id())
        .unwrap()
        .status();
    assert_eq!(
        closed_status,
        OrderStatus::Denied,
        "an already-closed leg must be left untouched, the FSM has no transition to re-reject it",
    );
}

/// A deferred submit whose order is no longer in the cache when its latency elapses cannot be
/// applied; since `OrderSubmitted` already went out, the failure must terminalize the order.
#[rstest]
fn test_inbound_latency_deferred_submit_rejects_when_it_cannot_be_applied(
    trader_id: TraderId,
    account_id: AccountId,
    venue: Venue,
    instrument: InstrumentAny,
) {
    const INSERT_LATENCY_NS: u64 = 1_000_000_000; // 1s

    let mut harness = setup_latency_harness(
        trader_id,
        account_id,
        venue,
        &instrument,
        Some(static_latency_model(INSERT_LATENCY_NS, 0, 0)),
    );

    let submit_time = harness.test_clock.borrow().timestamp_ns();
    let order = submit_resting_limit(
        &harness,
        trader_id,
        &instrument,
        "O-GONE-1",
        "100.00",
        submit_time,
    );
    let _ = drain_order_events(&mut harness.rx);

    harness
        .cache
        .borrow_mut()
        .purge_order(order.client_order_id());

    assert_eq!(
        advance_and_fire(
            &harness.test_clock,
            UnixNanos::from(*submit_time + INSERT_LATENCY_NS),
        ),
        1,
    );

    let events = settle(&mut harness);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, OrderEventAny::Rejected(rejected)
            if rejected.client_order_id == order.client_order_id())),
        "a deferred submit that cannot be applied must reject its order",
    );

    harness.client.stop().unwrap();
}

/// Replaces the harness `ExecEngine.process` stand-in with one that records every transition the
/// FSM refuses, so an event the cache drops shows up instead of being swallowed.
fn install_fsm_checking_endpoint(
    cache: &Rc<RefCell<Cache>>,
    events: &Rc<RefCell<Vec<OrderEventAny>>>,
) -> Rc<RefCell<Vec<String>>> {
    let refused: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let cache = cache.clone();
    let events = events.clone();
    let sink = refused.clone();
    msgbus::register_order_event_endpoint(
        MessagingSwitchboard::exec_engine_process(),
        TypedIntoHandler::from(move |event: OrderEventAny| {
            if let Err(e) = cache.borrow_mut().update_order(&event) {
                sink.borrow_mut()
                    .push(format!("{}: {e}", order_event_kind(&event)));
            }
            events.borrow_mut().push(event);
        }),
    );
    refused
}

/// Short name for an order event, for assertion messages.
fn order_event_kind(event: &OrderEventAny) -> &'static str {
    match event {
        OrderEventAny::Initialized(_) => "initialized",
        OrderEventAny::Submitted(_) => "submitted",
        OrderEventAny::Accepted(_) => "accepted",
        OrderEventAny::Rejected(_) => "rejected",
        OrderEventAny::Canceled(_) => "canceled",
        OrderEventAny::Expired(_) => "expired",
        OrderEventAny::Triggered(_) => "triggered",
        OrderEventAny::PendingUpdate(_) => "pending_update",
        OrderEventAny::PendingCancel(_) => "pending_cancel",
        OrderEventAny::ModifyRejected(_) => "modify_rejected",
        OrderEventAny::CancelRejected(_) => "cancel_rejected",
        OrderEventAny::Updated(_) => "updated",
        OrderEventAny::Filled(_) => "filled",
        _ => "other",
    }
}

/// The client's own "sent to venue" record and the events the matching engine emits for the same
/// order take one ordered route, so a runner lagged past the inbound leg cannot invert them.
#[rstest]
fn test_inbound_latency_client_record_and_venue_events_share_one_route(
    trader_id: TraderId,
    account_id: AccountId,
    venue: Venue,
    instrument: InstrumentAny,
) {
    const INSERT_LATENCY_NS: u64 = 1_000_000_000; // 1s

    let mut harness = setup_latency_harness(
        trader_id,
        account_id,
        venue,
        &instrument,
        Some(static_latency_model(INSERT_LATENCY_NS, 0, 0)),
    );
    let refused = install_fsm_checking_endpoint(&harness.cache, &harness.events);

    let submit_time = harness.test_clock.borrow().timestamp_ns();
    let order = submit_resting_limit(
        &harness,
        trader_id,
        &instrument,
        "O-ONE-ROUTE-1",
        "100.00",
        submit_time,
    );

    // Deliberately not settled: models a runner that has not drained the execution channel by the
    // time the inbound leg elapses, which is what makes the two routes observable.
    let due = UnixNanos::from(*submit_time + INSERT_LATENCY_NS);
    assert_eq!(advance_and_fire(&harness.test_clock, due), 1);

    let kinds: Vec<&str> = flushed_at_barrier(&harness)
        .iter()
        .filter(|event| event.client_order_id() == order.client_order_id())
        .map(order_event_kind)
        .collect();
    assert_eq!(
        kinds,
        vec!["submitted", "accepted"],
        "the client's own record must reach the engine before the venue's acceptance",
    );
    assert!(
        drain_order_events(&mut harness.rx).is_empty(),
        "with a latency model no order event may take the execution channel: the barrier is \
         the single route",
    );
    assert!(
        refused.borrow().is_empty(),
        "no event may be refused by the FSM: {:?}",
        refused.borrow(),
    );

    let cache = harness.cache.borrow();
    let cached = cache.order(&order.client_order_id()).unwrap();
    assert_eq!(cached.status(), OrderStatus::Accepted);
    assert!(
        cached.ts_submitted().is_some(),
        "the order's OrderSubmitted must have been applied, not dropped by the FSM",
    );
}

/// No command is released while an order-event dispatch is active on the thread.
#[rstest]
fn test_inbound_latency_no_release_during_order_event_dispatch(
    trader_id: TraderId,
    account_id: AccountId,
    venue: Venue,
    instrument: InstrumentAny,
) {
    const INSERT_LATENCY_NS: u64 = 1_000_000_000; // 1s

    let harness = setup_latency_harness(
        trader_id,
        account_id,
        venue,
        &instrument,
        Some(static_latency_model(INSERT_LATENCY_NS, 0, 0)),
    );

    // Stands in for the engine's own `RefCell`, held across the dispatch and marked the same way
    // the real endpoint marks it.
    let engine: Rc<RefCell<u32>> = Rc::new(RefCell::new(0));
    let reentered = Rc::new(Cell::new(false));
    let reentrant_dispatch = Rc::new(Cell::new(false));
    {
        let cache = harness.cache.clone();
        let events = harness.events.clone();
        let reentered = reentered.clone();
        let reentrant_dispatch = reentrant_dispatch.clone();
        let reentry_id = InstrumentId::from("REENTRY-DISPATCH-PERP.SIM");
        msgbus::register_order_event_endpoint(
            MessagingSwitchboard::exec_engine_process(),
            TypedIntoHandler::from(move |event: OrderEventAny| {
                let Ok(mut depth) = engine.try_borrow_mut() else {
                    // In production this is `rc.borrow_mut()`, which panics rather than Err
                    reentrant_dispatch.set(true);
                    return;
                };
                let _dispatching = OrderEventDispatchGuard::enter();
                *depth += 1;

                let _ = cache.borrow_mut().update_order(&event);
                events.borrow_mut().push(event);

                // An actor subscribed to what the engine publishes from inside its borrow publishes
                // market data, reaching the sandbox's own synchronous subscription
                if !reentered.replace(true) {
                    let quote = create_quote_tick(reentry_id, 2000.00, 2010.00);
                    msgbus::publish_quote(
                        format!("data.quotes.{}.{}", reentry_id.venue, reentry_id).into(),
                        &quote,
                    );
                }
            }),
        );
    }

    let submit_time = harness.test_clock.borrow().timestamp_ns();
    let order = submit_resting_limit(
        &harness,
        trader_id,
        &instrument,
        "O-DISPATCH-1",
        "100.00",
        submit_time,
    );

    // The command is due, but nothing has drained it yet
    harness
        .test_clock
        .borrow_mut()
        .set_time(UnixNanos::from(*submit_time + INSERT_LATENCY_NS));
    reentered.set(false);

    // The runner delivers another client's event into the engine, entering an engine frame with no
    // sandbox drain on the stack
    msgbus::send_order_event(
        MessagingSwitchboard::exec_engine_process(),
        OrderEventAny::PendingCancel(OrderPendingCancel::new(
            trader_id,
            order.strategy_id(),
            order.instrument_id(),
            ClientOrderId::from("O-OTHER-CLIENT-1"),
            order.account_id(),
            UUID4::new(),
            submit_time,
            submit_time,
            false,
            order.venue_order_id(),
        )),
    );

    assert!(
        reentered.get(),
        "the engine dispatch must have re-entered the sandbox, or this test proves nothing",
    );
    assert!(
        !reentrant_dispatch.get(),
        "a release reached from inside an order-event dispatch would flush back into the \
         engine's own borrow and panic the runner",
    );

    // Skipping loses nothing: the next tick outside any engine frame releases the command
    harness
        .client
        .process_quote_tick(&create_quote_tick(instrument.id(), 99.00, 99.50))
        .unwrap();
    assert!(
        harness
            .events
            .borrow()
            .iter()
            .any(|event| matches!(event, OrderEventAny::Accepted(accepted)
            if accepted.client_order_id == order.client_order_id())),
        "the command skipped during the dispatch must be released by the next tick",
    );
}

/// A zero-leg command created from a strategy callback during a flush is queued, not applied
/// inline, so it reads the cache only once the command that triggered it has fully settled.
#[rstest]
fn test_inbound_latency_zero_leg_command_from_callback_is_not_applied_inline(
    trader_id: TraderId,
    account_id: AccountId,
    venue: Venue,
    instrument: InstrumentAny,
) {
    const INSERT_LATENCY_NS: u64 = 1_000_000_000; // 1s; the delete leg is zero

    let harness = setup_latency_harness(
        trader_id,
        account_id,
        venue,
        &instrument,
        Some(static_latency_model(INSERT_LATENCY_NS, 0, 0)),
    );
    let LatencyHarness {
        client,
        cache,
        test_clock,
        mut rx,
        events,
    } = harness;
    // Behind an `Rc` so the stand-in can re-enter the client the way a strategy callback does
    let client = Rc::new(client);
    let endpoint = MessagingSwitchboard::exec_engine_process();

    // A market, so the order below emits `Accepted` *and* `Filled` from one command
    client
        .process_quote_tick(&create_quote_tick(instrument.id(), 100.0, 101.0))
        .unwrap();

    let submit_time = test_clock.borrow().timestamp_ns();
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .price(Price::from("102.00")) // marketable against the 101.00 ask
        .quantity(Quantity::from("1.000"))
        .client_order_id("O-CALLBACK-INLINE-1".into())
        .ts_init(submit_time)
        .build();
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    client
        .submit_order(SubmitOrder::from_order(
            &order,
            trader_id,
            Some(client.client_id()),
            None,
            UUID4::new(),
            submit_time,
        ))
        .unwrap();
    events.borrow_mut().clear();

    let reentered = Rc::new(Cell::new(false));
    {
        let cache = cache.clone();
        let events = events.clone();
        let reentered = reentered.clone();
        let strategy_id = order.strategy_id();
        let instrument_id = instrument.id();
        msgbus::register_order_event_endpoint(
            endpoint,
            TypedIntoHandler::from(move |event: OrderEventAny| {
                let _ = cache.borrow_mut().update_order(&event);
                let is_accepted = matches!(event, OrderEventAny::Accepted(_));
                events.borrow_mut().push(event);

                // What a strategy's `on_order_accepted` would do: a zero-delete-leg cancel-all
                // issued while the command's `Filled` has not been applied yet
                if is_accepted && !reentered.replace(true) {
                    client
                        .cancel_all_orders(CancelAllOrders::new(
                            trader_id,
                            Some(client.client_id()),
                            strategy_id,
                            instrument_id,
                            OrderSide::NoOrderSide,
                            UUID4::new(),
                            submit_time,
                            None,
                            None,
                        ))
                        .unwrap();
                }
            }),
        );
    }

    let due = UnixNanos::from(*submit_time + INSERT_LATENCY_NS);
    assert_eq!(advance_and_fire(&test_clock, due), 1);

    for event in std::iter::from_fn(|| rx.try_recv().ok()) {
        if let ExecutionEvent::Order(order_event) = event {
            msgbus::send_order_event(endpoint, order_event);
        }
    }

    assert!(
        reentered.get(),
        "the flush must have re-entered the client, or this test proves nothing",
    );

    let kinds: Vec<&str> = events.borrow().iter().map(order_event_kind).collect();
    assert_eq!(
        kinds,
        vec!["accepted", "filled"],
        "the cancel-all must see the filled order and cancel nothing: {kinds:?}",
    );
    assert_eq!(
        cache
            .borrow()
            .order(&order.client_order_id())
            .unwrap()
            .status(),
        OrderStatus::Filled,
    );
}

/// Stop unwinds several pending commands on one order without emitting an event the FSM refuses.
#[rstest]
fn test_stop_unwinds_stacked_pending_commands_without_refused_events(
    trader_id: TraderId,
    account_id: AccountId,
    venue: Venue,
    instrument: InstrumentAny,
) {
    const LEG_NS: u64 = 1_000_000_000; // 1s on every leg, so all three stay in flight

    let mut harness = setup_latency_harness(
        trader_id,
        account_id,
        venue,
        &instrument,
        Some(static_latency_model(LEG_NS, LEG_NS, LEG_NS)),
    );
    let refused = install_fsm_checking_endpoint(&harness.cache, &harness.events);

    let submit_time = harness.test_clock.borrow().timestamp_ns();
    let order = submit_resting_limit(
        &harness,
        trader_id,
        &instrument,
        "O-STACKED-1",
        "100.00",
        submit_time,
    );
    let _ = settle(&mut harness); // the order is SUBMITTED

    // `Strategy::modify_order` / `cancel_order` establish these before their commands arrive
    mark_pending_update(&harness.cache, &order, trader_id, submit_time);
    harness
        .client
        .modify_order(modify_for(&harness, trader_id, &order, submit_time))
        .unwrap();
    mark_pending_cancel(&harness.cache, &order, trader_id, submit_time);
    harness
        .client
        .cancel_order(cancel_for(&harness, trader_id, &order, submit_time))
        .unwrap();

    // Submit, modify and cancel are all still in flight
    harness.client.stop().unwrap();

    let kinds: Vec<&str> = settle(&mut harness)
        .iter()
        .filter(|event| event.client_order_id() == order.client_order_id())
        .map(order_event_kind)
        .collect();
    assert_eq!(
        kinds,
        vec!["cancel_rejected", "rejected"],
        "the pending state is released once, by the command that established it, before the \
         submit is rejected: {kinds:?}",
    );
    assert!(
        refused.borrow().is_empty(),
        "every event emitted at stop must be applicable by the FSM: {:?}",
        refused.borrow(),
    );

    let cache = harness.cache.borrow();
    let cached = cache.order(&order.client_order_id()).unwrap();
    assert_eq!(cached.status(), OrderStatus::Rejected);
    assert!(!cached.is_pending_update() && !cached.is_pending_cancel());
}

/// A command queued from a callback during a drain must not arm an inbound alert.
#[rstest]
fn test_inbound_latency_command_queued_during_a_drain_does_not_arm_an_alert(
    trader_id: TraderId,
    account_id: AccountId,
    venue: Venue,
    instrument: InstrumentAny,
) {
    const INSERT_LATENCY_NS: u64 = 1_000_000_000; // 1s; the delete leg is zero

    let harness = setup_latency_harness(
        trader_id,
        account_id,
        venue,
        &instrument,
        Some(static_latency_model(INSERT_LATENCY_NS, 0, 0)),
    );
    let LatencyHarness {
        client,
        cache,
        test_clock,
        mut rx,
        events,
    } = harness;
    let client = Rc::new(client);
    let endpoint = MessagingSwitchboard::exec_engine_process();
    let alert_name = format!("SANDBOX-INBOUND-{}", client.client_id());

    let submit_time = test_clock.borrow().timestamp_ns();
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .price(Price::from("100.00"))
        .quantity(Quantity::from("1.000"))
        .client_order_id("O-DRAIN-ARM-1".into())
        .ts_init(submit_time)
        .build();
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    client
        .submit_order(SubmitOrder::from_order(
            &order,
            trader_id,
            Some(client.client_id()),
            None,
            UUID4::new(),
            submit_time,
        ))
        .unwrap();

    for event in std::iter::from_fn(|| rx.try_recv().ok()) {
        if let ExecutionEvent::Order(order_event) = event {
            msgbus::send_order_event(endpoint, order_event);
        }
    }
    events.borrow_mut().clear();

    // Observed from inside the flush, right after the callback's command is queued
    let armed_during_flush: Rc<RefCell<Option<Option<UnixNanos>>>> = Rc::new(RefCell::new(None));
    {
        let cache = cache.clone();
        let test_clock = test_clock.clone();
        let armed_during_flush = armed_during_flush.clone();
        let alert_name = alert_name.clone();
        let strategy_id = order.strategy_id();
        let instrument_id = instrument.id();
        msgbus::register_order_event_endpoint(
            endpoint,
            TypedIntoHandler::from(move |event: OrderEventAny| {
                let _ = cache.borrow_mut().update_order(&event);
                let is_accepted = matches!(event, OrderEventAny::Accepted(_));
                events.borrow_mut().push(event);

                if is_accepted && armed_during_flush.borrow().is_none() {
                    // Hoisted: a `borrow()` in the argument list would still be held when
                    // `cancel_all_orders` takes the clock mutably
                    let ts_now = test_clock.borrow().timestamp_ns();
                    client
                        .cancel_all_orders(CancelAllOrders::new(
                            trader_id,
                            Some(client.client_id()),
                            strategy_id,
                            instrument_id,
                            OrderSide::NoOrderSide,
                            UUID4::new(),
                            ts_now,
                            None,
                            None,
                        ))
                        .unwrap();
                    let armed = test_clock.borrow().next_time_ns(&alert_name);
                    *armed_during_flush.borrow_mut() = Some(armed);
                }
            }),
        );
    }

    let due = UnixNanos::from(*submit_time + INSERT_LATENCY_NS);
    assert_eq!(advance_and_fire(&test_clock, due), 1);

    let armed = armed_during_flush
        .borrow()
        .expect("the flush must have re-entered the client, or this test proves nothing");
    assert_eq!(
        armed, None,
        "a command queued during a drain is applied by the running pass, so arming for it only \
         asks the clock for an alert at a time already past",
    );

    // The command still settles in the same pass
    assert_eq!(
        cache
            .borrow()
            .order(&order.client_order_id())
            .unwrap()
            .status(),
        OrderStatus::Canceled,
    );
    assert_eq!(
        test_clock.borrow().next_time_ns(&alert_name),
        None,
        "nothing is left queued, so no alert may remain armed",
    );
}

/// A command issued from a strategy callback, while an order event is being dispatched, must not
/// send back into the execution engine.
#[rstest]
fn test_inbound_latency_command_from_a_callback_defers_its_records(
    trader_id: TraderId,
    account_id: AccountId,
    venue: Venue,
    instrument: InstrumentAny,
) {
    const INSERT_LATENCY_NS: u64 = 1_000_000_000;

    let harness = setup_latency_harness(
        trader_id,
        account_id,
        venue,
        &instrument,
        Some(static_latency_model(
            INSERT_LATENCY_NS,
            INSERT_LATENCY_NS,
            0,
        )),
    );
    let LatencyHarness {
        client,
        cache,
        test_clock,
        mut rx,
        events,
    } = harness;
    let client = Rc::new(client);
    let endpoint = MessagingSwitchboard::exec_engine_process();

    let engine: Rc<RefCell<u32>> = Rc::new(RefCell::new(0));
    let reentrant = Rc::new(Cell::new(false));
    let fired = Rc::new(Cell::new(false));
    {
        let cache = cache.clone();
        let events = events.clone();
        let reentrant = reentrant.clone();
        let fired = fired.clone();
        let client = client.clone();
        let instrument = instrument.clone();
        msgbus::register_order_event_endpoint(
            endpoint,
            TypedIntoHandler::from(move |event: OrderEventAny| {
                let Ok(mut depth) = engine.try_borrow_mut() else {
                    reentrant.set(true);
                    return;
                };
                let _dispatching = OrderEventDispatchGuard::enter();
                *depth += 1;
                let _ = cache.borrow_mut().update_order(&event);
                let is_accepted = matches!(event, OrderEventAny::Accepted(_));
                events.borrow_mut().push(event);

                // The commonest strategy pattern there is: submit from on_order_accepted
                if is_accepted && !fired.replace(true) {
                    let ts = UnixNanos::from(2_000_000_000);
                    let next = OrderTestBuilder::new(OrderType::Limit)
                        .instrument_id(instrument.id())
                        .side(OrderSide::Buy)
                        .price(Price::from("99.00"))
                        .quantity(Quantity::from("1.000"))
                        .client_order_id("O-FROM-CALLBACK-1".into())
                        .ts_init(ts)
                        .build();
                    cache
                        .borrow_mut()
                        .add_order(next.clone(), None, None, false)
                        .unwrap();
                    client
                        .submit_order(SubmitOrder::from_order(
                            &next,
                            trader_id,
                            Some(client.client_id()),
                            None,
                            UUID4::new(),
                            ts,
                        ))
                        .unwrap();
                }
            }),
        );
    }

    let submit_time = test_clock.borrow().timestamp_ns();
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .price(Price::from("100.00"))
        .quantity(Quantity::from("1.000"))
        .client_order_id("O-HOLE-1".into())
        .ts_init(submit_time)
        .build();
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    client
        .submit_order(SubmitOrder::from_order(
            &order,
            trader_id,
            Some(client.client_id()),
            None,
            UUID4::new(),
            submit_time,
        ))
        .unwrap();

    for e in std::iter::from_fn(|| rx.try_recv().ok()) {
        if let ExecutionEvent::Order(oe) = e {
            msgbus::send_order_event(endpoint, oe);
        }
    }
    events.borrow_mut().clear();

    let due = UnixNanos::from(*submit_time + INSERT_LATENCY_NS);
    assert_eq!(advance_and_fire(&test_clock, due), 1);

    assert!(
        fired.get(),
        "the callback must have run, or this test proves nothing"
    );
    assert!(
        !reentrant.get(),
        "a client command issued from inside an engine dispatch flushed back into the \
         engine's own borrow - in production that is a panic",
    );
}
