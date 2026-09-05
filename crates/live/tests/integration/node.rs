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

//! Integration tests for LiveNode lifecycle and handle control.
//!
//! These tests use global logging state (one logger per process).
//! Run with cargo-nextest for process isolation, or use --test-threads=1.

use std::{
    cell::{Cell, RefCell},
    fmt::Debug,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use nautilus_common::{
    actor::{DataActor, DataActorCore, data_actor::DataActorConfig},
    cache::CacheView,
    clients::{DataClient, ExecutionClient},
    clock::Clock,
    component::Component,
    enums::Environment,
    factories::{ClientConfig, DataClientFactory, ExecutionClientFactory},
    live::dst,
    messages::{
        execution::{
            CancelOrder, GenerateOrderStatusReport, GenerateOrderStatusReports,
            GeneratePositionStatusReports, QueryOrder,
        },
        system::{QueueStateChanged, ShutdownSystem},
    },
    msgbus::{self, MessagingSwitchboard, ShareableMessageHandler, switchboard},
    nautilus_actor,
    testing::{wait_until, wait_until_async},
};
use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_live::{
    builder::LiveNodeBuilder,
    config::{LiveExecutionEngineConfig, LiveNodeConfig},
    node::{LiveNode, LiveNodeHandle, NodeState},
};
use nautilus_model::{
    accounts::AccountAny,
    enums::{OmsType, OrderSide, OrderStatus, OrderType, TimeInForce},
    events::OrderEventAny,
    identifiers::{
        AccountId, ClientId, ClientOrderId, ExecAlgorithmId, InstrumentId, StrategyId, TraderId,
        Venue, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny, stubs::crypto_perpetual_ethusdt},
    orders::{Order, OrderAny, OrderTestBuilder, stubs::TestOrderEventStubs},
    reports::{ExecutionMassStatus, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance, Price, Quantity},
};
use nautilus_trading::{
    ExecutionAlgorithmConfig, ExecutionAlgorithmCore, nautilus_execution_algorithm,
    nautilus_strategy,
    strategy::{Strategy, StrategyConfig, StrategyCore},
};
use parking_lot::Mutex;
use rstest::rstest;

#[derive(Debug)]
struct TestActor {
    core: DataActorCore,
}

impl TestActor {
    fn new(config: DataActorConfig) -> Self {
        Self {
            core: DataActorCore::new(config),
        }
    }
}

impl DataActor for TestActor {}

nautilus_actor!(TestActor);

#[derive(Debug)]
struct TestStrategy {
    core: StrategyCore,
}

impl TestStrategy {
    fn new(config: StrategyConfig) -> Self {
        Self {
            core: StrategyCore::new(config),
        }
    }
}

impl DataActor for TestStrategy {}

nautilus_strategy!(TestStrategy);

#[derive(Debug)]
struct FailingStartStrategy {
    core: StrategyCore,
}

impl FailingStartStrategy {
    fn new(config: StrategyConfig) -> Self {
        Self {
            core: StrategyCore::new(config),
        }
    }
}

impl DataActor for FailingStartStrategy {
    fn on_start(&mut self) -> anyhow::Result<()> {
        anyhow::bail!("simulated live node strategy start failure")
    }
}

nautilus_strategy!(FailingStartStrategy);

#[derive(Debug)]
struct StopOnStartStrategy {
    core: StrategyCore,
    handle: LiveNodeHandle,
    instrument_id: InstrumentId,
    stop_count: Arc<AtomicUsize>,
}

impl StopOnStartStrategy {
    fn new(
        config: StrategyConfig,
        handle: LiveNodeHandle,
        instrument_id: InstrumentId,
        stop_count: Arc<AtomicUsize>,
    ) -> Self {
        Self {
            core: StrategyCore::new(config),
            handle,
            instrument_id,
            stop_count,
        }
    }
}

impl DataActor for StopOnStartStrategy {
    fn on_start(&mut self) -> anyhow::Result<()> {
        self.handle.stop();
        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        self.stop_count.fetch_add(1, Ordering::Relaxed);
        self.cancel_all_orders(self.instrument_id, None, None, true, None)
    }
}

nautilus_strategy!(StopOnStartStrategy);

#[derive(Debug)]
struct ClaimingTestStrategy {
    core: StrategyCore,
    external_order_instrument_ids: Vec<InstrumentId>,
}

impl ClaimingTestStrategy {
    fn new(strategy_id: StrategyId, instrument_id: InstrumentId) -> Self {
        let external_order_instrument_ids = vec![instrument_id];
        Self {
            core: StrategyCore::new(StrategyConfig {
                strategy_id: Some(strategy_id),
                external_order_instrument_ids: Some(external_order_instrument_ids.clone()),
                ..Default::default()
            }),
            external_order_instrument_ids,
        }
    }
}

impl DataActor for ClaimingTestStrategy {}

nautilus_strategy!(ClaimingTestStrategy, {
    fn external_order_instrument_ids(&self) -> Option<Vec<InstrumentId>> {
        Some(self.external_order_instrument_ids.clone())
    }
});

#[derive(Debug)]
struct TestExecutionAlgorithm {
    core: ExecutionAlgorithmCore,
}

impl TestExecutionAlgorithm {
    fn new(config: ExecutionAlgorithmConfig) -> Self {
        Self {
            core: ExecutionAlgorithmCore::new(config),
        }
    }
}

impl DataActor for TestExecutionAlgorithm {}

nautilus_execution_algorithm!(TestExecutionAlgorithm, {
    fn on_order(&mut self, _order: OrderAny) -> anyhow::Result<()> {
        Ok(())
    }
});

#[rstest]
fn test_handle_initial_state() {
    let handle = LiveNodeHandle::new();

    assert_eq!(handle.state(), NodeState::Idle);
    assert!(!handle.should_stop());
    assert!(!handle.is_running());
}

#[rstest]
fn test_handle_stop_sets_flag() {
    let handle = LiveNodeHandle::new();

    handle.stop();

    assert!(handle.should_stop());
}

#[rstest]
fn test_handle_clone_shares_state() {
    let handle1 = LiveNodeHandle::new();
    let handle2 = handle1.clone();

    handle1.stop();

    assert!(handle2.should_stop());
}

#[rstest]
fn test_node_state_values() {
    assert_eq!(NodeState::Idle.as_u8(), 0);
    assert_eq!(NodeState::Starting.as_u8(), 1);
    assert_eq!(NodeState::Running.as_u8(), 2);
    assert_eq!(NodeState::ShuttingDown.as_u8(), 3);
    assert_eq!(NodeState::Stopped.as_u8(), 4);
}

#[rstest]
fn test_node_state_is_running() {
    assert!(!NodeState::Idle.is_running());
    assert!(!NodeState::Starting.is_running());
    assert!(NodeState::Running.is_running());
    assert!(!NodeState::ShuttingDown.is_running());
    assert!(!NodeState::Stopped.is_running());
}

#[rstest]
fn test_builder_rejects_backtest_environment() {
    let result = LiveNode::builder(TraderId::from("TESTER-001"), Environment::Backtest);

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("Backtest"),
        "Expected Backtest error, was: {err}"
    );
}

#[rstest]
fn test_builder_accepts_sandbox() {
    let result = LiveNode::builder(TraderId::from("TESTER-001"), Environment::Sandbox);

    assert!(result.is_ok());
}

#[rstest]
fn test_builder_accepts_live() {
    let result = LiveNode::builder(TraderId::from("TESTER-001"), Environment::Live);

    assert!(result.is_ok());
}

// -- LiveNode construction tests (require process isolation via nextest) --------------------------
// These tests initialize global logging state and require isolated processes.
// Run with: cargo nextest run -p nautilus-live --test integration node::

mod serial_tests {
    use super::*;

    #[derive(Clone, Debug, Default)]
    struct StartupMassStatusClientState {
        connected: Arc<AtomicBool>,
        disconnect_attempted: Arc<AtomicBool>,
        factory_trader_id: Arc<Mutex<Option<TraderId>>>,
        mass_status_requested: Arc<AtomicBool>,
        mass_status: Arc<Mutex<Option<ExecutionMassStatus>>>,
        registered_external_orders: Arc<Mutex<Vec<ClientOrderId>>>,
        cancel_orders_received: Arc<AtomicUsize>,
        cancel_orders_while_connected: Arc<AtomicBool>,
    }

    #[derive(Clone, Debug, Default)]
    struct FailingDisconnectDataClientState {
        disconnect_attempted: Arc<AtomicBool>,
    }

    #[derive(Clone, Debug, Default)]
    struct LifecycleClientState {
        connected: Arc<AtomicBool>,
        connect_attempted: Arc<AtomicBool>,
        disconnect_attempted: Arc<AtomicBool>,
    }

    #[derive(Clone, Copy, Debug)]
    enum LifecycleClientBehavior {
        Connects,
        ConnectPending,
        ReadinessPending,
        ConnectDelayedReadinessPending,
        DisconnectPending,
        DisconnectKeepsConnected,
    }

    #[derive(Clone, Copy, Debug)]
    enum StartupMassStatusBehavior {
        Available,
        Unavailable,
        Error,
        Pending,
    }

    struct StartupMassStatusExecutionClient {
        state: StartupMassStatusClientState,
        behavior: StartupMassStatusBehavior,
        client_id: ClientId,
        account_id: AccountId,
        venue: Venue,
        handles_all_order_venues: bool,
    }

    struct FailingDisconnectDataClient {
        state: FailingDisconnectDataClientState,
    }

    struct LifecycleDataClient {
        state: LifecycleClientState,
        behavior: LifecycleClientBehavior,
    }

    struct LifecycleExecutionClient {
        state: LifecycleClientState,
        behavior: LifecycleClientBehavior,
    }

    impl StartupMassStatusExecutionClient {
        const CLIENT_ID: &'static str = "STARTUP-MASS-STATUS";

        fn new(
            state: StartupMassStatusClientState,
            behavior: StartupMassStatusBehavior,
            client_id: ClientId,
            account_id: AccountId,
            venue: Venue,
            handles_all_order_venues: bool,
        ) -> Self {
            Self {
                state,
                behavior,
                client_id,
                account_id,
                venue,
                handles_all_order_venues,
            }
        }
    }

    impl FailingDisconnectDataClient {
        const CLIENT_ID: &'static str = "FAILING-DISCONNECT-DATA";

        fn new(state: FailingDisconnectDataClientState) -> Self {
            Self { state }
        }
    }

    #[derive(Debug)]
    struct StartupMassStatusExecutionClientConfig;

    #[derive(Debug)]
    struct FailingDisconnectDataClientConfig;

    #[derive(Debug)]
    struct LifecycleDataClientConfig;

    #[derive(Debug)]
    struct LifecycleExecutionClientConfig;

    impl ClientConfig for StartupMassStatusExecutionClientConfig {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    impl ClientConfig for FailingDisconnectDataClientConfig {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    impl ClientConfig for LifecycleDataClientConfig {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    impl ClientConfig for LifecycleExecutionClientConfig {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[derive(Debug)]
    struct StartupMassStatusExecutionClientFactory {
        state: StartupMassStatusClientState,
        behavior: StartupMassStatusBehavior,
        client_id: ClientId,
        account_id: AccountId,
        venue: Venue,
        handles_all_order_venues: bool,
    }

    #[derive(Debug)]
    struct FailingDisconnectDataClientFactory {
        state: FailingDisconnectDataClientState,
    }

    #[derive(Debug)]
    struct LifecycleDataClientFactory {
        state: LifecycleClientState,
        behavior: LifecycleClientBehavior,
    }

    #[derive(Debug)]
    struct LifecycleExecutionClientFactory {
        state: LifecycleClientState,
        behavior: LifecycleClientBehavior,
    }

    impl StartupMassStatusExecutionClientFactory {
        fn new(state: StartupMassStatusClientState, behavior: StartupMassStatusBehavior) -> Self {
            Self {
                state,
                behavior,
                client_id: ClientId::from(StartupMassStatusExecutionClient::CLIENT_ID),
                account_id: AccountId::from("STARTUP-MASS-STATUS-001"),
                venue: crypto_perpetual_ethusdt().id().venue,
                handles_all_order_venues: false,
            }
        }

        fn with_identity(
            mut self,
            client_id: ClientId,
            account_id: AccountId,
            venue: Venue,
        ) -> Self {
            self.client_id = client_id;
            self.account_id = account_id;
            self.venue = venue;
            self
        }

        fn with_handles_all_order_venues(mut self) -> Self {
            self.handles_all_order_venues = true;
            self
        }
    }

    impl FailingDisconnectDataClientFactory {
        fn new(state: FailingDisconnectDataClientState) -> Self {
            Self { state }
        }
    }

    impl LifecycleDataClientFactory {
        fn new(state: LifecycleClientState, behavior: LifecycleClientBehavior) -> Self {
            Self { state, behavior }
        }
    }

    impl LifecycleExecutionClientFactory {
        fn new(state: LifecycleClientState, behavior: LifecycleClientBehavior) -> Self {
            Self { state, behavior }
        }
    }

    impl ExecutionClientFactory for StartupMassStatusExecutionClientFactory {
        fn create(
            &self,
            trader_id: TraderId,
            _name: &str,
            _config: &dyn ClientConfig,
            _cache: CacheView,
        ) -> anyhow::Result<Box<dyn ExecutionClient>> {
            *self.state.factory_trader_id.lock() = Some(trader_id);
            Ok(Box::new(StartupMassStatusExecutionClient::new(
                self.state.clone(),
                self.behavior,
                self.client_id,
                self.account_id,
                self.venue,
                self.handles_all_order_venues,
            )))
        }

        fn name(&self) -> &'static str {
            "startup-mass-status"
        }

        fn config_type(&self) -> &'static str {
            stringify!(StartupMassStatusExecutionClientConfig)
        }
    }

    impl DataClientFactory for FailingDisconnectDataClientFactory {
        fn create(
            &self,
            _name: &str,
            _config: &dyn ClientConfig,
            _cache: CacheView,
            _clock: Rc<RefCell<dyn Clock>>,
        ) -> anyhow::Result<Box<dyn DataClient>> {
            Ok(Box::new(FailingDisconnectDataClient::new(
                self.state.clone(),
            )))
        }

        fn name(&self) -> &'static str {
            "failing-disconnect-data"
        }

        fn config_type(&self) -> &'static str {
            stringify!(FailingDisconnectDataClientConfig)
        }
    }

    impl DataClientFactory for LifecycleDataClientFactory {
        fn create(
            &self,
            _name: &str,
            _config: &dyn ClientConfig,
            _cache: CacheView,
            _clock: Rc<RefCell<dyn Clock>>,
        ) -> anyhow::Result<Box<dyn DataClient>> {
            Ok(Box::new(LifecycleDataClient {
                state: self.state.clone(),
                behavior: self.behavior,
            }))
        }

