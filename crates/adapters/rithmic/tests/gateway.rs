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
//! These tests cover both disconnected guard rails and deterministic connected-path
//! mock transport behavior for the Rithmic gateway's public async API.

mod common;

use nautilus_rithmic::{
    PnlEvent, TimeBarType,
    common::enums::ConnectionState,
    execution::ExecutionEvent,
    providers::{AccountEvent, PositionEvent},
};
use tokio::time::{Duration, timeout};

use crate::common::{
    MockOrderPlant, MockPnlPlant, assert_connection_error, test_gateway,
    test_order_only_gateway_config, test_pnl_only_gateway_config,
};

#[tokio::test]
async fn subscribe_market_data_requires_connected_ticker_plant() {
    let gateway = test_gateway();

    let err = gateway
        .subscribe_market_data("ESM6", "CME")
        .await
        .unwrap_err();

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
        .request_bars(
            "ESM6",
            "CME",
            TimeBarType::MinuteBar,
            1,
            1_700_000_000,
            1_700_000_060,
        )
        .await
        .unwrap_err();

    assert_connection_error(err, "History plant not connected");
}

#[tokio::test]
async fn list_accounts_connected_path_authenticates_and_collects_multi_response_accounts() {
    let server = MockOrderPlant::start().await;
    let config = test_order_only_gateway_config(&server.url);
    let mut gateway = nautilus_rithmic::RithmicGateway::new(config);
    let mut rx = gateway.take_execution_receiver().unwrap();

    gateway.connect().await.unwrap();
    assert!(gateway.is_connected());

    expect_execution_authenticated(&mut rx).await;
    assert!(gateway.order_updates_available());

    let accounts = gateway.list_accounts().await.unwrap();
    assert_eq!(
        accounts,
        vec!["account-1".to_string(), "account-2".to_string()]
    );

    gateway.disconnect().await.unwrap();
    server.wait().await;
}

#[tokio::test]
async fn request_pnl_snapshot_connected_path_emits_balance_and_position_updates() {
    let server = MockPnlPlant::start().await;
    let config = test_pnl_only_gateway_config(&server.url);
    let mut gateway = nautilus_rithmic::RithmicGateway::new(config);
    let mut rx = gateway.take_pnl_receiver().unwrap();

    gateway.connect().await.unwrap();
    assert!(gateway.is_connected());

    gateway.request_pnl_snapshot().await.unwrap();
    assert_eq!(server.snapshot_requests(), 1);

    let mut balance_seen = false;
    let mut position_seen = false;

    for _ in 0..4 {
        match next_pnl_event(&mut rx).await {
            PnlEvent::Account(AccountEvent::BalanceUpdate(balance)) => {
                assert_eq!(balance.account_id, "account");
                assert_eq!(balance.currency, "USD");
                assert_eq!(balance.total, 100000.50);
                assert_eq!(balance.available, 75000.25);
                assert_eq!(balance.locked, 25000.25);
                assert_eq!(balance.unrealized_pnl, 1250.75);
                assert_eq!(balance.realized_pnl, 100.25);
                assert_eq!(balance.ts_event, 1_700_000_001_456_789_000);
                balance_seen = true;
            }
            PnlEvent::Position(PositionEvent::Updated(position)) => {
                assert_eq!(position.account_id, "account");
                assert_eq!(position.symbol, "ESM6");
                assert_eq!(position.exchange, "CME");
                assert_eq!(position.quantity, 2.0);
                assert_eq!(position.avg_price, 4500.25);
                assert_eq!(position.unrealized_pnl, 300.5);
                assert_eq!(position.realized_pnl, 25.25);
                assert_eq!(position.ts_event, 1_700_000_001_654_321_000);
                position_seen = true;
            }
            other => panic!("unexpected pnl event {other:?}"),
        }

        if balance_seen && position_seen {
            break;
        }
    }

    assert!(balance_seen, "expected balance update");
    assert!(position_seen, "expected position update");

    gateway.disconnect().await.unwrap();
    server.wait().await;
}

async fn next_execution_event(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
) -> ExecutionEvent {
    timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("timed out waiting for execution event")
        .expect("execution channel closed unexpectedly")
}

async fn expect_execution_authenticated(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
) {
    for _ in 0..4 {
        match next_execution_event(rx).await {
            ExecutionEvent::Authenticated => return,
            ExecutionEvent::ConnectionState(ConnectionState::Connecting)
            | ExecutionEvent::ConnectionState(ConnectionState::Connected) => {}
            other => panic!("expected authenticated execution event, got {other:?}"),
        }
    }

    panic!("did not observe execution authenticated event");
}

async fn next_pnl_event(rx: &mut tokio::sync::mpsc::UnboundedReceiver<PnlEvent>) -> PnlEvent {
    timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("timed out waiting for pnl event")
        .expect("pnl channel closed unexpectedly")
}
