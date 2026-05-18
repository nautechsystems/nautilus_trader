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

//! Integration tests for the Betfair stream client.
//!
//! Scenarios covered:
//! - Connect: server sends `Connection`, client sends `Authentication`
//! - Subscribe: client sends `MarketSubscription` / `OrderSubscription`
//! - Data flow: server sends MCM with clk, handler is invoked
//! - Reconnection: client re-sends auth + subscriptions with latest clk after a
//!   server-side drop

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use nautilus_betfair::{
    common::{credential::BetfairCredential, enums::MarketDataFilterField},
    stream::{
        client::BetfairStreamClient,
        config::BetfairStreamConfig,
        error::BetfairStreamError,
        messages::{MarketDataFilter, OrderFilter, StreamMarketFilter},
    },
};
use nautilus_common::testing::wait_until_async;
use nautilus_network::socket::TcpMessageHandler;
use rstest::rstest;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{
        TcpListener,
        tcp::{OwnedReadHalf, OwnedWriteHalf},
    },
};

async fn bind() -> (u16, TcpListener) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    (port, listener)
}

async fn read_line(reader: &mut BufReader<OwnedReadHalf>) -> String {
    let mut line = String::new();
    reader.read_line(&mut line).await.expect("read line");
    line.trim_end_matches("\r\n")
        .trim_end_matches('\n')
        .to_string()
}

async fn write_line(writer: &mut OwnedWriteHalf, msg: &str) {
    writer
        .write_all(format!("{msg}\r\n").as_bytes())
        .await
        .expect("write line");
}

fn test_credential() -> BetfairCredential {
    BetfairCredential::new(
        "testuser".to_string(),
        "testpass".to_string(),
        "test-app-key".to_string(),
    )
}

fn plain_config(port: u16) -> BetfairStreamConfig {
    BetfairStreamConfig {
        host: "127.0.0.1".to_string(),
        port,
        heartbeat_ms: 5_000,
        idle_timeout_ms: 60_000,
        reconnect_delay_initial_ms: 200,
        reconnect_delay_max_ms: 1_000,
        use_tls: false,
    }
}

/// Client connects and immediately sends an `Authentication` message.
#[rstest]
#[tokio::test]
async fn test_connect_sends_auth() {
    let (port, listener) = bind().await;

    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (read_half, mut write_half) = socket.into_split();
        let mut reader = BufReader::new(read_half);

        write_line(
            &mut write_half,
            r#"{"op":"connection","connectionId":"test-001"}"#,
        )
        .await;

        read_line(&mut reader).await
    });

    let cred = test_credential();
    let handler: TcpMessageHandler = Arc::new(|_| {});
    let client =
        BetfairStreamClient::connect(&cred, "sess-token".to_string(), handler, plain_config(port))
            .await
            .unwrap();

    let first_msg = server.await.unwrap();
    let json: serde_json::Value = serde_json::from_str(&first_msg).unwrap();

    assert_eq!(json["op"], "authentication");
    assert_eq!(json["appKey"], "test-app-key");
    assert_eq!(json["session"], "sess-token");

    client.close().await;
}

/// `marketSubscription` payload must include `marketFilter.marketIds` and the
/// requested `marketDataFilter.fields` so the venue knows what to stream back.
#[rstest]
#[tokio::test]
async fn test_subscribe_markets_includes_market_filter_and_fields() {
    let (port, listener) = bind().await;

    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (read_half, mut write_half) = socket.into_split();
        let mut reader = BufReader::new(read_half);

        write_line(
            &mut write_half,
            r#"{"op":"connection","connectionId":"test-mf"}"#,
        )
        .await;

        read_line(&mut reader).await; // auth (from connect)
        read_line(&mut reader).await // market subscription
    });

    let cred = test_credential();
    let handler: TcpMessageHandler = Arc::new(|_| {});
    let client =
        BetfairStreamClient::connect(&cred, "tok".to_string(), handler, plain_config(port))
            .await
            .unwrap();

    let market_filter = StreamMarketFilter {
        market_ids: Some(vec!["1.123456".to_string(), "1.789012".to_string()]),
        ..Default::default()
    };
    let data_filter = MarketDataFilter {
        fields: Some(vec![
            MarketDataFilterField::ExAllOffers,
            MarketDataFilterField::ExTraded,
        ]),
        ladder_levels: None,
    };

    client
        .subscribe_markets(market_filter, data_filter, None, None)
        .await
        .unwrap();

    let msg = server.await.unwrap();
    let json: serde_json::Value = serde_json::from_str(&msg).unwrap();
    assert_eq!(json["op"], "marketSubscription");

    let market_ids = json["marketFilter"]["marketIds"]
        .as_array()
        .expect("marketIds must be present");
    let ids: Vec<&str> = market_ids.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        ids.contains(&"1.123456") && ids.contains(&"1.789012"),
        "expected both market ids in payload, was: {ids:?}"
    );

    let fields = json["marketDataFilter"]["fields"]
        .as_array()
        .expect("fields must be present");
    let field_strings: Vec<&str> = fields.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        field_strings.contains(&"EX_ALL_OFFERS") && field_strings.contains(&"EX_TRADED"),
        "expected requested fields in payload, was: {field_strings:?}"
    );

    client.close().await;
}