        fn name(&self) -> &'static str {
            "lifecycle-data"
        }

        fn config_type(&self) -> &'static str {
            stringify!(LifecycleDataClientConfig)
        }
    }

    impl ExecutionClientFactory for LifecycleExecutionClientFactory {
        fn create(
            &self,
            _trader_id: TraderId,
            _name: &str,
            _config: &dyn ClientConfig,
            _cache: CacheView,
        ) -> anyhow::Result<Box<dyn ExecutionClient>> {
            Ok(Box::new(LifecycleExecutionClient {
                state: self.state.clone(),
                behavior: self.behavior,
            }))
        }

        fn name(&self) -> &'static str {
            "lifecycle-exec"
        }

        fn config_type(&self) -> &'static str {
            stringify!(LifecycleExecutionClientConfig)
        }
    }

    #[async_trait(?Send)]
    impl DataClient for FailingDisconnectDataClient {
        fn client_id(&self) -> ClientId {
            ClientId::from(Self::CLIENT_ID)
        }

        fn venue(&self) -> Option<Venue> {
            None
        }

        fn start(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn stop(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn reset(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn dispose(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn is_connected(&self) -> bool {
            false
        }

        fn is_disconnected(&self) -> bool {
            true
        }

        async fn disconnect(&mut self) -> anyhow::Result<()> {
            self.state
                .disconnect_attempted
                .store(true, Ordering::Relaxed);
            anyhow::bail!("simulated data client disconnect failure")
        }
    }

    #[async_trait(?Send)]
    impl DataClient for LifecycleDataClient {
        fn client_id(&self) -> ClientId {
            ClientId::from("LIFECYCLE-DATA")
        }

        fn venue(&self) -> Option<Venue> {
            None
        }

        fn start(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn stop(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn reset(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn dispose(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn is_connected(&self) -> bool {
            self.state.connected.load(Ordering::Relaxed)
        }

        fn is_disconnected(&self) -> bool {
            !self.state.connected.load(Ordering::Relaxed)
        }

        async fn connect(&mut self) -> anyhow::Result<()> {
            self.state.connect_attempted.store(true, Ordering::Relaxed);

            match self.behavior {
                LifecycleClientBehavior::ConnectPending => {
                    std::future::pending::<anyhow::Result<()>>().await
                }
                LifecycleClientBehavior::ReadinessPending => Ok(()),
                LifecycleClientBehavior::ConnectDelayedReadinessPending => {
                    dst::time::sleep(Duration::from_millis(25)).await;
                    Ok(())
                }
                LifecycleClientBehavior::Connects
                | LifecycleClientBehavior::DisconnectPending
                | LifecycleClientBehavior::DisconnectKeepsConnected => {
                    self.state.connected.store(true, Ordering::Relaxed);
                    Ok(())
                }
            }
        }

        async fn disconnect(&mut self) -> anyhow::Result<()> {
            self.state
                .disconnect_attempted
                .store(true, Ordering::Relaxed);

            if matches!(self.behavior, LifecycleClientBehavior::DisconnectPending) {
                return std::future::pending::<anyhow::Result<()>>().await;
            }

            if matches!(
                self.behavior,
                LifecycleClientBehavior::DisconnectKeepsConnected
            ) {
                return Ok(());
            }
            self.state.connected.store(false, Ordering::Relaxed);
            Ok(())
        }
    }

    fn live_node_with_startup_mass_status_client(
        name: &str,
        config: LiveNodeConfig,
        behavior: StartupMassStatusBehavior,
    ) -> (LiveNode, StartupMassStatusClientState) {
        let state = StartupMassStatusClientState::default();
        let factory = StartupMassStatusExecutionClientFactory::new(state.clone(), behavior);

        let node = LiveNodeBuilder::from_config(config)
            .unwrap()
            .with_name(name)
            .add_exec_client(
                Some("startup-mass-status".to_string()),
                Box::new(factory),
                Box::new(StartupMassStatusExecutionClientConfig),
            )
            .unwrap()
            .build()
            .unwrap();

        (node, state)
    }

    #[rstest]
    fn test_execution_factory_receives_live_node_trader_id() {
        let trader_id = TraderId::from("NODE-TRADER-001");
        let config = LiveNodeConfig {
            trader_id,
            ..Default::default()
        };

        let (_node, state) = live_node_with_startup_mass_status_client(
            "TraderIdentityNode",
            config,
            StartupMassStatusBehavior::Unavailable,
        );

        assert_eq!(*state.factory_trader_id.lock(), Some(trader_id));
    }

    #[async_trait(?Send)]
    impl ExecutionClient for StartupMassStatusExecutionClient {
        fn is_connected(&self) -> bool {
            self.state.connected.load(Ordering::Relaxed)
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
            self.handles_all_order_venues || self.venue == venue
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

        fn cancel_order(&self, _cmd: CancelOrder) -> anyhow::Result<()> {
            self.state
                .cancel_orders_received
                .fetch_add(1, Ordering::Relaxed);
            self.state.cancel_orders_while_connected.store(
                self.state.connected.load(Ordering::Relaxed),
                Ordering::Relaxed,
            );
            Ok(())
        }

        async fn connect(&mut self) -> anyhow::Result<()> {
            self.state.connected.store(true, Ordering::Relaxed);
            Ok(())
        }

        async fn disconnect(&mut self) -> anyhow::Result<()> {
            self.state
                .disconnect_attempted
                .store(true, Ordering::Relaxed);
            self.state.connected.store(false, Ordering::Relaxed);
            Ok(())
        }

        async fn generate_mass_status(
            &self,
            _lookback_mins: Option<u64>,
        ) -> anyhow::Result<Option<ExecutionMassStatus>> {
            self.state
                .mass_status_requested
                .store(true, Ordering::Relaxed);

            match self.behavior {
                StartupMassStatusBehavior::Available => Ok(self.state.mass_status.lock().clone()),
                StartupMassStatusBehavior::Unavailable => Ok(None),
                StartupMassStatusBehavior::Error => Err(anyhow::anyhow!("mass status failed")),
                StartupMassStatusBehavior::Pending => {
                    std::future::pending::<anyhow::Result<Option<ExecutionMassStatus>>>().await
                }
            }
        }

        fn register_external_order(
            &self,
            client_order_id: ClientOrderId,
            _venue_order_id: VenueOrderId,
            _instrument_id: InstrumentId,
            _strategy_id: StrategyId,
            _ts_init: UnixNanos,
        ) {
            self.state
                .registered_external_orders
                .lock()
                .push(client_order_id);
        }
    }

    #[async_trait(?Send)]
    impl ExecutionClient for LifecycleExecutionClient {
        fn is_connected(&self) -> bool {
            self.state.connected.load(Ordering::Relaxed)
        }

        fn client_id(&self) -> ClientId {
            ClientId::from("LIFECYCLE-EXEC")
        }

        fn account_id(&self) -> AccountId {
            AccountId::from("LIFECYCLE-EXEC-001")
        }

        fn venue(&self) -> Venue {
            crypto_perpetual_ethusdt().id().venue
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

        async fn connect(&mut self) -> anyhow::Result<()> {
            self.state.connect_attempted.store(true, Ordering::Relaxed);

            match self.behavior {
                LifecycleClientBehavior::ConnectPending => {
                    std::future::pending::<anyhow::Result<()>>().await
                }
                LifecycleClientBehavior::ReadinessPending => Ok(()),
                LifecycleClientBehavior::ConnectDelayedReadinessPending => {
                    dst::time::sleep(Duration::from_millis(25)).await;
                    Ok(())
                }
                LifecycleClientBehavior::Connects
                | LifecycleClientBehavior::DisconnectPending
                | LifecycleClientBehavior::DisconnectKeepsConnected => {
                    self.state.connected.store(true, Ordering::Relaxed);
                    Ok(())
                }
            }
        }

        async fn disconnect(&mut self) -> anyhow::Result<()> {
            self.state
                .disconnect_attempted
                .store(true, Ordering::Relaxed);

            if matches!(self.behavior, LifecycleClientBehavior::DisconnectPending) {
                return std::future::pending::<anyhow::Result<()>>().await;
            }

            if matches!(
                self.behavior,
                LifecycleClientBehavior::DisconnectKeepsConnected
            ) {
                return Ok(());
            }
            self.state.connected.store(false, Ordering::Relaxed);
            Ok(())
        }
    }

    fn live_node_with_lifecycle_clients(
        name: &str,
        data_behavior: LifecycleClientBehavior,
        exec_behavior: LifecycleClientBehavior,
    ) -> (LiveNode, LifecycleClientState, LifecycleClientState) {
        live_node_with_lifecycle_clients_timeout(
            name,
            data_behavior,
            exec_behavior,
            Duration::from_millis(50),
        )
    }

    fn live_node_with_lifecycle_clients_timeout(
        name: &str,
        data_behavior: LifecycleClientBehavior,
        exec_behavior: LifecycleClientBehavior,
        timeout_connection: Duration,
    ) -> (LiveNode, LifecycleClientState, LifecycleClientState) {
        let config = LiveNodeConfig {
            exec_engine: LiveExecutionEngineConfig {
                reconciliation: false,
                ..Default::default()
            },
            delay_post_stop: Duration::ZERO,
            timeout_connection,
            timeout_disconnection: Duration::from_millis(50),
            ..Default::default()
        };
        let data_state = LifecycleClientState::default();
        let exec_state = LifecycleClientState::default();
        let node = LiveNodeBuilder::from_config(config)
            .unwrap()
            .with_name(name)
            .add_data_client(
                Some("lifecycle-data".to_string()),
                Box::new(LifecycleDataClientFactory::new(
                    data_state.clone(),
                    data_behavior,
                )),
                Box::new(LifecycleDataClientConfig),
            )
            .unwrap()
            .add_exec_client(
                Some("lifecycle-exec".to_string()),
                Box::new(LifecycleExecutionClientFactory::new(
                    exec_state.clone(),
                    exec_behavior,
                )),
                Box::new(LifecycleExecutionClientConfig),
            )
            .unwrap()
            .build()
            .unwrap();

        (node, data_state, exec_state)
    }

    #[derive(Clone, Debug, Default)]
    struct BlockingReportClientState {
        query_order_received: Arc<AtomicBool>,
        query_order_ids: Arc<Mutex<Vec<ClientOrderId>>>,
        bulk_order_report_requested: Arc<AtomicBool>,
        bulk_order_report_count: Arc<AtomicUsize>,
        targeted_order_report_ids: Arc<Mutex<Vec<ClientOrderId>>>,
        position_report_requested: Arc<AtomicBool>,
        position_report_count: Arc<AtomicUsize>,
        instrument_received: Arc<AtomicBool>,
    }

    struct BlockingReportExecutionClient {
        connected: Cell<bool>,
        client_id: ClientId,
        account_id: AccountId,
        venue: Venue,
        state: BlockingReportClientState,
        order_reports: Vec<OrderStatusReport>,
        order_reports_complete: bool,
        block_every_second_order_report: bool,
        position_reports_complete: bool,
        block_every_second_targeted_report: bool,
        report_release: Option<Arc<tokio::sync::Notify>>,
    }

    impl BlockingReportExecutionClient {
        fn new(factory: &BlockingReportExecutionClientFactory) -> Self {
            Self {
                connected: Cell::new(false),
                client_id: factory.client_id,
                account_id: factory.account_id,
                venue: factory.venue,
                state: factory.state.clone(),
                order_reports: factory.order_reports.clone(),
                order_reports_complete: factory.order_reports_complete,
                block_every_second_order_report: factory.block_every_second_order_report,
                position_reports_complete: factory.position_reports_complete,
                block_every_second_targeted_report: factory.block_every_second_targeted_report,
                report_release: factory.report_release.clone(),
            }
        }
    }

    #[derive(Debug)]
    struct BlockingReportExecutionClientConfig;

    impl ClientConfig for BlockingReportExecutionClientConfig {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[derive(Debug)]
    struct BlockingReportExecutionClientFactory {
        client_id: ClientId,
        account_id: AccountId,
        venue: Venue,
        state: BlockingReportClientState,
        order_reports: Vec<OrderStatusReport>,
        order_reports_complete: bool,
        block_every_second_order_report: bool,
        position_reports_complete: bool,
        block_every_second_targeted_report: bool,
        report_release: Option<Arc<tokio::sync::Notify>>,
    }

    impl BlockingReportExecutionClientFactory {
        fn new(
            query_order_received: Arc<AtomicBool>,
            blocking_order_report_requested: Arc<AtomicBool>,
            position_report_requested: Arc<AtomicBool>,
            instrument_received: Arc<AtomicBool>,
            report_release: Option<Arc<tokio::sync::Notify>>,
        ) -> Self {
            Self {
                client_id: ClientId::from("BLOCKING-REPORT"),
                account_id: AccountId::from("BLOCKING-REPORT-001"),
                venue: crypto_perpetual_ethusdt().id().venue,
                state: BlockingReportClientState {
                    query_order_received,
                    bulk_order_report_requested: blocking_order_report_requested,
                    position_report_requested,
                    instrument_received,
                    ..Default::default()
                },
                order_reports: Vec::new(),
                order_reports_complete: false,
                block_every_second_order_report: false,
                position_reports_complete: false,
                block_every_second_targeted_report: false,
                report_release,
            }
        }

        fn configurable(
            client_id: ClientId,
            account_id: AccountId,
            state: BlockingReportClientState,
        ) -> Self {
            Self {
                client_id,
                account_id,
                venue: crypto_perpetual_ethusdt().id().venue,
                state,
                order_reports: Vec::new(),
                order_reports_complete: false,
                block_every_second_order_report: false,
                position_reports_complete: false,
                block_every_second_targeted_report: false,
                report_release: None,
            }
        }

        fn with_order_reports(mut self, reports: Vec<OrderStatusReport>) -> Self {
            self.order_reports = reports;
            self.order_reports_complete = true;
            self
        }

        fn with_venue(mut self, venue: Venue) -> Self {
            self.venue = venue;
            self
        }

        fn with_position_reports_complete(mut self) -> Self {
            self.position_reports_complete = true;
            self
        }

        fn with_block_every_second_order_report(mut self) -> Self {
            self.block_every_second_order_report = true;
            self
        }

        fn with_block_every_second_targeted_report(mut self) -> Self {
            self.block_every_second_targeted_report = true;
            self
        }
    }

    impl ExecutionClientFactory for BlockingReportExecutionClientFactory {
        fn create(
            &self,
            _trader_id: TraderId,
            _name: &str,
            _config: &dyn ClientConfig,
            _cache: CacheView,
        ) -> anyhow::Result<Box<dyn ExecutionClient>> {
            Ok(Box::new(BlockingReportExecutionClient::new(self)))
        }

        fn name(&self) -> &'static str {
            "blocking-report"
        }

        fn config_type(&self) -> &'static str {
            stringify!(BlockingReportExecutionClientConfig)
        }
    }

    fn live_node_with_blocking_exec_client(
        name: &str,
        config: LiveNodeConfig,
        query_order_received: Arc<AtomicBool>,
        blocking_order_report_requested: Arc<AtomicBool>,
        position_report_requested: Arc<AtomicBool>,
        instrument_received: Arc<AtomicBool>,
        report_release: Option<Arc<tokio::sync::Notify>>,
    ) -> LiveNode {
        let factory = BlockingReportExecutionClientFactory::new(
            query_order_received,
            blocking_order_report_requested,
            position_report_requested,
            instrument_received,
            report_release,
        );

        LiveNodeBuilder::from_config(config)
            .unwrap()
            .with_name(name)
            .add_exec_client(
                Some("blocking-report".to_string()),
                Box::new(factory),
                Box::new(BlockingReportExecutionClientConfig),
            )
            .unwrap()
            .build()
            .unwrap()
    }

    #[async_trait(?Send)]
    impl ExecutionClient for BlockingReportExecutionClient {
        fn is_connected(&self) -> bool {
            self.connected.get()
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

        fn query_order(&self, cmd: QueryOrder) -> anyhow::Result<()> {
            self.state
                .query_order_received
                .store(true, Ordering::Relaxed);
            self.state.query_order_ids.lock().push(cmd.client_order_id);
            Ok(())
        }

        fn on_instrument(&mut self, _instrument: InstrumentAny) {
            self.state
                .instrument_received
                .store(true, Ordering::Relaxed);
        }

        async fn connect(&mut self) -> anyhow::Result<()> {
            self.connected.set(true);
            Ok(())
        }

        async fn disconnect(&mut self) -> anyhow::Result<()> {
            self.connected.set(false);
            Ok(())
        }

        async fn generate_order_status_reports(
            &self,
            _cmd: &GenerateOrderStatusReports,
        ) -> anyhow::Result<Vec<OrderStatusReport>> {
            self.state
                .bulk_order_report_requested
                .store(true, Ordering::Relaxed);
            let request_count = self
                .state
                .bulk_order_report_count
                .fetch_add(1, Ordering::Relaxed)
                + 1;

            if self.block_every_second_order_report && request_count.is_multiple_of(2) {
                return std::future::pending::<anyhow::Result<Vec<OrderStatusReport>>>().await;
            }

            if self.order_reports_complete {
                return Ok(self.order_reports.clone());
            }

            if let Some(release) = &self.report_release {
                release.notified().await;
                Ok(Vec::new())
            } else {
                std::future::pending::<anyhow::Result<Vec<OrderStatusReport>>>().await
            }
        }

        async fn generate_order_status_report(
            &self,
            cmd: &GenerateOrderStatusReport,
        ) -> anyhow::Result<Option<OrderStatusReport>> {
            let client_order_id = cmd
                .client_order_id
                .expect("targeted report command must carry a client order ID");
            let request_count = {
                let mut ids = self.state.targeted_order_report_ids.lock();
                ids.push(client_order_id);
                ids.len()
            };

            if self.block_every_second_targeted_report && request_count.is_multiple_of(2) {
                return std::future::pending::<anyhow::Result<Option<OrderStatusReport>>>().await;
            }

            Ok(None)
        }

        async fn generate_position_status_reports(
            &self,
            _cmd: &GeneratePositionStatusReports,
        ) -> anyhow::Result<Vec<PositionStatusReport>> {
            self.state
                .position_report_requested
                .store(true, Ordering::Relaxed);
            self.state
                .position_report_count
                .fetch_add(1, Ordering::Relaxed);

            if self.position_reports_complete {
                return Ok(Vec::new());
            }

            if let Some(release) = &self.report_release {
                release.notified().await;
                Ok(Vec::new())
            } else {
                std::future::pending::<anyhow::Result<Vec<PositionStatusReport>>>().await
            }
        }
    }

    fn add_accepted_test_order(
        node: &LiveNode,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        client_id: ClientId,
    ) {
        add_accepted_test_order_with_origin(node, client_order_id, venue_order_id, Some(client_id));
    }

    fn add_accepted_test_order_with_origin(
        node: &LiveNode,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        client_id: Option<ClientId>,
    ) {
        let instrument_id = crypto_perpetual_ethusdt().id();
        let account_id = AccountId::from("BLOCKING-REPORT-001");
        let order = OrderTestBuilder::new(OrderType::Limit)
            .client_order_id(client_order_id)
            .instrument_id(instrument_id)
            .quantity(Quantity::from("10.0"))
            .price(Price::from("100.0"))
            .build();
        let submitted = TestOrderEventStubs::submitted(&order, account_id);
        node.kernel()
            .cache
            .borrow_mut()
            .add_order(order, None, client_id, false)
            .unwrap();
        let order = node
            .kernel()
            .cache
            .borrow_mut()
            .update_order(&submitted)
            .unwrap();
        let accepted = TestOrderEventStubs::accepted(&order, account_id, venue_order_id);
        node.kernel()
            .cache
            .borrow_mut()
            .update_order(&accepted)
            .unwrap();
    }

    fn canceled_order_report(
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
    ) -> OrderStatusReport {
        test_order_report(
            AccountId::from("BLOCKING-REPORT-001"),
            client_order_id,
            venue_order_id,
            OrderStatus::Canceled,
        )
    }

    fn test_order_report(
        account_id: AccountId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        order_status: OrderStatus,
    ) -> OrderStatusReport {
        OrderStatusReport::new(
            account_id,
            crypto_perpetual_ethusdt().id(),
            Some(client_order_id),
            venue_order_id,
            OrderSide::Buy.into(),
            OrderType::Limit,
            TimeInForce::Gtc,
            order_status,
            Quantity::from("10.0"),
            Quantity::from("0.0"),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            None,
        )
        .with_price(Price::from("100.0"))
    }

    fn live_node_with_available_mass_status(
        name: &str,
        state: StartupMassStatusClientState,
        client_id: ClientId,
        account_id: AccountId,
        venue: Venue,
    ) -> LiveNode {
        let config = LiveNodeConfig {
            exec_engine: LiveExecutionEngineConfig {
                reconciliation: true,
                ..Default::default()
            },
            delay_post_stop: Duration::ZERO,
            timeout_disconnection: Duration::from_millis(50),
            ..Default::default()
        };
        let factory = StartupMassStatusExecutionClientFactory::new(
            state,
            StartupMassStatusBehavior::Available,
        )
        .with_identity(client_id, account_id, venue)
        .with_handles_all_order_venues();
        let node = LiveNodeBuilder::from_config(config)
            .unwrap()
            .with_name(name)
            .add_exec_client(
                Some("source-client".to_string()),
                Box::new(factory),
                Box::new(StartupMassStatusExecutionClientConfig),
            )
            .unwrap()
            .build()
            .unwrap();
        node.kernel()
            .cache()
            .borrow_mut()
            .add_instrument(InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt()))
            .unwrap();
        node
    }

    #[rstest]
    fn test_live_node_build_with_default_config() {
        let node = LiveNode::build("TestNode".to_string(), None).unwrap();

        assert_eq!(node.state(), NodeState::Idle);
        assert_eq!(node.environment(), Environment::Live);
        assert!(!node.is_running());
    }

    #[rstest]
    fn test_live_node_build_preserves_sandbox_environment() {
        let config = LiveNodeConfig {
            environment: Environment::Sandbox,
            trader_id: TraderId::from("TESTER-001"),
            ..Default::default()
        };

        let node = LiveNode::build("TestNode".to_string(), Some(config)).unwrap();

        assert_eq!(node.environment(), Environment::Sandbox);
        assert_eq!(node.trader_id(), TraderId::from("TESTER-001"));
    }

    #[rstest]
    fn test_live_node_build_rejects_backtest_environment() {
        let config = LiveNodeConfig {
            environment: Environment::Backtest,
            ..Default::default()
        };

        let err = LiveNode::build("TestNode".to_string(), Some(config))
            .expect_err("build should reject Backtest");

        assert!(
            err.to_string().contains("Backtest"),
            "unexpected error: {err:#}"
        );
    }

    #[rstest]
    fn test_live_node_returns_handle() {
        let node = LiveNode::build("TestNode".to_string(), None).unwrap();
        let handle = node.handle();

        assert_eq!(handle.state(), NodeState::Idle);
        assert!(!handle.should_stop());
    }

    #[rstest]
    fn test_live_node_config_with_disabled_reconciliation() {
        let config = LiveNodeConfig {
            exec_engine: LiveExecutionEngineConfig {
                reconciliation: false,
                ..Default::default()
            },
            ..Default::default()
        };

        let node = LiveNode::build("TestNode".to_string(), Some(config)).unwrap();

        assert_eq!(node.state(), NodeState::Idle);
    }

    #[rstest]
    fn test_live_node_builds_reject_invalid_exec_interval() {
        let config = LiveNodeConfig {
            exec_engine: LiveExecutionEngineConfig {
                open_check_interval_secs: Some(f64::INFINITY),
                ..Default::default()
            },
            ..Default::default()
        };

        let direct_error = LiveNode::build("DirectNode".to_string(), Some(config.clone()))
            .expect_err("direct build should reject an invalid execution interval");
        let builder_error = LiveNodeBuilder::from_config(config)
            .unwrap()
            .build()
            .expect_err("builder should reject an invalid execution interval");

        for error in [direct_error, builder_error] {
            assert!(
                error
                    .to_string()
                    .contains("LiveExecutionEngineConfig.open_check_interval_secs"),
                "unexpected error: {error:#}"
            );
        }
    }

    #[rstest]
    fn test_add_actor() {
        let mut node = LiveNode::build("TestNode".to_string(), None).unwrap();

        let actor = TestActor::new(DataActorConfig::default());

        let result = node.add_actor(actor);

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_add_strategy() {
        let mut node = LiveNode::build("TestNode".to_string(), None).unwrap();

        let strategy = TestStrategy::new(StrategyConfig::default());

        let result = node.add_strategy(strategy);

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_add_strategy_rejects_duplicate_external_order_claim() {
        let mut node = LiveNode::build("TestNode".to_string(), None).unwrap();
        let instrument_id = InstrumentId::from("AUDUSD.SIM");
        let first_strategy =
            ClaimingTestStrategy::new(StrategyId::from("CLAIM-001"), instrument_id);
        let duplicate_strategy =
            ClaimingTestStrategy::new(StrategyId::from("CLAIM-002"), instrument_id);

        node.add_strategy(first_strategy).unwrap();
        let result = node.add_strategy(duplicate_strategy);

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("already exists for CLAIM-001")
        );
    }

    #[rstest]
    fn test_add_exec_algorithm() {
        let mut node = LiveNode::build("TestNode".to_string(), None).unwrap();

        let config = ExecutionAlgorithmConfig {
            exec_algorithm_id: Some(ExecAlgorithmId::from("TEST_ALGO")),
            ..Default::default()
        };
        let algo = TestExecutionAlgorithm::new(config);

        let result = node.add_exec_algorithm(algo);

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_add_exec_algorithm_registers_execute_endpoint() {
        let mut node = LiveNode::build("TestNode".to_string(), None).unwrap();

        let config = ExecutionAlgorithmConfig {
            exec_algorithm_id: Some(ExecAlgorithmId::from("MY_ALGO")),
            ..Default::default()
        };
        let algo = TestExecutionAlgorithm::new(config);

        node.add_exec_algorithm(algo).unwrap();

        assert!(nautilus_common::msgbus::has_endpoint("MY_ALGO.execute"));
    }

    #[rstest]
    fn test_handle_from_node_shares_state() {
        let node = LiveNode::build("TestNode".to_string(), None).unwrap();
        let handle = node.handle();

        handle.stop();

        assert!(handle.should_stop());
    }

    #[rstest]
    fn test_node_starts_in_idle_state() {
        let node = LiveNode::build("TestNode".to_string(), None).unwrap();

        assert_eq!(node.state(), NodeState::Idle);
    }

    #[rstest]
    fn test_kernel_access() {
        let node = LiveNode::build("TestNode".to_string(), None).unwrap();

        let kernel = node.kernel();

        assert_eq!(kernel.trader_id(), TraderId::from("TRADER-001"));
    }

    #[rstest]
    fn test_exec_manager_access() {
        let node = LiveNode::build("TestNode".to_string(), None).unwrap();

        let _manager = node.exec_manager();
    }

    #[rstest]
    #[tokio::test]
    async fn test_stop_when_not_running_returns_error() {
        let mut node = LiveNode::build("TestNode".to_string(), None).unwrap();

        let result = node.stop().await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Not running"));
    }

    #[rstest]
    #[cfg_attr(
        not(all(feature = "simulation", madsim)),
        tokio::test(start_paused = true)
    )]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_start_hung_data_connect_times_out_fail_closed() {
        let (mut node, data_state, exec_state) = live_node_with_lifecycle_clients(
            "StartHungDataConnectNode",
            LifecycleClientBehavior::ConnectPending,
            LifecycleClientBehavior::Connects,
        );
        let handle = node.handle();

        let result = dst::time::timeout(Duration::from_millis(200), node.start())
            .await
            .expect("start should finish within the lifecycle timeout");
        let err = result.expect_err("start should fail on a data-connect timeout");

        assert!(
            err.to_string().contains("data-connect"),
            "unexpected error: {err:#}"
        );
        assert_eq!(handle.state(), NodeState::Stopped);
        assert!(!handle.is_running());
        assert!(data_state.connect_attempted.load(Ordering::Relaxed));
        assert!(data_state.disconnect_attempted.load(Ordering::Relaxed));
        assert!(!exec_state.connect_attempted.load(Ordering::Relaxed));
    }

    #[rstest]
    #[cfg_attr(
        not(all(feature = "simulation", madsim)),
        tokio::test(flavor = "current_thread", start_paused = true)
    )]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_run_hung_data_connect_times_out_fail_closed() {
        let (mut node, data_state, exec_state) = live_node_with_lifecycle_clients(
            "RunHungDataConnectNode",
            LifecycleClientBehavior::ConnectPending,
            LifecycleClientBehavior::Connects,
        );
        let handle = node.handle();

        let result = dst::time::timeout(Duration::from_millis(200), node.run())
            .await
            .expect("run should finish within the lifecycle timeout");
        let err = result.expect_err("run should fail on a data-connect timeout");

        assert!(
            err.to_string().contains("data-connect"),
            "unexpected error: {err:#}"
        );
        assert_eq!(handle.state(), NodeState::Stopped);
        assert!(!handle.is_running());
        assert!(data_state.disconnect_attempted.load(Ordering::Relaxed));
        assert!(!exec_state.connect_attempted.load(Ordering::Relaxed));
    }

    #[rstest]
    #[cfg_attr(
        not(all(feature = "simulation", madsim)),
        tokio::test(start_paused = true)
    )]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_start_hung_exec_connect_times_out_fail_closed() {
        let (mut node, _data_state, exec_state) = live_node_with_lifecycle_clients(
            "StartHungExecConnectNode",
            LifecycleClientBehavior::Connects,
            LifecycleClientBehavior::ConnectPending,
        );
        let handle = node.handle();

        let result = dst::time::timeout(Duration::from_millis(200), node.start())
            .await
            .expect("start should finish within the lifecycle timeout");
        let err = result.expect_err("start should fail on an exec-connect timeout");

        assert!(
            err.to_string().contains("exec-connect"),
            "unexpected error: {err:#}"
        );
        assert_eq!(handle.state(), NodeState::Stopped);
        assert!(!handle.is_running());
        assert!(exec_state.connect_attempted.load(Ordering::Relaxed));
        assert!(exec_state.disconnect_attempted.load(Ordering::Relaxed));
    }

    #[rstest]
    #[cfg_attr(
        not(all(feature = "simulation", madsim)),
        tokio::test(flavor = "current_thread", start_paused = true)
    )]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_run_hung_exec_connect_times_out_fail_closed() {
        let (mut node, _data_state, exec_state) = live_node_with_lifecycle_clients(
            "RunHungExecConnectNode",
            LifecycleClientBehavior::Connects,
            LifecycleClientBehavior::ConnectPending,
        );
        let handle = node.handle();

        let result = dst::time::timeout(Duration::from_millis(200), node.run())
            .await
            .expect("run should finish within the lifecycle timeout");
        let err = result.expect_err("run should fail on an exec-connect timeout");

        assert!(
            err.to_string().contains("exec-connect"),
            "unexpected error: {err:#}"
        );
        assert_eq!(handle.state(), NodeState::Stopped);
        assert!(!handle.is_running());
        assert!(exec_state.disconnect_attempted.load(Ordering::Relaxed));
    }

    #[rstest]
    #[cfg_attr(
        not(all(feature = "simulation", madsim)),
        tokio::test(start_paused = true)
    )]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_start_readiness_timeout_fails_closed() {
        let (mut node, data_state, exec_state) = live_node_with_lifecycle_clients(
            "StartReadinessTimeoutNode",
            LifecycleClientBehavior::ReadinessPending,
            LifecycleClientBehavior::Connects,
        );
        let handle = node.handle();

        let result = dst::time::timeout(Duration::from_millis(200), node.start())
            .await
            .expect("start should finish within the lifecycle timeout");
        let err = result.expect_err("start should fail on a readiness timeout");

        assert!(
            err.to_string().contains("readiness"),
            "unexpected error: {err:#}"
        );
        assert_eq!(handle.state(), NodeState::Stopped);
        assert!(!handle.is_running());
        assert!(data_state.disconnect_attempted.load(Ordering::Relaxed));
        assert!(exec_state.disconnect_attempted.load(Ordering::Relaxed));
    }

    #[rstest]
    #[cfg_attr(
        not(all(feature = "simulation", madsim)),
        tokio::test(flavor = "current_thread", start_paused = true)
    )]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_run_readiness_timeout_fails_closed() {
        let (mut node, data_state, exec_state) = live_node_with_lifecycle_clients(
            "RunReadinessTimeoutNode",
            LifecycleClientBehavior::ReadinessPending,
            LifecycleClientBehavior::Connects,
        );
        let handle = node.handle();

        let result = dst::time::timeout(Duration::from_millis(200), node.run())
            .await
            .expect("run should finish within the lifecycle timeout");
        let err = result.expect_err("run should fail on a readiness timeout");

        assert!(
            err.to_string().contains("readiness"),
            "unexpected error: {err:#}"
        );
        assert_eq!(handle.state(), NodeState::Stopped);
        assert!(!handle.is_running());
        assert!(data_state.disconnect_attempted.load(Ordering::Relaxed));
        assert!(exec_state.disconnect_attempted.load(Ordering::Relaxed));
    }

    #[rstest]
    #[cfg_attr(
        not(all(feature = "simulation", madsim)),
        tokio::test(start_paused = true)
    )]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_zero_timeout_connection_starts_without_clients() {
        // A zero `timeout_connection` with no clients must still start: the empty
        // connect completes on the first poll. Regression for the pre-stage bail
        // that rejected a zero budget before ever attempting the connect.
        let config = LiveNodeConfig {
            exec_engine: LiveExecutionEngineConfig {
                reconciliation: false,
                ..Default::default()
            },
            delay_post_stop: Duration::ZERO,
            timeout_connection: Duration::ZERO,
            timeout_disconnection: Duration::ZERO,
            ..Default::default()
        };
        let mut node =
            LiveNode::build("ZeroTimeoutNoClientsNode".to_string(), Some(config)).unwrap();
        let handle = node.handle();

        node.start().await.unwrap();
        assert_eq!(handle.state(), NodeState::Running);

        node.stop().await.unwrap();
        assert_eq!(handle.state(), NodeState::Stopped);
    }

    #[rstest]
    #[cfg_attr(
        not(all(feature = "simulation", madsim)),
        tokio::test(start_paused = true)
    )]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_zero_timeout_connection_still_bounds_hung_data_connect() {
        // Zero `timeout_connection` must still fail closed on a hung connect: the
        // bound is not disabled by a zero budget.
        let (mut node, data_state, _exec_state) = live_node_with_lifecycle_clients_timeout(
            "ZeroTimeoutHungConnectNode",
            LifecycleClientBehavior::ConnectPending,
            LifecycleClientBehavior::Connects,
            Duration::ZERO,
        );
        let handle = node.handle();

        let result = dst::time::timeout(Duration::from_millis(200), node.start())
            .await
            .expect("start should finish within the lifecycle timeout");
        let err = result.expect_err("start should fail on a data-connect timeout");

        assert!(
            err.to_string().contains("data-connect"),
            "unexpected error: {err:#}"
        );
        assert_eq!(handle.state(), NodeState::Stopped);
        assert!(!handle.is_running());
        assert!(data_state.connect_attempted.load(Ordering::Relaxed));
    }

    #[rstest]
    #[cfg_attr(
        not(all(feature = "simulation", madsim)),
        tokio::test(flavor = "current_thread", start_paused = true)
    )]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_run_zero_timeout_connection_still_bounds_hung_data_connect() {
        let (mut node, data_state, _exec_state) = live_node_with_lifecycle_clients_timeout(
            "RunZeroTimeoutHungConnectNode",
            LifecycleClientBehavior::ConnectPending,
            LifecycleClientBehavior::Connects,
            Duration::ZERO,
        );
        let handle = node.handle();

        let result = dst::time::timeout(Duration::from_millis(200), node.run())
            .await
            .expect("run should finish within the lifecycle timeout");
        let err = result.expect_err("run should fail on a data-connect timeout");

        assert!(
            err.to_string().contains("data-connect"),
            "unexpected error: {err:#}"
        );
        assert_eq!(handle.state(), NodeState::Stopped);
        assert!(!handle.is_running());
        // The connect was polled (attempt marked) before the zero-budget timeout,
        // distinguishing the fix from the old pre-stage bail that never polled.
        assert!(data_state.connect_attempted.load(Ordering::Relaxed));
    }

    #[rstest]
    #[cfg_attr(
        not(all(feature = "simulation", madsim)),
        tokio::test(start_paused = true)
    )]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_start_readiness_timeout_uses_shared_connection_budget() {
        let (mut node, _data_state, _exec_state) = live_node_with_lifecycle_clients(
            "SharedConnectionBudgetNode",
            LifecycleClientBehavior::ConnectDelayedReadinessPending,
            LifecycleClientBehavior::Connects,
        );
        let handle = node.handle();
        let started_at = dst::time::Instant::now();

        let result = dst::time::timeout(Duration::from_millis(200), node.start())
            .await
            .expect("start should finish within the lifecycle timeout");
        let elapsed = started_at.elapsed();
        let err = result.expect_err("start should fail on a readiness timeout");

        assert!(
            err.to_string().contains("readiness"),
            "unexpected error: {err:#}"
        );
        assert!(
            elapsed <= Duration::from_millis(60),
            "readiness timeout exceeded the shared 50ms connection budget: {elapsed:?}"
        );
        assert_eq!(handle.state(), NodeState::Stopped);
        assert!(!handle.is_running());
    }

    #[rstest]
    #[cfg_attr(
        not(all(feature = "simulation", madsim)),
        tokio::test(start_paused = true)
    )]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_stop_fails_when_disconnect_readiness_poll_times_out() {
        let (mut node, data_state, _exec_state) = live_node_with_lifecycle_clients(
            "DisconnectReadinessPollNode",
            LifecycleClientBehavior::DisconnectKeepsConnected,
            LifecycleClientBehavior::Connects,
        );
        let handle = node.handle();
        node.start().await.unwrap();

        let result = dst::time::timeout(Duration::from_millis(200), node.stop())
            .await
            .expect("stop should finish within the lifecycle timeout");
        let err = result.expect_err("stop should fail on a disconnect readiness timeout");

        assert!(
            err.to_string().contains("disconnect readiness"),
            "unexpected error: {err:#}"
        );
        assert_eq!(handle.state(), NodeState::Stopped);
        assert!(data_state.disconnect_attempted.load(Ordering::Relaxed));
    }

    #[rstest]
    #[cfg_attr(
        not(all(feature = "simulation", madsim)),
        tokio::test(start_paused = true)
    )]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_hung_data_disconnect_still_attempts_execution_disconnect() {
        let (mut node, data_state, exec_state) = live_node_with_lifecycle_clients(
            "HungDataDisconnectNode",
            LifecycleClientBehavior::DisconnectPending,
            LifecycleClientBehavior::Connects,
        );
        let handle = node.handle();
        node.start().await.unwrap();

        let result = dst::time::timeout(Duration::from_millis(200), node.stop())
            .await
            .expect("stop should finish within the lifecycle timeout");
        let err = result.expect_err("stop should fail on a disconnect timeout");

        assert!(
            err.to_string().contains("disconnect"),
            "unexpected error: {err:#}"
        );
        assert_eq!(handle.state(), NodeState::Stopped);
        assert!(data_state.disconnect_attempted.load(Ordering::Relaxed));
        assert!(exec_state.disconnect_attempted.load(Ordering::Relaxed));
        assert!(!exec_state.connected.load(Ordering::Relaxed));
    }

    #[rstest]
    #[tokio::test]
    async fn test_start_stop_dispose_releases_resources() {
        let config = LiveNodeConfig {
            exec_engine: LiveExecutionEngineConfig {
                reconciliation: false,
                ..Default::default()
            },
            delay_post_stop: Duration::ZERO,
            timeout_disconnection: Duration::ZERO,
            ..Default::default()
        };
        let mut node = LiveNode::build("LifecycleNode".to_string(), Some(config)).unwrap();
        node.add_strategy(TestStrategy::new(StrategyConfig {
            strategy_id: Some(StrategyId::from("LIFECYCLE-001")),
            ..Default::default()
        }))
        .unwrap();
        let handle = node.handle();

        node.start().await.unwrap();
        let trader_running = node.kernel().trader().borrow().is_running();
        let running_component_count = node.kernel().trader().borrow().component_count();
        node.stop().await.unwrap();
        let trader_stopped = node.kernel().trader().borrow().is_stopped();
        node.dispose();
        node.dispose();

        assert_eq!(handle.state(), NodeState::Stopped);
        assert!(trader_running);
        assert_eq!(running_component_count, 1);
        assert!(trader_stopped);
        assert!(node.kernel().trader().borrow().is_disposed());
        assert_eq!(node.kernel().trader().borrow().component_count(), 0);
    }

    #[rstest]
    #[tokio::test]
    async fn test_start_without_cache_backing_preserves_staged_cache() {
        let config = LiveNodeConfig {
            exec_engine: LiveExecutionEngineConfig {
                reconciliation: false,
                ..Default::default()
            },
            delay_post_stop: Duration::ZERO,
            timeout_disconnection: Duration::ZERO,
            ..Default::default()
        };
        let mut node = LiveNode::build("NoBackingNode".to_string(), Some(config)).unwrap();
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        let instrument_id = instrument.id();
        node.kernel()
            .cache()
            .borrow_mut()
            .add_instrument(instrument)
            .unwrap();

        node.start().await.unwrap();
        let retained = node
            .kernel()
            .cache()
            .borrow()
            .instrument(&instrument_id)
            .is_some();
        node.stop().await.unwrap();
        node.dispose();

        assert!(retained);
    }

    #[rstest]
    #[tokio::test]
    async fn test_run_twice_returns_error() {
        let config = LiveNodeConfig {
            exec_engine: LiveExecutionEngineConfig {
                reconciliation: false,
                ..Default::default()
            },
            delay_post_stop: Duration::from_millis(50),
            ..Default::default()
        };
        let mut node = LiveNode::build("TestNode".to_string(), Some(config)).unwrap();
        let handle = node.handle();

        let stop_handle = handle.clone();

        tokio::spawn(async move {
            wait_until_async(
                || async { stop_handle.is_running() },
                Duration::from_secs(5),
            )
            .await;
            stop_handle.stop();
        });

        // First run - completes and consumes the runner
        let _ = node.run().await;

        // Second run - should fail because runner is consumed
        let result = node.run().await;

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Runner already consumed")
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_handle_stop_triggers_graceful_shutdown() {
        let config = LiveNodeConfig {
            exec_engine: LiveExecutionEngineConfig {
                reconciliation: false,
                ..Default::default()
            },
            delay_post_stop: Duration::from_millis(50),
            ..Default::default()
        };
        let mut node = LiveNode::build("TestNode".to_string(), Some(config)).unwrap();
        let handle = node.handle();

        assert_eq!(handle.state(), NodeState::Idle);

        // Spawn task to stop after node enters Running state
        let stop_handle = handle.clone();

        tokio::spawn(async move {
            wait_until_async(
                || async { stop_handle.is_running() },
                Duration::from_secs(5),
            )
            .await;
            stop_handle.stop();
        });

        // With no clients, run() completes startup immediately and waits for stop signal
        let result = node.run().await;

        assert!(result.is_ok());
        assert_eq!(handle.state(), NodeState::Stopped);
    }

    #[rstest]
    #[tokio::test(flavor = "current_thread")]
    async fn test_shutdown_system_triggers_graceful_shutdown() {
        let config = LiveNodeConfig {
            exec_engine: LiveExecutionEngineConfig {
                reconciliation: false,
                ..Default::default()
            },
            delay_post_stop: Duration::from_millis(50),
            ..Default::default()
        };
        let mut node = LiveNode::build("TestNode".to_string(), Some(config)).unwrap();
        let handle = node.handle();
        let trader_id = node.kernel().trader_id();
        let ts = node.kernel().generate_timestamp_ns();

        // Publish ShutdownSystem once the node reaches Running. msgbus uses
        // thread-local storage, so the publish must happen on the same thread
        // as node.run(). The test runtime is pinned to current_thread above
        // so tokio::spawn stays on this thread.
        let state_handle = handle.clone();

        tokio::spawn(async move {
            wait_until_async(
                || async { state_handle.is_running() },
                Duration::from_secs(5),
            )
            .await;
            let command = ShutdownSystem::new(
                trader_id,
                ustr::Ustr::from("TestComponent"),
                Some("integration test".to_string()),
                UUID4::new(),
                ts,
                None, // correlation_id
            );
            msgbus::publish_any(
                MessagingSwitchboard::shutdown_system_topic(),
                command.as_any(),
            );
        });

        let result = node.run().await;

        assert!(result.is_ok());
        assert_eq!(handle.state(), NodeState::Stopped);
    }

    #[rstest]
    #[tokio::test(flavor = "current_thread")]
    async fn test_error_log_triggers_graceful_shutdown() {
        let config = LiveNodeConfig {
            shutdown_on_error: true,
            exec_engine: LiveExecutionEngineConfig {
                reconciliation: false,
                ..Default::default()
            },
            delay_post_stop: Duration::from_millis(50),
            ..Default::default()
        };
        let mut node = LiveNode::build("TestNode".to_string(), Some(config)).unwrap();
        let handle = node.handle();
        let state_handle = handle.clone();

        let log_thread = std::thread::spawn(move || {
            wait_until(|| state_handle.is_running(), Duration::from_secs(5));
            log::error!("LiveNode shutdown-on-error smoke test");
        });

        let result = node.run().await;
        log_thread.join().unwrap();

        assert!(result.is_ok());
        assert_eq!(handle.state(), NodeState::Stopped);
    }

    #[rstest]
    #[tokio::test]
    async fn test_handle_stop_completes_within_timeout() {
        let config = LiveNodeConfig {
            exec_engine: LiveExecutionEngineConfig {
                reconciliation: false,
                ..Default::default()
            },
            delay_post_stop: Duration::from_millis(50),
            ..Default::default()
        };
        let mut node = LiveNode::build("TestNode".to_string(), Some(config)).unwrap();
        let handle = node.handle();

        let stop_handle = handle.clone();

        tokio::spawn(async move {
            wait_until_async(
                || async { stop_handle.is_running() },
                Duration::from_secs(5),
            )
            .await;
            stop_handle.stop();
        });

        // The biased select in the event loop prioritizes signals over data,
        // so stop should complete well within 5 seconds even under load
        let result = tokio::time::timeout(Duration::from_secs(5), node.run()).await;

        assert!(
            result.is_ok(),
            "run() should complete within 5 seconds after stop"
        );
        assert_eq!(handle.state(), NodeState::Stopped);
    }

    #[rstest]
    #[tokio::test]
    async fn test_start_continues_when_mass_status_unavailable() {
        let config = LiveNodeConfig {
            exec_engine: LiveExecutionEngineConfig {
                reconciliation: true,
                ..Default::default()
            },
            delay_post_stop: Duration::ZERO,
            timeout_disconnection: Duration::from_millis(50),
            ..Default::default()
        };
        let (mut node, state) = live_node_with_startup_mass_status_client(
            "StartupMassStatusUnavailableNode",
            config,
            StartupMassStatusBehavior::Unavailable,
        );
        let handle = node.handle();

        let result = node.start().await;

        assert!(result.is_ok(), "unexpected error: {result:#?}");
        assert!(state.mass_status_requested.load(Ordering::Relaxed));
        assert_eq!(handle.state(), NodeState::Running);
        assert!(state.connected.load(Ordering::Relaxed));

        node.stop().await.unwrap();

        node.dispose();

        assert_eq!(handle.state(), NodeState::Stopped);
        assert!(!state.connected.load(Ordering::Relaxed));
        assert!(node.kernel().trader().borrow().is_disposed());
        assert_eq!(node.kernel().trader().borrow().component_count(), 0);
    }

    #[rstest]
    #[tokio::test]
    async fn test_startup_mass_status_registers_external_order_with_source_client() {
        let config = LiveNodeConfig {
            exec_engine: LiveExecutionEngineConfig {
                reconciliation: true,
                ..Default::default()
            },
            delay_post_stop: Duration::ZERO,
            timeout_disconnection: Duration::from_millis(50),
            ..Default::default()
        };
        let instrument = crypto_perpetual_ethusdt();
        let instrument_id = instrument.id();
        let venue_client_id = ClientId::from("VENUE-CLIENT");
        let source_client_id = ClientId::from("ROUTING-CLIENT");
        let source_account_id = AccountId::from("ROUTING-001");
        let source_venue = Venue::from("ROUTING");
        let client_order_id = ClientOrderId::from("O-EXT-SOURCE");
        let venue_order_id = VenueOrderId::from("V-EXT-SOURCE");
        let venue_state = StartupMassStatusClientState::default();
        let source_state = StartupMassStatusClientState::default();
        let report = test_order_report(
            source_account_id,
            client_order_id,
            venue_order_id,
            OrderStatus::Accepted,
        );
        let mut mass_status = ExecutionMassStatus::new(
            source_client_id,
            source_account_id,
            source_venue,
            UnixNanos::default(),
            None,
        );
        mass_status.add_order_reports(vec![report]);
        *source_state.mass_status.lock() = Some(mass_status);

        let venue_factory = StartupMassStatusExecutionClientFactory::new(
            venue_state.clone(),
            StartupMassStatusBehavior::Unavailable,
        )
        .with_identity(
            venue_client_id,
            AccountId::from("VENUE-001"),
            instrument_id.venue,
        )
        .with_handles_all_order_venues();
        let source_factory = StartupMassStatusExecutionClientFactory::new(
            source_state.clone(),
            StartupMassStatusBehavior::Available,
        )
        .with_identity(source_client_id, source_account_id, source_venue)
        .with_handles_all_order_venues();
        let mut node = LiveNodeBuilder::from_config(config)
            .unwrap()
            .with_name("StartupMassStatusSourceNode")
            .add_exec_client(
                Some("venue-client".to_string()),
                Box::new(venue_factory),
                Box::new(StartupMassStatusExecutionClientConfig),
            )
            .unwrap()
            .add_exec_client(
                Some("source-client".to_string()),
                Box::new(source_factory),
                Box::new(StartupMassStatusExecutionClientConfig),
            )
            .unwrap()
            .build()
            .unwrap();
        node.kernel()
            .cache()
            .borrow_mut()
            .add_instrument(InstrumentAny::CryptoPerpetual(instrument))
            .unwrap();

        node.start().await.unwrap();

        assert_eq!(
            node.kernel()
                .cache()
                .borrow()
                .client_id(&client_order_id)
                .copied(),
            Some(source_client_id)
        );
        assert!(venue_state.registered_external_orders.lock().is_empty());
        assert_eq!(
            *source_state.registered_external_orders.lock(),
            vec![client_order_id]
        );

        node.stop().await.unwrap();
        node.dispose();
    }

    #[rstest]
    #[case("OTHER", "SOURCE-001", "SOURCE", "client ID")]
    #[case("SOURCE", "OTHER-001", "SOURCE", "account ID")]
    #[case("SOURCE", "SOURCE-001", "OTHER", "venue")]
    #[tokio::test]
    async fn test_startup_mass_status_rejects_mismatched_source_identity(
        #[case] reported_client_id: &str,
        #[case] reported_account_id: &str,
        #[case] reported_venue: &str,
        #[case] expected_error: &str,
    ) {
        let source_client_id = ClientId::from("SOURCE");
        let source_account_id = AccountId::from("SOURCE-001");
        let source_venue = Venue::from("SOURCE");
        let client_order_id = ClientOrderId::from("O-MISMATCHED-SOURCE");
        let venue_order_id = VenueOrderId::from("V-MISMATCHED-SOURCE");
        let state = StartupMassStatusClientState::default();
        let mut mass_status = ExecutionMassStatus::new(
            ClientId::from(reported_client_id),
            AccountId::from(reported_account_id),
            Venue::from(reported_venue),
            UnixNanos::default(),
            None,
        );
        mass_status.add_order_reports(vec![test_order_report(
            AccountId::from(reported_account_id),
            client_order_id,
            venue_order_id,
            OrderStatus::Accepted,
        )]);
        *state.mass_status.lock() = Some(mass_status);
        let mut node = live_node_with_available_mass_status(
            "StartupMassStatusIdentityNode",
            state.clone(),
            source_client_id,
            source_account_id,
            source_venue,
        );
        let raw_topic = MessagingSwitchboard::reconciliation_raw_order_status_report_topic();
        let raw_pattern: msgbus::MStr<msgbus::Pattern> = raw_topic.into();
        let (raw_handler, raw_saver) =
            nautilus_common::msgbus::stubs::get_any_saving_handler::<OrderStatusReport>(None);
        msgbus::subscribe_any(raw_pattern, raw_handler.clone(), None);
        let event_topic = switchboard::get_event_order_topic(StrategyId::from("EXTERNAL"));
        let (event_handler, event_saver) =
            nautilus_common::msgbus::stubs::get_typed_message_saving_handler::<OrderEventAny>(None);
        msgbus::subscribe_order_events(event_topic.into(), event_handler.clone(), None);

        let result = node.start().await;

        msgbus::unsubscribe_any(raw_pattern, &raw_handler);
        msgbus::unsubscribe_order_events(event_topic.into(), &event_handler);
        let error = result.expect_err("mismatched mass status identity should abort startup");

        assert!(
            format!("{error:#}").contains(expected_error),
            "unexpected error: {error:#}"
        );
        assert_eq!(node.state(), NodeState::Stopped);
        assert!(raw_saver.get_messages().is_empty());
        assert!(event_saver.get_messages().is_empty());
        assert!(
            node.kernel()
                .cache()
                .borrow()
                .order(&client_order_id)
                .is_none()
        );
        assert!(state.registered_external_orders.lock().is_empty());
        node.dispose();
    }

    #[rstest]
    #[case::missing_origin(None)]
    #[case::conflicting_origin(Some("OTHER"))]
    #[tokio::test]
    async fn test_startup_mass_status_warns_on_untrusted_cached_order_origin(
        #[case] cached_client_id: Option<&str>,
    ) {
        let source_client_id = ClientId::from("SOURCE-CLIENT");
        let source_account_id = AccountId::from("BLOCKING-REPORT-001");
        let source_venue = Venue::from("SOURCE");
        let client_order_id = ClientOrderId::from("O-UNTRUSTED-SOURCE");
        let venue_order_id = VenueOrderId::from("V-UNTRUSTED-SOURCE");
        let state = StartupMassStatusClientState::default();
        let mut mass_status = ExecutionMassStatus::new(
            source_client_id,
            source_account_id,
            source_venue,
            UnixNanos::default(),
            None,
        );
        mass_status.add_order_reports(vec![test_order_report(
            source_account_id,
            client_order_id,
            venue_order_id,
            OrderStatus::Canceled,
        )]);
        *state.mass_status.lock() = Some(mass_status);
        let mut node = live_node_with_available_mass_status(
            "StartupMassStatusUntrustedOriginNode",
            state.clone(),
            source_client_id,
            source_account_id,
            source_venue,
        );
        add_accepted_test_order_with_origin(
            &node,
            client_order_id,
            venue_order_id,
            cached_client_id.map(ClientId::from),
        );

        node.start()
            .await
            .expect("untrusted cached order origin should warn, not abort startup");

        assert!(state.registered_external_orders.lock().is_empty());
        {
            let cache = node.kernel().cache();
            let cache = cache.borrow();
            assert_eq!(
                cache.order(&client_order_id).unwrap().status(),
                OrderStatus::Canceled
            );
            assert_eq!(
                cache.client_id(&client_order_id).copied(),
                cached_client_id.map(ClientId::from)
            );
        }

        node.stop().await.unwrap();
        node.dispose();
    }

    #[rstest]
    #[tokio::test]
    async fn test_startup_mass_status_aborts_if_source_disappears_during_raw_publish() {
        let source_client_id = ClientId::from("SOURCE-CLIENT");
        let source_account_id = AccountId::from("BLOCKING-REPORT-001");
        let source_venue = Venue::from("SOURCE");
        let client_order_id = ClientOrderId::from("O-DISAPPEARING-SOURCE");
        let venue_order_id = VenueOrderId::from("V-DISAPPEARING-SOURCE");
        let state = StartupMassStatusClientState::default();
        let mut mass_status = ExecutionMassStatus::new(
            source_client_id,
            source_account_id,
            source_venue,
            UnixNanos::default(),
            None,
        );
        mass_status.add_order_reports(vec![test_order_report(
            source_account_id,
            client_order_id,
            venue_order_id,
            OrderStatus::Canceled,
        )]);
        *state.mass_status.lock() = Some(mass_status);
        let mut node = live_node_with_available_mass_status(
            "StartupMassStatusDisappearingSourceNode",
            state.clone(),
            source_client_id,
            source_account_id,
            source_venue,
        );
        add_accepted_test_order(&node, client_order_id, venue_order_id, source_client_id);

        let exec_engine = node.kernel().exec_engine().clone();
        let handler = ShareableMessageHandler::from_typed(move |_report: &OrderStatusReport| {
            exec_engine
                .borrow_mut()
                .deregister_client(source_client_id)
                .expect("source execution client should still be registered");
        });
        let raw_pattern: msgbus::MStr<msgbus::Pattern> =
            MessagingSwitchboard::reconciliation_raw_order_status_report_topic().into();
        msgbus::subscribe_any(raw_pattern, handler.clone(), None);

        let result = node.start().await;

        msgbus::unsubscribe_any(raw_pattern, &handler);
        let error = result.expect_err("disappearing source client should abort startup");
        assert!(
            error
                .to_string()
                .contains("disappeared during startup reconciliation"),
            "unexpected error: {error:#}"
        );
        assert_eq!(node.state(), NodeState::Stopped);
        assert!(state.registered_external_orders.lock().is_empty());
        assert_eq!(
            node.kernel()
                .cache()
                .borrow()
                .order(&client_order_id)
                .unwrap()
                .status(),
            OrderStatus::Accepted
        );

        node.dispose();
    }

    #[rstest]
    #[tokio::test]
    async fn test_strategy_start_failure_stops_partial_start_and_disposes_resources() {
        let config = LiveNodeConfig {
            exec_engine: LiveExecutionEngineConfig {
                reconciliation: false,
                ..Default::default()
            },
            timeout_disconnection: Duration::from_millis(50),
            ..Default::default()
        };
        let (mut node, state) = live_node_with_startup_mass_status_client(
            "StrategyStartFailureNode",
            config,
            StartupMassStatusBehavior::Unavailable,
        );
        node.add_strategy(TestStrategy::new(StrategyConfig {
            strategy_id: Some(StrategyId::from("MANAGED-STOP-001")),
            manage_stop: true,
            ..Default::default()
        }))
        .unwrap();
        node.add_strategy(FailingStartStrategy::new(StrategyConfig {
            strategy_id: Some(StrategyId::from("FAILING-START-001")),
            order_id_tag: Some("002".to_string()),
            ..Default::default()
        }))
        .unwrap();
        let handle = node.handle();

        let err = node.start().await.expect_err("strategy start should fail");

        assert!(
            err.to_string()
                .contains("simulated live node strategy start failure"),
            "unexpected error: {err:#}"
        );
        assert_eq!(handle.state(), NodeState::Stopped);
        assert!(!state.connected.load(Ordering::Relaxed));
        assert!(node.kernel().trader().borrow().is_stopped());
        assert_eq!(node.kernel().trader().borrow().component_count(), 2);

        node.dispose();

        assert!(node.kernel().trader().borrow().is_disposed());
        assert_eq!(node.kernel().trader().borrow().component_count(), 0);
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    #[tokio::test]
    async fn test_strategy_stop_request_during_start_aborts_running_transition(#[case] run: bool) {
        let config = LiveNodeConfig {
            exec_engine: LiveExecutionEngineConfig {
                reconciliation: false,
                ..Default::default()
            },
            delay_post_stop: Duration::from_millis(10),
            timeout_disconnection: Duration::from_millis(50),
            ..Default::default()
        };
        let (mut node, state) = live_node_with_startup_mass_status_client(
            "StrategyStopDuringStartNode",
            config,
            StartupMassStatusBehavior::Unavailable,
        );
        let handle = node.handle();
        let strategy_id = StrategyId::from("STOP-ON-START-001");
        let instrument = crypto_perpetual_ethusdt();
        let instrument_id = instrument.id();
        let account_id = AccountId::from("STARTUP-MASS-STATUS-001");
        let client_id = ClientId::from(StartupMassStatusExecutionClient::CLIENT_ID);
        let stop_count = Arc::new(AtomicUsize::new(0));

        node.kernel()
            .cache
            .borrow_mut()
            .add_instrument(InstrumentAny::CryptoPerpetual(instrument))
            .unwrap();
        let order = OrderTestBuilder::new(OrderType::Limit)
            .strategy_id(strategy_id)
            .instrument_id(instrument_id)
            .quantity(Quantity::from("1.0"))
            .price(Price::from("100.0"))
            .build();
        let submitted = TestOrderEventStubs::submitted(&order, account_id);
        node.kernel()
            .cache
            .borrow_mut()
            .add_order(order, None, Some(client_id), false)
            .unwrap();
        let order = node
            .kernel()
            .cache
            .borrow_mut()
            .update_order(&submitted)
            .unwrap();
        let accepted =
            TestOrderEventStubs::accepted(&order, account_id, VenueOrderId::from("V-STOP-001"));
        node.kernel()
            .cache
            .borrow_mut()
            .update_order(&accepted)
            .unwrap();

        node.add_strategy(StopOnStartStrategy::new(
            StrategyConfig {
                strategy_id: Some(strategy_id),
                ..Default::default()
            },
            handle.clone(),
            instrument_id,
            stop_count.clone(),
        ))
        .unwrap();

        let result = if run {
            node.run().await
        } else {
            node.start().await
        };

        assert!(result.is_ok(), "unexpected error: {result:#?}");
        assert_eq!(handle.state(), NodeState::Stopped);
        assert!(handle.should_stop());
        assert!(!state.connected.load(Ordering::Relaxed));
        assert!(node.kernel().trader().borrow().is_stopped());
        assert_eq!(stop_count.load(Ordering::Relaxed), 1);
        assert_eq!(state.cancel_orders_received.load(Ordering::Relaxed), 1);
        assert!(state.cancel_orders_while_connected.load(Ordering::Relaxed));

        node.dispose();

        assert!(node.kernel().trader().borrow().is_disposed());
        assert_eq!(node.kernel().trader().borrow().component_count(), 0);
    }

    #[rstest]
    #[tokio::test]
    async fn test_data_disconnect_failure_still_attempts_execution_disconnect() {
        let config = LiveNodeConfig {
            exec_engine: LiveExecutionEngineConfig {
                reconciliation: false,
                ..Default::default()
            },
            ..Default::default()
        };
        let data_state = FailingDisconnectDataClientState::default();
        let exec_state = StartupMassStatusClientState::default();
        let mut node = LiveNodeBuilder::from_config(config)
            .unwrap()
            .with_name("DisconnectFailureNode")
            .add_data_client(
                Some("failing-disconnect-data".to_string()),
                Box::new(FailingDisconnectDataClientFactory::new(data_state.clone())),
                Box::new(FailingDisconnectDataClientConfig),
            )
            .unwrap()
            .add_exec_client(
                Some("startup-mass-status".to_string()),
                Box::new(StartupMassStatusExecutionClientFactory::new(
                    exec_state.clone(),
                    StartupMassStatusBehavior::Unavailable,
                )),
                Box::new(StartupMassStatusExecutionClientConfig),
            )
            .unwrap()
            .build()
            .unwrap();

        let err = node
            .kernel_mut()
            .disconnect_clients()
            .await
            .expect_err("data client disconnect should fail");
        node.dispose();

        assert!(
            err.to_string()
                .contains("simulated data client disconnect failure"),
            "unexpected error: {err:#}"
        );
        assert!(data_state.disconnect_attempted.load(Ordering::Relaxed));
        assert!(exec_state.disconnect_attempted.load(Ordering::Relaxed));
    }

    #[rstest]
    #[tokio::test(flavor = "current_thread")]
    async fn test_run_continues_when_mass_status_unavailable() {
        let config = LiveNodeConfig {
            exec_engine: LiveExecutionEngineConfig {
                reconciliation: true,
                ..Default::default()
            },
            delay_post_stop: Duration::ZERO,
            timeout_disconnection: Duration::from_millis(50),
            ..Default::default()
        };
        let (mut node, state) = live_node_with_startup_mass_status_client(
            "RunStartupMassStatusUnavailableNode",
            config,
            StartupMassStatusBehavior::Unavailable,
        );
        let handle = node.handle();
        let stop_handle = handle.clone();

        tokio::spawn(async move {
            wait_until_async(
                || async { stop_handle.is_running() },
                Duration::from_secs(5),
            )
            .await;
            stop_handle.stop();
        });

        let result = node.run().await;

        assert!(result.is_ok(), "unexpected error: {result:#?}");
        assert!(state.mass_status_requested.load(Ordering::Relaxed));
        assert_eq!(handle.state(), NodeState::Stopped);
        assert!(!state.connected.load(Ordering::Relaxed));
    }

    #[rstest]
    #[tokio::test]
    async fn test_start_aborts_startup_when_mass_status_errors() {
        let config = LiveNodeConfig {
            exec_engine: LiveExecutionEngineConfig {
                reconciliation: true,
                ..Default::default()
            },
            timeout_disconnection: Duration::from_millis(50),
            ..Default::default()
        };
        let (mut node, state) = live_node_with_startup_mass_status_client(
            "StartStartupMassStatusErrorNode",
            config,
            StartupMassStatusBehavior::Error,
        );
        let handle = node.handle();

        let err = node.start().await.expect_err("start should fail");
        let err = format!("{err:#}");

        assert!(
            err.contains("Failed to get mass status from") && err.contains("mass status failed"),
            "unexpected error: {err}"
        );
        assert!(state.mass_status_requested.load(Ordering::Relaxed));
        assert_eq!(handle.state(), NodeState::Stopped);
        assert!(!state.connected.load(Ordering::Relaxed));
    }

    #[rstest]
    #[tokio::test(flavor = "current_thread")]
    async fn test_run_aborts_startup_when_mass_status_errors() {
        let config = LiveNodeConfig {
            exec_engine: LiveExecutionEngineConfig {
                reconciliation: true,
                ..Default::default()
            },
            timeout_disconnection: Duration::from_millis(50),
            ..Default::default()
        };
        let (mut node, state) = live_node_with_startup_mass_status_client(
            "StartupMassStatusErrorNode",
            config,
            StartupMassStatusBehavior::Error,
        );
        let handle = node.handle();

        let err = node.run().await.expect_err("run should fail");
        let err = format!("{err:#}");

        assert!(
            err.contains("Failed to get mass status from") && err.contains("mass status failed"),
            "unexpected error: {err}"
        );
        assert!(state.mass_status_requested.load(Ordering::Relaxed));
        assert_eq!(handle.state(), NodeState::Stopped);
        assert!(!state.connected.load(Ordering::Relaxed));
    }

    #[rstest]
    #[cfg_attr(
        not(all(feature = "simulation", madsim)),
        tokio::test(flavor = "current_thread")
    )]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_startup_reconciliation_times_out_waiting_for_mass_status() {
        let config = LiveNodeConfig {
            exec_engine: LiveExecutionEngineConfig {
                reconciliation: true,
                ..Default::default()
            },
            timeout_reconciliation: Duration::from_millis(50),
            timeout_disconnection: Duration::from_millis(50),
            ..Default::default()
        };
        let (mut node, state) = live_node_with_startup_mass_status_client(
            "StartupMassStatusTimeoutNode",
            config,
            StartupMassStatusBehavior::Pending,
        );
        let handle = node.handle();

        let result = dst::time::timeout(Duration::from_secs(1), node.run()).await;

        assert!(
            result.is_ok(),
            "startup reconciliation timeout should fire before the test timeout"
        );
        let err = result
            .unwrap()
            .expect_err("run should fail on startup reconciliation timeout");
        let err = format!("{err:#}");
        assert!(
            err.contains("Startup reconciliation timeout reached"),
            "unexpected error: {err}"
        );
        assert!(state.mass_status_requested.load(Ordering::Relaxed));
        assert_eq!(handle.state(), NodeState::Stopped);
        assert!(!state.connected.load(Ordering::Relaxed));
    }

    // The maintenance dispatcher is a single `select!` arm in `LiveNode::run`
    // that fires up to six periodic tasks. With reconciliation disabled, the
    // only sub-second-cadenced task that can fire in a short test window is
    // the own-books audit (interval is `Option<f64>` seconds). Configuring it
    // at 0.1s and holding the node Running for ~250ms guarantees the
    // maintenance arm is polled multiple times and dispatches at least one
    // body. If the dispatcher panics, deadlocks the cache `borrow_mut()`, or
    // otherwise breaks the loop, `run()` will not return cleanly.
    #[rstest]
    #[tokio::test(flavor = "current_thread")]
    async fn test_maintenance_dispatcher_runs_while_running() {
        let config = LiveNodeConfig {
            exec_engine: LiveExecutionEngineConfig {
                reconciliation: false,
                own_books_audit_interval_secs: Some(0.1),
                ..Default::default()
            },
            delay_post_stop: Duration::from_millis(50),
            ..Default::default()
        };
        let mut node = LiveNode::build("MaintenanceTestNode".to_string(), Some(config)).unwrap();
        let handle = node.handle();

        let stop_handle = handle.clone();

        tokio::spawn(async move {
            wait_until_async(
                || async { stop_handle.is_running() },
                Duration::from_secs(5),
            )
            .await;
            tokio::time::sleep(Duration::from_millis(250)).await;
            stop_handle.stop();
        });

        let result = tokio::time::timeout(Duration::from_secs(5), node.run()).await;

        assert!(result.is_ok(), "run() should complete within timeout");
        assert!(
            result.unwrap().is_ok(),
            "run() should succeed after maintenance dispatcher fires"
        );
        assert_eq!(handle.state(), NodeState::Stopped);
    }

    #[rstest]
    #[tokio::test(flavor = "current_thread")]
    async fn test_queue_monitor_unset_does_not_publish() {
        let config = LiveNodeConfig {
            exec_engine: LiveExecutionEngineConfig {
                reconciliation: false,
                ..Default::default()
            },
            delay_post_stop: Duration::from_millis(50),
            ..Default::default()
        };
        let mut node = LiveNode::build("QueueMonitorUnsetNode".to_string(), Some(config)).unwrap();
        let handle = node.handle();
        let received = Rc::new(RefCell::new(Vec::<QueueStateChanged>::new()));

        let handler = ShareableMessageHandler::from_typed({
            let received = received.clone();
            move |event: &QueueStateChanged| received.borrow_mut().push(event.clone())
        });

        msgbus::subscribe_any(
            MessagingSwitchboard::queue_state_changed_topic().into(),
            handler,
            None,
        );
        let stop_handle = handle.clone();

        tokio::spawn(async move {
            wait_until_async(
                || async { stop_handle.is_running() },
                Duration::from_secs(5),
            )
            .await;
            tokio::time::sleep(Duration::from_millis(250)).await;
            stop_handle.stop();
        });

        let result = tokio::time::timeout(Duration::from_secs(5), node.run()).await;

        assert!(result.is_ok(), "run() should complete within timeout");
        assert!(result.unwrap().is_ok(), "run() should succeed");
        assert_eq!(handle.state(), NodeState::Stopped);
        assert!(received.borrow().is_empty());
        msgbus::get_message_bus().borrow_mut().dispose();
    }

    #[rstest]
    #[tokio::test(flavor = "current_thread")]
    async fn test_continuous_reconciliation_does_not_block_on_report_generation() {
        let config = LiveNodeConfig {
            exec_engine: LiveExecutionEngineConfig {
                reconciliation: false,
                open_check_interval_secs: Some(0.1),
                ..Default::default()
            },
            delay_post_stop: Duration::from_millis(50),
            ..Default::default()
        };
        let query_order_received = Arc::new(AtomicBool::new(false));
        let blocking_order_report_requested = Arc::new(AtomicBool::new(false));
        let position_report_requested = Arc::new(AtomicBool::new(false));
        let instrument_received = Arc::new(AtomicBool::new(false));
        let mut node = live_node_with_blocking_exec_client(
            "NonBlockingReconciliationNode",
            config,
            query_order_received.clone(),
            blocking_order_report_requested.clone(),
            position_report_requested.clone(),
            instrument_received,
            None,
        );
        let handle = node.handle();

        let client_id = ClientId::from("BLOCKING-REPORT");
        let account_id = AccountId::from("BLOCKING-REPORT-001");
        let venue_order_id = VenueOrderId::from("V-NONBLOCK-001");
        let instrument = crypto_perpetual_ethusdt();
        let instrument_id = instrument.id();
        let client_order_id = ClientOrderId::from("O-NONBLOCK-001");

        node.kernel()
            .cache
            .borrow_mut()
            .add_instrument(InstrumentAny::CryptoPerpetual(instrument))
            .unwrap();
        let order = OrderTestBuilder::new(OrderType::Limit)
            .client_order_id(client_order_id)
            .instrument_id(instrument_id)
            .quantity(Quantity::from("10.0"))
            .price(Price::from("100.0"))
            .build();
        let submitted = TestOrderEventStubs::submitted(&order, account_id);
        node.kernel()
            .cache
            .borrow_mut()
            .add_order(order, None, Some(client_id), false)
            .unwrap();
        let order = node
            .kernel()
            .cache
            .borrow_mut()
            .update_order(&submitted)
            .unwrap();
        let accepted = TestOrderEventStubs::accepted(&order, account_id, venue_order_id);
        node.kernel()
            .cache
            .borrow_mut()
            .update_order(&accepted)
            .unwrap();

        let stop_handle = handle.clone();
        let order_report_observed = blocking_order_report_requested.clone();

        tokio::spawn(async move {
            wait_until_async(
                || async { stop_handle.is_running() },
                Duration::from_secs(5),
            )
            .await;
            wait_until_async(
                || async { order_report_observed.load(Ordering::Relaxed) },
                Duration::from_secs(5),
            )
            .await;
            stop_handle.stop();
        });

        let result = tokio::time::timeout(Duration::from_secs(2), node.run()).await;

        assert!(
            result.is_ok(),
            "run() should not block on report generation"
        );
        assert!(
            result.unwrap().is_ok(),
            "run() should stop cleanly after continuous reconciliation fires"
        );
        assert!(blocking_order_report_requested.load(Ordering::Relaxed));
        assert!(!query_order_received.load(Ordering::Relaxed));
        assert!(!position_report_requested.load(Ordering::Relaxed));
        assert_eq!(handle.state(), NodeState::Stopped);
    }

    #[rstest]
    #[tokio::test(flavor = "current_thread")]
    async fn test_continuous_report_reconciliation_serializes_open_and_position_requests() {
        let config = LiveNodeConfig {
            exec_engine: LiveExecutionEngineConfig {
                reconciliation: false,
                inflight_check_interval_ms: 0,
                open_check_interval_secs: Some(0.1),
                position_check_interval_secs: Some(0.1),
                ..Default::default()
            },
            delay_post_stop: Duration::from_millis(50),
            ..Default::default()
        };
        let query_order_received = Arc::new(AtomicBool::new(false));
        let blocking_order_report_requested = Arc::new(AtomicBool::new(false));
        let position_report_requested = Arc::new(AtomicBool::new(false));
        let instrument_received = Arc::new(AtomicBool::new(false));
        let mut node = live_node_with_blocking_exec_client(
            "SerializedReportReconciliationNode",
            config,
            query_order_received.clone(),
            blocking_order_report_requested.clone(),
            position_report_requested.clone(),
            instrument_received,
            None,
        );
        let handle = node.handle();

        let stop_handle = handle.clone();
        let order_report_observed = blocking_order_report_requested.clone();

        tokio::spawn(async move {
            wait_until_async(
                || async { stop_handle.is_running() },
                Duration::from_secs(5),
            )
            .await;
            wait_until_async(
                || async { order_report_observed.load(Ordering::Relaxed) },
                Duration::from_secs(5),
            )
            .await;
            tokio::time::sleep(Duration::from_millis(250)).await;
            stop_handle.stop();
        });

        let result = tokio::time::timeout(Duration::from_secs(2), node.run()).await;

        assert!(
            result.is_ok(),
            "run() should not block while a report request is pending"
        );
        assert!(
            result.unwrap().is_ok(),
            "run() should stop cleanly after serializing report reconciliation"
        );
        assert!(blocking_order_report_requested.load(Ordering::Relaxed));
        assert!(!position_report_requested.load(Ordering::Relaxed));
        assert!(!query_order_received.load(Ordering::Relaxed));
        assert_eq!(handle.state(), NodeState::Stopped);
    }

    #[rstest]
    #[tokio::test(flavor = "current_thread")]
    async fn test_continuous_report_reconciliation_runs_position_after_open_completes() {
        let config = LiveNodeConfig {
            exec_engine: LiveExecutionEngineConfig {
                reconciliation: false,
                inflight_check_interval_ms: 0,
                open_check_interval_secs: Some(0.1),
                position_check_interval_secs: Some(0.1),
                ..Default::default()
            },
            delay_post_stop: Duration::from_millis(50),
            ..Default::default()
        };
        let query_order_received = Arc::new(AtomicBool::new(false));
        let blocking_order_report_requested = Arc::new(AtomicBool::new(false));
        let position_report_requested = Arc::new(AtomicBool::new(false));
        let instrument_received = Arc::new(AtomicBool::new(false));
        let report_release = Arc::new(tokio::sync::Notify::new());
        let mut node = live_node_with_blocking_exec_client(
            "AlternatingReportReconciliationNode",
            config,
            query_order_received.clone(),
            blocking_order_report_requested.clone(),
            position_report_requested.clone(),
            instrument_received,
            Some(report_release.clone()),
        );
        let handle = node.handle();

        let stop_handle = handle.clone();
        let order_report_observed = blocking_order_report_requested.clone();
        let position_report_observed = position_report_requested.clone();

        tokio::spawn(async move {
            wait_until_async(
                || async { stop_handle.is_running() },
                Duration::from_secs(5),
            )
            .await;
            wait_until_async(
                || async { order_report_observed.load(Ordering::Relaxed) },
                Duration::from_secs(5),
            )
            .await;
            report_release.notify_one();
            wait_until_async(
                || async { position_report_observed.load(Ordering::Relaxed) },
                Duration::from_secs(5),
            )
            .await;
            stop_handle.stop();
        });

        let result = tokio::time::timeout(Duration::from_secs(2), node.run()).await;

        assert!(
            result.is_ok(),
            "run() should not block when alternating report reconciliation checks"
        );
        assert!(
            result.unwrap().is_ok(),
            "run() should stop cleanly after the position report request fires"
        );
        assert!(blocking_order_report_requested.load(Ordering::Relaxed));
        assert!(position_report_requested.load(Ordering::Relaxed));
        assert!(!query_order_received.load(Ordering::Relaxed));
        assert_eq!(handle.state(), NodeState::Stopped);
    }

    #[rstest]
    #[tokio::test(flavor = "current_thread")]
    async fn test_instrument_update_during_open_order_report_does_not_panic() {
        let config = LiveNodeConfig {
            exec_engine: LiveExecutionEngineConfig {
                reconciliation: false,
                open_check_interval_secs: Some(0.1),
                ..Default::default()
            },
            delay_post_stop: Duration::from_millis(50),
            ..Default::default()
        };
        let query_order_received = Arc::new(AtomicBool::new(false));
        let blocking_order_report_requested = Arc::new(AtomicBool::new(false));
        let position_report_requested = Arc::new(AtomicBool::new(false));
        let instrument_received = Arc::new(AtomicBool::new(false));
        let order_report_release = Arc::new(tokio::sync::Notify::new());
        let mut node = live_node_with_blocking_exec_client(
            "InstrumentUpdateDuringReportNode",
            config,
            query_order_received.clone(),
            blocking_order_report_requested.clone(),
            position_report_requested.clone(),
            instrument_received.clone(),
            Some(order_report_release.clone()),
        );
        let handle = node.handle();

        let client_id = ClientId::from("BLOCKING-REPORT");
        let account_id = AccountId::from("BLOCKING-REPORT-001");
        let venue_order_id = VenueOrderId::from("V-INST-001");
        let instrument = crypto_perpetual_ethusdt();
        let instrument_id = instrument.id();
        let client_order_id = ClientOrderId::from("O-INST-001");

        node.kernel()
            .cache
            .borrow_mut()
            .add_instrument(InstrumentAny::CryptoPerpetual(instrument))
            .unwrap();
        let order = OrderTestBuilder::new(OrderType::Limit)
            .client_order_id(client_order_id)
            .instrument_id(instrument_id)
            .quantity(Quantity::from("10.0"))
            .price(Price::from("100.0"))
            .build();
        let submitted = TestOrderEventStubs::submitted(&order, account_id);
        node.kernel()
            .cache
            .borrow_mut()
            .add_order(order, None, Some(client_id), false)
            .unwrap();
        let order = node
            .kernel()
            .cache
            .borrow_mut()
            .update_order(&submitted)
            .unwrap();
        let accepted = TestOrderEventStubs::accepted(&order, account_id, venue_order_id);
        node.kernel()
            .cache
            .borrow_mut()
            .update_order(&accepted)
            .unwrap();

        let stop_handle = handle.clone();
        let order_report_observed = blocking_order_report_requested.clone();
        let instrument_observed = instrument_received.clone();

        tokio::spawn(async move {
            wait_until_async(
                || async { stop_handle.is_running() },
                Duration::from_secs(5),
            )
            .await;
            wait_until_async(
                || async { order_report_observed.load(Ordering::Relaxed) },
                Duration::from_secs(5),
            )
            .await;

            let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
            let topic = switchboard::get_instrument_topic(instrument.id());
            msgbus::publish_instrument(topic, &instrument);
            order_report_release.notify_one();

            wait_until_async(
                || async { instrument_observed.load(Ordering::Relaxed) },
                Duration::from_secs(5),
            )
            .await;
            stop_handle.stop();
        });

        let result = tokio::time::timeout(Duration::from_secs(3), node.run()).await;

        assert!(
            result.is_ok(),
            "run() should not panic when an instrument update arrives during report generation"
        );
        assert!(
            result.unwrap().is_ok(),
            "run() should stop cleanly after flushing deferred instrument updates"
        );
        assert!(blocking_order_report_requested.load(Ordering::Relaxed));
        assert!(instrument_received.load(Ordering::Relaxed));
        assert!(!query_order_received.load(Ordering::Relaxed));
        assert!(!position_report_requested.load(Ordering::Relaxed));
        assert_eq!(handle.state(), NodeState::Stopped);
    }

    #[rstest]
    #[tokio::test(flavor = "current_thread")]
    async fn test_instrument_update_during_position_report_does_not_panic() {
        let config = LiveNodeConfig {
            exec_engine: LiveExecutionEngineConfig {
                reconciliation: false,
                inflight_check_interval_ms: 0,
                position_check_interval_secs: Some(0.1),
                ..Default::default()
            },
            delay_post_stop: Duration::from_millis(50),
            ..Default::default()
        };
        let query_order_received = Arc::new(AtomicBool::new(false));
        let blocking_order_report_requested = Arc::new(AtomicBool::new(false));
        let position_report_requested = Arc::new(AtomicBool::new(false));
        let instrument_received = Arc::new(AtomicBool::new(false));
        let position_report_release = Arc::new(tokio::sync::Notify::new());
        let mut node = live_node_with_blocking_exec_client(
            "InstrumentUpdateDuringPositionReportNode",
            config,
            query_order_received.clone(),
            blocking_order_report_requested.clone(),
            position_report_requested.clone(),
            instrument_received.clone(),
            Some(position_report_release.clone()),
        );
        let handle = node.handle();

        let stop_handle = handle.clone();
        let position_report_observed = position_report_requested.clone();
        let instrument_observed = instrument_received.clone();

        tokio::spawn(async move {
            wait_until_async(
                || async { stop_handle.is_running() },
                Duration::from_secs(5),
            )
            .await;
            wait_until_async(
                || async { position_report_observed.load(Ordering::Relaxed) },
                Duration::from_secs(5),
            )
            .await;

            let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
            let topic = switchboard::get_instrument_topic(instrument.id());
            msgbus::publish_instrument(topic, &instrument);
            position_report_release.notify_one();

            wait_until_async(
                || async { instrument_observed.load(Ordering::Relaxed) },
                Duration::from_secs(5),
            )
            .await;
            stop_handle.stop();
        });

        let result = tokio::time::timeout(Duration::from_secs(3), node.run()).await;

        assert!(
            result.is_ok(),
            "run() should not panic when an instrument update arrives during position reports"
        );
        assert!(
            result.unwrap().is_ok(),
            "run() should stop cleanly after flushing deferred instrument updates"
        );
        assert!(position_report_requested.load(Ordering::Relaxed));
        assert!(instrument_received.load(Ordering::Relaxed));
        assert!(!query_order_received.load(Ordering::Relaxed));
        assert!(!blocking_order_report_requested.load(Ordering::Relaxed));
        assert_eq!(handle.state(), NodeState::Stopped);
    }

    #[rstest]
    #[tokio::test(flavor = "current_thread")]
    async fn test_position_only_continuous_reconciliation_requests_reports() {
        let config = LiveNodeConfig {
            exec_engine: LiveExecutionEngineConfig {
                reconciliation: false,
                inflight_check_interval_ms: 0,
                position_check_interval_secs: Some(0.1),
                ..Default::default()
            },
            delay_post_stop: Duration::from_millis(50),
            ..Default::default()
        };
        let query_order_received = Arc::new(AtomicBool::new(false));
        let blocking_order_report_requested = Arc::new(AtomicBool::new(false));
        let position_report_requested = Arc::new(AtomicBool::new(false));
        let instrument_received = Arc::new(AtomicBool::new(false));
        let mut node = live_node_with_blocking_exec_client(
            "PositionOnlyReconciliationNode",
            config,
            query_order_received.clone(),
            blocking_order_report_requested.clone(),
            position_report_requested.clone(),
            instrument_received,
            None,
        );
        let handle = node.handle();

        let stop_handle = handle.clone();
        let position_report_observed = position_report_requested.clone();

        tokio::spawn(async move {
            wait_until_async(
                || async { stop_handle.is_running() },
                Duration::from_secs(5),
            )
            .await;
            wait_until_async(
                || async { position_report_observed.load(Ordering::Relaxed) },
                Duration::from_secs(5),
            )
            .await;
            stop_handle.stop();
        });

        let result = tokio::time::timeout(Duration::from_secs(2), node.run()).await;

        assert!(
            result.is_ok(),
            "run() should not block when only position reconciliation is configured"
        );
        assert!(
            result.unwrap().is_ok(),
            "run() should stop cleanly after requesting position reports"
        );
        assert!(!query_order_received.load(Ordering::Relaxed));
        assert!(!blocking_order_report_requested.load(Ordering::Relaxed));
        assert!(position_report_requested.load(Ordering::Relaxed));
        assert_eq!(handle.state(), NodeState::Stopped);
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn test_hung_open_report_task_times_out_and_position_check_starts() {
        let config = LiveNodeConfig {
            exec_engine: LiveExecutionEngineConfig {
                reconciliation: false,
                inflight_check_interval_ms: 0,
                open_check_interval_secs: Some(0.1),
                position_check_interval_secs: Some(0.1),
                ..Default::default()
            },
            timeout_reconciliation: Duration::from_millis(250),
            delay_post_stop: Duration::ZERO,
            ..Default::default()
        };
        let state = BlockingReportClientState::default();
        let factory = BlockingReportExecutionClientFactory::configurable(
            ClientId::from("BLOCKING-REPORT"),
            AccountId::from("BLOCKING-REPORT-001"),
            state.clone(),
        );
        let mut node = LiveNodeBuilder::from_config(config)
            .unwrap()
            .with_name("OpenReportTimeoutNode")
            .add_exec_client(
                Some("blocking-report".to_string()),
                Box::new(factory),
                Box::new(BlockingReportExecutionClientConfig),
            )
            .unwrap()
            .build()
            .unwrap();
        let handle = node.handle();
        let stop_handle = handle.clone();
        let position_report_requested = state.position_report_requested.clone();

        tokio::spawn(async move {
            wait_until_async(
                || async { position_report_requested.load(Ordering::Relaxed) },
                Duration::from_secs(1),
            )
            .await;
            stop_handle.stop();
        });

        let result = tokio::time::timeout(Duration::from_secs(2), node.run()).await;

        assert!(
            result.is_ok(),
            "hung open report task should release its slot"
        );
        assert!(result.unwrap().is_ok());
        assert!(state.bulk_order_report_requested.load(Ordering::Relaxed));
        assert!(state.position_report_requested.load(Ordering::Relaxed));
        assert_eq!(handle.state(), NodeState::Stopped);
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn test_hung_position_report_task_times_out_and_open_check_starts() {
        let config = LiveNodeConfig {
            exec_engine: LiveExecutionEngineConfig {
                reconciliation: false,
                inflight_check_interval_ms: 0,
                open_check_interval_secs: Some(0.2),
                position_check_interval_secs: Some(0.1),
                ..Default::default()
            },
            timeout_reconciliation: Duration::from_millis(250),
            delay_post_stop: Duration::ZERO,
            ..Default::default()
        };
        let state = BlockingReportClientState::default();
        let factory = BlockingReportExecutionClientFactory::configurable(
            ClientId::from("BLOCKING-REPORT"),
            AccountId::from("BLOCKING-REPORT-001"),
            state.clone(),
        );
        let mut node = LiveNodeBuilder::from_config(config)
            .unwrap()
            .with_name("PositionReportTimeoutNode")
            .add_exec_client(
                Some("blocking-report".to_string()),
                Box::new(factory),
                Box::new(BlockingReportExecutionClientConfig),
            )
            .unwrap()
            .build()
            .unwrap();
        let handle = node.handle();
        let stop_handle = handle.clone();
        let bulk_order_report_requested = state.bulk_order_report_requested.clone();

        tokio::spawn(async move {
            wait_until_async(
                || async { bulk_order_report_requested.load(Ordering::Relaxed) },
                Duration::from_secs(1),
            )
            .await;
            stop_handle.stop();
        });

        let result = tokio::time::timeout(Duration::from_secs(2), node.run()).await;

        assert!(
            result.is_ok(),
            "hung position report task should release its slot"
        );
        assert!(result.unwrap().is_ok());
        assert!(state.position_report_requested.load(Ordering::Relaxed));
        assert!(state.bulk_order_report_requested.load(Ordering::Relaxed));
        assert_eq!(handle.state(), NodeState::Stopped);
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn test_hung_targeted_report_task_cleans_markers_and_checks_resume() {
        let config = LiveNodeConfig {
            exec_engine: LiveExecutionEngineConfig {
                reconciliation: false,
                inflight_check_interval_ms: 200,
                inflight_check_threshold_ms: 0,
                inflight_check_retries: 100,
                open_check_interval_secs: Some(0.1),
                open_check_lookback_mins: None,
                open_check_threshold_ms: 0,
                open_check_missing_retries: 1,
                open_check_open_only: false,
                max_single_order_queries_per_cycle: 2,
                single_order_query_delay_ms: 0,
                position_check_interval_secs: Some(0.1),
                ..Default::default()
            },
            timeout_reconciliation: Duration::from_millis(250),
            delay_post_stop: Duration::ZERO,
            ..Default::default()
        };
        let state = BlockingReportClientState::default();
        let factory = BlockingReportExecutionClientFactory::configurable(
            ClientId::from("BLOCKING-REPORT"),
            AccountId::from("BLOCKING-REPORT-001"),
            state.clone(),
        )
        .with_order_reports(Vec::new())
        .with_position_reports_complete()
        .with_block_every_second_targeted_report();
        let mut node = LiveNodeBuilder::from_config(config)
            .unwrap()
            .with_name("TargetedReportTimeoutNode")
            .add_exec_client(
                Some("blocking-report".to_string()),
                Box::new(factory),
                Box::new(BlockingReportExecutionClientConfig),
            )
            .unwrap()
            .build()
            .unwrap();
        node.kernel()
            .cache
            .borrow_mut()
            .add_instrument(InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt()))
            .unwrap();
        let client_id = ClientId::from("BLOCKING-REPORT");
        let order_a = ClientOrderId::from("O-TARGETED-A");
        let order_b = ClientOrderId::from("O-TARGETED-B");
        let order_c = ClientOrderId::from("O-TARGETED-C");
        add_accepted_test_order(
            &node,
            order_a,
            VenueOrderId::from("V-TARGETED-A"),
            client_id,
        );
        add_accepted_test_order(
            &node,
            order_b,
            VenueOrderId::from("V-TARGETED-B"),
            client_id,
        );
        add_accepted_test_order(
            &node,
            order_c,
            VenueOrderId::from("V-TARGETED-C"),
            client_id,
        );
        node.exec_manager_mut().register_inflight(order_a);
        assert_eq!(
            node.kernel()
                .cache
                .borrow()
                .orders_open(None, None, None, None, None)
                .len(),
            3,
        );
        let handle = node.handle();
        let stop_handle = handle.clone();
        let targeted_order_report_ids = state.targeted_order_report_ids.clone();
        let query_order_received = state.query_order_received.clone();
        let query_order_ids = state.query_order_ids.clone();
        let position_report_requested = state.position_report_requested.clone();

        tokio::spawn(async move {
            wait_until_async(
                || async { targeted_order_report_ids.lock().len() >= 2 },
                Duration::from_secs(1),
            )
            .await;
            query_order_received.store(false, Ordering::Relaxed);
            query_order_ids.lock().clear();
            wait_until_async(
                || async {
                    query_order_received.load(Ordering::Relaxed)
                        && position_report_requested.load(Ordering::Relaxed)
                        && targeted_order_report_ids.lock().len() >= 5
                },
                Duration::from_secs(2),
            )
            .await;
            stop_handle.stop();
        });

        let result = tokio::time::timeout(Duration::from_secs(3), node.run()).await;
        let observed_targeted_ids = state.targeted_order_report_ids.lock().clone();

        assert!(
            result.is_ok(),
            "hung targeted report tasks should release their slot: bulk={}, targeted={observed_targeted_ids:?}, position={}, inflight={}, open={}, retries=[{}, {}, {}]",
            state.bulk_order_report_count.load(Ordering::Relaxed),
            state.position_report_count.load(Ordering::Relaxed),
            state.query_order_received.load(Ordering::Relaxed),
            node.kernel()
                .cache
                .borrow()
                .orders_open(None, None, None, None, None)
                .len(),
            node.exec_manager().recon_check_retry_count(&order_a),
            node.exec_manager().recon_check_retry_count(&order_b),
            node.exec_manager().recon_check_retry_count(&order_c),
        );
        assert!(result.unwrap().is_ok());
        assert!(state.query_order_received.load(Ordering::Relaxed));
        assert!(
            state.query_order_ids.lock().contains(&order_a),
            "check_inflight_orders should query the planned order after its marker times out"
        );
        assert!(state.position_report_requested.load(Ordering::Relaxed));
        let targeted_ids = observed_targeted_ids;
        assert_eq!(
            &targeted_ids[..5],
            &[order_a, order_b, order_c, order_b, order_b],
            "all planned markers should clear while query recency remains"
        );
        assert!(node.exec_manager().recon_check_retry_count(&order_a) > 0);
        assert!(node.exec_manager().recon_check_retry_count(&order_b) > 0);
        assert!(node.exec_manager().recon_check_retry_count(&order_c) > 0);
        assert_eq!(handle.state(), NodeState::Stopped);
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn test_timed_out_open_report_task_discards_earlier_client_reports() {
        let config = LiveNodeConfig {
            exec_engine: LiveExecutionEngineConfig {
                reconciliation: false,
                inflight_check_interval_ms: 0,
                open_check_interval_secs: Some(0.1),
                position_check_interval_secs: Some(0.1),
                ..Default::default()
            },
            timeout_reconciliation: Duration::from_millis(50),
            delay_post_stop: Duration::ZERO,
            ..Default::default()
        };
        let client_order_id = ClientOrderId::from("O-PARTIAL-DISCARD");
        let venue_order_id = VenueOrderId::from("V-PARTIAL-DISCARD");
        let partial_state = BlockingReportClientState::default();
        let responsive_factory = BlockingReportExecutionClientFactory::configurable(
            ClientId::from("RESPONSIVE-REPORT"),
            AccountId::from("BLOCKING-REPORT-001"),
            partial_state.clone(),
        )
        .with_order_reports(vec![canceled_order_report(client_order_id, venue_order_id)])
        .with_position_reports_complete()
        .with_block_every_second_order_report();
        let blocking_factory = BlockingReportExecutionClientFactory::configurable(
            ClientId::from("BLOCKING-REPORT"),
            AccountId::from("BLOCKING-REPORT-001"),
            partial_state.clone(),
        )
        .with_venue(Venue::from("ROUTING"))
        .with_order_reports(vec![canceled_order_report(client_order_id, venue_order_id)])
        .with_position_reports_complete()
        .with_block_every_second_order_report();
        let mut node = LiveNodeBuilder::from_config(config)
            .unwrap()
            .with_name("PartialReportDiscardNode")
            .add_exec_client(
                Some("responsive-report".to_string()),
                Box::new(responsive_factory),
                Box::new(BlockingReportExecutionClientConfig),
            )
            .unwrap()
            .add_exec_client(
                Some("blocking-report".to_string()),
                Box::new(blocking_factory),
                Box::new(BlockingReportExecutionClientConfig),
            )
            .unwrap()
            .build()
            .unwrap();
        node.kernel()
            .cache
            .borrow_mut()
            .add_instrument(InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt()))
            .unwrap();
        add_accepted_test_order(
            &node,
            client_order_id,
            venue_order_id,
            ClientId::from("RESPONSIVE-REPORT"),
        );
        let handle = node.handle();
        let stop_handle = handle.clone();
        let position_report_requested = partial_state.position_report_requested.clone();

        tokio::spawn(async move {
            wait_until_async(
                || async { position_report_requested.load(Ordering::Relaxed) },
                Duration::from_secs(1),
            )
            .await;
            stop_handle.stop();
        });

        let result = tokio::time::timeout(Duration::from_secs(2), node.run()).await;

        assert!(result.is_ok(), "timed-out batch should release its slot");
        assert!(result.unwrap().is_ok());
        assert!(
            partial_state
                .bulk_order_report_requested
                .load(Ordering::Relaxed)
        );
        assert_eq!(
            partial_state
                .bulk_order_report_count
                .load(Ordering::Relaxed),
            2,
            "one client should report before the later client hangs"
        );
        assert_eq!(
            node.kernel()
                .cache
                .borrow()
                .order(&client_order_id)
                .unwrap()
                .status(),
            OrderStatus::Accepted,
            "reports collected before the expiry should not be reconciled"
        );
        assert_eq!(handle.state(), NodeState::Stopped);
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn test_timed_out_report_task_flushes_deferred_instrument_update() {
        let config = LiveNodeConfig {
            exec_engine: LiveExecutionEngineConfig {
                reconciliation: false,
                inflight_check_interval_ms: 0,
                open_check_interval_secs: Some(0.1),
                ..Default::default()
            },
            timeout_reconciliation: Duration::from_millis(50),
            delay_post_stop: Duration::ZERO,
            ..Default::default()
        };
        let state = BlockingReportClientState::default();
        let factory = BlockingReportExecutionClientFactory::configurable(
            ClientId::from("BLOCKING-REPORT"),
            AccountId::from("BLOCKING-REPORT-001"),
            state.clone(),
        );
        let mut node = LiveNodeBuilder::from_config(config)
            .unwrap()
            .with_name("ReportTimeoutInstrumentFlushNode")
            .add_exec_client(
                Some("blocking-report".to_string()),
                Box::new(factory),
                Box::new(BlockingReportExecutionClientConfig),
            )
            .unwrap()
            .build()
            .unwrap();
        let handle = node.handle();
        let stop_handle = handle.clone();
        let bulk_order_report_requested = state.bulk_order_report_requested.clone();
        let instrument_received = state.instrument_received.clone();

        tokio::spawn(async move {
            wait_until_async(
                || async { bulk_order_report_requested.load(Ordering::Relaxed) },
                Duration::from_secs(1),
            )
            .await;
            let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
            let topic = switchboard::get_instrument_topic(instrument.id());
            msgbus::publish_instrument(topic, &instrument);
            wait_until_async(
                || async { instrument_received.load(Ordering::Relaxed) },
                Duration::from_secs(1),
            )
            .await;
            stop_handle.stop();
        });

        let result = tokio::time::timeout(Duration::from_secs(2), node.run()).await;

        assert!(
            result.is_ok(),
            "timeout cancellation should flush deferred instruments"
        );
        assert!(result.unwrap().is_ok());
        assert!(state.bulk_order_report_requested.load(Ordering::Relaxed));
        assert!(state.instrument_received.load(Ordering::Relaxed));
        assert_eq!(handle.state(), NodeState::Stopped);
    }
}
