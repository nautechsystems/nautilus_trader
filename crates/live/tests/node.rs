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
        atomic::{AtomicBool, Ordering},
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
        execution::{GenerateOrderStatusReports, GeneratePositionStatusReports, QueryOrder},
        system::ShutdownSystem,
    },
    msgbus::{self, MessagingSwitchboard, switchboard},
    nautilus_actor,
    testing::{wait_until, wait_until_async},
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_live::{
    builder::LiveNodeBuilder,
    config::{LiveExecEngineConfig, LiveNodeConfig},
    node::{LiveNode, LiveNodeHandle, NodeState},
};
use nautilus_model::{
    accounts::AccountAny,
    enums::{OmsType, OrderType},
    identifiers::{
        AccountId, ClientId, ClientOrderId, ExecAlgorithmId, InstrumentId, StrategyId, TraderId,
        Venue, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny, stubs::crypto_perpetual_ethusdt},
    orders::{OrderAny, OrderTestBuilder, stubs::TestOrderEventStubs},
    reports::{ExecutionMassStatus, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance, Price, Quantity},
};
use nautilus_trading::{
    ExecutionAlgorithmConfig, ExecutionAlgorithmCore, nautilus_execution_algorithm,
    nautilus_strategy,
    strategy::{StrategyConfig, StrategyCore},
};
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
struct ClaimingTestStrategy {
    core: StrategyCore,
    external_order_claims: Vec<InstrumentId>,
}

impl ClaimingTestStrategy {
    fn new(strategy_id: StrategyId, instrument_id: InstrumentId) -> Self {
        let external_order_claims = vec![instrument_id];
        Self {
            core: StrategyCore::new(StrategyConfig {
                strategy_id: Some(strategy_id),
                external_order_claims: Some(external_order_claims.clone()),
                ..Default::default()
            }),
            external_order_claims,
        }
    }
}

impl DataActor for ClaimingTestStrategy {}

nautilus_strategy!(ClaimingTestStrategy, {
    fn external_order_claims(&self) -> Option<Vec<InstrumentId>> {
        Some(self.external_order_claims.clone())
    }
});

#[derive(Debug)]
struct TestExecAlgorithm {
    core: ExecutionAlgorithmCore,
}

impl TestExecAlgorithm {
    fn new(config: ExecutionAlgorithmConfig) -> Self {
        Self {
            core: ExecutionAlgorithmCore::new(config),
        }
    }
}

impl DataActor for TestExecAlgorithm {}