/// After subscribing, the subscription message arrives at the server.
#[rstest]
#[tokio::test]
async fn test_subscribe_markets_sends_subscription() {
    let (port, listener) = bind().await;

    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (read_half, mut write_half) = socket.into_split();
        let mut reader = BufReader::new(read_half);

        write_line(
            &mut write_half,
            r#"{"op":"connection","connectionId":"test-002"}"#,
        )
        .await;

        read_line(&mut reader).await; // auth (from connect)
        read_line(&mut reader).await // market subscription
    });

    let cred = test_credential();
    let handler: TcpMessageHandler = Arc::new(|_| {});
    let client =
        BetfairStreamClient::connect(&cred, "tok".to_string(), handler, plain_config(port))
            .await
            .unwrap();

    client
        .subscribe_markets(Default::default(), Default::default(), None, None)
        .await
        .unwrap();

    let msg = server.await.unwrap();
    let json: serde_json::Value = serde_json::from_str(&msg).unwrap();
    assert_eq!(json["op"], "marketSubscription");

    client.close().await;
}

/// `orderSubscription` payload must include the supplied `OrderFilter` so the
/// venue partitions matched orders by strategy ref / account id as requested.
#[rstest]
#[tokio::test]
async fn test_subscribe_orders_includes_order_filter_payload() {
    let (port, listener) = bind().await;

    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (read_half, mut write_half) = socket.into_split();
        let mut reader = BufReader::new(read_half);

        write_line(
            &mut write_half,
            r#"{"op":"connection","connectionId":"test-of"}"#,
        )
        .await;

        read_line(&mut reader).await; // auth (from connect)
        read_line(&mut reader).await // order subscription
    });

    let cred = test_credential();
    let handler: TcpMessageHandler = Arc::new(|_| {});
    let client =
        BetfairStreamClient::connect(&cred, "tok".to_string(), handler, plain_config(port))
            .await
            .unwrap();

    let order_filter = OrderFilter {
        include_overall_position: false,
        customer_strategy_refs: Some(vec!["strategy-A".to_string(), "strategy-B".to_string()]),
        partition_matched_by_strategy_ref: true,
        account_ids: Some(vec![123_456]),
    };

    client
        .subscribe_orders(Some(order_filter), None)
        .await
        .unwrap();

    let msg = server.await.unwrap();
    let json: serde_json::Value = serde_json::from_str(&msg).unwrap();
    assert_eq!(json["op"], "orderSubscription");
    assert_eq!(json["orderFilter"]["includeOverallPosition"], false);
    assert_eq!(json["orderFilter"]["partitionMatchedByStrategyRef"], true);

    let strategy_refs = json["orderFilter"]["customerStrategyRefs"]
        .as_array()
        .expect("customerStrategyRefs must be present");
    let refs: Vec<&str> = strategy_refs.iter().filter_map(|v| v.as_str()).collect();
    assert_eq!(refs, vec!["strategy-A", "strategy-B"]);

    let account_ids = json["orderFilter"]["accountIds"]
        .as_array()
        .expect("accountIds must be present");
    let ids: Vec<u64> = account_ids.iter().filter_map(|v| v.as_u64()).collect();
    assert_eq!(ids, vec![123_456]);

    client.close().await;
}

