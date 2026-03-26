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

//! Integration tests for the public `RithmicGateway` API.
//!
//! The current Rithmic crate does not yet expose a standalone mock plant transport like the
//! mature HTTP/WebSocket adapters. These smoke tests establish the `tests/` harness and
//! verify the public async guard rails before the deeper transport mock layer is added.

mod common;

use nautilus_rithmic::TimeBarType;

use crate::common::{assert_connection_error, test_gateway};

#[tokio::test]
async fn subscribe_market_data_requires_connected_ticker_plant() {
    let gateway = test_gateway();

    let err = gateway.subscribe_market_data("ESM6", "CME").await.unwrap_err();

    assert_connection_error(err, "Ticker plant not connected");
}

#[tokio::test]
async fn list_accounts_requires_connected_order_plant() {
    let gateway = test_gateway();

    let err = gateway.list_accounts().await.unwrap_err();

    assert_connection_error(err, "Order plant not connected");
}

#[tokio::test]
async fn request_pnl_snapshot_requires_connected_pnl_plant() {
    let gateway = test_gateway();

    let err = gateway.request_pnl_snapshot().await.unwrap_err();

    assert_connection_error(err, "PnL plant not connected");
}

#[tokio::test]
async fn request_bars_requires_connected_history_plant() {
    let gateway = test_gateway();

    let err = gateway
        .request_bars("ESM6", "CME", TimeBarType::MinuteBar, 1, 1_700_000_000, 1_700_000_060)
        .await
        .unwrap_err();

    assert_connection_error(err, "History plant not connected");
}
