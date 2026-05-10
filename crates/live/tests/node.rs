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

use std::time::Duration;

use nautilus_common::{enums::Environment, testing::wait_until_async};
use nautilus_live::{
    config::{LiveExecEngineConfig, LiveNodeConfig},
    node::{LiveNode, LiveNodeHandle, NodeState},
};
use nautilus_model::identifiers::TraderId;
use rstest::rstest;

// -- LiveNodeHandle tests (no global state, can run in parallel) ----------------------------------

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

// -- LiveNode builder validation tests (no global state conflicts) --------------------------------

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

    #[rstest]
    fn test_live_node_build_with_default_config() {
        let node = LiveNode::build("TestNode".to_string(), None).unwrap();

        assert_eq!(node.state(), NodeState::Idle);
        assert_eq!(node.environment(), Environment::Live);
        assert!(!node.is_running());
    }

    #[rstest]
    fn test_live_node_build_with_sandbox_environment() {
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
}
