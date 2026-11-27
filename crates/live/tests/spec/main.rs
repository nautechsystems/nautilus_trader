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

#![allow(
    clippy::needless_pass_by_ref_mut,
    clippy::needless_pass_by_value,
    reason = "Cucumber requires mutable worlds and parses owned step arguments through FromStr"
)]

use std::{
    cell::{Cell, RefCell},
    ops::{Deref, DerefMut},
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
    sync::atomic::Ordering,
    time::Duration,
};

use cucumber::{World, cli::Parser as _, given, then, when, writer::Stats as _};
use nautilus_common::{
    actor::{Component, DataActor, DataActorCore, data_actor::DataActorConfig},
    live::dst,
    logging::logger::LoggerConfig,
    messages::ExecutionReport,
};
use nautilus_core::UnixNanos;
use nautilus_live::{
    builder::LiveNodeBuilder,
    config::LiveNodeConfig,
    node::{LiveNode, LiveNodeHandle, NodeState},
    runner::AsyncRunner,
};
use nautilus_model::{
    accounts::{AccountAny, stubs::cash_account_million_usd},
    enums::{LiquiditySide, OmsType, OrderSide, OrderStatus, OrderType, PositionSide, TimeInForce},
    events::{OrderEventAny, account::stubs::cash_account_state_million_usd},
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, PositionId, StrategyId, Symbol, TradeId,
        VenueOrderId,
    },
    instruments::{
        Instrument, InstrumentAny,
        stubs::{crypto_perpetual_ethusdt, equity_aapl, futures_contract_es},
    },
    orders::{Order, OrderAny, OrderTestBuilder, stubs::TestOrderEventStubs},
    position::Position,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{Currency, Money, Price, Quantity},
};
use nautilus_trading::{
    nautilus_strategy,
    strategy::{Strategy, StrategyConfig, StrategyCore},
};
use rust_decimal::Decimal;

#[path = "../common/startup.rs"]
mod startup;

use startup::{
    StartupActor, StartupMassStatusBehavior, StartupMassStatusClientState,
    StartupMassStatusExecutionClient, StartupMassStatusExecutionClientConfig,
    StartupMassStatusExecutionClientFactory, add_reconciliation_test_account,
    add_startup_margin_account,
};

const AAPL_ACCOUNT_ID: &str = "XNAS-001";
const AAPL_STRATEGY_ID: &str = "AAPL-001";

// Cucumber moves the world by value through nested futures
#[derive(Debug, World)]
#[world(init = Self::new)]
struct StartupWorld(Box<StartupState>);

const _: () = assert!(
    size_of::<StartupWorld>() == size_of::<usize>(),
    "Cucumber world must remain pointer-sized"
);

#[derive(Debug)]
struct StartupState {
    config: LiveNodeConfig,
    instrument: Option<InstrumentAny>,
    excluded_instrument: Option<InstrumentAny>,
    behavior: StartupMassStatusBehavior,
    oms_type: OmsType,
    order_report: Option<OrderStatusReport>,
    position_reports: Vec<PositionStatusReport>,
    delayed_report: Option<OrderStatusReport>,
    delayed_observation: Option<DelayedReportObservation>,
    retained_position: Option<RetainedPosition>,
    claim_external_orders: bool,
    closing: Option<ClosingRecovery>,
    client_state: StartupMassStatusClientState,
    node_state: Option<NodeState>,
    actor_starts: Rc<Cell<usize>>,
    observation: Rc<RefCell<Option<StartupObservation>>>,
    result: Option<Result<(), String>>,
    position_count: Option<usize>,
}

impl StartupWorld {
    fn new() -> Self {
        Self(Box::new(StartupState {
            config: LiveNodeConfig {
                logging: LoggerConfig {
                    stdout_level: log::LevelFilter::Off,
                    ..Default::default()
                },
                delay_post_stop: Duration::ZERO,
                timeout_disconnection: Duration::from_secs(1),
                ..Default::default()
            },
            instrument: None,
            excluded_instrument: None,
            behavior: StartupMassStatusBehavior::Available,
            oms_type: OmsType::Hedging,
            order_report: None,
            position_reports: Vec::new(),
            delayed_report: None,
            delayed_observation: None,
            retained_position: None,
            claim_external_orders: false,
            closing: None,
            client_state: StartupMassStatusClientState::default(),
            node_state: None,
            actor_starts: Rc::new(Cell::new(0)),
            observation: Rc::new(RefCell::new(None)),
            result: None,
            position_count: None,
        }))
    }

    fn instrument(&self) -> &InstrumentAny {
        self.instrument
            .as_ref()
            .expect("scenario requires an instrument")
    }

    fn account_id(&self) -> AccountId {
        AccountId::from(match self.instrument() {
            InstrumentAny::Equity(_) => AAPL_ACCOUNT_ID,
            InstrumentAny::FuturesContract(_) => "GLBX-001",
            _ => "BLOCKING-REPORT-001",
        })
    }

    fn assert_qty_unit(&self, unit: &str) {
        let instrument = self.instrument();
        let expected = match instrument {
            InstrumentAny::FuturesContract(contract) => {
                format!("{} contracts", contract.raw_symbol)
            }
            _ => instrument.base_currency().unwrap().to_string(),
        };

        assert_eq!(unit, expected);
    }
}

impl Deref for StartupWorld {
    type Target = StartupState;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for StartupWorld {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[tokio::test(flavor = "current_thread")]
async fn startup_reconciliation() {
    // Parse Cucumber's tag filter without consuming libtest or nextest arguments
    let cli = cucumber::cli::Opts::parse_from(["spec"]);
    let writer = StartupWorld::cucumber()
        .with_cli::<cucumber::cli::Empty>(cli)
        .max_concurrent_scenarios(1)
        .fail_on_skipped()
        .run(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/spec/features/startup_reconciliation.feature"
        ))
        .await;
    assert!(!writer.execution_has_failed(), "startup scenarios failed");
    assert!(
        writer.passed_steps() > 0,
        "no startup scenarios matched the filter"
    );
}

#[given("startup reconciliation is enabled")]
fn reconciliation_enabled(world: &mut StartupWorld) {
    world.config.exec_engine.reconciliation = true;
}

#[given("startup reconciliation is disabled")]
fn reconciliation_disabled(world: &mut StartupWorld) {
    world.config.exec_engine.reconciliation = false;
}

#[given("position reports are included in reconciliation")]
fn position_reports_included(world: &mut StartupWorld) {
    world.config.exec_engine.filter_position_reports = false;
}

#[given("missing-order generation is enabled")]
fn generation_enabled(world: &mut StartupWorld) {
    world.config.exec_engine.generate_missing_orders = true;
}

#[given("missing-order generation is disabled")]
fn generation_disabled(world: &mut StartupWorld) {
    world.config.exec_engine.generate_missing_orders = false;
}

#[given("the cache has a trading instrument and account but no orders or positions")]
fn cache_initialized(world: &mut StartupWorld) {
    world.instrument = Some(InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt()));
}

