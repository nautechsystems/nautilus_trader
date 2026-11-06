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
    enums::{LiquiditySide, OmsType, OrderSide, OrderStatus, OrderType, PositionSide, TimeInForce},
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, PositionId, StrategyId, TradeId,
        VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny, stubs::crypto_perpetual_ethusdt},
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
};

#[derive(Debug, World)]
#[world(init = Self::new)]
struct StartupWorld {
    config: LiveNodeConfig,
    setup_requested: bool,
    behavior: StartupMassStatusBehavior,
    oms_type: OmsType,
    position_report: Option<PositionStatusReport>,
    delayed_report: Option<OrderStatusReport>,
    delayed_observation: Option<DelayedReportObservation>,
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
        Self {
            config: LiveNodeConfig {
                logging: LoggerConfig {
                    stdout_level: log::LevelFilter::Off,
                    ..Default::default()
                },
                delay_post_stop: Duration::ZERO,
                timeout_disconnection: Duration::from_secs(1),
                ..Default::default()
            },
            setup_requested: false,
            behavior: StartupMassStatusBehavior::Available,
            oms_type: OmsType::Hedging,
            position_report: None,
            delayed_report: None,
            delayed_observation: None,
            closing: None,
            client_state: StartupMassStatusClientState::default(),
            node_state: None,
            actor_starts: Rc::new(Cell::new(0)),
            observation: Rc::new(RefCell::new(None)),
            result: None,
            position_count: None,
        }
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

#[given(
    "the cache has an ETH/USDT perpetual instrument and margin account but no orders or positions"
)]
fn cache_initialized(world: &mut StartupWorld) {
    world.setup_requested = true;
}

#[given("the execution client uses NETTING accounting")]
fn netting_client(world: &mut StartupWorld) {
    world.oms_type = OmsType::Netting;
}

#[given("the venue reports no orders, fills, or positions")]
fn venue_flat(world: &mut StartupWorld) {
    world.behavior = StartupMassStatusBehavior::Available;
    world.position_report = None;
}

#[given(regex = "^the venue reports a (long|short) position of ([0-9.]+) ETH$")]
fn venue_position(world: &mut StartupWorld, side: String, qty: Quantity) {
    let instrument_id = crypto_perpetual_ethusdt().id();
    world.position_report = Some(PositionStatusReport::new(
        AccountId::from("BLOCKING-REPORT-001"),
        instrument_id,
        position_side(&side),
        qty,
        UnixNanos::from(1_000_000),
        UnixNanos::from(2_000_000),
        None,
        Some(PositionId::from("VENUE-POSITION-042")),
        None,
    ));
}

#[given("the position report has no venue position ID")]
fn no_venue_position_id(world: &mut StartupWorld) {
    world.position_report.as_mut().unwrap().venue_position_id = None;
}

#[given(regex = "^its average opening price is ([0-9.]+) USDT$")]
fn opening_price(world: &mut StartupWorld, price: Decimal) {
    world.position_report.as_mut().unwrap().avg_px_open = Some(price);
}

#[given("its average opening price is unavailable")]
fn opening_price_unavailable(world: &mut StartupWorld) {
    world.position_report.as_mut().unwrap().avg_px_open = None;
}

