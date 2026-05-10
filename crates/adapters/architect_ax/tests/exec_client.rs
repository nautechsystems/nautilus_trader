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

//! Integration tests for AxExecutionClient with mocked event channels.
//!
//! NOTE: Full integration tests require HTTP mocking for authentication.
//! The WebSocket-level tests in websocket.rs cover the orders WS connection behavior.
//! Additional tests for full execution client flow would require an HTTP mock server
//! or integration with the Architect sandbox environment.

mod common;

use nautilus_architect_ax::config::AxExecClientConfig;
use nautilus_common::{live::runner::set_exec_event_sender, messages::ExecutionEvent};
use rstest::rstest;

#[allow(dead_code)]
fn setup_exec_channel() -> tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent> {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
    set_exec_event_sender(sender);
    receiver
}

#[allow(dead_code)]
fn create_test_config() -> AxExecClientConfig {
    AxExecClientConfig {
        api_key: Some("test_api_key".to_string()),
        api_secret: Some("test_api_secret".to_string()),
        is_sandbox: true,
        ..Default::default()
    }
}

#[rstest]
#[tokio::test]
async fn test_exec_config_creation() {
    let config = create_test_config();

    assert_eq!(config.api_key, Some("test_api_key".to_string()));
    assert!(config.is_sandbox);
}

// Additional tests would require:
// 1. ExecutionClientCore setup (trader_id, account_id, cache, msgbus)
// 2. HTTP mock server for authentication
// 3. Mock orders WebSocket server (already available in common::server)
//
// The websocket.rs tests cover:
// - Orders WebSocket connection
// - Order placement/cancellation via WS
// - Open orders retrieval