#[given("the cache has AAPL equity on NASDAQ and a USD cash account")]
fn aapl_instrument(world: &mut StartupWorld) {
    world.instrument = Some(InstrumentAny::Equity(equity_aapl()));
}

#[given("the cache has the ESZ21 futures contract and a USD margin account")]
fn es_instrument(world: &mut StartupWorld) {
    world.instrument = Some(es_contract("ESZ21", None));
}

#[given("unclaimed external orders are filtered")]
fn filter_unclaimed_orders(world: &mut StartupWorld) {
    world.config.exec_engine.filter_unclaimed_external_orders = true;
}

#[given(
    regex = "^the venue reports an accepted buy limit order ([A-Z0-9-]+) for ([0-9]+) AAPL shares at ([0-9.]+) USD$"
)]
fn aapl_open_order(world: &mut StartupWorld, venue_order_id: String, qty: Quantity, price: Price) {
    let ts = UnixNanos::from(1_000_000);
    world.order_report = Some(
        OrderStatusReport::new(
            AccountId::from(AAPL_ACCOUNT_ID),
            equity_aapl().id(),
            None,
            VenueOrderId::from(venue_order_id.as_str()),
            Some(OrderSide::Buy),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            qty,
            Quantity::zero(0),
            ts,
            ts,
            ts,
            None,
        )
        .with_price(price),
    );
}

#[given("the strategy claims external orders for AAPL on NASDAQ")]
fn claim_aapl_orders(world: &mut StartupWorld) {
    world.claim_external_orders = true;
}

#[given("no strategy claims external orders for AAPL on NASDAQ")]
fn leave_aapl_orders_unclaimed(world: &mut StartupWorld) {
    world.claim_external_orders = false;
}

#[given("the execution client uses NETTING accounting")]
fn netting_client(world: &mut StartupWorld) {
    world.oms_type = OmsType::Netting;
}

#[given("the scripted execution client uses HEDGING accounting")]
fn hedging_client(world: &mut StartupWorld) {
    world.oms_type = OmsType::Hedging;
}

#[given("the venue reports no orders, fills, or positions")]
fn venue_flat(world: &mut StartupWorld) {
    world.behavior = StartupMassStatusBehavior::Available;
    world.position_reports.clear();
}

#[given(regex = "^the venue reports a (long|short) position of ([0-9.]+) (ETH|ESZ21 contracts)$")]
fn venue_position(world: &mut StartupWorld, side: String, qty: Quantity, unit: String) {
    world.assert_qty_unit(&unit);
    let report = position_report(
        world.account_id(),
        world.instrument().id(),
        position_side(&side),
        qty,
        Some(PositionId::from("VENUE-POSITION-042")),
        None,
    );
    world.position_reports.push(report);
}

#[given(
    regex = "^the venue reports a (long|short) position of ([0-9]+) ESZ21 contracts at ([0-9.]+) USD with venue position ID ([A-Z-]+)$"
)]
fn venue_hedge_leg(
    world: &mut StartupWorld,
    side: String,
    qty: Quantity,
    price: Decimal,
    venue_position_id: String,
) {
    let report = position_report(
        world.account_id(),
        world.instrument().id(),
        position_side(&side),
        qty,
        Some(PositionId::from(venue_position_id.as_str())),
        Some(price),
    );
    world.position_reports.push(report);
}

#[given(regex = r"^startup reconciliation is limited to (\S+)$")]
fn reconciliation_scope(world: &mut StartupWorld, instrument_id: InstrumentId) {
    world.config.exec_engine.reconciliation_instrument_ids = Some(vec![instrument_id.to_string()]);
}

#[given(
    regex = "^the venue also reports a long position of ([0-9]+) ESH22 contracts without an opening price$"
)]
fn venue_excluded_position(world: &mut StartupWorld, qty: Quantity) {
    let expiration = "2022-03-18T00:00:00Z".parse().unwrap();
    let instrument = es_contract("ESH22", Some(expiration));
    let report = position_report(
        world.account_id(),
        instrument.id(),
        PositionSide::Long,
        qty,
        None,
        None,
    );
    world.position_reports.push(report);
    world.excluded_instrument = Some(instrument);
}

fn es_contract(symbol: &str, expiration: Option<UnixNanos>) -> InstrumentAny {
    let mut contract = futures_contract_es(None, expiration);
    contract.raw_symbol = Symbol::from(symbol);
    contract.id = InstrumentId::new(contract.raw_symbol, contract.id.venue);
    contract.price_increment = Price::from("0.25");
    contract.multiplier = Quantity::from(50);

    InstrumentAny::FuturesContract(contract)
}

fn position_report(
    account_id: AccountId,
    instrument_id: InstrumentId,
    side: PositionSide,
    qty: Quantity,
    venue_position_id: Option<PositionId>,
    avg_px_open: Option<Decimal>,
) -> PositionStatusReport {
    PositionStatusReport::new(
        account_id,
        instrument_id,
        side,
        qty,
        UnixNanos::from(1_000_000),
        UnixNanos::from(2_000_000),
        None,
        venue_position_id,
        avg_px_open,
    )
}

#[given("the position report has no venue position ID")]
fn no_venue_position_id(world: &mut StartupWorld) {
    world.position_reports.last_mut().unwrap().venue_position_id = None;
}

#[given(regex = "^its average opening price is ([0-9.]+) (USDT|USD)$")]
fn opening_price(world: &mut StartupWorld, price: Decimal, currency: Currency) {
    assert_eq!(currency, world.instrument().quote_currency());
    world.position_reports.last_mut().unwrap().avg_px_open = Some(price);
}

#[given("its average opening price is unavailable")]
fn opening_price_unavailable(world: &mut StartupWorld) {
    world.position_reports.last_mut().unwrap().avg_px_open = None;
}