/// After auth, a Status message from the server is informational and must
/// not tear down the connection. The client should stay active and continue
/// processing further messages.
#[rstest]
#[tokio::test]
async fn test_stream_status_message_keeps_client_active() {
    let (port, listener) = bind().await;

    // The handler fires per inbound frame (connection + status + MCM), so
    // counting frames cannot distinguish "MCM after status was processed"
    // from "only the connection frame was processed". Instead, set a flag
    // when we observe the unique post-status marker `clk-after-status`.
    let recovery_seen = Arc::new(AtomicBool::new(false));
    let recovery_seen_handler = Arc::clone(&recovery_seen);

    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (read_half, mut write_half) = socket.into_split();
        let mut reader = BufReader::new(read_half);

        write_line(
            &mut write_half,
            r#"{"op":"connection","connectionId":"test-st"}"#,
        )
        .await;
        read_line(&mut reader).await; // auth

        // Informational status (not connection-closed) should not affect lifecycle.
        write_line(
            &mut write_half,
            r#"{"op":"status","id":1,"statusCode":"SUCCESS","connectionClosed":false}"#,
        )
        .await;

        // Subsequent valid MCM proves the client is still listening on the same socket.
        write_line(
            &mut write_half,
            r#"{"op":"mcm","pt":1000,"clk":"clk-after-status","mc":[{"id":"1.234"}]}"#,
        )
        .await;

        // Drain reads until the test closes the client (EOF unblocks the loop)
        // so we don't hold the socket open with an arbitrary sleep.
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
        }
    });

    let handler: TcpMessageHandler = Arc::new(move |data: &[u8]| {
        if data
            .windows(b"clk-after-status".len())
            .any(|w| w == b"clk-after-status")
        {
            recovery_seen_handler.store(true, Ordering::Relaxed);
        }
    });
    let cred = test_credential();
    let client =
        BetfairStreamClient::connect(&cred, "tok".to_string(), handler, plain_config(port))
            .await
            .unwrap();

    wait_until_async(
        || {
            let r = Arc::clone(&recovery_seen);
            async move { r.load(Ordering::Relaxed) }
        },
        Duration::from_secs(2),
    )
    .await;

    assert!(
        recovery_seen.load(Ordering::Relaxed),
        "MCM after a non-closing status frame must reach the handler"
    );
    assert!(
        client.is_active(),
        "client must remain active after a non-closing status message",
    );

    client.close().await;
    server.await.unwrap();
}

/// Calling `subscribe_orders` twice must reset the cached order `clk` so that
/// a subsequent reconnection does not replay a stale token. The
/// `OrderSubscription` struct is built with `clk: None` by construction, so
/// the immediate on-wire payload always lacks `clk`; the *load-bearing*
/// behaviour is that the post-reconnection resubscribe also omits the prior
/// OCM's `clk`. Force a reconnect after the second subscribe to exercise
/// that path.
#[rstest]
#[tokio::test]
async fn test_subscribe_orders_resubscribe_resets_clk_for_reconnect() {
    let (port, listener) = bind().await;

    let server = tokio::spawn(async move {
        // First connection: deliver an OCM whose clk would normally be
        // replayed on reconnect.
        let (socket, _) = listener.accept().await.unwrap();
        let (read_half, mut write_half) = socket.into_split();
        let mut reader = BufReader::new(read_half);

        write_line(
            &mut write_half,
            r#"{"op":"connection","connectionId":"resub-first"}"#,
        )
        .await;
        read_line(&mut reader).await; // auth
        read_line(&mut reader).await; // first orderSubscription

        write_line(
            &mut write_half,
            r#"{"op":"ocm","id":1,"pt":1000,"clk":"first-clk","oc":[]}"#,
        )
        .await;

        // Wait for the client to ingest the OCM and cache the clk.
        tokio::time::sleep(Duration::from_millis(150)).await;

        // The test will issue a second subscribe_orders; that call resets
        // the cached clk to None.
        read_line(&mut reader).await; // second orderSubscription

        // Drop to force a reconnect.
        drop(write_half);
        drop(reader);

        // Second connection: capture the resubscribe payload. The
        // post-reconnection auth + sub arrive as separate lines on the
        // order channel.
        let (socket, _) = listener.accept().await.unwrap();
        let (read_half, mut write_half) = socket.into_split();
        let mut reader = BufReader::new(read_half);

        write_line(
            &mut write_half,
            r#"{"op":"connection","connectionId":"resub-second"}"#,
        )
        .await;
        read_line(&mut reader).await; // auth replay
        read_line(&mut reader).await // resubscribed orderSubscription
    });

    let cred = test_credential();
    let handler: TcpMessageHandler = Arc::new(|_| {});
    let config = BetfairStreamConfig {
        reconnect_delay_initial_ms: 100,
        reconnect_delay_max_ms: 500,
        ..plain_config(port)
    };
    let client = BetfairStreamClient::connect(&cred, "tok".to_string(), handler, config)
        .await
        .unwrap();

    client.subscribe_orders(None, None).await.unwrap();

    // Brief pause for the OCM to round-trip before the second subscribe.
    tokio::time::sleep(Duration::from_millis(250)).await;

    // The second subscribe_orders is the call under test: it must clear the
    // cached order clk so the reconnect-driven resubscribe below carries no clk.
    client.subscribe_orders(None, None).await.unwrap();

    let resub = server.await.unwrap();
    let resub_json: serde_json::Value = serde_json::from_str(&resub).unwrap();

    assert_eq!(resub_json["op"], "orderSubscription");

    let clk = resub_json.get("clk");
    assert!(
        clk.is_none() || clk.unwrap().is_null(),
        "resubscribe-on-reconnect after the second subscribe_orders must not replay stale clk, was: {resub_json}",
    );

    let initial_clk = resub_json.get("initialClk");
    assert!(
        initial_clk.is_none() || initial_clk.unwrap().is_null(),
        "resubscribe-on-reconnect must not replay stale initialClk, was: {resub_json}",
    );

    client.close().await;
}

