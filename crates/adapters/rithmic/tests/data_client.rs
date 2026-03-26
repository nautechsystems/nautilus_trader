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

//! Integration tests for the public `RithmicDataClient` API.
//!
//! These tests cover the first crate-level smoke harness for the Rithmic shadow port.
//! They intentionally exercise the public client and gateway API without live plants so
//! the `tests/` tree exists before the deeper mock-transport layer lands.

mod common;

use nautilus_rithmic::common::enums::ConnectionState;

use crate::common::{assert_connection_error, test_data_client};

#[tokio::test]
async fn subscribe_quotes_requires_connected_gateway_without_mutating_tracking() {
    let client = test_data_client();

    let err = client.subscribe_quotes("ESM6", "CME").await.unwrap_err();

    assert_connection_error(err, "Not connected");
    assert_eq!(client.connection_state(), ConnectionState::Disconnected);
    assert!(!client.is_connected());
    assert_eq!(client.subscription_count(), 0);
    assert!(client.subscriptions().is_empty());
    assert!(!client.is_subscribed_quotes("ESM6", "CME"));
    assert!(!client.is_subscribed_trades("ESM6", "CME"));
}

#[tokio::test]
async fn subscribe_trades_requires_connected_gateway_without_mutating_tracking() {
    let client = test_data_client();

    let err = client.subscribe_trades("ESM6", "CME").await.unwrap_err();

    assert_connection_error(err, "Not connected");
    assert_eq!(client.subscription_count(), 0);
    assert!(!client.is_subscribed_quotes("ESM6", "CME"));
    assert!(!client.is_subscribed_trades("ESM6", "CME"));
}

#[tokio::test]
async fn subscribe_combined_requires_connected_gateway_without_mutating_tracking() {
    let client = test_data_client();

    let err = client.subscribe("ESM6", "CME").await.unwrap_err();

    assert_connection_error(err, "Not connected");
    assert_eq!(client.subscription_count(), 0);
    assert!(client.subscriptions().is_empty());
}