#[given(
    "the execution in the delayed Filled order report is already included in the position snapshot"
)]
fn delayed_execution(world: &mut StartupWorld) {
    let position = world.position_report.as_ref().unwrap();

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
    assert!(world.setup_requested);
    let instrument = crypto_perpetual_ethusdt();
    let account_id = AccountId::from("BLOCKING-REPORT-001");
    let client_id = ClientId::from(StartupMassStatusExecutionClient::CLIENT_ID);

    let mut mass_status = ExecutionMassStatus::new(
        client_id,
        account_id,
        instrument.id().venue,
        UnixNanos::default(),
        None,
    );
    mass_status.add_position_reports(world.position_report.iter().cloned().collect());
    if let Some(closing) = &world.closing {
        closing.add_reports(&mut mass_status);
    }

    *world.client_state.mass_status.lock() = Some(mass_status);
    let factory =
        StartupMassStatusExecutionClientFactory::new(world.client_state.clone(), world.behavior)
            .with_identity(client_id, account_id, instrument.id().venue)
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
    let mut node = StartupNodeGuard(node);
    let node = &mut node.0;
    node.kernel()
        .cache()
        .borrow_mut()
        .add_instrument(InstrumentAny::CryptoPerpetual(instrument))
        .unwrap();
    add_reconciliation_test_account(node);
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

    if let Some(closing) = &world.closing {
        closing.populate_cache(node);
    }

    node.add_actor(StartupActor {
        core: DataActorCore::new(DataActorConfig::default()),
        starts: world.actor_starts.clone(),
    })
    .unwrap();

    let strategy_config = world
        .closing
        .as_ref()
        .map_or_else(StrategyConfig::default, |closing| StrategyConfig {
            strategy_id: Some(closing_strategy_id()),
            oms_type: Some(OmsType::Netting),
            external_order_instrument_ids: (closing.source == ClosingSource::Claimed)
                .then(|| vec![crypto_perpetual_ethusdt().id()]),
            ..Default::default()
        });

    node.add_strategy(StartupStrategy {
        core: StrategyCore::new(strategy_config),
        handle: node.handle(),
        observation: world.observation.clone(),
        stop_on_start: world.delayed_report.is_none(),
    })
    .unwrap();

    if let Some(closing) = &world.closing {
        let expected_claim = (closing.source == ClosingSource::Claimed).then(closing_strategy_id);
        assert_eq!(
            node.kernel()
                .cache()
                .borrow()
                .external_order_claim(&crypto_perpetual_ethusdt().id()),
            expected_claim
        );
    }

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
        assert!(report.ts_last < world.position_report.as_ref().unwrap().ts_last);
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

    world.result = Some(result.map_err(|e| format!("{e:#}")));
    world.node_state = Some(node.state());
    if let Some(closing) = &mut world.closing {
        let cache = node.kernel().cache();
        let cache = cache.borrow();
        closing.order_status = cache.order(&closing.order_id()).map(|order| order.status());
    }

    world.position_count = Some(
        node.kernel()
            .cache()
            .borrow()
            .positions_total_count(None, None, None, None, None),
    );
}

