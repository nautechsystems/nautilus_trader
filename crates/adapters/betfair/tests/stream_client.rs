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
    common::credential::BetfairCredential,
    stream::{client::BetfairStreamClient, config::BetfairStreamConfig, error::BetfairStreamError},
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
        read_line(&mut reader).await; // auth (from subscribe combined write)
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
        read_line(&mut reader).await; // auth (from subscribe combined write)
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
        read_line(&mut reader).await; // auth (from subscribe combined write)
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
        read_line(&mut reader).await; // auth (from subscribe_markets combined)
        read_line(&mut reader).await; // market sub
        read_line(&mut reader).await; // auth (from subscribe_orders combined)
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

        // Second connection — post_reconnection sends auth+market_sub and auth+order_sub
        // (2 combined writes = 4 lines total)
        let (socket, _) = listener.accept().await.unwrap();
        let (read_half, mut write_half) = socket.into_split();
        let mut reader = BufReader::new(read_half);

        write_line(&mut write_half, r#"{"op":"connection","connectionId":"s"}"#).await;

        for _ in 0..4 {
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
