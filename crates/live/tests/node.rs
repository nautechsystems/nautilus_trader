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
    cell::Cell,
    fmt::Debug,
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
    clients::ExecutionClient,
    enums::Environment,
    factories::{ClientConfig, ExecutionClientFactory},
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
    reports::{OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance, Price, Quantity},
};
#[cfg(feature = "streaming")]
use nautilus_system::config::{RotationConfig, StreamingConfig};
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

#[cfg(feature = "streaming")]
#[rstest]
fn test_builder_accepts_streaming_config() {
    let catalog_path =
        std::env::temp_dir().join(format!("nautilus-live-streaming-{}", UUID4::new()));
    std::fs::create_dir_all(&catalog_path).unwrap();

    let node = LiveNode::builder(TraderId::from("TESTER-001"), Environment::Sandbox)
        .unwrap()
        .with_streaming_config(StreamingConfig::new(
            catalog_path.to_string_lossy().to_string(),
            "file".to_string(),
            1_000,
            true,
            RotationConfig::NoRotation,
        ))
        .build()
        .unwrap();

    assert_eq!(node.environment(), Environment::Sandbox);
}

// -- LiveNode construction tests (require process isolation via nextest) --------------------------
// These tests initialize global logging state and require isolated processes.
// Run with: cargo nextest run -p nautilus-live --test node

mod serial_tests {
    use super::*;

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
    fn test_live_node_build_overrides_environment_to_live() {
        let config = LiveNodeConfig {
            environment: Environment::Sandbox,
            trader_id: TraderId::from("TESTER-001"),
            ..Default::default()
        };

        let node = LiveNode::build("TestNode".to_string(), Some(config)).unwrap();

        // Environment is overridden to Live when using build()
        assert_eq!(node.environment(), Environment::Live);
        assert_eq!(node.trader_id(), TraderId::from("TESTER-001"));
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
