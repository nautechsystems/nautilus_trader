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

use std::{cell::RefCell, rc::Rc};

use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    clock::{Clock, TestClock},
    live::set_exec_event_sender,
    messages::{
        ExecutionEvent,
        execution::{SubmitOrder, TradingCommand},
    },
    msgbus::{
        self, MessageBus, MessagingSwitchboard, TypedHandler,
        stubs::get_typed_into_message_saving_handler,
    },
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_data::engine::DataEngine;
use nautilus_execution::{
    client::core::ExecutionClientCore,
    engine::ExecutionEngine,
    models::fee::{FeeModelAny, ProbabilityPriceFeeModel},
};
use nautilus_model::{
    accounts::AccountAny,
    data::{Bar, BarType, Data, InstrumentClose, InstrumentStatus, QuoteTick, TradeTick},
    enums::{
        AccountType, AggressorSide, BookType, InstrumentCloseType, MarketStatusAction, OmsType,
        OrderSide, OrderType,
    },
    events::{AccountState, OrderEventAny, OrderFilled},
    identifiers::{AccountId, ClientId, InstrumentId, PositionId, TradeId, TraderId, Venue},
    instruments::{
        CryptoPerpetual, Instrument, InstrumentAny,
        stubs::{binary_option, crypto_perpetual_ethusdt},
    },
    orders::OrderTestBuilder,
    position::Position,
    types::{Currency, Money, Price, Quantity},
};
use nautilus_sandbox::{SandboxExecutionClient, SandboxExecutionClientConfig};
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
    trader_id: TraderId,
    account_id: AccountId,
    venue: Venue,
) -> SandboxExecutionClientConfig {
    let usd = Currency::USD();
    SandboxExecutionClientConfig {
        trader_id,
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
        frozen_account: false,
        bar_execution: false,
        trade_execution: false,
        reject_stop_orders: true,
        support_gtd_orders: true,
        support_contingent_orders: true,
        use_position_ids: true,
        use_random_ids: false,
        use_reduce_only: true,
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
        config.trader_id,
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
        AggressorSide::Buyer,
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
        config.trader_id,
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

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
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

    submit_market_open_order(
        &client,
        &cache,
        trader_id,
        &instrument,
        &format!("OPEN-{client_order_suffix}"),
        10,
    );

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
    opening_order_id: &str,
    resting_order_id: &str,
    probe_order_id: &str,
    position_id: &str,
) {
    let mut seen_resting_canceled = false;
    let mut seen_probe_rejected = false;
    let mut seeded_position = false;

    for event in std::iter::from_fn(|| harness.rx.try_recv().ok()) {
        if let ExecutionEvent::Order(order_event) = event {
            match order_event {
                OrderEventAny::Filled(fill)
                    if fill.client_order_id.as_str() == opening_order_id =>
                {
                    seed_binary_option_position_from_fill(
                        &harness.context.cache,
                        &harness.instrument,
                        fill,
                        position_id,
                    );
                    seeded_position = true;
                }
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
    assert!(
        seeded_position,
        "expected opening fill so a position can be settled on InstrumentClose"
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

#[rstest]
fn test_config_default() {
    let config = SandboxExecutionClientConfig::default();

    assert_eq!(config.trader_id, TraderId::from("SANDBOX-001"));
    assert_eq!(config.account_id, AccountId::from("SANDBOX-001"));
    assert_eq!(config.venue, Venue::new("SANDBOX"));
    assert!(config.starting_balances.is_empty());
    assert!(config.base_currency.is_none());
    assert_eq!(config.oms_type, OmsType::Netting);
    assert_eq!(config.account_type, AccountType::Margin);
    assert_eq!(config.default_leverage, Decimal::ONE);
    assert_eq!(config.book_type, BookType::L1_MBP);
    assert!(config.fee_model.is_none());
    assert!(!config.frozen_account);
    assert!(config.bar_execution);
    assert!(config.trade_execution);
    assert!(config.reject_stop_orders);
    assert!(config.support_gtd_orders);
    assert!(config.support_contingent_orders);
    assert!(config.use_position_ids);
    assert!(!config.use_random_ids);
    assert!(config.use_reduce_only);
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
    config.fee_model = Some(FeeModelAny::ProbabilityPrice(ProbabilityPriceFeeModel));

    let core = ExecutionClientCore::new(
        config.trader_id,
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
fn test_config_builder(trader_id: TraderId, account_id: AccountId, venue: Venue) {
    let usd = Currency::USD();
    let starting_balances = vec![Money::new(50_000.0, usd)];

    let config = SandboxExecutionClientConfig::builder()
        .trader_id(trader_id)
        .account_id(account_id)
        .venue(venue)
        .starting_balances(starting_balances)
        .build();

    assert_eq!(config.trader_id, trader_id);
    assert_eq!(config.account_id, account_id);
    assert_eq!(config.venue, venue);
    assert_eq!(config.starting_balances.len(), 1);
    assert_eq!(config.starting_balances[0].as_f64(), 50_000.0);
}

#[rstest]
fn test_config_builder_with_overrides(trader_id: TraderId, account_id: AccountId, venue: Venue) {
    let usd = Currency::USD();
    let starting_balances = vec![Money::new(50_000.0, usd)];

    let config = SandboxExecutionClientConfig::builder()
        .trader_id(trader_id)
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
    assert_pending_resolution_transition(
        &mut harness,
        "OPEN-BO-PAPER",
        "REST-BO-PAPER",
        "PROBE-BO-PAPER",
        "P-BO-PAPER",
    );

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
    assert_pending_resolution_transition(
        &mut harness,
        "OPEN-BO-DE",
        "REST-BO-DE",
        "PROBE-BO-DE",
        "P-BO-DE",
    );

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
        config.trader_id,
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
        AggressorSide::Buyer,
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
        config.trader_id,
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
        config.trader_id,
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
        trader_id,
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
        frozen_account: false,
        bar_execution: false,
        trade_execution: false,
        reject_stop_orders: true,
        support_gtd_orders: true,
        support_contingent_orders: true,
        use_position_ids: true,
        use_random_ids: false,
        use_reduce_only: true,
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