nautilus_execution_algorithm!(TestExecAlgorithm, {
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
// Run with: cargo nextest run -p nautilus-live --test node

mod serial_tests {
    use super::*;

    #[derive(Clone, Debug, Default)]
    struct StartupMassStatusClientState {
        connected: Arc<AtomicBool>,
        disconnect_attempted: Arc<AtomicBool>,
        mass_status_requested: Arc<AtomicBool>,
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
        Unavailable,
        Error,
        Pending,
    }

    struct StartupMassStatusExecutionClient {
        state: StartupMassStatusClientState,
        behavior: StartupMassStatusBehavior,
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

        fn new(state: StartupMassStatusClientState, behavior: StartupMassStatusBehavior) -> Self {
            Self { state, behavior }
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
            Self { state, behavior }
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
            _name: &str,
            _config: &dyn ClientConfig,
            _cache: CacheView,
        ) -> anyhow::Result<Box<dyn ExecutionClient>> {
            Ok(Box::new(StartupMassStatusExecutionClient::new(
                self.state.clone(),
                self.behavior,
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

    #[async_trait(?Send)]
    impl ExecutionClient for StartupMassStatusExecutionClient {
        fn is_connected(&self) -> bool {
            self.state.connected.load(Ordering::Relaxed)
        }

        fn client_id(&self) -> ClientId {
            ClientId::from(Self::CLIENT_ID)
        }

        fn account_id(&self) -> AccountId {
            AccountId::from("STARTUP-MASS-STATUS-001")
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
                StartupMassStatusBehavior::Unavailable => Ok(None),
                StartupMassStatusBehavior::Error => Err(anyhow::anyhow!("mass status failed")),
                StartupMassStatusBehavior::Pending => {
                    std::future::pending::<anyhow::Result<Option<ExecutionMassStatus>>>().await
                }
            }
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
            exec_engine: LiveExecEngineConfig {
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

    struct BlockingReportExecutionClient {
        connected: Cell<bool>,
        query_order_received: Arc<AtomicBool>,
        blocking_order_report_requested: Arc<AtomicBool>,
        position_report_requested: Arc<AtomicBool>,
        instrument_received: Arc<AtomicBool>,
        report_release: Option<Arc<tokio::sync::Notify>>,
    }

    impl BlockingReportExecutionClient {
        fn new(
            query_order_received: Arc<AtomicBool>,
            blocking_order_report_requested: Arc<AtomicBool>,
            position_report_requested: Arc<AtomicBool>,
            instrument_received: Arc<AtomicBool>,
            report_release: Option<Arc<tokio::sync::Notify>>,
        ) -> Self {
            Self {
                connected: Cell::new(false),
                query_order_received,
                blocking_order_report_requested,
                position_report_requested,
                instrument_received,
                report_release,
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
        query_order_received: Arc<AtomicBool>,
        blocking_order_report_requested: Arc<AtomicBool>,
        position_report_requested: Arc<AtomicBool>,
        instrument_received: Arc<AtomicBool>,
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
                query_order_received,
                blocking_order_report_requested,
                position_report_requested,
                instrument_received,
                report_release,
            }
        }
    }

    impl ExecutionClientFactory for BlockingReportExecutionClientFactory {
        fn create(
            &self,
            _name: &str,
            _config: &dyn ClientConfig,
            _cache: CacheView,
        ) -> anyhow::Result<Box<dyn ExecutionClient>> {
            Ok(Box::new(BlockingReportExecutionClient::new(
                self.query_order_received.clone(),
                self.blocking_order_report_requested.clone(),
                self.position_report_requested.clone(),
                self.instrument_received.clone(),
                self.report_release.clone(),
            )))
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
            ClientId::from("BLOCKING-REPORT")
        }

        fn account_id(&self) -> AccountId {
            AccountId::from("BLOCKING-REPORT-001")
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
        ) -> anyhow::Result<()> {
            Ok(())
        }

        fn start(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn stop(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn query_order(&self, _cmd: QueryOrder) -> anyhow::Result<()> {
            self.query_order_received.store(true, Ordering::Relaxed);
            Ok(())
        }

        fn on_instrument(&mut self, _instrument: InstrumentAny) {
            self.instrument_received.store(true, Ordering::Relaxed);
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
            self.blocking_order_report_requested
                .store(true, Ordering::Relaxed);

            if let Some(release) = &self.report_release {
                release.notified().await;
                Ok(Vec::new())
            } else {
                std::future::pending::<anyhow::Result<Vec<OrderStatusReport>>>().await
            }
        }

        async fn generate_position_status_reports(
            &self,
            _cmd: &GeneratePositionStatusReports,
        ) -> anyhow::Result<Vec<PositionStatusReport>> {
            self.position_report_requested
                .store(true, Ordering::Relaxed);

            if let Some(release) = &self.report_release {
                release.notified().await;
                Ok(Vec::new())
            } else {
                std::future::pending::<anyhow::Result<Vec<PositionStatusReport>>>().await
            }
        }
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
            exec_engine: LiveExecEngineConfig {
                reconciliation: false,
                ..Default::default()
            },
            ..Default::default()
        };

        let node = LiveNode::build("TestNode".to_string(), Some(config)).unwrap();

        assert_eq!(node.state(), NodeState::Idle);
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
        let algo = TestExecAlgorithm::new(config);

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
        let algo = TestExecAlgorithm::new(config);

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
            exec_engine: LiveExecEngineConfig {
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
        let elapsed = dst::time::Instant::now() - started_at;
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
            exec_engine: LiveExecEngineConfig {
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
            exec_engine: LiveExecEngineConfig {
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
            exec_engine: LiveExecEngineConfig {
                reconciliation: false,
                ..Default::default()
            },
            delay_post_stop: Duration::from_millis(50),
            ..Default::default()
        };
        let mut node = LiveNode::build("TestNode".to_string(), Some(config)).unwrap();
        let handle = node.handle();

        // Must stop after node enters Running (stop flag is cleared on Running transition)
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
            exec_engine: LiveExecEngineConfig {
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
            exec_engine: LiveExecEngineConfig {
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
            exec_engine: LiveExecEngineConfig {
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
            exec_engine: LiveExecEngineConfig {
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
            exec_engine: LiveExecEngineConfig {
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
    async fn test_strategy_start_failure_stops_partial_start_and_disposes_resources() {
        let config = LiveNodeConfig {
            exec_engine: LiveExecEngineConfig {
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
    #[tokio::test]
    async fn test_data_disconnect_failure_still_attempts_execution_disconnect() {
        let config = LiveNodeConfig {
            exec_engine: LiveExecEngineConfig {
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
            exec_engine: LiveExecEngineConfig {
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
            exec_engine: LiveExecEngineConfig {
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
            exec_engine: LiveExecEngineConfig {
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
            exec_engine: LiveExecEngineConfig {
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
            exec_engine: LiveExecEngineConfig {
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
    async fn test_continuous_reconciliation_does_not_block_on_report_generation() {
        let config = LiveNodeConfig {
            exec_engine: LiveExecEngineConfig {
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
            exec_engine: LiveExecEngineConfig {
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
            exec_engine: LiveExecEngineConfig {
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
            exec_engine: LiveExecEngineConfig {
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
            exec_engine: LiveExecEngineConfig {
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
            exec_engine: LiveExecEngineConfig {
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
}
