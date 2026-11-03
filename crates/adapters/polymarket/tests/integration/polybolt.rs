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

//! PolyBolt protocol tests through the public Polymarket data client.

use std::{net::SocketAddr, sync::atomic::Ordering, time::Duration};

use nautilus_common::{
    clients::DataClient,
    live::runner::{replace_data_event_sender, replace_system_event_sender},
    messages::{
        DataEvent, SystemEvent,
        data::{SubscribeCustomData, UnsubscribeCustomData},
        system::SocketState,
    },
    testing::wait_until_async,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_live::{SocketReconnectRegistry, SocketReconnectRequestOutcome};
use nautilus_model::data::{CustomData, Data as NautilusData, DataType};
use nautilus_network::{retry::RetryConfig, websocket::TransportBackend};
use nautilus_polymarket::{
    common::consts::{POLYMARKET_CLIENT_ID, POLYMARKET_VENUE, WS_DEFAULT_SUBSCRIPTIONS},
    config::PolymarketDataClientConfig,
    data::PolymarketDataClient,
    data_types::{PolymarketCryptoPrice, PolymarketCryptoTwap, PolymarketEquityPrice},
    http::{
        clob::PolymarketClobPublicClient, data_api::PolymarketDataApiHttpClient,
        gamma::PolymarketGammaHttpClient,
    },
    websocket::pool::PolymarketMarketConnectionPool,
};
use rstest::rstest;
use rust_decimal_macros::dec;
use serde_json::{Value, json};

use super::harness::polybolt::*;

const SEQUENCE_GAP_STEP: u64 = 1000;
const SUBSCRIPTION_INTERVAL_MS: u128 = 53;

#[rstest]
#[tokio::test]
async fn test_connect_authenticates_before_subscribing() {
    let state = TestServerState::default();
    let addr = start_polybolt_server(state.clone()).await;
    let (mut client, _rx) = make_client(addr);
    subscribe(&mut client, crypto_data_type("btcusd")).expect("track BTC");
    subscribe(&mut client, crypto_twap_data_type("btcusd", 60)).expect("track TWAP");

    client.connect().await.expect("connect client");
    wait_for_op(&state, "subscribe").await;

    let payloads = state.payloads().await;
    assert_eq!(ops(&payloads), vec!["auth", "subscribe"]);
    assert_eq!(
        subscription_pairs(&payloads[1]),
        vec![
            ("price.crypto".to_string(), "btcusd".to_string()),
            ("price.crypto.twap".to_string(), "btcusd".to_string()),
        ]
    );
    assert!(client.is_connected());
    client.disconnect().await.expect("disconnect client");
}

#[rstest]
#[tokio::test]
async fn test_subscribe_waits_for_authed_acknowledgement() {
    let state = TestServerState::default();
    state.hold_auth.store(true, Ordering::Relaxed);
    let addr = start_polybolt_server(state.clone()).await;
    let (mut client, _rx) = make_client(addr);
    subscribe(&mut client, crypto_data_type("btcusd")).expect("track BTC");

    {
        let connect = client.connect();
        tokio::pin!(connect);
        tokio::select! {
            result = &mut connect => panic!("connect completed before authentication: {result:?}"),
            () = wait_for_op(&state, "auth") => {},
        }
        tokio::select! {
            result = &mut connect => panic!("connect completed before acknowledgement: {result:?}"),
            () = tokio::time::sleep(Duration::from_millis(200)) => {},
        }
        assert_eq!(ops(&state.payloads().await), vec!["auth"]);
        state.send_text_to_all(AUTHED_FIXTURE.to_string()).await;
        connect.await.expect("connect after late authed");
    }

    wait_for_op(&state, "subscribe").await;

    assert_eq!(ops(&state.payloads().await), vec!["auth", "subscribe"]);
    client.disconnect().await.expect("disconnect client");
}

#[rstest]
#[tokio::test]
async fn test_unavailable_auth_keeps_connect_alive_and_retries_later() {
    let state = TestServerState::default();
    state.unavailable_auth_remaining.store(1, Ordering::Relaxed);
    let addr = start_polybolt_server(state.clone()).await;
    let (mut client, _rx) = make_client(addr);
    subscribe(&mut client, crypto_data_type("btcusd")).expect("track BTC");

    client
        .connect()
        .await
        .expect("a venue-side auth outage must not fail connect");

    assert_eq!(ops(&state.payloads().await), vec!["auth"]);

    // The reconcile worker waits out the retry delay, re-authenticates, then subscribes.
    wait_until_async(
        || {
            let state = state.clone();
            async move { ops(&state.payloads().await) == vec!["auth", "auth", "subscribe"] }
        },
        Duration::from_secs(10),
    )
    .await;

    assert!(client.is_connected());
    assert_eq!(state.auth_count.load(Ordering::Relaxed), 2);
    client.disconnect().await.expect("disconnect client");
}

#[rstest]
#[tokio::test]
async fn test_auth_attempts_error_recovers_on_a_fresh_connection() {
    let state = TestServerState::default();
    state.auth_attempts_once.store(true, Ordering::Relaxed);
    let addr = start_polybolt_server(state.clone()).await;
    let (mut client, _rx) = make_client(addr);
    subscribe(&mut client, crypto_data_type("btcusd")).expect("track BTC");

    client
        .connect()
        .await
        .expect("a spent auth budget must not fail connect");

    // The reconcile worker waits out the retry delay, then authenticates on a new connection.
    wait_until_async(
        || {
            let state = state.clone();
            async move { ops(&state.payloads().await) == vec!["auth", "auth", "subscribe"] }
        },
        Duration::from_secs(10),
    )
    .await;

    assert_eq!(state.connection_count().await, 2);
    assert!(client.is_connected());
    client.disconnect().await.expect("disconnect client");
}

#[rstest]
#[tokio::test]
async fn test_sequence_gap_resubscribes_only_the_affected_channel() {
    let state = TestServerState::default();
    let addr = start_polybolt_server(state.clone()).await;
    let (mut client, _rx) = make_client(addr);
    subscribe(&mut client, crypto_data_type("btcusd")).expect("track BTC");
    subscribe(&mut client, equity_data_type("aapl")).expect("track AAPL");
    client.connect().await.expect("connect client");
    wait_for_op(&state, "subscribe").await;
    state.clear_received_payloads().await;

    state.send_text_to_all(crypto_update_with_seq(1)).await;
    state.send_text_to_all(crypto_update_with_seq(2)).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        state.payloads().await.is_empty(),
        "contiguous seq must not resubscribe"
    );

    state.send_text_to_all(crypto_update_with_seq(4)).await;
    wait_until_async(
        || {
            let state = state.clone();
            async move { ops(&state.payloads().await) == vec!["unsubscribe", "subscribe"] }
        },
        Duration::from_secs(5),
    )
    .await;

    for payload in &state.payloads().await {
        let subscriptions = payload["subscriptions"].as_array().expect("subscriptions");
        assert_eq!(subscriptions.len(), 1, "{payload}");
        assert_eq!(subscriptions[0]["channel"], "price.crypto");
        assert_eq!(subscriptions[0]["filter"]["symbol"], "btcusd");
    }

    client.disconnect().await.expect("disconnect client");
}