#[given(
    "the execution in the delayed Filled order report is already included in the position snapshot"
)]
fn delayed_execution(world: &mut StartupWorld) {
    let position = world.position_reports.first().unwrap();
    let side = if position.position_side == PositionSide::Long {
        OrderSide::Buy
    } else {
        OrderSide::Sell
    };

    world.delayed_report = Some(
        OrderStatusReport::new(
            position.account_id,
            position.instrument_id,
            None,
            VenueOrderId::from("V-BEFORE-RESTART"),
            Some(side),
            OrderType::Market,
            TimeInForce::Gtc,
            OrderStatus::Filled,
            position.quantity,
            position.quantity,
            UnixNanos::from(100_000),
            UnixNanos::from(500_000),
            UnixNanos::from(3_000_000),
            None,
        )
        .with_avg_px(position.avg_px_open.unwrap()),
    );
}

#[given("the venue reports an explicit flat position and no orders or fills")]
fn venue_explicit_flat(world: &mut StartupWorld) {
    let instrument_id = crypto_perpetual_ethusdt().id();
    let ts = UnixNanos::from(4_000_000);
    world.position_reports = vec![PositionStatusReport::new(
        AccountId::from("BLOCKING-REPORT-001"),
        instrument_id,
        PositionSide::Flat,
        Quantity::from("0.000"),
        ts,
        ts,
        None,
        None,
        None,
    )];
}

#[given("the venue cannot provide mass status")]
fn mass_status_unavailable(world: &mut StartupWorld) {
    world.behavior = StartupMassStatusBehavior::Unavailable;
}

#[given("the venue returns an error for mass-status requests")]
fn mass_status_error(world: &mut StartupWorld) {
    world.behavior = StartupMassStatusBehavior::Error;
}

#[given("the venue never completes the mass-status request")]
fn mass_status_pending(world: &mut StartupWorld) {
    world.behavior = StartupMassStatusBehavior::Pending;
    world.config.timeout_reconciliation = Duration::from_millis(50);
}

#[when(regex = "^the live node starts using (start|run)$")]
async fn start_node(world: &mut StartupWorld, entrypoint: String) {
    let stop_on_start = world.delayed_report.is_none();
    let mut node = build_startup_node(world, None, stop_on_start);
    let node = &mut node.0;

    let result = dst::time::timeout(Duration::from_secs(5), async {
        match entrypoint.as_str() {
            "start" => node.start().await,
            "run" => node.run().await,
            _ => unreachable!(),
        }
    })
    .await
    .expect("node startup must finish before the test deadline");

    if let Some(report) = &world.delayed_report {
        assert!(
            result.is_ok(),
            "startup must succeed before report delivery"
        );
        assert_eq!(node.state(), NodeState::Running);
        assert!(report.ts_last < world.position_reports.first().unwrap().ts_last);
        assert!(
            node.kernel()
                .cache()
                .borrow()
                .client_order_id(&report.venue_order_id)
                .is_none()
        );

        AsyncRunner::handle_exec_report(ExecutionReport::Order(Box::new(report.clone())));

        world.delayed_observation = {
            let cache = node.kernel().cache();
            let cache = cache.borrow();
            let client_order_id = cache.client_order_id(&report.venue_order_id).unwrap();
            let positions = cache.positions_open(None, None, None, None, None);
            assert_eq!(positions.len(), 1);
            let position = &positions[0];
            Some(DelayedReportObservation {
                order: cache.order(client_order_id).unwrap().clone(),
                position: PositionObservation::new(position, cache.oms_type(&position.id)),
                net_qty: node
                    .kernel()
                    .portfolio()
                    .net_position(&report.instrument_id),
                node_state: node.state(),
            })
        };

        node.stop().await.expect("restarted node must stop cleanly");
    }

    record_startup(world, node, result);
}

fn build_startup_node(
    world: &StartupWorld,
    restored: Option<RetainedClosingCache>,
    stop_on_start: bool,
) -> StartupNodeGuard {
    let instrument = world.instrument();
    let instrument_id = instrument.id();
    let equity = matches!(instrument, InstrumentAny::Equity(_));

    let account_id = world.account_id();

    let client_id = ClientId::from(StartupMassStatusExecutionClient::CLIENT_ID);

    let mut mass_status = ExecutionMassStatus::new(
        client_id,
        account_id,
        instrument_id.venue,
        UnixNanos::default(),
        None,
    );

    mass_status.add_order_reports(world.order_report.iter().cloned().collect());
    mass_status.add_position_reports(world.position_reports.clone());
    if let Some(closing) = &world.closing {
        let qty = world.retained_position.as_ref().unwrap().qty;
        closing.add_reports(&mut mass_status, instrument_id, qty);
    }

    *world.client_state.mass_status.lock() = Some(mass_status);
    let factory =
        StartupMassStatusExecutionClientFactory::new(world.client_state.clone(), world.behavior)
            .with_identity(client_id, account_id, instrument_id.venue)
            .with_oms_type(world.oms_type)
            .with_handles_all_order_venues();
    let node = LiveNodeBuilder::from_config(world.config.clone())
        .unwrap()
        .with_name("StartupSpecification")
        .add_exec_client(
            Some("startup-mass-status".to_string()),
            Box::new(factory),
            Box::new(StartupMassStatusExecutionClientConfig),
        )
        .unwrap()
        .build()
        .unwrap();
    let mut guard = StartupNodeGuard(node);
    let node = &mut guard.0;
    populate_startup_cache(world, node, account_id, restored);

    node.add_actor(StartupActor {
        core: DataActorCore::new(DataActorConfig::default()),
        starts: world.actor_starts.clone(),
    })
    .unwrap();

    let claims_external_orders = world.claim_external_orders
        || world
            .closing
            .as_ref()
            .is_some_and(|closing| closing.source == ClosingSource::Claimed);

    let strategy_id = if world.retained_position.is_some() {
        Some(closing_strategy_id())
    } else {
        equity.then(|| StrategyId::from(AAPL_STRATEGY_ID))
    };

    let strategy_config = StrategyConfig {
        strategy_id,
        oms_type: world
            .retained_position
            .is_some()
            .then_some(OmsType::Netting),
        external_order_instrument_ids: claims_external_orders.then(|| vec![instrument_id]),
        ..Default::default()
    };

    node.add_strategy(StartupStrategy {
        core: StrategyCore::new(strategy_config),
        handle: node.handle(),
        instrument_id,
        observation: world.observation.clone(),
        closing_order_id: world.closing.as_ref().map(ClosingRecovery::order_id),
        stop_on_start,
    })
    .unwrap();

    let expected_claim = strategy_id.filter(|_| claims_external_orders);
    assert_eq!(
        node.kernel()
            .cache()
            .borrow()
            .external_order_claim(&instrument_id),
        expected_claim
    );

    guard
}