/// Malformed lines must not bring the connection down. The handler observes
/// raw bytes and the lower transport keeps reading; subsequent valid messages
/// continue to flow.
#[rstest]
#[tokio::test]
async fn test_stream_invalid_json_does_not_drop_connection() {
    let (port, listener) = bind().await;

    // The handler fires for every framed line. Counting alone cannot prove
    // the recovery MCM was received: the connection frame plus the malformed
    // line could already satisfy `>= 2`. Watch for the unique recovery
    // marker instead.
    let recovery_seen = Arc::new(AtomicBool::new(false));
    let recovery_seen_handler = Arc::clone(&recovery_seen);

    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (read_half, mut write_half) = socket.into_split();
        let mut reader = BufReader::new(read_half);

        write_line(
            &mut write_half,
            r#"{"op":"connection","connectionId":"test-bad"}"#,
        )
        .await;
        read_line(&mut reader).await; // auth

        write_line(&mut write_half, "this is not json").await;
        write_line(
            &mut write_half,
            r#"{"op":"mcm","pt":2000,"clk":"clk-recovery","mc":[{"id":"1.555"}]}"#,
        )
        .await;

        // Hold the socket open until the test closes the client so the
        // recovery MCM has time to round-trip without a fixed sleep.
        loop {
            let mut buf = String::new();
            match reader.read_line(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
        }
    });

    let handler: TcpMessageHandler = Arc::new(move |data: &[u8]| {
        if data
            .windows(b"clk-recovery".len())
            .any(|w| w == b"clk-recovery")
        {
            recovery_seen_handler.store(true, Ordering::Relaxed);
        }
    });
    let cred = test_credential();
    let client =
        BetfairStreamClient::connect(&cred, "tok".to_string(), handler, plain_config(port))
            .await
            .unwrap();

    wait_until_async(
        || {
            let r = Arc::clone(&recovery_seen);
            async move { r.load(Ordering::Relaxed) }
        },
        Duration::from_secs(2),
    )
    .await;

    assert!(
        recovery_seen.load(Ordering::Relaxed),
        "recovery MCM after a malformed line must reach the handler"
    );
    assert!(
        client.is_active(),
        "client must remain active after a malformed message",
    );

    client.close().await;
    server.await.unwrap();
}

/// After subscribing to orders, the order subscription arrives at the server.
#[rstest]
#[tokio::test]
async fn test_subscribe_orders_sends_subscription() {
    let (port, listener) = bind().await;

    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (read_half, mut write_half) = socket.into_split();
        let mut reader = BufReader::new(read_half);

        write_line(
            &mut write_half,
            r#"{"op":"connection","connectionId":"test-003"}"#,
        )
        .await;

        read_line(&mut reader).await; // auth (from connect)
        read_line(&mut reader).await // order subscription
    });

    let cred = test_credential();
    let handler: TcpMessageHandler = Arc::new(|_| {});
    let client =
        BetfairStreamClient::connect(&cred, "tok".to_string(), handler, plain_config(port))
            .await
            .unwrap();

    client.subscribe_orders(None, None).await.unwrap();

    let msg = server.await.unwrap();
    let json: serde_json::Value = serde_json::from_str(&msg).unwrap();
    assert_eq!(json["op"], "orderSubscription");

    client.close().await;
}

/// MCM messages with a `clk` are forwarded to the user handler.
#[rstest]
#[tokio::test]
async fn test_mcm_data_reaches_handler() {
    let (port, listener) = bind().await;

    let received = Arc::new(AtomicUsize::new(0));
    let received2 = Arc::clone(&received);

    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (read_half, mut write_half) = socket.into_split();
        let mut reader = BufReader::new(read_half);

        write_line(
            &mut write_half,
            r#"{"op":"connection","connectionId":"test-004"}"#,
        )
        .await;
        read_line(&mut reader).await; // auth

        write_line(
            &mut write_half,
            r#"{"op":"mcm","pt":1000,"clk":"clkA","mc":[{"id":"1.234567"}]}"#,
        )
        .await;
    });

    let handler: TcpMessageHandler = Arc::new(move |_data: &[u8]| {
        received2.fetch_add(1, Ordering::Relaxed);
    });
    let cred = test_credential();
    let client =
        BetfairStreamClient::connect(&cred, "tok".to_string(), handler, plain_config(port))
            .await
            .unwrap();

    server.await.unwrap();

    wait_until_async(
        || {
            let r = Arc::clone(&received);
            async move { r.load(Ordering::Relaxed) > 0 }
        },
        Duration::from_secs(2),
    )
    .await;

    assert!(received.load(Ordering::Relaxed) > 0);
    client.close().await;
}