#[rstest]
#[tokio::test]
async fn test_dropped_frames_on_first_envelope_resubscribe_channel() {
    let state = TestServerState::default();
    let addr = start_polybolt_server(state.clone()).await;
    let (mut client, _rx) = make_client(addr);
    subscribe(&mut client, crypto_data_type("btcusd")).expect("track BTC");
    client.connect().await.expect("connect client");
    wait_for_op(&state, "subscribe").await;
    state.clear_received_payloads().await;
    let mut frame: Value = serde_json::from_str(&crypto_update_with_seq(3)).unwrap();
    frame["dropped"] = json!(2);

    state.send_text_to_all(frame.to_string()).await;
    wait_until_async(
        || {
            let state = state.clone();
            async move { ops(&state.payloads().await) == vec!["unsubscribe", "subscribe"] }
        },
        Duration::from_secs(5),
    )
    .await;

    let payloads = state.payloads().await;
    assert_eq!(ops(&payloads), vec!["unsubscribe", "subscribe"]);

    for payload in payloads {
        assert_eq!(
            subscription_pairs(&payload),
            vec![("price.crypto".to_string(), "btcusd".to_string())]
        );
    }

    client.disconnect().await.expect("disconnect client");
}

#[rstest]
#[case(None)]
#[case(Some(4002))]
#[tokio::test]
async fn test_server_disconnect_reauthenticates_and_resubscribes(#[case] close_code: Option<u16>) {
    let state = TestServerState::default();
    let addr = start_polybolt_server(state.clone()).await;
    let (mut client, _rx) = make_client(addr);
    subscribe(&mut client, equity_data_type("aapl")).expect("track AAPL");
    subscribe(&mut client, crypto_data_type("btcusd")).unwrap();
    subscribe(&mut client, crypto_data_type("ethusd")).unwrap();
    subscribe(&mut client, crypto_twap_data_type("btcusd", 60)).unwrap();
    client.connect().await.expect("connect client");
    wait_for_op(&state, "subscribe").await;

    state.clear_received_payloads().await;
    match close_code {
        Some(code) => state.close_all_connections_with_code(code).await,
        None => state.close_all_connections().await,
    }

    wait_until_async(
        || {
            let state = state.clone();
            async move { state.connection_count().await >= 2 }
        },
        Duration::from_secs(10),
    )
    .await;

    wait_for_op(&state, "subscribe").await;

    let payloads = state.payloads().await;
    assert_eq!(ops(&payloads), vec!["auth", "subscribe"]);
    assert_eq!(
        subscription_pairs(&payloads[1]),
        vec![
            ("price.crypto".into(), "btcusd".into()),
            ("price.crypto".into(), "ethusd".into()),
            ("price.crypto.twap".into(), "btcusd".into()),
            ("price.equity".into(), "aapl".into())
        ]
    );
    assert_eq!(state.auth_count.load(Ordering::Relaxed), 2);
    client.disconnect().await.expect("disconnect client");
}