fn populate_startup_cache(
    world: &StartupWorld,
    node: &LiveNode,
    account_id: AccountId,
    restored: Option<RetainedClosingCache>,
) {
    let instrument = world.instrument();
    let instrument_id = instrument.id();
    node.kernel()
        .cache()
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();

    if let Some(excluded) = &world.excluded_instrument {
        node.kernel()
            .cache()
            .borrow_mut()
            .add_instrument(excluded.clone())
            .unwrap();
    }

    add_startup_account(node, account_id, instrument);

    assert_eq!(
        node.kernel()
            .cache()
            .borrow()
            .orders_total_count(None, None, None, None, None),
        0
    );
    assert_eq!(
        node.kernel()
            .cache()
            .borrow()
            .positions_total_count(None, None, None, None, None),
        0
    );

    if let Some(restored) = restored {
        restored.populate_cache(node);
        return;
    }

    if let Some(retained) = &world.retained_position {
        populate_retained_position(node, instrument, account_id, retained);
        if let Some(closing) = &world.closing {
            closing.populate_cache(node, instrument_id, account_id, retained.qty);
        }
    }
}

fn add_startup_account(node: &LiveNode, account_id: AccountId, instrument: &InstrumentAny) {
    if matches!(instrument, InstrumentAny::Equity(_)) {
        let mut account_state =
            cash_account_state_million_usd("1000000 USD", "0 USD", "1000000 USD");
        account_state.account_id = account_id;
        let account = cash_account_million_usd(account_state);
        node.kernel()
            .cache()
            .borrow_mut()
            .add_account(AccountAny::Cash(account))
            .unwrap();
    } else if matches!(instrument, InstrumentAny::FuturesContract(_)) {
        add_startup_margin_account(node, account_id, Currency::USD());
    } else {
        add_reconciliation_test_account(node);
    }
}

fn record_startup(world: &mut StartupWorld, node: &LiveNode, result: anyhow::Result<()>) {
    world.result = Some(result.map_err(|e| format!("{e:#}")));
    world.node_state = Some(node.state());
    let cache = node.kernel().cache();
    let cache = cache.borrow();
    if let Some(closing) = &mut world.closing {
        closing.order_status = cache.order(&closing.order_id()).map(|order| order.status());
    }

    world.position_count = Some(cache.positions_total_count(None, None, None, None, None));
}

#[when(
    "the live node restarts and receives the delayed Filled order report after entering Running"
)]
async fn restart_before_delayed_report(world: &mut StartupWorld) {
    assert!(world.delayed_report.is_some());
    start_node(world, "start".to_string()).await;
}

#[when(
    "the live node starts and restarts using start, replaying the same reports against the recovered orders and position"
)]
async fn restart_with_closing_executions(world: &mut StartupWorld) {
    let (restored, profit, fees) = {
        let mut node = build_startup_node(world, None, false);
        let result = start_and_stop_replay_node(&mut node.0).await;
        record_startup(world, &node.0, result);
        startup_succeeds(world);
        closing_order_filled(world);
        closing_executions_once(world);
        single_position(world);
        components_started(world);

        let observation = world.observation.borrow_mut().take().unwrap();
        assert!(observation.positions.is_empty());
        assert_eq!(observation.net_qty, Decimal::ZERO);
        let position = observation.retained_position.unwrap();
        let position_id = closing_position_id(world.instrument().id());
        let restored = RetainedClosingCache::capture(&node.0, position_id);

        (restored, position.realized_pnl, position.commissions())
    };

    world.actor_starts.set(0);
    assert!(
        world
            .client_state
            .mass_status_requested
            .swap(false, Ordering::Relaxed)
    );
    world
        .client_state
        .disconnect_attempted
        .store(false, Ordering::Relaxed);

    let mut node = build_startup_node(world, Some(restored), false);
    let order_id = world.closing.as_ref().unwrap().order_id();
    let restored_order = node
        .0
        .kernel()
        .cache()
        .borrow()
        .order_owned(&order_id)
        .expect("the closing order must be in the cache before replay");
    assert_eq!(restored_order.strategy_id(), closing_strategy_id());
    assert_eq!(restored_order.status(), OrderStatus::Filled);

    let result = start_and_stop_replay_node(&mut node.0).await;
    record_startup(world, &node.0, result);
    startup_succeeds(world);

    let observation = world.observation.borrow();
    let position = observation
        .as_ref()
        .unwrap()
        .retained_position
        .as_ref()
        .unwrap();
    assert_eq!(position.realized_pnl, profit);
    assert_eq!(position.commissions(), fees);
}

async fn start_and_stop_replay_node(node: &mut LiveNode) -> anyhow::Result<()> {
    dst::time::timeout(Duration::from_secs(5), async {
        node.start().await?;
        assert_eq!(node.state(), NodeState::Running);
        node.stop().await
    })
    .await
    .expect("node startup and shutdown must finish before the test deadline")
}

struct RetainedClosingCache {
    orders: Vec<(OrderAny, Option<PositionId>, Option<ClientId>)>,
    position: Position,
    oms_type: OmsType,
}

impl RetainedClosingCache {
    fn capture(node: &LiveNode, position_id: PositionId) -> Self {
        let cache = node.kernel().cache();
        let cache = cache.borrow();
        let position = cache.position_owned(&position_id).unwrap();
        assert_eq!(position.side, PositionSide::Flat);
        let oms_type = cache.oms_type(&position_id).unwrap();

        let orders = cache
            .orders(None, None, None, None, None)
            .into_iter()
            .map(|order| {
                let id = order.client_order_id();

                (
                    order.clone(),
                    cache.position_id(&id).copied(),
                    cache.client_id(&id).copied(),
                )
            })
            .collect();

        Self {
            orders,
            position,
            oms_type,
        }
    }

    fn populate_cache(self, node: &LiveNode) {
        let cache = node.kernel().cache();
        let mut cache = cache.borrow_mut();
        for (order, position_id, client_id) in self.orders {
            cache
                .add_order(order, position_id, client_id, false)
                .unwrap();
        }

        cache.add_position(&self.position, self.oms_type).unwrap();
    }
}

#[then(regex = "^the delayed order is FILLED for ([0-9.]+) ETH at ([0-9.]+) USDT$")]
fn delayed_order_filled(world: &mut StartupWorld, qty: Quantity, price: Decimal) {
    let observation = world.delayed_observation.as_ref().unwrap();
    let report = world.delayed_report.as_ref().unwrap();
    let order = &observation.order;

    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.quantity(), qty);
    assert_eq!(order.filled_qty(), qty);
    assert_eq!(order.avg_px(), Some(price));
    assert_eq!(order.account_id(), Some(report.account_id));
    assert_eq!(order.instrument_id(), report.instrument_id);
    assert_eq!(order.venue_order_id(), Some(report.venue_order_id));
    assert_eq!(Some(order.order_side()), report.order_side);
    assert_eq!(order.trade_ids().len(), 1);
    assert_eq!(order.ts_last(), report.ts_last);
}

