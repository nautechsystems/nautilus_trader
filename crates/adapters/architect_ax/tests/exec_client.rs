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

//! Integration tests for AxExecutionClient.
//!
//! These tests use the mock HTTP+WS server from `common::server` to verify
//! client creation, connection lifecycle, and account state handling.

mod common;

use std::{cell::RefCell, net::SocketAddr, rc::Rc};

use nautilus_architect_ax::{config::AxExecClientConfig, execution::AxExecutionClient};
use nautilus_common::{
    cache::Cache, clients::ExecutionClient, live::runner::set_exec_event_sender,
    messages::ExecutionEvent,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    accounts::{AccountAny, MarginAccount},
    enums::{AccountType, OmsType},
    events::AccountState,
    identifiers::{AccountId, ClientId, TraderId, Venue},
    types::{AccountBalance, Money},
};
use rstest::rstest;

use crate::common::server::start_test_server;

fn setup_exec_channel() -> tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent> {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
    set_exec_event_sender(sender);
    receiver
}

fn create_test_exec_config(addr: SocketAddr) -> AxExecClientConfig {
    AxExecClientConfig {
        api_key: Some("test_api_key".to_string()),
        api_secret: Some("test_api_secret".to_string()),
        is_sandbox: true,
        base_url_http: Some(format!("http://{addr}")),
        base_url_orders: Some(format!("http://{addr}")),
        base_url_ws_private: Some(format!("ws://{addr}/orders/ws")),
        ..Default::default()
    }
}

fn create_test_execution_client(
    addr: SocketAddr,
) -> (
    AxExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("AX-001");
    let client_id = ClientId::from("AX");

    let cache = Rc::new(RefCell::new(Cache::default()));

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        Venue::from("AX"),
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        cache.clone(),
    );

    let config = create_test_exec_config(addr);
    let rx = setup_exec_channel();
    let client = AxExecutionClient::new(core, config).expect("Failed to create exec client");

    (client, rx, cache)
}

fn add_test_account_to_cache(cache: &Rc<RefCell<Cache>>, account_id: AccountId) {
    let account_state = AccountState::new(
        account_id,
        AccountType::Margin,
        vec![AccountBalance::new(
            Money::from("100000.50 USD"),
            Money::from("0 USD"),
            Money::from("100000.50 USD"),
        )],
        vec![],
        true,
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        None,
    );

    let account = AccountAny::Margin(MarginAccount::new(account_state, true));
    cache.borrow_mut().add_account(account).unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_config_creation() {
    let config = AxExecClientConfig {
        api_key: Some("test_api_key".to_string()),
        api_secret: Some("test_api_secret".to_string()),
        is_sandbox: true,
        ..Default::default()
    };

    assert_eq!(config.api_key, Some("test_api_key".to_string()));
    assert!(config.is_sandbox);
    assert_eq!(config.trader_id, TraderId::from("TRADER-001"));
    assert_eq!(config.account_id, AccountId::from("AX-001"));
}

#[rstest]
#[tokio::test]
async fn test_exec_client_creation() {
    let (addr, _state) = start_test_server().await.unwrap();
    let (client, _rx, _cache) = create_test_execution_client(addr);

    assert_eq!(client.client_id(), ClientId::from("AX"));
    assert_eq!(client.venue(), Venue::from("AX"));
    assert_eq!(client.oms_type(), OmsType::Netting);
    assert_eq!(client.account_id(), AccountId::from("AX-001"));
    assert!(!client.is_connected());
}

#[rstest]
#[tokio::test]
async fn test_exec_client_connect_disconnect() {
    let (addr, _state) = start_test_server().await.unwrap();
    let (mut client, _rx, cache) = create_test_execution_client(addr);

    // Pre-register account so await_account_registered succeeds
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));

    assert!(!client.is_connected());

    client.connect().await.expect("Failed to connect");
    assert!(client.is_connected());

    client.disconnect().await.expect("Failed to disconnect");
    assert!(!client.is_connected());
}

#[rstest]
#[tokio::test]
async fn test_exec_client_emits_account_state_on_connect() {
    let (addr, _state) = start_test_server().await.unwrap();
    let (mut client, mut rx, cache) = create_test_execution_client(addr);

    add_test_account_to_cache(&cache, AccountId::from("AX-001"));

    // start() wires the emitter's sender so events reach the channel
    client.start().expect("Failed to start");
    client.connect().await.expect("Failed to connect");

    // The connect flow calls request_account_state and emits via the channel
    let mut found_account = false;
    while let Ok(event) = rx.try_recv() {
        if matches!(event, ExecutionEvent::Account(_)) {
            found_account = true;
            break;
        }
    }

    assert!(found_account, "Expected account state event on connect");
    client.disconnect().await.expect("Failed to disconnect");
}

#[rstest]
#[tokio::test]
async fn test_exec_client_get_account_returns_cached() {
    let (addr, _state) = start_test_server().await.unwrap();
    let (client, _rx, cache) = create_test_execution_client(addr);

    assert!(client.get_account().is_none());

    add_test_account_to_cache(&cache, AccountId::from("AX-001"));

    let account = client.get_account();
    assert!(account.is_some());
}

#[rstest]
#[tokio::test]
async fn test_exec_config_url_overrides() {
    let config = AxExecClientConfig {
        base_url_http: Some("http://custom:1234".to_string()),
        base_url_orders: Some("http://custom:5678".to_string()),
        base_url_ws_private: Some("ws://custom:9012/ws".to_string()),
        ..Default::default()
    };

    assert_eq!(config.http_base_url(), "http://custom:1234");
    assert_eq!(config.orders_base_url(), "http://custom:5678");
    assert_eq!(config.ws_private_url(), "ws://custom:9012/ws");
}

#[rstest]
#[tokio::test]
async fn test_exec_config_sandbox_defaults() {
    let config = AxExecClientConfig {
        is_sandbox: true,
        ..Default::default()
    };

    assert!(config.http_base_url().contains("sandbox"));
    assert!(config.orders_base_url().contains("sandbox"));
    assert!(config.ws_private_url().contains("sandbox"));
}

#[rstest]
#[tokio::test]
async fn test_exec_config_production_defaults() {
    let config = AxExecClientConfig {
        is_sandbox: false,
        ..Default::default()
    };

    assert!(!config.http_base_url().contains("sandbox"));
    assert!(!config.orders_base_url().contains("sandbox"));
    assert!(!config.ws_private_url().contains("sandbox"));
}