/// On reconnection, the client resends auth and the market subscription with the
/// latest `clk` token injected.
#[rstest]
#[tokio::test]
async fn test_reconnect_resends_auth_and_subscription_with_clk() {
    let (port, listener) = bind().await;

    let reconnected = Arc::new(AtomicBool::new(false));
    let reconnect_auth_key = Arc::new(tokio::sync::Mutex::new(String::new()));
    let reconnect_clk = Arc::new(tokio::sync::Mutex::new(String::new()));
    let mcm_received = Arc::new(AtomicBool::new(false));

    let reconnected2 = Arc::clone(&reconnected);
    let reconnect_auth_key2 = Arc::clone(&reconnect_auth_key);
    let reconnect_clk2 = Arc::clone(&reconnect_clk);
    let mcm_received_server = Arc::clone(&mcm_received);
    let mcm_received_handler = Arc::clone(&mcm_received);

    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (read_half, mut write_half) = socket.into_split();
        let mut reader = BufReader::new(read_half);

        write_line(
            &mut write_half,
            r#"{"op":"connection","connectionId":"first"}"#,
        )
        .await;

        read_line(&mut reader).await; // auth (from connect)
        read_line(&mut reader).await; // market subscription

        write_line(
            &mut write_half,
            r#"{"op":"mcm","id":1,"pt":2000,"clk":"clk-xyz","mc":[{"id":"1.111"}]}"#,
        )
        .await;

        // Wait until the client has processed the MCM and stored the clk
        wait_until_async(
            || {
                let r = Arc::clone(&mcm_received_server);
                async move { r.load(Ordering::Relaxed) }
            },
            Duration::from_secs(2),
        )
        .await;

        // Drop the connection to trigger reconnect
        drop(write_half);
        drop(reader);

        let (socket, _) = listener.accept().await.unwrap();
        let (read_half, mut write_half) = socket.into_split();
        let mut reader = BufReader::new(read_half);

        write_line(
            &mut write_half,
            r#"{"op":"connection","connectionId":"second"}"#,
        )
        .await;

        let auth_msg = read_line(&mut reader).await;
        let auth_json: serde_json::Value = serde_json::from_str(&auth_msg).unwrap();
        *reconnect_auth_key2.lock().await = auth_json["appKey"].as_str().unwrap().to_string();

        // Clk from the preceding MCM must be injected into the resubscription
        let sub_msg = read_line(&mut reader).await;
        let sub_json: serde_json::Value = serde_json::from_str(&sub_msg).unwrap();
        if let Some(clk) = sub_json["clk"].as_str() {
            *reconnect_clk2.lock().await = clk.to_string();
        }

        reconnected2.store(true, Ordering::Relaxed);
        drop(write_half);
    });

    let cred = test_credential();
    let handler: TcpMessageHandler = Arc::new(move |data: &[u8]| {
        if data.windows(b"clk-xyz".len()).any(|w| w == b"clk-xyz") {
            mcm_received_handler.store(true, Ordering::Relaxed);
        }
    });
    let config = BetfairStreamConfig {
        reconnect_delay_initial_ms: 100,
        reconnect_delay_max_ms: 500,
        ..plain_config(port)
    };

    let client = BetfairStreamClient::connect(&cred, "sess".to_string(), handler, config)
        .await
        .unwrap();

    // Subscribe to markets before the disconnect
    client
        .subscribe_markets(Default::default(), Default::default(), None, None)
        .await
        .unwrap();

    server.await.unwrap();

    wait_until_async(
        || {
            let r = Arc::clone(&reconnected);
            async move { r.load(Ordering::Relaxed) }
        },
        Duration::from_secs(5),
    )
    .await;

    assert!(
        reconnected.load(Ordering::Relaxed),
        "client should have reconnected"
    );

    let auth_key = reconnect_auth_key.lock().await;
    assert_eq!(
        *auth_key, "test-app-key",
        "auth replayed with correct app key"
    );

    let clk = reconnect_clk.lock().await;
    assert_eq!(
        *clk, "clk-xyz",
        "subscription replayed with latest clk token"
    );

    client.close().await;
}

/// `is_active()` returns true after connection and false after close.
#[rstest]
#[tokio::test]
async fn test_is_active_lifecycle() {
    let (port, listener) = bind().await;

    tokio::spawn(async move {
        loop {
            if let Ok((socket, _)) = listener.accept().await {
                let (read_half, mut write_half) = socket.into_split();
                let mut reader = BufReader::new(read_half);
                write_line(
                    &mut write_half,
                    r#"{"op":"connection","connectionId":"lc"}"#,
                )
                .await;
                // Drain reads so the connection stays open
                loop {
                    let line = read_line(&mut reader).await;
                    if line.is_empty() {
                        break;
                    }
                }
            }
        }
    });

    let cred = test_credential();
    let handler: TcpMessageHandler = Arc::new(|_| {});
    let client =
        BetfairStreamClient::connect(&cred, "tok".to_string(), handler, plain_config(port))
            .await
            .unwrap();

    assert!(client.is_active());

    client.close().await;
    assert!(!client.is_active());
}