#[then("the recovered position and portfolio exposure stay unchanged after the delayed report")]
fn delayed_exposure_unchanged(world: &mut StartupWorld) {
    let before = world.observation.borrow();
    let before = before.as_ref().unwrap();
    let after = world.delayed_observation.as_ref().unwrap();

    assert_eq!(after.node_state, NodeState::Running);
    assert_eq!(before.positions.len(), 1);
    assert_eq!(after.position, before.positions[0]);
    assert_eq!(after.net_qty, before.net_qty);
}

#[derive(Debug)]
struct DelayedReportObservation {
    order: OrderAny,
    position: PositionObservation,
    net_qty: Decimal,
    node_state: NodeState,
}

#[given(
    regex = "^a retained long position holds ([0-9.]+) (ETH|ESZ21 contracts) bought at ([0-9.]+) (USDT|USD) with a ([0-9.]+ (?:USDT|USD)) fee$"
)]
fn retained_long(
    world: &mut StartupWorld,
    qty: Quantity,
    unit: String,
    price: Price,
    price_currency: Currency,
    fee: Money,
) {
    world.assert_qty_unit(&unit);
    assert_eq!(price_currency, world.instrument().quote_currency());
    assert_eq!(fee.currency, world.instrument().settlement_currency());
    world.retained_position = Some(RetainedPosition { qty, price, fee });
}

#[derive(Debug)]
struct RetainedPosition {
    qty: Quantity,
    price: Price,
    fee: Money,
}

#[given("the strategy claims external orders for the retained position's instrument")]
fn claim_retained_instrument(world: &mut StartupWorld) {
    assert!(world.retained_position.is_some());
    world.claim_external_orders = true;
}

#[given("the client cannot provide complete position coverage")]
fn position_coverage_unavailable(world: &mut StartupWorld) {
    world
        .client_state
        .bulk_position_coverage_unavailable
        .store(true, Ordering::Relaxed);
}

#[given(regex = "^the closing order is (cached|claimed)$")]
fn closing_source(world: &mut StartupWorld, source: String) {
    world.closing = Some(ClosingRecovery {
        source: match source.as_str() {
            "cached" => ClosingSource::Cached,
            "claimed" => ClosingSource::Claimed,
            _ => unreachable!(),
        },
        window_start: None,
        reports_complete: true,
        avg_px: None,
        fills: Vec::new(),
        order_status: None,
    });
}

#[given("the bounded history starts after the opening trade")]
fn closing_window(world: &mut StartupWorld) {
    world.config.exec_engine.reconciliation_lookback_mins = Some(1);
    world.closing.as_mut().unwrap().window_start = Some(UnixNanos::from(2_000_000));
}

#[given("the venue reports a filled closing order and no position")]
fn closing_report(world: &mut StartupWorld) {
    closing_report_at(world, Decimal::from(3112), Currency::USDT());
}

#[given(
    regex = "^the venue reports a closing order filled at an average of ([0-9.]+) (USDT|USD) and no position$"
)]
fn closing_report_at(world: &mut StartupWorld, avg_px: Decimal, currency: Currency) {
    assert_eq!(currency, world.instrument().quote_currency());
    world.closing.as_mut().unwrap().avg_px = Some(avg_px);
    world.position_reports.clear();
    world.behavior = StartupMassStatusBehavior::Available;
}

#[given("the venue reports these closing executions")]
fn closing_executions(world: &mut StartupWorld, step: &cucumber::gherkin::Step) {
    let account_id = world.account_id();
    let instrument_id = world.instrument().id();
    let currency = world.instrument().settlement_currency();
    let closing = world.closing.as_mut().unwrap();
    let table = step.table.as_ref().expect("closing execution table");
    assert_eq!(table.rows[0], ["quantity", "price", "fee"]);
    closing.fills = table
        .rows
        .iter()
        .skip(1)
        .enumerate()
        .map(|(i, row)| {
            assert_eq!(row.len(), 3);
            let ts = UnixNanos::from(3_000_000 + i as u64);

            FillReport::new(
                account_id,
                instrument_id,
                VenueOrderId::from("V-CLOSE"),
                TradeId::new(format!("T-CLOSE-{i}")),
                OrderSide::Sell,
                Quantity::from(row[0].as_str()),
                Price::from(row[1].as_str()),
                Money::from(format!("{} {currency}", row[2]).as_str()),
                LiquiditySide::Taker,
                (closing.source == ClosingSource::Cached).then(|| closing.order_id()),
                None,
                ts,
                ts,
                None,
            )
        })
        .collect();
}

#[given("the venue marks the history incomplete")]
fn closing_history_incomplete(world: &mut StartupWorld) {
    world.closing.as_mut().unwrap().reports_complete = false;
}

#[given("the venue supplies no closing executions")]
fn closing_executions_unavailable(world: &mut StartupWorld) {
    world.closing.as_mut().unwrap().fills.clear();
}

#[then("the closing order is FILLED")]
fn closing_order_filled(world: &mut StartupWorld) {
    assert_eq!(
        world.closing.as_ref().unwrap().order_status,
        Some(OrderStatus::Filled)
    );
}

#[then(
    "the closing executions are recorded once on the order and position when the strategy starts"
)]
fn closing_executions_once(world: &mut StartupWorld) {
    let closing = world.closing.as_ref().unwrap();
    let observation = world.observation.borrow();
    let observation = observation.as_ref().unwrap();
    let order = observation.closing_order.as_ref().unwrap();
    let position = observation.retained_position.as_ref().unwrap();
    assert_eq!(order.strategy_id(), closing_strategy_id());

    let order_fills: Vec<_> = order
        .events()
        .into_iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => {
                Some((fill.trade_id, fill.last_qty, fill.last_px, fill.commission))
            }
            _ => None,
        })
        .collect();

    let expected_order_fills: Vec<_> = closing
        .fills
        .iter()
        .map(|fill| {
            (
                fill.trade_id,
                fill.last_qty,
                fill.last_px,
                Some(fill.commission),
            )
        })
        .collect();

    assert_eq!(order_fills, expected_order_fills);

    let opening_trade_id = TradeId::from("T-OPEN");
    let expected_position_trades: Vec<_> = std::iter::once(opening_trade_id)
        .chain(closing.fills.iter().map(|fill| fill.trade_id))
        .collect();
    let position_trades: Vec<_> = position.events.iter().map(|event| event.trade_id).collect();
    assert_eq!(position_trades, expected_position_trades);
}