#[rstest]
#[tokio::test]
async fn test_incremental_sync_subscribes_only_new_symbol_while_connected() {
    let state = TestServerState::default();
    let addr = start_polybolt_server(state.clone()).await;
    let (mut client, _rx) = make_client(addr);
    subscribe(&mut client, crypto_data_type("btcusd")).expect("track BTC");
    client.connect().await.expect("connect client");
    wait_for_op(&state, "subscribe").await;
    state.clear_received_payloads().await;

    subscribe(&mut client, crypto_data_type("ethusd")).expect("track ETH");
    wait_for_op(&state, "subscribe").await;

    let payloads = state.payloads().await;
    assert_eq!(
        ops(&payloads),
        vec!["subscribe"],
        "no re-auth, no unsubscribe"
    );
    assert_eq!(
        subscription_pairs(&payloads[0]),
        vec![("price.crypto".to_string(), "ethusd".to_string())]
    );
    client.disconnect().await.expect("disconnect client");
}

#[rstest]
#[tokio::test]
async fn test_incremental_sync_unsubscribes_only_removed_symbol() {
    let state = TestServerState::default();
    let addr = start_polybolt_server(state.clone()).await;
    let (mut client, _rx) = make_client(addr);
    let aapl = equity_data_type("aapl");
    let msft = equity_data_type("msft");
    subscribe(&mut client, aapl).expect("track AAPL");
    subscribe(&mut client, msft.clone()).expect("track MSFT");
    client.connect().await.expect("connect client");
    wait_for_op(&state, "subscribe").await;
    state.clear_received_payloads().await;

    unsubscribe(&mut client, &msft).expect("track unsubscribe");
    wait_for_op(&state, "unsubscribe").await;

    let payloads = state.payloads().await;
    assert_eq!(ops(&payloads), vec!["unsubscribe"]);
    assert_eq!(
        subscription_pairs(&payloads[0]),
        vec![("price.equity".to_string(), "msft".to_string())]
    );
    client.disconnect().await.expect("disconnect client");
}

#[rstest]
#[tokio::test]
async fn test_server_disconnect_reconnects_and_resumes_multi_symbol_updates() {
    let state = TestServerState::default();
    let addr = start_polybolt_server(state.clone()).await;
    let (mut client, mut rx) = make_client(addr);
    subscribe(&mut client, crypto_data_type("btcusd")).expect("track BTC");
    subscribe(&mut client, crypto_data_type("ethusd")).expect("track ETH");
    client.connect().await.expect("connect client");
    wait_for_op(&state, "subscribe").await;

    state
        .send_text_to_all(build_crypto_update(
            "btcusd",
            "61035.860000000000000000",
            1791355670000,
            1791355670142,
        ))
        .await;
    state
        .send_text_to_all(build_crypto_update(
            "ethusd",
            "2450.110000000000000000",
            1791355671000,
            1791355671142,
        ))
        .await;

    wait_until_async(|| async { rx.len() >= 2 }, Duration::from_secs(2)).await;
    assert_eq!(
        collect_crypto_symbols(&mut rx, 2),
        vec!["btcusd".to_string(), "ethusd".to_string()],
        "both retained symbols should emit before disconnect",
    );

    state.clear_received_payloads().await;
    state.close_all_connections().await;
    wait_until_async(
        || {
            let state = state.clone();
            async move { state.connection_count().await >= 2 }
        },
        Duration::from_secs(10),
    )
    .await;

    wait_for_op(&state, "subscribe").await;

    state
        .send_text_to_all(build_crypto_update(
            "btcusd",
            "61040.120000000000000000",
            1791355672000,
            1791355672142,
        ))
        .await;
    state
        .send_text_to_all(build_crypto_update(
            "ethusd",
            "2455.550000000000000000",
            1791355673000,
            1791355673142,
        ))
        .await;

    wait_until_async(|| async { rx.len() >= 2 }, Duration::from_secs(2)).await;
    assert_eq!(
        collect_crypto_symbols(&mut rx, 2),
        vec!["btcusd".to_string(), "ethusd".to_string()],
        "both retained symbols should resume after server-side reconnect",
    );

    client.disconnect().await.expect("disconnect client");
}