/// Subscribing after `close()` returns a `Disconnected` error for both market
/// and order subscriptions.
#[rstest]
#[tokio::test]
async fn test_subscribe_after_close_returns_error() {
    let (port, listener) = bind().await;

    tokio::spawn(async move {
        loop {
            if let Ok((socket, _)) = listener.accept().await {
                let (read_half, mut write_half) = socket.into_split();
                let mut reader = BufReader::new(read_half);
                write_line(
                    &mut write_half,
                    r#"{"op":"connection","connectionId":"sc-err"}"#,
                )
                .await;

                loop {
                    let line = read_line(&mut reader).await;
                    if line.is_empty() {
                        break;
                    }
                }
            }
        }
    });

    let cred = test_credential();
    let handler: TcpMessageHandler = Arc::new(|_| {});
    let client =
        BetfairStreamClient::connect(&cred, "tok".to_string(), handler, plain_config(port))
            .await
            .unwrap();

    client.close().await;

    let market_err = client
        .subscribe_markets(Default::default(), Default::default(), None, None)
        .await;
    let order_err = client.subscribe_orders(None, None).await;

    assert!(
        matches!(market_err, Err(BetfairStreamError::Disconnected(_))),
        "expected Disconnected for market subscribe after close, was {market_err:?}"
    );
    assert!(
        matches!(order_err, Err(BetfairStreamError::Disconnected(_))),
        "expected Disconnected for order subscribe after close, was {order_err:?}"
    );
}