#[then(
    regex = "^the retained position is flat with realized profit of ([0-9.]+ (?:USDT|USD)) and total fees of ([0-9.]+ (?:USDT|USD)) when the strategy starts$"
)]
fn closing_economics(world: &mut StartupWorld, profit: Money, fees: Money) {
    let closing = world.closing.as_ref().unwrap();
    let observation = world.observation.borrow();
    let position = observation
        .as_ref()
        .unwrap()
        .retained_position
        .as_ref()
        .unwrap();
    assert_eq!(position.side, PositionSide::Flat);
    assert_eq!(position.quantity.as_decimal(), Decimal::ZERO);
    assert_eq!(position.realized_pnl, Some(profit));
    assert_eq!(position.commissions(), vec![fees]);
    assert_eq!(position.closing_order_id, Some(closing.order_id()));
    let expected_trades = std::iter::once(TradeId::from("T-OPEN"))
        .chain(closing.fills.iter().map(|fill| fill.trade_id))
        .collect();
    assert_eq!(position.trade_ids, expected_trades);
}

#[then(
    regex = "^the retained position remains long ([0-9.]+) ETH with realized profit of (-?[0-9.]+) USDT and total fees of ([0-9.]+) USDT when the strategy starts$"
)]
fn retained_economics(world: &mut StartupWorld, qty: Quantity, profit: Decimal, fees: Decimal) {
    let observation = world.observation.borrow();
    let position = observation
        .as_ref()
        .unwrap()
        .retained_position
        .as_ref()
        .unwrap();
    assert_eq!(position.side, PositionSide::Long);
    assert_eq!(position.quantity, qty);
    assert_eq!(
        position.realized_pnl,
        Some(Money::from_decimal(profit, Currency::USDT()).unwrap())
    );
    assert_eq!(
        position.commissions(),
        vec![Money::from_decimal(fees, Currency::USDT()).unwrap()]
    );
    assert_eq!(position.closing_order_id, None);
    assert_eq!(
        position.trade_ids,
        [TradeId::from("T-OPEN")].into_iter().collect()
    );
}

#[then(
    regex = "^the retained position is (flat|long) with ([0-9.]+) ETH when the strategy starts$"
)]
fn retained_position_at_start(world: &mut StartupWorld, side: String, qty: Quantity) {
    let observation = world.observation.borrow();
    let observation = observation.as_ref().unwrap();
    let position = observation.retained_position.as_ref().unwrap();

    let expected_side = match side.as_str() {
        "flat" => PositionSide::Flat,
        "long" => PositionSide::Long,
        _ => unreachable!(),
    };

    let instrument_id = world.instrument().id();
    assert_eq!(position.id, closing_position_id(instrument_id));
    assert_eq!(position.account_id, world.account_id());
    assert_eq!(position.instrument_id, instrument_id);
    assert_eq!(position.strategy_id, closing_strategy_id());
    assert_eq!(position.side, expected_side);
    assert_eq!(position.quantity, qty);
    assert_eq!(position.quantity.precision, qty.precision);
    assert_eq!(position.avg_px_open, Decimal::from(3000));
    assert!(observation.orders_open.is_empty());
    assert_eq!(
        observation.positions.len(),
        usize::from(expected_side != PositionSide::Flat)
    );
}

#[then("exactly one position is cached")]
fn single_position(world: &mut StartupWorld) {
    assert_eq!(world.position_count, Some(1));
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum ClosingSource {
    Cached,
    Claimed,
}

#[derive(Debug)]
struct ClosingRecovery {
    source: ClosingSource,
    window_start: Option<UnixNanos>,
    reports_complete: bool,
    avg_px: Option<Decimal>,
    fills: Vec<FillReport>,
    order_status: Option<OrderStatus>,
}

impl ClosingRecovery {
    fn order_id(&self) -> ClientOrderId {
        ClientOrderId::from(match self.source {
            ClosingSource::Cached => "O-CLOSE",
            ClosingSource::Claimed => "V-CLOSE",
        })
    }

    fn add_reports(
        &self,
        mass_status: &mut ExecutionMassStatus,
        instrument_id: InstrumentId,
        qty: Quantity,
    ) {
        let ts = UnixNanos::from(4_000_000);
        let report = OrderStatusReport::new(
            mass_status.account_id,
            instrument_id,
            (self.source == ClosingSource::Cached).then(|| self.order_id()),
            VenueOrderId::from("V-CLOSE"),
            Some(OrderSide::Sell),
            OrderType::Market,
            TimeInForce::Gtc,
            OrderStatus::Filled,
            qty,
            qty,
            UnixNanos::from(2_500_000),
            ts,
            ts,
            None,
        )
        .with_avg_px(self.avg_px.unwrap());
        mass_status.add_order_reports(vec![report]);
        mass_status.add_fill_reports(self.fills.clone());
        mass_status.set_report_window(self.window_start, self.reports_complete);
    }

    fn populate_cache(
        &self,
        node: &LiveNode,
        instrument_id: InstrumentId,
        account_id: AccountId,
        qty: Quantity,
    ) {
        let client_id = ClientId::from(StartupMassStatusExecutionClient::CLIENT_ID);
        let cache = node.kernel().cache();
        let mut cache = cache.borrow_mut();

        if self.source == ClosingSource::Cached {
            let mut closing = OrderTestBuilder::new(OrderType::Market)
                .trader_id(node.kernel().trader_id())
                .client_order_id(self.order_id())
                .strategy_id(closing_strategy_id())
                .instrument_id(instrument_id)
                .side(OrderSide::Sell)
                .quantity(qty)
                .build();
            accept_order(&mut closing, account_id, VenueOrderId::from("V-CLOSE"));
            cache
                .add_order(closing, None, Some(client_id), false)
                .unwrap();
        }
    }
}

fn populate_retained_position(
    node: &LiveNode,
    instrument: &InstrumentAny,
    account_id: AccountId,
    retained: &RetainedPosition,
) {
    let client_id = ClientId::from(StartupMassStatusExecutionClient::CLIENT_ID);
    let mut opening = OrderTestBuilder::new(OrderType::Market)
        .trader_id(node.kernel().trader_id())
        .client_order_id(ClientOrderId::from("O-OPEN"))
        .strategy_id(closing_strategy_id())
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .quantity(retained.qty)
        .build();
    accept_order(&mut opening, account_id, VenueOrderId::from("V-OPEN"));
    let fill = TestOrderEventStubs::filled(
        &opening,
        instrument,
        Some(TradeId::from("T-OPEN")),
        Some(closing_position_id(instrument.id())),
        Some(retained.price),
        Some(retained.qty),
        Some(LiquiditySide::Maker),
        Some(retained.fee),
        Some(UnixNanos::from(1_000_000)),
        Some(account_id),
    );
    opening.apply(fill.clone()).unwrap();
    let position = Position::new(instrument, fill.into());
    let cache = node.kernel().cache();
    let mut cache = cache.borrow_mut();
    cache
        .add_order(opening, Some(position.id), Some(client_id), false)
        .unwrap();
    cache.add_position(&position, OmsType::Netting).unwrap();
}

fn closing_strategy_id() -> StrategyId {
    StrategyId::from("RECOVERY-001")
}

fn closing_position_id(instrument_id: InstrumentId) -> PositionId {
    let strategy_id = closing_strategy_id();
    PositionId::new(format!("{instrument_id}-{strategy_id}"))
}

fn accept_order(order: &mut OrderAny, account_id: AccountId, venue_order_id: VenueOrderId) {
    order
        .apply(TestOrderEventStubs::submitted(order, account_id))
        .unwrap();
    order
        .apply(TestOrderEventStubs::accepted(
            order,
            account_id,
            venue_order_id,
        ))
        .unwrap();
}

#[then("startup succeeds")]
fn startup_succeeds(world: &mut StartupWorld) {
    assert_eq!(world.result, Some(Ok(())));
    disconnected(world);
}

#[then(
    regex = "^startup fails because (the opening price is missing|missing-order generation is disabled|the mass-status request failed|the reconciliation deadline expires)$"
)]
fn startup_fails(world: &mut StartupWorld, reason: String) {
    let client_id = StartupMassStatusExecutionClient::CLIENT_ID;

    let detail = match reason.as_str() {
        "the opening price is missing" => "missing avg_px_open for position recovery",
        "missing-order generation is disabled" => "generate_missing_orders is disabled",
        "the mass-status request failed" => {
            let expected =
                format!("Failed to get mass status from {client_id}: mass status failed");
            assert_eq!(world.result, Some(Err(expected)));
            return;
        }
        "the reconciliation deadline expires" => {
            let expected = format!(
                "Startup reconciliation timeout reached while requesting mass status from {client_id}"
            );
            assert_eq!(world.result, Some(Err(expected)));
            return;
        }
        _ => panic!("unknown startup failure: {reason}"),
    };

    let report = world.position_reports.first().unwrap();
    let expected = format!(
        "Unresolved positions during startup reconciliation for {client_id}: account={}, instrument={}, venue_position_id=Some(\"VENUE-POSITION-042\"), venue_quantity={}: {detail}",
        report.account_id, report.instrument_id, report.signed_decimal_qty,
    );
    assert_eq!(world.result, Some(Err(expected)));
}