#[when(
    "the live node restarts and receives the delayed Filled order report after entering Running"
)]
async fn restart_before_delayed_report(world: &mut StartupWorld) {
    assert!(world.delayed_report.is_some());
    start_node(world, "start".to_string()).await;
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

#[given("a retained long position holds 1.000 ETH bought at 3000.00 USDT with a 0.60 USDT fee")]
fn retained_long(world: &mut StartupWorld) {
    world.closing = Some(ClosingRecovery {
        source: ClosingSource::Cached,
        window_start: None,
        reports_complete: true,
        fills: Vec::new(),
        order_status: None,
    });
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
    world.closing.as_mut().unwrap().source = match source.as_str() {
        "cached" => ClosingSource::Cached,
        "claimed" => ClosingSource::Claimed,
        _ => unreachable!(),
    };
}

#[given("the bounded history starts after the opening trade")]
fn closing_window(world: &mut StartupWorld) {
    world.config.exec_engine.reconciliation_lookback_mins = Some(1);
    world.closing.as_mut().unwrap().window_start = Some(UnixNanos::from(2_000_000));
}

#[given("the venue reports a filled closing order and no position")]
fn closing_report(world: &mut StartupWorld) {
    world.position_report = None;
    world.behavior = StartupMassStatusBehavior::Available;
}

#[given("the venue reports these closing executions")]
fn closing_executions(world: &mut StartupWorld, step: &cucumber::gherkin::Step) {
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
                AccountId::from("BLOCKING-REPORT-001"),
                crypto_perpetual_ethusdt().id(),
                VenueOrderId::from("V-CLOSE"),
                TradeId::new(format!("T-CLOSE-{i}")),
                OrderSide::Sell,
                Quantity::from(row[0].as_str()),
                Price::from(row[1].as_str()),
                Money::from(format!("{} USDT", row[2]).as_str()),
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
    regex = "^the retained position is flat with realized profit of ([0-9.]+) USDT and total fees of ([0-9.]+) USDT when the strategy starts$"
)]
fn closing_economics(world: &mut StartupWorld, profit: Decimal, fees: Decimal) {
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
    assert_eq!(
        position.realized_pnl,
        Some(Money::from_decimal(profit, Currency::USDT()).unwrap())
    );
    assert_eq!(
        position.commissions(),
        vec![Money::from_decimal(fees, Currency::USDT()).unwrap()]
    );
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

    fn add_reports(&self, mass_status: &mut ExecutionMassStatus) {
        let ts = UnixNanos::from(4_000_000);
        let qty = Quantity::from("1.000");
        let report = OrderStatusReport::new(
            mass_status.account_id,
            crypto_perpetual_ethusdt().id(),
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
        .with_avg_px(Decimal::from(3112));
        mass_status.add_order_reports(vec![report]);
        mass_status.add_fill_reports(self.fills.clone());
        mass_status.set_report_window(self.window_start, self.reports_complete);
    }

    fn populate_cache(&self, node: &LiveNode) {
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        let account_id = AccountId::from("BLOCKING-REPORT-001");
        let client_id = ClientId::from(StartupMassStatusExecutionClient::CLIENT_ID);
        let qty = Quantity::from("1.000");
        let price = Price::from("3000.00");
        let mut opening = OrderTestBuilder::new(OrderType::Market)
            .trader_id(node.kernel().trader_id())
            .client_order_id(ClientOrderId::from("O-OPEN"))
            .strategy_id(closing_strategy_id())
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(qty)
            .build();
        accept_order(&mut opening, account_id, VenueOrderId::from("V-OPEN"));
        let fill = TestOrderEventStubs::filled(
            &opening,
            &instrument,
            Some(TradeId::from("T-OPEN")),
            Some(closing_position_id()),
            Some(price),
            Some(qty),
            Some(LiquiditySide::Maker),
            Some(Money::from("0.60 USDT")),
            Some(UnixNanos::from(1_000_000)),
            Some(account_id),
        );
        opening.apply(fill.clone()).unwrap();
        let position = Position::new(&instrument, fill.into());
        let cache = node.kernel().cache();
        let mut cache = cache.borrow_mut();
        cache
            .add_order(opening, Some(position.id), Some(client_id), false)
            .unwrap();
        cache.add_position(&position, OmsType::Netting).unwrap();

        if self.source == ClosingSource::Cached {
            let mut closing = OrderTestBuilder::new(OrderType::Market)
                .trader_id(node.kernel().trader_id())
                .client_order_id(self.order_id())
                .strategy_id(closing_strategy_id())
                .instrument_id(instrument.id())
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

fn closing_strategy_id() -> StrategyId {
    StrategyId::from("RECOVERY-001")
}

fn closing_position_id() -> PositionId {
    PositionId::new(format!(
        "{}-{}",
        crypto_perpetual_ethusdt().id(),
        closing_strategy_id()
    ))
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

    let report = world.position_report.as_ref().unwrap();
    let expected = format!(
        "Unresolved positions during startup reconciliation for {client_id}: account={}, instrument={}, venue_position_id=Some(\"VENUE-POSITION-042\"), venue_quantity={}: {detail}",
        report.account_id, report.instrument_id, report.signed_decimal_qty,
    );
    assert_eq!(world.result, Some(Err(expected)));
}

#[then(
    regex = "^the strategy observes a (long|short) position of ([0-9.]+) ETH at ([0-9.]+) USDT when it starts$"
)]
fn reconciled_position(world: &mut StartupWorld, side: String, qty: Quantity, price: Decimal) {
    let observation = world.observation.borrow();
    let observation = observation.as_ref().expect("strategy must start");
    let positions = &observation.positions;
    assert_eq!(positions.len(), 1);
    assert_eq!(observation.orders_open, 0);
    let instrument_id = crypto_perpetual_ethusdt().id();

    let position_id = if world.oms_type == OmsType::Netting {
        PositionId::from("ETHUSDT-PERP.BINANCE-EXTERNAL")
    } else {
        PositionId::from("VENUE-POSITION-042")
    };

    assert_eq!(
        positions[0],
        PositionObservation {
            id: position_id,
            account_id: AccountId::from("BLOCKING-REPORT-001"),
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

#[then(regex = "^the portfolio exposes (-?[0-9.]+) ETH when the strategy starts$")]
fn portfolio_exposure(world: &mut StartupWorld, qty: Decimal) {
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
            orders_open: 0,
            positions: Vec::new(),
            net_qty: Decimal::ZERO,
            retained_position: None,
        })
    );
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
    orders_open: usize,
    positions: Vec<PositionObservation>,
    net_qty: Decimal,
    retained_position: Option<Position>,
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
    observation: Rc<RefCell<Option<StartupObservation>>>,
    stop_on_start: bool,
}

impl DataActor for StartupStrategy {
    fn on_start(&mut self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.observation.borrow().is_none(),
            "strategy must start only once"
        );

        let cache = self.cache();
        let orders_open = cache.orders_open_count(None, None, None, None, None);
        let positions = cache
            .positions_open(None, None, None, None, None)
            .iter()
            .map(|position| PositionObservation::new(position, cache.oms_type(&position.id)))
            .collect();

        let instrument_id = crypto_perpetual_ethusdt().id();
        let net_qty = self.portfolio().net_position(&instrument_id);
        *self.observation.borrow_mut() = Some(StartupObservation {
            orders_open,
            positions,
            net_qty,
            retained_position: cache.position(&closing_position_id()),
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
    let config = StartupWorld::new().config;
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
