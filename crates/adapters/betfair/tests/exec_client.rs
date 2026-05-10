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

//! Integration tests for `BetfairExecutionClient`.

mod common;

use std::{cell::RefCell, net::SocketAddr, rc::Rc, time::Duration};

use nautilus_betfair::execution::BetfairExecutionClient;
use nautilus_common::{
    cache::Cache, clients::ExecutionClient, live::runner::set_exec_event_sender,
    messages::ExecutionEvent,
};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    enums::{AccountType, OmsType},
    identifiers::{AccountId, ClientId, TraderId, Venue},
    types::Currency,
};
use rstest::rstest;
use serde_json::Value;

use crate::common::*;

fn create_test_execution_client(
    addr: SocketAddr,
    stream_port: u16,
) -> (
    BetfairExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BETFAIR-001");
    let client_id = ClientId::from("BETFAIR");
    let cache = Rc::new(RefCell::new(Cache::default()));

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        Venue::from("BETFAIR"),
        OmsType::Netting,
        account_id,
        AccountType::Betting,
        None,
        cache.clone(),
    );

    let http_client = create_test_http_client(addr);
    let currency = Currency::GBP();

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let mut client = BetfairExecutionClient::new(
        core,
        http_client,
        test_credential(),
        plain_stream_config(stream_port),
        currency,
    );
    client.start().unwrap();

    (client, rx, cache)
}

#[rstest]
#[tokio::test]
async fn test_exec_client_creation() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, _listener) = start_mock_stream().await;
    let (client, _rx, _cache) = create_test_execution_client(addr, stream_port);

    assert_eq!(client.client_id(), ClientId::from("BETFAIR"));
    assert_eq!(client.account_id(), AccountId::from("BETFAIR-001"));
    assert_eq!(client.venue(), Venue::from("BETFAIR"));
    assert_eq!(client.oms_type(), OmsType::Netting);
    assert!(!client.is_connected());
}

#[rstest]
#[tokio::test]
async fn test_exec_client_connect_disconnect() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, _rx, _cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (mut reader, write_half) = accept_and_auth(&listener).await;

        // Skip auth line from subscribe_orders combined write
        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();
        line.clear();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();

        let json: Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(json["op"], "orderSubscription");

        tokio::time::sleep(Duration::from_secs(2)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    assert!(client.is_connected());
    assert!(state.login_count.load(std::sync::atomic::Ordering::Relaxed) > 0);

    client.disconnect().await.unwrap();
    assert!(!client.is_connected());

    let _ = server.await;
}

#[rstest]
#[tokio::test]
async fn test_exec_client_connect_emits_account_state() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_auth(&listener).await;
        tokio::time::sleep(Duration::from_secs(2)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    let mut found_account_state = false;
    while let Ok(event) = rx.try_recv() {
        if matches!(event, ExecutionEvent::Account(_)) {
            found_account_state = true;
            break;
        }
    }

    assert!(
        found_account_state,
        "Expected AccountState event during connect"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

#[rstest]
#[tokio::test]
async fn test_ocm_handler_emits_order_status_report() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _cache) = create_test_execution_client(addr, stream_port);

    let ocm_fixture = load_fixture("stream/ocm_FILLED.json");
    let server = tokio::spawn(async move {
        let (_reader, mut write_half) = accept_and_auth(&listener).await;

        // Allow subscribe_orders to complete before sending data
        tokio::time::sleep(Duration::from_millis(200)).await;

        tokio::io::AsyncWriteExt::write_all(
            &mut write_half,
            format!("{}\r\n", ocm_fixture.trim()).as_bytes(),
        )
        .await
        .unwrap();

        tokio::time::sleep(Duration::from_secs(2)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for OCM event")
        .expect("channel closed");

    assert!(
        matches!(event, ExecutionEvent::Report(_)),
        "Expected Report event from OCM, found: {event:?}"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}