#[rstest]
#[tokio::test]
async fn test_server_disconnect_does_not_restore_unsubscribed_symbol() {
    let state = TestServerState::default();
    let addr = start_polybolt_server(state.clone()).await;
    let (mut client, mut rx) = make_client(addr);
    let btc = crypto_data_type("btcusd");
    let eth = crypto_data_type("ethusd");
    subscribe(&mut client, btc.clone()).expect("track BTC");
    subscribe(&mut client, eth.clone()).expect("track ETH");
    client.connect().await.expect("connect client");
    wait_for_op(&state, "subscribe").await;

    unsubscribe(&mut client, &eth).expect("unsubscribe ETH");
    wait_for_op(&state, "unsubscribe").await;

    state.clear_received_payloads().await;
    state.close_all_connections().await;
    wait_until_async(
        || {
            let state = state.clone();
            async move { state.connection_count().await >= 2 }
        },
        Duration::from_secs(10),
    )
    .await;

    wait_for_op(&state, "subscribe").await;

    let payloads = state.payloads().await;
    let replay = payloads
        .iter()
        .find(|payload| payload["op"] == "subscribe")
        .expect("replay payload");
    assert_eq!(
        subscription_pairs(replay),
        vec![("price.crypto".to_string(), "btcusd".to_string())]
    );

    state
        .send_text_to_all(build_crypto_update(
            "btcusd",
            "61050.010000000000000000",
            1791355674000,
            1791355674142,
        ))
        .await;
    state
        .send_text_to_all(build_crypto_update(
            "ethusd",
            "2460.010000000000000000",
            1791355675000,
            1791355675142,
        ))
        .await;

    wait_until_async(|| async { !rx.is_empty() }, Duration::from_secs(2)).await;
    assert_eq!(
        collect_crypto_symbols(&mut rx, 1),
        vec!["btcusd".to_string()],
        "reconnect should restore only still-retained symbols",
    );
    assert!(
        rx.try_recv().is_err(),
        "ETH should not be replayed after unsubscribe"
    );

    client.disconnect().await.expect("disconnect client");
}