#[then(
    regex = "^the strategy observes a (long|short) position of ([0-9.]+) (ETH|ESZ21 contracts) at ([0-9.]+) (USDT|USD) when it starts$"
)]
fn reconciled_position(
    world: &mut StartupWorld,
    side: String,
    qty: Quantity,
    unit: String,
    price: Decimal,
    price_currency: Currency,
) {
    world.assert_qty_unit(&unit);
    assert_eq!(price_currency, world.instrument().quote_currency());
    let observation = world.observation.borrow();
    let observation = observation.as_ref().expect("strategy must start");
    let positions = &observation.positions;
    assert_eq!(positions.len(), 1);
    assert!(observation.orders_open.is_empty());
    let instrument_id = world.instrument().id();
    let position_id = if world.oms_type == OmsType::Netting {
        PositionId::new(format!("{instrument_id}-EXTERNAL"))
    } else {
        PositionId::from("VENUE-POSITION-042")
    };

    assert_eq!(
        positions[0],
        PositionObservation {
            id: position_id,
            account_id: world.account_id(),
            instrument_id,
            strategy_id: StrategyId::from("EXTERNAL"),
            oms_type: Some(world.oms_type),
            side: position_side(&side),
            qty,
            avg_px_open: price,
        }
    );
    assert_eq!(positions[0].qty.precision, qty.precision);
    assert_eq!(world.position_count, Some(1));
}

#[then("the strategy observes both reported hedge legs when it starts")]
fn reconciled_hedge_legs(world: &mut StartupWorld) {
    let observation = world.observation.borrow();
    let observation = observation.as_ref().expect("strategy must start");
    assert_eq!(observation.positions.len(), 2);
    assert_eq!(world.position_count, Some(2));

    for report in &world.position_reports {
        let id = report.venue_position_id.unwrap();
        let position = observation
            .positions
            .iter()
            .find(|position| position.id == id)
            .expect("reported hedge leg must be recovered");
        assert_eq!(
            *position,
            PositionObservation {
                id,
                account_id: report.account_id,
                instrument_id: report.instrument_id,
                strategy_id: StrategyId::external(),
                oms_type: Some(OmsType::Hedging),
                side: report.position_side,
                qty: report.quantity,
                avg_px_open: report.avg_px_open.unwrap(),
            }
        );
        assert_eq!(position.qty.precision, report.quantity.precision);
    }
}

#[then(
    regex = "^the portfolio exposes (-?[0-9.]+) (ETH|ESZ21 contracts) when the strategy starts$"
)]
fn portfolio_exposure(world: &mut StartupWorld, qty: Decimal, unit: String) {
    world.assert_qty_unit(&unit);
    assert_eq!(world.observation.borrow().as_ref().unwrap().net_qty, qty);
}

#[then("the actor and strategy each start once")]
fn components_started(world: &mut StartupWorld) {
    assert_eq!(world.actor_starts.get(), 1);
    assert!(world.observation.borrow().is_some());
}

#[then("no actor or strategy starts")]
fn components_not_started(world: &mut StartupWorld) {
    assert_eq!(world.actor_starts.get(), 0);
    assert_eq!(*world.observation.borrow(), None);
}

#[then("no position is cached")]
fn no_position(world: &mut StartupWorld) {
    assert_eq!(world.position_count, Some(0));
}

#[then("the strategy observes no open orders or positions when it starts")]
fn flat_startup(world: &mut StartupWorld) {
    assert_eq!(
        *world.observation.borrow(),
        Some(StartupObservation {
            orders_open: Vec::new(),
            orders_total: 0,
            positions: Vec::new(),
            net_qty: Decimal::ZERO,
            retained_position: None,
            closing_order: None,
        })
    );
}

