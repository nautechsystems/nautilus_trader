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
};
use nautilus_core::UnixNanos;
use nautilus_live::{
    builder::LiveNodeBuilder,
    config::LiveNodeConfig,
    node::{LiveNode, LiveNodeHandle, NodeState},
};
use nautilus_model::{
    enums::PositionSide,
    identifiers::{AccountId, ClientId, InstrumentId, PositionId, StrategyId},
    instruments::{Instrument, InstrumentAny, stubs::crypto_perpetual_ethusdt},
    reports::{ExecutionMassStatus, PositionStatusReport},
    types::Quantity,
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
    position_report: Option<PositionStatusReport>,
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
            position_report: None,
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

#[given(regex = "^its average opening price is ([0-9.]+) USDT$")]
fn opening_price(world: &mut StartupWorld, price: Decimal) {
    world.position_report.as_mut().unwrap().avg_px_open = Some(price);
}

#[given("its average opening price is unavailable")]
fn opening_price_unavailable(world: &mut StartupWorld) {
    world.position_report.as_mut().unwrap().avg_px_open = None;
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
    *world.client_state.mass_status.lock() = Some(mass_status);
    let factory =
        StartupMassStatusExecutionClientFactory::new(world.client_state.clone(), world.behavior)
            .with_identity(client_id, account_id, instrument.id().venue)
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
    node.add_actor(StartupActor {
        core: DataActorCore::new(DataActorConfig::default()),
        starts: world.actor_starts.clone(),
    })
    .unwrap();

    node.add_strategy(StartupStrategy {
        core: StrategyCore::new(StrategyConfig::default()),
        handle: node.handle(),
        observation: world.observation.clone(),
    })
    .unwrap();

    let result = dst::time::timeout(Duration::from_secs(5), async {
        match entrypoint.as_str() {
            "start" => node.start().await,
            "run" => node.run().await,
            _ => unreachable!(),
        }
    })
    .await
    .expect("node startup must finish before the test deadline");

    world.result = Some(result.map_err(|e| format!("{e:#}")));
    world.node_state = Some(node.state());
    world.position_count = Some(
        node.kernel()
            .cache()
            .borrow()
            .positions_total_count(None, None, None, None, None),
    );
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
    let positions = &observation.as_ref().expect("strategy must start").positions;
    assert_eq!(positions.len(), 1);
    let instrument_id = crypto_perpetual_ethusdt().id();
    assert_eq!(
        positions[0],
        PositionObservation {
            id: PositionId::from("VENUE-POSITION-042"),
            account_id: AccountId::from("BLOCKING-REPORT-001"),
            instrument_id,
            strategy_id: StrategyId::from("EXTERNAL"),
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
    positions: Vec<PositionObservation>,
    net_qty: Decimal,
}

#[derive(Debug, PartialEq)]
struct PositionObservation {
    id: PositionId,
    account_id: AccountId,
    instrument_id: InstrumentId,
    strategy_id: StrategyId,
    side: PositionSide,
    qty: Quantity,
    avg_px_open: Decimal,
}

#[derive(Debug)]
struct StartupStrategy {
    core: StrategyCore,
    handle: LiveNodeHandle,
    observation: Rc<RefCell<Option<StartupObservation>>>,
}

impl DataActor for StartupStrategy {
    fn on_start(&mut self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.observation.borrow().is_none(),
            "strategy must start only once"
        );

        let positions = self
            .cache()
            .positions_open(None, None, None, None, None)
            .iter()
            .map(|position| PositionObservation {
                id: position.id,
                account_id: position.account_id,
                instrument_id: position.instrument_id,
                strategy_id: position.strategy_id,
                side: position.side,
                qty: position.quantity,
                avg_px_open: position.avg_px_open,
            })
            .collect();

        let instrument_id = crypto_perpetual_ethusdt().id();
        let net_qty = self.portfolio().net_position(&instrument_id);
        *self.observation.borrow_mut() = Some(StartupObservation { positions, net_qty });
        self.handle.stop();
        Ok(())
    }
}

nautilus_strategy!(StartupStrategy);

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