#[rstest]
#[case(4001)]
#[case(4008)]
#[tokio::test]
async fn test_terminal_peer_close_stops_reconnects_until_explicit_connect(#[case] close_code: u16) {
    let state = TestServerState::default();
    let addr = start_polybolt_server(state.clone()).await;
    let (mut client, _rx) = make_client(addr);
    subscribe(&mut client, crypto_data_type("btcusd")).unwrap();
    client.connect().await.unwrap();
    wait_for_op(&state, "subscribe").await;
    state.clear_received_payloads().await;
    state.close_all_connections_with_code(close_code).await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    subscribe(&mut client, crypto_data_type("ethusd")).unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(state.connection_count().await, 1);
    assert_eq!(state.payloads().await, Vec::<Value>::new());

    client.disconnect().await.unwrap();
    client.connect().await.unwrap();
    wait_for_op(&state, "subscribe").await;
    let payloads = state.payloads().await;
    assert_eq!(state.connection_count().await, 2);
    assert_eq!(ops(&payloads), vec!["auth", "subscribe"]);
    assert_eq!(
        subscription_pairs(&payloads[1]),
        vec![
            ("price.crypto".into(), "btcusd".into()),
            ("price.crypto".into(), "ethusd".into())
        ]
    );
    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_draining_close_reconnects_after_the_drain_delay() {
    let state = TestServerState::default();
    let addr = start_polybolt_server(state.clone()).await;
    let (mut client, _rx) = make_client(addr);
    subscribe(&mut client, crypto_data_type("btcusd")).unwrap();
    client.connect().await.unwrap();
    wait_for_op(&state, "subscribe").await;
    state.clear_received_payloads().await;
    state.close_all_connections_with_code(4003).await;
    wait_until_async(
        || {
            let state = state.clone();
            async move { ops(&state.payloads().await) == vec!["auth", "subscribe"] }
        },
        Duration::from_secs(15),
    )
    .await;

    let payloads = state.payloads().await;
    assert_eq!(state.connection_count().await, 2);
    assert_eq!(
        subscription_pairs(&payloads[1]),
        vec![("price.crypto".into(), "btcusd".into())]
    );
    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_rejected_credentials_fail_connect() {
    let state = TestServerState::default();
    state.reject_auth.store(true, Ordering::Relaxed);
    let addr = start_polybolt_server(state.clone()).await;
    let (mut client, _rx) = make_client(addr);
    subscribe(&mut client, crypto_data_type("btcusd")).unwrap();
    let error = client
        .connect()
        .await
        .expect_err("rejected credentials must fail connect");
    assert!(error.to_string().contains("rejected"), "{error}");
    assert_eq!(state.auth_count.load(Ordering::Relaxed), 1);
    assert_eq!(ops(&state.payloads().await), vec!["auth"]);
    assert!(!client.is_connected());
    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_background_auth_rejection_stops_auth_retries() {
    let state = TestServerState::default();
    let addr = start_polybolt_server(state.clone()).await;
    let registry = SocketReconnectRegistry::default();
    let (mut client, mut rx) = registry.scope(|| make_client(addr));
    subscribe(&mut client, crypto_data_type("btcusd")).unwrap();
    client.connect().await.unwrap();
    wait_for_op(&state, "subscribe").await;
    let endpoint = ustr::Ustr::from("polymarket-polybolt-streams");
    let handle = registry.handle(*POLYMARKET_CLIENT_ID, endpoint).unwrap();
    state.clear_received_payloads().await;
    state.reject_auth.store(true, Ordering::Relaxed);
    state.close_all_connections().await;
    wait_for_op(&state, "auth").await;

    // A subsequent price frame proves the reader has processed the preceding rejection
    state
        .send_text_to_all(CRYPTO_UPDATE_FIXTURE.to_string())
        .await;
    let custom = next_custom(&mut rx).await;
    let payload = custom
        .data
        .as_any()
        .downcast_ref::<PolymarketCryptoPrice>()
        .unwrap();
    assert_eq!(payload.value, dec!(84290.833784311415570432));
    assert!(client.is_connected());
    subscribe(&mut client, crypto_data_type("ethusd")).unwrap();
    assert_eq!(
        handle.request_reconnect(),
        SocketReconnectRequestOutcome::Accepted
    );
    wait_until_async(
        || {
            let state = state.clone();
            async move { state.connection_count().await == 3 }
        },
        Duration::from_secs(5),
    )
    .await;

    let mut update = fixture_json(CRYPTO_UPDATE_FIXTURE);
    update["payload"]["timestamp"] = json!(1791355667001_u64);
    state.send_text_to_all(update.to_string()).await;
    let custom = next_custom(&mut rx).await;
    let payload = custom
        .data
        .as_any()
        .downcast_ref::<PolymarketCryptoPrice>()
        .unwrap();
    assert_eq!(payload.price_timestamp_ms, 1791355667001);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(*state.auth_counts.lock(), vec![1, 1, 0]);
    assert_eq!(ops(&state.payloads().await), vec!["auth"]);
    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_socket_state_events_use_polybolt_endpoint() {
    let state = TestServerState::default();
    let addr = start_polybolt_server(state).await;
    let (system_tx, mut system_rx) = tokio::sync::mpsc::unbounded_channel();
    replace_system_event_sender(system_tx);
    let registry = SocketReconnectRegistry::default();
    let (mut client, _rx) = registry.scope(|| make_client(addr));
    subscribe(&mut client, crypto_data_type("btcusd")).unwrap();
    client.connect().await.unwrap();
    let event = tokio::time::timeout(Duration::from_secs(5), system_rx.recv())
        .await
        .unwrap()
        .unwrap();
    let SystemEvent::SocketState(change) = event;
    let endpoint = ustr::Ustr::from("polymarket-polybolt-streams");
    let handle = registry.handle(*POLYMARKET_CLIENT_ID, endpoint).unwrap();
    assert_eq!(change.client_id, *POLYMARKET_CLIENT_ID);
    assert_eq!(change.venue, Some(*POLYMARKET_VENUE));
    assert_eq!(change.endpoint, endpoint);
    assert_eq!(change.state, SocketState::Connected);
    assert_eq!(
        handle.request_reconnect(),
        SocketReconnectRequestOutcome::Accepted
    );
    let event = tokio::time::timeout(Duration::from_secs(5), system_rx.recv())
        .await
        .unwrap()
        .unwrap();
    let SystemEvent::SocketState(change) = event;
    assert_eq!(change.client_id, *POLYMARKET_CLIENT_ID);
    assert_eq!(change.venue, Some(*POLYMARKET_VENUE));
    assert_eq!(change.endpoint, endpoint);
    assert_eq!(change.state, SocketState::Disconnected);
    client.disconnect().await.unwrap();
    assert!(registry.handle(*POLYMARKET_CLIENT_ID, endpoint).is_none());
}

#[rstest]
#[tokio::test]
async fn test_server_disconnect_preserves_twap_replay_fingerprint() {
    let state = TestServerState::default();
    let addr = start_polybolt_server(state.clone()).await;
    let (mut client, mut rx) = make_client(addr);
    subscribe(&mut client, crypto_twap_data_type("btcusd", 60)).unwrap();
    client.connect().await.unwrap();
    wait_for_op(&state, "subscribe").await;
    state
        .send_text_to_all(TWAP_UPDATE_FIXTURE.to_string())
        .await;
    let initial = next_custom(&mut rx).await;
    let initial = initial
        .data
        .as_any()
        .downcast_ref::<PolymarketCryptoTwap>()
        .unwrap();
    assert_eq!(initial.value, dec!(84291.577380840025358336));
    assert_eq!(initial.observation_timestamp_ms, 1791355667000);
    state.clear_received_payloads().await;
    state.close_all_connections().await;
    wait_for_op(&state, "subscribe").await;
    let mut conflict = fixture_json(TWAP_UPDATE_FIXTURE);
    conflict["payload"]["full_accuracy_value"] = json!("84291.577380840025358337");
    state.send_text_to_all(conflict.to_string()).await;
    state
        .send_text_to_all(TWAP_UPDATE_FIXTURE.to_string())
        .await;
    let mut next = fixture_json(TWAP_UPDATE_FIXTURE);
    next["payload"]["timestamp"] = json!(1791355667001_u64);
    state.send_text_to_all(next.to_string()).await;
    let custom = next_custom(&mut rx).await;
    let payload = custom
        .data
        .as_any()
        .downcast_ref::<PolymarketCryptoTwap>()
        .unwrap();
    assert_eq!(payload.symbol, "btcusd");
    assert_eq!(payload.value, dec!(84291.577380840025358336));
    assert_eq!(payload.observation_timestamp_ms, 1791355667001);
    assert!(rx.try_recv().is_err());
    assert_eq!(state.connection_count().await, 2);
    assert_eq!(ops(&state.payloads().await), vec!["auth", "subscribe"]);
    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_connect_without_subscriptions_clears_closing_for_later_subscribe() {
    let state = TestServerState::default();
    let addr = start_polybolt_server(state.clone()).await;
    let (mut client, _rx) = make_client(addr);
    client.connect().await.unwrap();
    client.disconnect().await.unwrap();
    client.connect().await.unwrap();
    assert_eq!(state.connection_count().await, 0);
    assert_eq!(state.payloads().await, Vec::<Value>::new());
    subscribe(&mut client, crypto_data_type("btcusd")).unwrap();
    wait_for_op(&state, "subscribe").await;
    assert_eq!(state.connection_count().await, 1);
    assert_eq!(ops(&state.payloads().await), vec!["auth", "subscribe"]);
    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_spent_auth_budget_moves_to_a_fresh_connection() {
    let state = TestServerState::default();
    state.unavailable_auth_remaining.store(8, Ordering::Relaxed);
    let addr = start_polybolt_server(state.clone()).await;
    let (mut client, _rx) = make_client(addr);
    subscribe(&mut client, crypto_data_type("btcusd")).unwrap();
    client
        .connect()
        .await
        .expect("transient authentication failures preserve the client");
    wait_until_async(
        || {
            let state = state.clone();
            async move {
                state
                    .payloads()
                    .await
                    .iter()
                    .any(|payload| payload["op"] == "subscribe")
            }
        },
        Duration::from_secs(50),
    )
    .await;

    assert_eq!(*state.auth_counts.lock(), vec![8, 1]);
    assert_eq!(state.connection_count().await, 2);
    let expected_ops = std::iter::repeat_n("auth", 9)
        .chain(["subscribe"])
        .collect::<Vec<_>>();
    assert_eq!(ops(&state.payloads().await), expected_ops);
    client.disconnect().await.unwrap();
}

#[rstest]
#[case(100)]
#[case(300)]
#[tokio::test]
async fn test_measure_gap_recovery(#[case] rounds: usize) {
    let state = TestServerState::default();
    state.snapshots.store(true, Ordering::Relaxed);
    let addr = start_polybolt_server(state.clone()).await;
    let (mut client, mut rx) = make_client(addr);
    for symbol in ["btcusd", "ethusd"] {
        subscribe(&mut client, crypto_data_type(symbol)).unwrap();
    }

    for symbol in ["aapl", "msft"] {
        subscribe(&mut client, equity_data_type(symbol)).unwrap();
    }

    client.connect().await.unwrap();
    let mut observations = Vec::new();
    for _ in 0..12 {
        observations.push(price_observation(&next_custom(&mut rx).await));
    }

    assert_eq!(
        observations.len(),
        state.snapshot_points.load(Ordering::Relaxed)
    );
    state.clear_received_payloads().await;
    state.subscription_times.lock().await.clear();
    state.snapshot_times.lock().clear();
    let started = std::time::Instant::now();

    for round in 1..=rounds {
        for fixture in [CRYPTO_UPDATE_FIXTURE, EQUITY_UPDATE_FIXTURE] {
            let mut frame = fixture_json(fixture);
            let sequence = round as u64 * SEQUENCE_GAP_STEP;
            frame["seq"] = json!(sequence);
            frame["dropped"] = json!(1);
            frame["payload"]["timestamp"] = json!(price_timestamp_ms(sequence, 5));
            state.send_text_to_all(frame.to_string()).await;
        }

        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    let last_drop = std::time::Instant::now();

    let frames = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            while let Ok(event) = rx.try_recv() {
                observations.push(price_observation(&custom_data(event)));
            }

            let frames = state.sent_frames.lock().clone();
            let mut expected = frame_observations(&frames);
            let threshold = price_timestamp_ms(rounds as u64 * SEQUENCE_GAP_STEP + 1, 0);

            let recovered = ["btcusd", "ethusd", "aapl", "msft"]
                .into_iter()
                .all(|symbol| {
                    observations
                        .iter()
                        .any(|(name, ts)| name == symbol && *ts >= threshold)
                });

            expected.sort();
            let mut actual = observations.clone();
            actual.sort();
            if recovered && actual == expected {
                break frames;
            }

            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("all final recovery snapshots must reach the public data channel");

    let received = std::time::Instant::now();
    let elapsed = started.elapsed();
    client.disconnect().await.unwrap();
    assert!(rx.try_recv().is_err());
    let payloads = state.payloads().await;
    let subscribes = payloads.iter().filter(|p| p["op"] == "subscribe").count();
    let unsubscribes = payloads.iter().filter(|p| p["op"] == "unsubscribe").count();
    assert_eq!(subscribes, unsubscribes);
    let subscription_times = state.subscription_times.lock().await.clone();
    for window in subscription_times.windows(21) {
        assert!(
            window[20].duration_since(window[0]) >= Duration::from_secs(1),
            "more than 20 subscription frames in a rolling second"
        );
    }

    let snapshot_times = state.snapshot_times.lock();
    assert!(snapshot_times[0].duration_since(started) < Duration::from_millis(500));
    assert!(
        snapshot_times
            .last()
            .unwrap()
            .checked_duration_since(last_drop)
            .unwrap_or_default()
            < Duration::from_millis(500)
    );
    assert!(received.duration_since(last_drop) < Duration::from_millis(500));
    assert!(
        subscribes * 2 <= (elapsed.as_millis() / SUBSCRIPTION_INTERVAL_MS + 2) as usize,
        "coalescing must bound snapshot work during sustained drops"
    );

    if rounds == 300 {
        assert!(
            subscription_times.len() >= 21,
            "the sustained workload must exercise a full frame-limit window"
        );
    }

    let snapshots = frames
        .iter()
        .filter(|frame| frame["snapshot"] == true)
        .count();
    assert_eq!(snapshots, state.snapshot_frames.load(Ordering::Relaxed));
    assert_eq!(state.snapshot_points.load(Ordering::Relaxed), snapshots * 3);
    assert_eq!(observations.len(), snapshots * 3 + rounds * 2);
    let mut expected = frame_observations(&frames);
    expected.sort();
    observations.sort();
    assert_eq!(observations, expected);
}

fn price_observation(custom: &CustomData) -> (String, u64) {
    if let Some(payload) = custom.data.as_any().downcast_ref::<PolymarketCryptoPrice>() {
        return (payload.symbol.clone(), payload.price_timestamp_ms);
    }

    let payload = custom
        .data
        .as_any()
        .downcast_ref::<PolymarketEquityPrice>()
        .expect("reference-price payload");
    (payload.symbol.clone(), payload.price_timestamp_ms)
}

fn frame_observations(frames: &[Value]) -> Vec<(String, u64)> {
    let mut observations = Vec::new();

    for frame in frames {
        let payload = &frame["payload"];
        let symbol = payload["symbol"].as_str().unwrap();
        if let Some(points) = payload["data"].as_array() {
            observations.extend(
                points
                    .iter()
                    .map(|point| (symbol.to_string(), point["timestamp"].as_u64().unwrap())),
            );
        } else {
            observations.push((symbol.to_string(), payload["timestamp"].as_u64().unwrap()));
        }
    }

    observations
}

fn make_client(
    addr: SocketAddr,
) -> (
    PolymarketDataClient,
    tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    replace_data_event_sender(tx);
    let base_url = format!("http://{addr}");

    let config = PolymarketDataClientConfig {
        base_url_http: Some(base_url.clone()),
        base_url_gamma: Some(base_url.clone()),
        base_url_data_api: Some(base_url.clone()),
        base_url_ws: Some(format!("ws://{addr}/ws")),
        base_url_reference_prices: Some(format!("ws://{addr}/polybolt")),
        api_key: Some("fixture-key".into()),
        api_secret: Some("Zml4dHVyZS1zZWNyZXQ=".into()),
        passphrase: Some("fixture-passphrase".into()),
        resolve_poll_enabled: false,
        ..PolymarketDataClientConfig::default()
    };

    let gamma =
        PolymarketGammaHttpClient::new(Some(base_url.clone()), 5, RetryConfig::default()).unwrap();
    let clob = PolymarketClobPublicClient::new(Some(base_url.clone()), 5).unwrap();
    let data_api = PolymarketDataApiHttpClient::new(Some(base_url), 5).unwrap();

    let ws_client = PolymarketMarketConnectionPool::new(
        Some(format!("ws://{addr}/ws/market")),
        false,
        TransportBackend::default(),
        WS_DEFAULT_SUBSCRIPTIONS,
    );
    let client = PolymarketDataClient::new(
        *POLYMARKET_CLIENT_ID,
        config,
        gamma,
        clob,
        data_api,
        ws_client,
    )
    .unwrap();
    (client, rx)
}

fn subscribe(client: &mut PolymarketDataClient, data_type: DataType) -> anyhow::Result<()> {
    client.subscribe(SubscribeCustomData::new(
        Some(*POLYMARKET_CLIENT_ID),
        None,
        data_type,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ))
}

fn unsubscribe(client: &mut PolymarketDataClient, data_type: &DataType) -> anyhow::Result<()> {
    client.unsubscribe(&UnsubscribeCustomData::new(
        Some(*POLYMARKET_CLIENT_ID),
        None,
        data_type.clone(),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ))
}

async fn next_custom(rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>) -> CustomData {
    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("custom data timeout")
        .expect("data channel closed");
    custom_data(event)
}

fn custom_data(event: DataEvent) -> CustomData {
    let DataEvent::Data(NautilusData::Custom(custom)) = event else {
        panic!("expected custom data event");
    };

    custom
}

fn crypto_update_with_seq(seq: u64) -> String {
    let mut update = fixture_json(CRYPTO_UPDATE_FIXTURE);
    update["seq"] = json!(seq);
    update.to_string()
}

fn build_crypto_update(
    symbol: &str,
    exact_value: &str,
    price_timestamp_ms: u64,
    message_timestamp_ms: u64,
) -> String {
    let mut update = fixture_json(CRYPTO_UPDATE_FIXTURE);
    update["payload"]["symbol"] = json!(symbol);
    update["payload"]["full_accuracy_value"] = json!(exact_value);
    update["payload"]["timestamp"] = json!(price_timestamp_ms);
    update["ts"] = json!(message_timestamp_ms);
    update.to_string()
}

fn collect_crypto_symbols(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    expected_count: usize,
) -> Vec<String> {
    let mut symbols = Vec::with_capacity(expected_count);
    for _ in 0..expected_count {
        let custom = rx.try_recv().expect("crypto data event");

        let DataEvent::Data(NautilusData::Custom(custom)) = custom else {
            panic!("expected custom data event");
        };

        let payload = custom
            .data
            .as_any()
            .downcast_ref::<PolymarketCryptoPrice>()
            .unwrap();
        symbols.push(payload.symbol.clone());
    }

    symbols.sort_unstable();
    symbols
}