#[then(
    regex = "^the strategy owns one open AAPL buy limit order ([A-Z0-9-]+) for ([0-9]+) shares at ([0-9.]+) USD when it starts$"
)]
fn claimed_aapl_order(
    world: &mut StartupWorld,
    venue_order_id: String,
    qty: Quantity,
    price: Price,
) {
    let observation = world.observation.borrow();
    let observation = observation.as_ref().expect("strategy must start");
    assert_eq!(observation.orders_total, 1);
    assert_eq!(observation.orders_open.len(), 1);
    let order = &observation.orders_open[0];
    assert_eq!(order.strategy_id(), StrategyId::from(AAPL_STRATEGY_ID));
    assert_eq!(order.instrument_id(), equity_aapl().id());
    assert_eq!(order.account_id(), Some(AccountId::from(AAPL_ACCOUNT_ID)));
    assert_eq!(
        order.client_order_id(),
        ClientOrderId::from(venue_order_id.as_str())
    );
    assert_eq!(
        order.venue_order_id(),
        Some(VenueOrderId::from(venue_order_id.as_str()))
    );
    assert_eq!(order.status(), OrderStatus::Accepted);
    assert_eq!(order.order_side(), OrderSide::Buy);
    assert_eq!(order.order_type(), OrderType::Limit);
    assert_eq!(order.time_in_force(), TimeInForce::Gtc);
    assert_eq!(order.price(), Some(price));
    assert_eq!(order.quantity(), qty);
    assert_eq!(order.filled_qty(), Quantity::zero(0));
}

#[then("the strategy observes an empty order cache when it starts")]
fn empty_order_cache_at_start(world: &mut StartupWorld) {
    let observation = world.observation.borrow();
    let observation = observation.as_ref().expect("strategy must start");
    assert_eq!(observation.orders_total, 0);
    assert!(observation.orders_open.is_empty());
}

#[then("the venue receives a startup mass-status request")]
fn mass_status_requested(world: &mut StartupWorld) {
    assert!(
        world
            .client_state
            .mass_status_requested
            .load(Ordering::Relaxed)
    );
}

#[then("the venue receives no startup mass-status request")]
fn mass_status_not_requested(world: &mut StartupWorld) {
    assert!(
        !world
            .client_state
            .mass_status_requested
            .load(Ordering::Relaxed)
    );
}

#[then("the execution client is disconnected")]
fn disconnected(world: &mut StartupWorld) {
    assert_eq!(world.node_state, Some(NodeState::Stopped));
    assert!(
        world
            .client_state
            .disconnect_attempted
            .load(Ordering::Relaxed)
    );
    assert!(!world.client_state.connected.load(Ordering::Relaxed));
}

fn position_side(side: &str) -> PositionSide {
    match side {
        "long" => PositionSide::Long,
        "short" => PositionSide::Short,
        _ => panic!("unknown position side: {side}"),
    }
}

#[derive(Debug, PartialEq)]
struct StartupObservation {
    orders_open: Vec<OrderAny>,
    orders_total: usize,
    positions: Vec<PositionObservation>,
    net_qty: Decimal,
    retained_position: Option<Position>,
    closing_order: Option<OrderAny>,
}

#[derive(Debug, PartialEq)]
struct PositionObservation {
    id: PositionId,
    account_id: AccountId,
    instrument_id: InstrumentId,
    strategy_id: StrategyId,
    oms_type: Option<OmsType>,
    side: PositionSide,
    qty: Quantity,
    avg_px_open: Decimal,
}

impl PositionObservation {
    fn new(position: &Position, oms_type: Option<OmsType>) -> Self {
        Self {
            id: position.id,
            account_id: position.account_id,
            instrument_id: position.instrument_id,
            strategy_id: position.strategy_id,
            oms_type,
            side: position.side,
            qty: position.quantity,
            avg_px_open: position.avg_px_open,
        }
    }
}

#[derive(Debug)]
struct StartupStrategy {
    core: StrategyCore,
    handle: LiveNodeHandle,
    instrument_id: InstrumentId,
    observation: Rc<RefCell<Option<StartupObservation>>>,
    closing_order_id: Option<ClientOrderId>,
    stop_on_start: bool,
}

impl DataActor for StartupStrategy {
    fn on_start(&mut self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.observation.borrow().is_none(),
            "strategy must start only once"
        );

        let cache = self.cache();
        let orders_open = cache.orders_open(None, None, None, None, None);
        let orders_total = cache.orders_total_count(None, None, None, None, None);
        let positions = cache
            .positions_open(None, None, None, None, None)
            .iter()
            .map(|position| PositionObservation::new(position, cache.oms_type(&position.id)))
            .collect();

        let net_qty = self.portfolio().net_position(&self.instrument_id);
        let closing_order = self.closing_order_id.and_then(|id| cache.order(&id));
        *self.observation.borrow_mut() = Some(StartupObservation {
            orders_open,
            orders_total,
            positions,
            net_qty,
            retained_position: cache.position(&closing_position_id(self.instrument_id)),
            closing_order,
        });

        if self.stop_on_start {
            self.handle.stop();
        }

        Ok(())
    }
}

nautilus_strategy!(StartupStrategy, {
    fn external_order_instrument_ids(&self) -> Option<Vec<InstrumentId>> {
        self.core.config.external_order_instrument_ids.clone()
    }
});

struct StartupNodeGuard(LiveNode);

impl Drop for StartupNodeGuard {
    fn drop(&mut self) {
        self.0.dispose();
    }
}

#[tokio::test(flavor = "current_thread")]
async fn node_disposes_after_panicking_step() {
    let config = StartupWorld::new().0.config;
    let node = LiveNodeBuilder::from_config(config)
        .unwrap()
        .build()
        .unwrap();
    let mut node = StartupNodeGuard(node);
    node.0
        .add_actor(StartupActor {
            core: DataActorCore::new(DataActorConfig::default()),
            starts: Rc::new(Cell::new(0)),
        })
        .unwrap();

    let trader = node.0.kernel().trader().clone();
    assert_eq!(trader.borrow().component_count(), 1);

    let result = catch_unwind(AssertUnwindSafe(move || {
        let _node = node;
        panic!("scenario step failed");
    }));

    assert_eq!(
        result.unwrap_err().downcast_ref::<&str>(),
        Some(&"scenario step failed")
    );
    assert!(trader.borrow().is_disposed());
    assert_eq!(trader.borrow().component_count(), 0);
}