/// Two independent subscriptions (market + order) are both stored and replayed
/// on reconnection.
#[rstest]
#[tokio::test]
async fn test_reconnect_replays_both_subscriptions() {
    let (port, listener) = bind().await;

    let reconnected = Arc::new(AtomicBool::new(false));
    let reconnect_ops: Arc<tokio::sync::Mutex<Vec<String>>> =
        Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let mcm_received = Arc::new(AtomicBool::new(false));

    let reconnected2 = Arc::clone(&reconnected);
    let reconnect_ops2 = Arc::clone(&reconnect_ops);
    let mcm_received_server = Arc::clone(&mcm_received);
    let mcm_received_handler = Arc::clone(&mcm_received);

    let server = tokio::spawn(async move {
        // First connection
        let (socket, _) = listener.accept().await.unwrap();
        let (read_half, mut write_half) = socket.into_split();
        let mut reader = BufReader::new(read_half);

        write_line(&mut write_half, r#"{"op":"connection","connectionId":"f"}"#).await;
        read_line(&mut reader).await; // auth (from connect)
        read_line(&mut reader).await; // market sub
        read_line(&mut reader).await; // order sub

        write_line(&mut write_half, r#"{"op":"mcm","pt":1000,"clk":"ckX"}"#).await;
        wait_until_async(
            || {
                let r = Arc::clone(&mcm_received_server);
                async move { r.load(Ordering::Relaxed) }
            },
            Duration::from_secs(2),
        )
        .await;
        drop(write_half);
        drop(reader);

        // Second connection, post_reconnection sends auth, then each subscription
        let (socket, _) = listener.accept().await.unwrap();
        let (read_half, mut write_half) = socket.into_split();
        let mut reader = BufReader::new(read_half);

        write_line(&mut write_half, r#"{"op":"connection","connectionId":"s"}"#).await;

        for _ in 0..3 {
            let msg = read_line(&mut reader).await;
            if msg.is_empty() {
                break;
            }
            let v: serde_json::Value = serde_json::from_str(&msg).unwrap();
            if let Some(op) = v["op"].as_str() {
                reconnect_ops2.lock().await.push(op.to_string());
            }
        }

        reconnected2.store(true, Ordering::Relaxed);
        drop(write_half);
    });

    let cred = test_credential();
    let handler: TcpMessageHandler = Arc::new(move |data: &[u8]| {
        if data.windows(b"ckX".len()).any(|w| w == b"ckX") {
            mcm_received_handler.store(true, Ordering::Relaxed);
        }
    });
    let config = BetfairStreamConfig {
        reconnect_delay_initial_ms: 100,
        reconnect_delay_max_ms: 500,
        ..plain_config(port)
    };
    let client = BetfairStreamClient::connect(&cred, "s".to_string(), handler, config)
        .await
        .unwrap();

    client
        .subscribe_markets(Default::default(), Default::default(), None, None)
        .await
        .unwrap();
    client.subscribe_orders(None, None).await.unwrap();

    server.await.unwrap();

    wait_until_async(
        || {
            let r = Arc::clone(&reconnected);
            async move { r.load(Ordering::Relaxed) }
        },
        Duration::from_secs(5),
    )
    .await;

    let ops = reconnect_ops.lock().await;
    assert!(ops.contains(&"authentication".to_string()));
    assert!(ops.contains(&"marketSubscription".to_string()));
    assert!(ops.contains(&"orderSubscription".to_string()));

    client.close().await;
}

/// After calling `update_auth`, the next reconnection uses the refreshed session token.
#[rstest]
#[tokio::test]
async fn test_reconnect_uses_updated_auth_token() {
    let (port, listener) = bind().await;

    let reconnected = Arc::new(AtomicBool::new(false));
    let reconnect_session = Arc::new(tokio::sync::Mutex::new(String::new()));
    let mcm_received = Arc::new(AtomicBool::new(false));

    let reconnected2 = Arc::clone(&reconnected);
    let reconnect_session2 = Arc::clone(&reconnect_session);
    let mcm_received_server = Arc::clone(&mcm_received);
    let mcm_received_handler = Arc::clone(&mcm_received);

    let server = tokio::spawn(async move {
        // First connection
        let (socket, _) = listener.accept().await.unwrap();
        let (read_half, mut write_half) = socket.into_split();
        let mut reader = BufReader::new(read_half);

        write_line(
            &mut write_half,
            r#"{"op":"connection","connectionId":"first"}"#,
        )
        .await;

        let auth_msg = read_line(&mut reader).await;
        let auth_json: serde_json::Value = serde_json::from_str(&auth_msg).unwrap();
        assert_eq!(auth_json["session"], "old-token");

        // Send MCM so clk is stored
        write_line(
            &mut write_half,
            r#"{"op":"mcm","pt":1000,"clk":"clk1","mc":[{"id":"1.111"}]}"#,
        )
        .await;

        wait_until_async(
            || {
                let r = Arc::clone(&mcm_received_server);
                async move { r.load(Ordering::Relaxed) }
            },
            Duration::from_secs(2),
        )
        .await;

        // Drop to trigger reconnect
        drop(write_half);
        drop(reader);

        // Second connection, should use refreshed token
        let (socket, _) = listener.accept().await.unwrap();
        let (read_half, mut write_half) = socket.into_split();
        let mut reader = BufReader::new(read_half);

        write_line(
            &mut write_half,
            r#"{"op":"connection","connectionId":"second"}"#,
        )
        .await;

        let auth_msg = read_line(&mut reader).await;
        let auth_json: serde_json::Value = serde_json::from_str(&auth_msg).unwrap();
        *reconnect_session2.lock().await = auth_json["session"].as_str().unwrap_or("").to_string();

        reconnected2.store(true, Ordering::Relaxed);
        drop(write_half);
    });

    let cred = test_credential();
    let handler: TcpMessageHandler = Arc::new(move |data: &[u8]| {
        if data.windows(b"clk1".len()).any(|w| w == b"clk1") {
            mcm_received_handler.store(true, Ordering::Relaxed);
        }
    });
    let config = BetfairStreamConfig {
        reconnect_delay_initial_ms: 100,
        reconnect_delay_max_ms: 500,
        ..plain_config(port)
    };

    let client = BetfairStreamClient::connect(&cred, "old-token".to_string(), handler, config)
        .await
        .unwrap();

    client
        .subscribe_markets(Default::default(), Default::default(), None, None)
        .await
        .unwrap();

    // Push a refreshed token before the reconnect happens
    wait_until_async(
        || {
            let r = Arc::clone(&mcm_received);
            async move { r.load(Ordering::Relaxed) }
        },
        Duration::from_secs(2),
    )
    .await;

    client.update_auth("test-app-key", "refreshed-token".to_string());

    server.await.unwrap();

    wait_until_async(
        || {
            let r = Arc::clone(&reconnected);
            async move { r.load(Ordering::Relaxed) }
        },
        Duration::from_secs(5),
    )
    .await;

    let session = reconnect_session.lock().await;
    assert_eq!(
        *session, "refreshed-token",
        "reconnect should use the token pushed via update_auth"
    );

    client.close().await;
}

/// `MAX_CONNECTION_LIMIT_EXCEEDED` from the race stream is unrecoverable
/// (TPD entitlement / quota issue). The race client must signal `race_fatal_tx`
/// so the data client can permanently disable race subscriptions instead of
/// reconnecting in a tight loop.
#[rstest]
#[tokio::test]
async fn test_race_stream_max_connection_limit_signals_fatal() {
    use nautilus_betfair::stream::client::BetfairRaceStreamClient;

    let (port, listener) = bind().await;

    let (race_fatal_tx, mut race_fatal_rx) = tokio::sync::mpsc::unbounded_channel();

    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (read_half, mut write_half) = socket.into_split();
        let mut reader = BufReader::new(read_half);

        write_line(
            &mut write_half,
            r#"{"op":"connection","connectionId":"race-fatal"}"#,
        )
        .await;

        // Drain the auth (and any race subscription that may piggyback).
        read_line(&mut reader).await;

        // Push a fatal status: the venue uses this when the app key is over
        // its concurrent connection limit.
        write_line(
            &mut write_half,
            r#"{"op":"status","id":1,"statusCode":"FAILURE","errorCode":"MAX_CONNECTION_LIMIT_EXCEEDED","errorMessage":"max concurrent","connectionClosed":true}"#,
        )
        .await;

        // Keep the socket open until the client closes; do not arbitrary-sleep.
        loop {
            let mut buf = String::new();
            match reader.read_line(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
        }
    });

    let cred = test_credential();
    let handler: TcpMessageHandler = Arc::new(|_| {});
    let config = BetfairStreamConfig {
        reconnect_delay_initial_ms: 100,
        reconnect_delay_max_ms: 500,
        ..plain_config(port)
    };

    let client =
        BetfairRaceStreamClient::connect(&cred, "tok".to_string(), handler, config, race_fatal_tx)
            .await
            .unwrap();

    tokio::time::timeout(Duration::from_secs(3), race_fatal_rx.recv())
        .await
        .expect("fatal_tx should fire within timeout")
        .expect("fatal channel must not be closed before signal");

    client.close().await;
    server.await.unwrap();
}

/// After calling `update_auth` on the race stream client, reconnection uses the
/// refreshed session token.
#[rstest]
#[tokio::test]
async fn test_race_stream_reconnect_uses_updated_auth_token() {
    use nautilus_betfair::stream::client::BetfairRaceStreamClient;

    let (port, listener) = bind().await;

    let reconnected = Arc::new(AtomicBool::new(false));
    let reconnect_session = Arc::new(tokio::sync::Mutex::new(String::new()));

    let reconnected2 = Arc::clone(&reconnected);
    let reconnect_session2 = Arc::clone(&reconnect_session);

    let (race_fatal_tx, _race_fatal_rx) = tokio::sync::mpsc::unbounded_channel();

    let server = tokio::spawn(async move {
        // First connection
        let (socket, _) = listener.accept().await.unwrap();
        let (read_half, mut write_half) = socket.into_split();
        let mut reader = BufReader::new(read_half);

        write_line(
            &mut write_half,
            r#"{"op":"connection","connectionId":"race-1"}"#,
        )
        .await;

        // Read auth + raceSubscription (may arrive as one or two lines)
        let _first = read_line(&mut reader).await;

        // Brief pause then drop to trigger reconnect
        tokio::time::sleep(Duration::from_millis(200)).await;
        drop(write_half);
        drop(reader);

        // Second connection
        let (socket, _) = listener.accept().await.unwrap();
        let (read_half, mut write_half) = socket.into_split();
        let mut reader = BufReader::new(read_half);

        write_line(
            &mut write_half,
            r#"{"op":"connection","connectionId":"race-2"}"#,
        )
        .await;

        // Read reconnect auth
        let msg = read_line(&mut reader).await;
        // post_reconnection sends auth + sub in one combined write; parse the auth portion
        if let Ok(auth_json) = serde_json::from_str::<serde_json::Value>(&msg) {
            *reconnect_session2.lock().await =
                auth_json["session"].as_str().unwrap_or("").to_string();
        }

        reconnected2.store(true, Ordering::Relaxed);
        drop(write_half);
    });

    let cred = test_credential();
    let handler: TcpMessageHandler = Arc::new(|_| {});
    let config = BetfairStreamConfig {
        reconnect_delay_initial_ms: 100,
        reconnect_delay_max_ms: 500,
        ..plain_config(port)
    };

    let client = BetfairRaceStreamClient::connect(
        &cred,
        "old-race-token".to_string(),
        handler,
        config,
        race_fatal_tx,
    )
    .await
    .unwrap();

    // Push refreshed token
    client.update_auth("test-app-key", "new-race-token".to_string());

    server.await.unwrap();

    wait_until_async(
        || {
            let r = Arc::clone(&reconnected);
            async move { r.load(Ordering::Relaxed) }
        },
        Duration::from_secs(5),
    )
    .await;

    let session = reconnect_session.lock().await;
    assert_eq!(
        *session, "new-race-token",
        "race stream reconnect should use the token pushed via update_auth"
    );

    client.close().await;
}
