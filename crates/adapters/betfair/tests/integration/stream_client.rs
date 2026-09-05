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

use std::{fmt::Debug, sync::Arc, time::Duration};

use nautilus_betfair::{
    common::{
        credential::BetfairCredential,
        enums::{MarketDataFilterField, SegmentType},
    },
    stream::{
        client::{BetfairStreamClient, StreamLifecycleState},
        config::BetfairStreamConfig,
        error::BetfairStreamError,
        messages::{
            MarketDataFilter, OrderFilter, StreamMarketFilter, StreamMessage, stream_decode,
        },
    },
};
use nautilus_network::socket::TcpMessageHandler;
use parking_lot::Mutex;
use rstest::rstest;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{
        TcpListener,
        tcp::{OwnedReadHalf, OwnedWriteHalf},
    },
    sync::watch,
};

/// Circuit breaker for a logical phase, not routine synchronization.
///
/// Waits are event-driven and unbounded on their own; this bound exists only so a
/// genuine regression reports the expected and observed state instead of hanging
/// until the harness kills the process. It is deliberately well below the harness
/// slow-test thresholds so that diagnostic still fires, and far above the ~2s
/// scheduling gaps that made the previous wall-clock waits flaky under load.
const PHASE_TIMEOUT: Duration = Duration::from_secs(15);

async fn wait_for_authentication_state(
    client: &BetfairStreamClient,
    expected: StreamLifecycleState,
) {
    tokio::time::timeout(
        PHASE_TIMEOUT,
        client.wait_for_authentication_state(expected),
    )
    .await
    .unwrap_or_else(|_| {
        panic!(
            "timed out waiting for authentication state {expected:?}, observed {:?}",
            client.authentication_state()
        )
    });
}

async fn wait_for_market_state(client: &BetfairStreamClient, expected: StreamLifecycleState) {
    tokio::time::timeout(
        PHASE_TIMEOUT,
        client.wait_for_market_subscription_state(expected),
    )
    .await
    .unwrap_or_else(|_| {
        panic!(
            "timed out waiting for market subscription state {expected:?}, observed {:?}",
            client.market_subscription_state()
        )
    });
}

async fn wait_for_order_state(client: &BetfairStreamClient, expected: StreamLifecycleState) {
    tokio::time::timeout(
        PHASE_TIMEOUT,
        client.wait_for_order_subscription_state(expected),
    )
    .await
    .unwrap_or_else(|_| {
        panic!(
            "timed out waiting for order subscription state {expected:?}, observed {:?}",
            client.order_subscription_state()
        )
    });
}

async fn wait_for_watch<T, F>(mut rx: watch::Receiver<T>, expected: &str, predicate: F)
where
    T: Debug,
    F: Fn(&T) -> bool,
{
    let wait = async {
        while !predicate(&rx.borrow_and_update()) {
            rx.changed().await.expect("test signal sender dropped");
        }
    };
    tokio::time::timeout(PHASE_TIMEOUT, wait)
        .await
        .unwrap_or_else(|_| {
            panic!(
                "timed out waiting for {expected}, observed {:?}",
                *rx.borrow()
            )
        });
}

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
        heartbeat_secs: None,
        heartbeat_timeout_secs: Some(60),
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
    assert_eq!(json["id"], 1);
    assert_eq!(json["appKey"], "test-app-key");
    assert_eq!(json["session"], "sess-token");

    client.close().await.expect("close stream");
}

#[rstest]
#[tokio::test]
async fn test_authentication_rejection_is_correlated() {
    let (port, listener) = bind().await;
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (read_half, mut write_half) = socket.into_split();
        let mut reader = BufReader::new(read_half);
        write_line(
            &mut write_half,
            r#"{"op":"connection","connectionId":"auth-rejected"}"#,
        )
        .await;
        let auth: serde_json::Value = serde_json::from_str(&read_line(&mut reader).await).unwrap();
        write_line(
            &mut write_half,
            &format!(
                r#"{{"op":"status","id":{},"statusCode":"FAILURE","errorCode":"INVALID_SESSION_INFORMATION","connectionClosed":false}}"#,
                auth["id"],
            ),
        )
        .await;
        tokio::time::sleep(Duration::from_secs(1)).await;
    });

    let client = BetfairStreamClient::connect(
        &test_credential(),
        "tok".to_string(),
        Arc::new(|_| {}),
        plain_config(port),
    )
    .await
    .unwrap();
    wait_for_authentication_state(&client, StreamLifecycleState::Rejected).await;

    assert_eq!(
        client.authentication_state(),
        StreamLifecycleState::Rejected
    );
    assert!(!client.is_authenticated());
    client.close().await.expect("close stream");
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscription_rejection_is_correlated() {
    let (port, listener) = bind().await;
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (read_half, mut write_half) = socket.into_split();
        let mut reader = BufReader::new(read_half);
        write_line(
            &mut write_half,
            r#"{"op":"connection","connectionId":"sub-rejected"}"#,
        )
        .await;
        let auth: serde_json::Value = serde_json::from_str(&read_line(&mut reader).await).unwrap();
        write_line(
            &mut write_half,
            &format!(
                r#"{{"op":"status","id":{},"statusCode":"SUCCESS","connectionClosed":false}}"#,
                auth["id"],
            ),
        )
        .await;
        let sub: serde_json::Value = serde_json::from_str(&read_line(&mut reader).await).unwrap();
        write_line(
            &mut write_half,
            &format!(
                r#"{{"op":"status","id":{},"statusCode":"FAILURE","errorCode":"INVALID_INPUT","connectionClosed":false}}"#,
                sub["id"],
            ),
        )
        .await;
        tokio::time::sleep(Duration::from_secs(1)).await;
    });

    let client = BetfairStreamClient::connect(
        &test_credential(),
        "tok".to_string(),
        Arc::new(|_| {}),
        plain_config(port),
    )
    .await
    .unwrap();
    client.subscribe_orders(None, None).await.unwrap();
    wait_for_order_state(&client, StreamLifecycleState::Rejected).await;

    assert_eq!(
        client.order_subscription_state(),
        StreamLifecycleState::Rejected
    );
    assert!(!client.is_order_ready());
    client.close().await.expect("close stream");
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_degraded_initial_image_reissues_order_subscription() {
    let (port, listener) = bind().await;
    let (recover_tx, recover_rx) = tokio::sync::oneshot::channel();
    let (image_tx, image_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (read_half, mut write_half) = socket.into_split();
        let mut reader = BufReader::new(read_half);
        write_line(
            &mut write_half,
            r#"{"op":"connection","connectionId":"degraded-image"}"#,
        )
        .await;
        let auth: serde_json::Value = serde_json::from_str(&read_line(&mut reader).await).unwrap();
        write_line(
            &mut write_half,
            &format!(
                r#"{{"op":"status","id":{},"statusCode":"SUCCESS","connectionClosed":false}}"#,
                auth["id"],
            ),
        )
        .await;
        let sub: serde_json::Value = serde_json::from_str(&read_line(&mut reader).await).unwrap();
        let id = sub["id"].as_u64().unwrap();
        write_line(
            &mut write_half,
            &format!(
                r#"{{"op":"status","id":{id},"statusCode":"SUCCESS","connectionClosed":false}}"#,
            ),
        )
        .await;
        write_line(
            &mut write_half,
            &format!(
                r#"{{"op":"ocm","id":{id},"pt":1000,"ct":"SUB_IMAGE","status":503,"oc":[]}}"#,
            ),
        )
        .await;
        recover_rx.await.unwrap();
        write_line(
            &mut write_half,
            &format!(r#"{{"op":"ocm","id":{id},"pt":1001,"ct":"HEARTBEAT"}}"#,),
        )
        .await;
        let replacement: serde_json::Value =
            serde_json::from_str(&read_line(&mut reader).await).unwrap();
        let replacement_id = replacement["id"].as_u64().unwrap();
        write_line(
            &mut write_half,
            &format!(
                r#"{{"op":"status","id":{replacement_id},"statusCode":"SUCCESS","connectionClosed":false}}"#,
            ),
        )
        .await;
        write_line(
            &mut write_half,
            &format!(r#"{{"op":"ocm","id":{id},"pt":1002,"ct":"SUB_IMAGE","oc":[]}}"#,),
        )
        .await;
        image_rx.await.unwrap();
        write_line(
            &mut write_half,
            &format!(r#"{{"op":"ocm","id":{replacement_id},"pt":1003,"ct":"SUB_IMAGE","oc":[]}}"#,),
        )
        .await;

        while !read_line(&mut reader).await.is_empty() {}
        (id, replacement)
    });

    let client = BetfairStreamClient::connect(
        &test_credential(),
        "tok".to_string(),
        Arc::new(|_| {}),
        plain_config(port),
    )
    .await
    .unwrap();
    wait_for_authentication_state(&client, StreamLifecycleState::Active).await;
    client.subscribe_orders(None, None).await.unwrap();
    wait_for_order_state(&client, StreamLifecycleState::Degraded).await;
    assert!(!client.is_order_ready());

    recover_tx.send(()).unwrap();
    wait_for_order_state(&client, StreamLifecycleState::Pending).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        client.order_subscription_state(),
        StreamLifecycleState::Pending
    );
    assert!(!client.is_order_ready());

    image_tx.send(()).unwrap();
    wait_for_order_state(&client, StreamLifecycleState::Active).await;
    assert!(client.is_order_ready());
    client.close().await.expect("close stream");
    let (original_id, replacement) = server.await.unwrap();
    assert_eq!(replacement["op"], "orderSubscription");
    assert_ne!(replacement["id"], original_id);
    assert!(replacement.get("clk").is_none());
    assert!(replacement.get("initialClk").is_none());
}

#[rstest]
#[tokio::test]
async fn test_market_subscription_lifecycle_degrades_and_recovers() {
    let (port, listener) = bind().await;
    let (degrade_tx, degrade_rx) = tokio::sync::oneshot::channel();
    let (recover_tx, recover_rx) = tokio::sync::oneshot::channel();
    let (finish_tx, finish_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (read_half, mut write_half) = socket.into_split();
        let mut reader = BufReader::new(read_half);
        write_line(
            &mut write_half,
            r#"{"op":"connection","connectionId":"lifecycle"}"#,
        )
        .await;
        let auth: serde_json::Value = serde_json::from_str(&read_line(&mut reader).await).unwrap();
        write_line(
            &mut write_half,
            &format!(
                r#"{{"op":"status","id":{},"statusCode":"SUCCESS","connectionClosed":false}}"#,
                auth["id"],
            ),
        )
        .await;
        let sub: serde_json::Value = serde_json::from_str(&read_line(&mut reader).await).unwrap();
        let id = sub["id"].as_u64().unwrap();
        write_line(
            &mut write_half,
            &format!(
                r#"{{"op":"status","id":{id},"statusCode":"SUCCESS","connectionClosed":false}}"#,
            ),
        )
        .await;
        write_line(
            &mut write_half,
            &format!(
                r#"{{"op":"mcm","id":{id},"pt":1000,"ct":"SUB_IMAGE","heartbeatMs":5000,"mc":[]}}"#,
            ),
        )
        .await;

        degrade_rx.await.unwrap();
        write_line(
            &mut write_half,
            &format!(
                r#"{{"op":"mcm","id":{id},"pt":1001,"status":503,"clk":"unreliable-clock","mc":[]}}"#,
            ),
        )
        .await;

        recover_rx.await.unwrap();
        write_line(
            &mut write_half,
            &format!(r#"{{"op":"mcm","id":{id},"pt":1002,"ct":"HEARTBEAT"}}"#,),
        )
        .await;
        let replacement: serde_json::Value =
            serde_json::from_str(&read_line(&mut reader).await).unwrap();
        let replacement_id = replacement["id"].as_u64().unwrap();
        write_line(
            &mut write_half,
            &format!(
                r#"{{"op":"status","id":{replacement_id},"statusCode":"SUCCESS","connectionClosed":false}}"#,
            ),
        )
        .await;
        write_line(
            &mut write_half,
            &format!(r#"{{"op":"mcm","id":{id},"pt":1003,"clk":"stale-clock","mc":[]}}"#,),
        )
        .await;
        write_line(
            &mut write_half,
            &format!(
                r#"{{"op":"mcm","id":{replacement_id},"pt":1004,"ct":"SUB_IMAGE","segmentType":"SEG_START","mc":[]}}"#,
            ),
        )
        .await;
        write_line(
            &mut write_half,
            &format!(
                r#"{{"op":"mcm","id":{replacement_id},"pt":1004,"status":503,"segmentType":"SEG","mc":[]}}"#,
            ),
        )
        .await;
        write_line(
            &mut write_half,
            &format!(
                r#"{{"op":"mcm","id":{replacement_id},"pt":1004,"ct":"SUB_IMAGE","segmentType":"SEG_END","mc":[]}}"#,
            ),
        )
        .await;

        let final_sub: serde_json::Value =
            serde_json::from_str(&read_line(&mut reader).await).unwrap();
        let final_id = final_sub["id"].as_u64().unwrap();
        write_line(
            &mut write_half,
            &format!(
                r#"{{"op":"status","id":{final_id},"statusCode":"SUCCESS","connectionClosed":false}}"#,
            ),
        )
        .await;
        finish_rx.await.unwrap();
        write_line(
            &mut write_half,
            &format!(r#"{{"op":"mcm","id":{final_id},"pt":1005,"ct":"SUB_IMAGE","mc":[]}}"#,),
        )
        .await;
        (id, replacement, final_sub)
    });

    let (degraded_forwarded_tx, degraded_forwarded) = watch::channel(false);
    let (stale_forwarded_tx, stale_forwarded) = watch::channel(false);
    let client = BetfairStreamClient::connect(
        &test_credential(),
        "tok".to_string(),
        Arc::new(move |data| {
            if data
                .windows(b"unreliable-clock".len())
                .any(|window| window == b"unreliable-clock")
            {
                degraded_forwarded_tx.send_replace(true);
            }

            if data
                .windows(b"stale-clock".len())
                .any(|window| window == b"stale-clock")
            {
                stale_forwarded_tx.send_replace(true);
            }
        }),
        plain_config(port),
    )
    .await
    .unwrap();
    wait_for_authentication_state(&client, StreamLifecycleState::Active).await;
    client
        .subscribe_markets(Default::default(), Default::default(), None, None)
        .await
        .unwrap();
    assert_eq!(
        client.market_subscription_state(),
        StreamLifecycleState::Pending
    );

    wait_for_market_state(&client, StreamLifecycleState::Active).await;
    assert!(client.is_market_ready());

    degrade_tx.send(()).unwrap();
    wait_for_market_state(&client, StreamLifecycleState::Degraded).await;
    assert!(!client.is_market_ready());
    assert!(!*degraded_forwarded.borrow());
    recover_tx.send(()).unwrap();
    wait_for_market_state(&client, StreamLifecycleState::Pending).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(client.is_active());
    assert!(!client.is_market_ready());
    assert!(!*stale_forwarded.borrow());

    finish_tx.send(()).unwrap();
    wait_for_market_state(&client, StreamLifecycleState::Active).await;
    assert!(client.is_market_ready());
    client.close().await.expect("close stream");
    let (original_id, replacement, final_sub) = server.await.unwrap();
    assert_eq!(replacement["op"], "marketSubscription");
    assert_ne!(replacement["id"], original_id);
    assert!(replacement.get("clk").is_none());
    assert!(replacement.get("initialClk").is_none());
    assert_ne!(final_sub["id"], replacement["id"]);
    assert!(final_sub.get("clk").is_none());
    assert!(final_sub.get("initialClk").is_none());
}

#[rstest]
#[tokio::test]
async fn test_stale_subscription_change_is_not_forwarded() {
    let (port, listener) = bind().await;
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (read_half, mut write_half) = socket.into_split();
        let mut reader = BufReader::new(read_half);
        write_line(
            &mut write_half,
            r#"{"op":"connection","connectionId":"stale-change"}"#,
        )
        .await;
        let auth: serde_json::Value = serde_json::from_str(&read_line(&mut reader).await).unwrap();
        write_line(
            &mut write_half,
            &format!(
                r#"{{"op":"status","id":{},"statusCode":"SUCCESS","connectionClosed":false}}"#,
                auth["id"],
            ),
        )
        .await;
        let sub: serde_json::Value = serde_json::from_str(&read_line(&mut reader).await).unwrap();
        let id = sub["id"].as_u64().unwrap();
        write_line(
            &mut write_half,
            &format!(
                r#"{{"op":"status","id":{id},"statusCode":"SUCCESS","connectionClosed":false}}"#,
            ),
        )
        .await;
        write_line(
            &mut write_half,
            &format!(r#"{{"op":"mcm","id":{id},"pt":1000,"ct":"SUB_IMAGE","mc":[]}}"#,),
        )
        .await;
        write_line(
            &mut write_half,
            &format!(
                r#"{{"op":"mcm","id":{},"pt":1001,"clk":"stale-clock","mc":[]}}"#,
                id + 1,
            ),
        )
        .await;
        write_line(
            &mut write_half,
            &format!(
                r#"{{"op":"mcm","id":{},"pt":1002,"ct":"HEARTBEAT","status":503}}"#,
                id + 1,
            ),
        )
        .await;
        tokio::time::sleep(Duration::from_millis(200)).await;
    });

    let (stale_forwarded_tx, stale_forwarded) = watch::channel(false);
    let handler: TcpMessageHandler = Arc::new(move |data: &[u8]| {
        if data
            .windows(b"stale-clock".len())
            .any(|window| window == b"stale-clock")
        {
            stale_forwarded_tx.send_replace(true);
        }
    });
    let client = BetfairStreamClient::connect(
        &test_credential(),
        "tok".to_string(),
        handler,
        plain_config(port),
    )
    .await
    .unwrap();
    wait_for_authentication_state(&client, StreamLifecycleState::Active).await;
    client
        .subscribe_markets(Default::default(), Default::default(), None, None)
        .await
        .unwrap();
    wait_for_market_state(&client, StreamLifecycleState::Active).await;
    server.await.unwrap();

    assert!(client.is_market_ready());
    assert!(!*stale_forwarded.borrow());
    client.close().await.expect("close stream");
}

#[rstest]
#[tokio::test]
async fn test_silent_subscribed_peer_reconnects_after_two_server_intervals() {
    let (port, listener) = bind().await;
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (read_half, mut write_half) = socket.into_split();
        let mut reader = BufReader::new(read_half);
        write_line(
            &mut write_half,
            r#"{"op":"connection","connectionId":"silent-first"}"#,
        )
        .await;
        read_line(&mut reader).await;
        read_line(&mut reader).await;

        let (socket, _) = tokio::time::timeout(Duration::from_secs(3), listener.accept())
            .await
            .expect("silent subscribed peer did not trigger reconnect")
            .unwrap();
        let (read_half, mut write_half) = socket.into_split();
        let mut reader = BufReader::new(read_half);
        write_line(
            &mut write_half,
            r#"{"op":"connection","connectionId":"silent-second"}"#,
        )
        .await;
        let auth = read_line(&mut reader).await;
        let sub = read_line(&mut reader).await;
        (auth, sub)
    });

    let config = BetfairStreamConfig {
        heartbeat_timeout_secs: None,
        reconnect_delay_initial_ms: 100,
        reconnect_delay_max_ms: 500,
        ..plain_config(port)
    };
    let client = BetfairStreamClient::connect(
        &test_credential(),
        "tok".to_string(),
        Arc::new(|_| {}),
        config,
    )
    .await
    .unwrap();
    client.subscribe_orders(None, Some(500)).await.unwrap();

    let (auth, sub) = server.await.unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&auth).unwrap()["op"],
        "authentication"
    );
    let sub = serde_json::from_str::<serde_json::Value>(&sub).unwrap();
    assert_eq!(sub["op"], "orderSubscription");
    assert_eq!(sub["heartbeatMs"], 500);
    client.close().await.expect("close stream");
}

#[rstest]
#[tokio::test]
async fn test_server_heartbeat_traffic_prevents_dead_peer_reconnect() {
    let (port, listener) = bind().await;
    let (result_tx, result_rx) = tokio::sync::oneshot::channel();
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (read_half, mut write_half) = socket.into_split();
        let mut reader = BufReader::new(read_half);
        write_line(
            &mut write_half,
            r#"{"op":"connection","connectionId":"heartbeat-traffic"}"#,
        )
        .await;
        read_line(&mut reader).await;
        let sub: serde_json::Value = serde_json::from_str(&read_line(&mut reader).await).unwrap();
        let id = sub["id"].as_u64().unwrap();

        for pt in 1_000..1_005 {
            tokio::time::sleep(Duration::from_millis(300)).await;
            write_line(
                &mut write_half,
                &format!(
                    r#"{{"op":"ocm","id":{id},"pt":{pt},"ct":"HEARTBEAT","heartbeatMs":500}}"#,
                ),
            )
            .await;
        }

        let reconnect = tokio::time::timeout(Duration::from_millis(400), listener.accept()).await;
        result_tx.send(reconnect.is_err()).unwrap();
        let _ = done_rx.await;
    });

    let config = BetfairStreamConfig {
        heartbeat_timeout_secs: None,
        reconnect_delay_initial_ms: 100,
        reconnect_delay_max_ms: 500,
        ..plain_config(port)
    };
    let client = BetfairStreamClient::connect(
        &test_credential(),
        "tok".to_string(),
        Arc::new(|_| {}),
        config,
    )
    .await
    .unwrap();
    client.subscribe_orders(None, Some(500)).await.unwrap();

    assert!(
        result_rx.await.unwrap(),
        "heartbeat traffic must keep the peer current",
    );
    assert!(client.is_active());
    client.close().await.expect("close stream");
    let _ = done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_connect_sends_configured_outbound_heartbeat() {
    let (port, listener) = bind().await;

    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (read_half, mut write_half) = socket.into_split();
        let mut reader = BufReader::new(read_half);

        write_line(
            &mut write_half,
            r#"{"op":"connection","connectionId":"heartbeat-on"}"#,
        )
        .await;
        read_line(&mut reader).await;
        read_line(&mut reader).await
    });

    let config = BetfairStreamConfig {
        heartbeat_secs: Some(1),
        ..plain_config(port)
    };
    let client = BetfairStreamClient::connect(
        &test_credential(),
        "tok".to_string(),
        Arc::new(|_| {}),
        config,
    )
    .await
    .unwrap();

    let heartbeat = tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .expect("configured outbound heartbeat was not sent")
        .unwrap();
    let heartbeat: serde_json::Value = serde_json::from_str(&heartbeat).unwrap();
    assert_eq!(heartbeat, serde_json::json!({"op": "heartbeat"}));

    client.close().await.expect("close stream");
}

#[rstest]
#[tokio::test]
async fn test_connect_without_heartbeat_keeps_idle_connection_active() {
    let (port, listener) = bind().await;

    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (read_half, mut write_half) = socket.into_split();
        let mut reader = BufReader::new(read_half);

        write_line(
            &mut write_half,
            r#"{"op":"connection","connectionId":"heartbeat-off"}"#,
        )
        .await;
        read_line(&mut reader).await;

        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(6), reader.read_line(&mut line)).await
    });

    let config = BetfairStreamConfig {
        heartbeat_timeout_secs: Some(10),
        ..plain_config(port)
    };
    let client = BetfairStreamClient::connect(
        &test_credential(),
        "tok".to_string(),
        Arc::new(|_| {}),
        config,
    )
    .await
    .unwrap();

    let heartbeat = server.await.unwrap();
    assert!(
        heartbeat.is_err(),
        "idle stream must not send a heartbeat or reconnect"
    );
    assert!(client.is_active());

    client.close().await.expect("close stream");
}

#[rstest]
#[tokio::test]
async fn test_aux_stream_without_heartbeat_keeps_idle_connection_active() {
    use nautilus_betfair::stream::client::BetfairRaceStreamClient;

    let (port, listener) = bind().await;

    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (read_half, _write_half) = socket.into_split();
        let mut reader = BufReader::new(read_half);

        read_line(&mut reader).await;
        read_line(&mut reader).await;

        tokio::time::timeout(Duration::from_secs(2), listener.accept()).await
    });

    let config = BetfairStreamConfig {
        heartbeat_timeout_secs: Some(1),
        reconnect_delay_initial_ms: 100,
        reconnect_delay_max_ms: 500,
        ..plain_config(port)
    };
    let (fatal_tx, _fatal_rx) = tokio::sync::mpsc::unbounded_channel();
    let client = BetfairRaceStreamClient::connect(
        &test_credential(),
        "tok".to_string(),
        Arc::new(|_| {}),
        config,
        fatal_tx,
    )
    .await
    .unwrap();

    let reconnect = server.await.unwrap();
    assert!(
        reconnect.is_err(),
        "idle auxiliary stream must not reconnect"
    );
    assert!(client.is_active());

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
        .subscribe_markets(market_filter, data_filter, Some(2_345), None)
        .await
        .unwrap();

    let msg = server.await.unwrap();
    let json: serde_json::Value = serde_json::from_str(&msg).unwrap();
    assert_eq!(json["op"], "marketSubscription");
    assert_eq!(json["heartbeatMs"], 2_345);
    assert_eq!(json["segmentationEnabled"], true);

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

    client.close().await.expect("close stream");
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
    assert_eq!(json["heartbeatMs"], 5_000);
    assert_eq!(json["segmentationEnabled"], true);

    client.close().await.expect("close stream");
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
        .subscribe_orders(Some(order_filter), Some(3_456))
        .await
        .unwrap();

    let msg = server.await.unwrap();
    let json: serde_json::Value = serde_json::from_str(&msg).unwrap();
    assert_eq!(json["op"], "orderSubscription");
    assert_eq!(json["heartbeatMs"], 3_456);
    assert_eq!(json["segmentationEnabled"], true);
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

    client.close().await.expect("close stream");
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
    let (recovery_seen_tx, recovery_seen) = watch::channel(false);

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
            recovery_seen_tx.send_replace(true);
        }
    });
    let cred = test_credential();
    let client =
        BetfairStreamClient::connect(&cred, "tok".to_string(), handler, plain_config(port))
            .await
            .unwrap();

    wait_for_watch(recovery_seen.clone(), "recovery marker", |seen| *seen).await;

    assert!(
        *recovery_seen.borrow(),
        "MCM after a non-closing status frame must reach the handler"
    );
    assert!(
        client.is_active(),
        "client must remain active after a non-closing status message",
    );

    client.close().await.expect("close stream");
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
            r#"{"op":"ocm","id":2,"pt":1000,"clk":"first-clk","oc":[]}"#,
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

    client.close().await.expect("close stream");
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
    let (recovery_seen_tx, recovery_seen) = watch::channel(false);

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
            recovery_seen_tx.send_replace(true);
        }
    });
    let cred = test_credential();
    let client =
        BetfairStreamClient::connect(&cred, "tok".to_string(), handler, plain_config(port))
            .await
            .unwrap();

    wait_for_watch(recovery_seen.clone(), "recovery marker", |seen| *seen).await;

    assert!(
        *recovery_seen.borrow(),
        "recovery MCM after a malformed line must reach the handler"
    );
    assert!(
        client.is_active(),
        "client must remain active after a malformed message",
    );

    client.close().await.expect("close stream");
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
    assert_eq!(json["heartbeatMs"], 5_000);
    assert_eq!(json["segmentationEnabled"], true);

    client.close().await.expect("close stream");
}

/// MCM messages with a `clk` are forwarded to the user handler.
#[rstest]
#[tokio::test]
async fn test_mcm_data_reaches_handler() {
    let (port, listener) = bind().await;

    let (received_tx, received) = watch::channel(0_usize);

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
        received_tx.send_modify(|count| *count += 1);
    });
    let cred = test_credential();
    let client =
        BetfairStreamClient::connect(&cred, "tok".to_string(), handler, plain_config(port))
            .await
            .unwrap();

    server.await.unwrap();

    wait_for_watch(received.clone(), "received frame count > 0", |count| {
        *count > 0
    })
    .await;

    assert!(*received.borrow() > 0);
    client.close().await.expect("close stream");
}

#[rstest]
#[tokio::test]
async fn test_segmented_mcm_survives_fragmented_and_coalesced_transport() {
    const SEQUENCE_COUNT: usize = 256;

    let (port, listener) = bind().await;
    let received = Arc::new(Mutex::new(Vec::new()));
    let received_handler = Arc::clone(&received);
    let (received_count_tx, received_count) = watch::channel(0_usize);

    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (read_half, mut write_half) = socket.into_split();
        let mut reader = BufReader::new(read_half);

        write_line(
            &mut write_half,
            r#"{"op":"connection","connectionId":"test-segments"}"#,
        )
        .await;
        read_line(&mut reader).await;

        let segments = include_str!("../../test_data/stream/mcm_SEGMENTS.jsonl");
        let payload = segments.repeat(SEQUENCE_COUNT).replace('\n', "\r\n");
        let chunk_sizes = [1, 7, 31, 256, 3, 1_024];
        let mut offset = 0;
        let mut chunk = 0;

        while offset < payload.len() {
            let end = (offset + chunk_sizes[chunk % chunk_sizes.len()]).min(payload.len());
            write_half
                .write_all(&payload.as_bytes()[offset..end])
                .await
                .unwrap();
            offset = end;
            chunk += 1;
        }

        let mut closed = String::new();
        reader.read_line(&mut closed).await.unwrap();
    });

    let handler: TcpMessageHandler = Arc::new(move |data: &[u8]| {
        if let Ok(StreamMessage::MarketChange(message)) = stream_decode(data) {
            let mut received = received_handler.lock();
            received.push(message.segment_type.unwrap());
            received_count_tx.send_replace(received.len());
        }
    });
    let client = BetfairStreamClient::connect(
        &test_credential(),
        "tok".to_string(),
        handler,
        plain_config(port),
    )
    .await
    .unwrap();

    wait_for_watch(received_count, "all segmented market changes", |count| {
        *count == SEQUENCE_COUNT * 3
    })
    .await;

    client.close().await.expect("close stream");
    server.await.unwrap();

    let received = received.lock();
    assert_eq!(received.len(), SEQUENCE_COUNT * 3);
    let (sequences, remainder) = received.as_chunks::<3>();
    assert!(remainder.is_empty());
    assert!(sequences.iter().all(|segments| {
        *segments == [SegmentType::SegStart, SegmentType::Seg, SegmentType::SegEnd]
    }));
}

/// On reconnection, the client resends auth and the market subscription with the
/// latest `clk` token injected.
#[rstest]
#[tokio::test]
async fn test_reconnect_resends_auth_and_subscription_with_clk() {
    let (port, listener) = bind().await;

    let (reconnected_tx, reconnected) = watch::channel(false);
    let reconnect_auth_key = Arc::new(tokio::sync::Mutex::new(String::new()));
    let reconnect_clk = Arc::new(tokio::sync::Mutex::new(String::new()));
    let (mcm_received_tx, mcm_received) = watch::channel(false);

    let reconnect_auth_key2 = Arc::clone(&reconnect_auth_key);
    let reconnect_clk2 = Arc::clone(&reconnect_clk);
    let mcm_received_server = mcm_received.clone();

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
            r#"{"op":"mcm","id":2,"pt":2000,"clk":"clk-xyz","mc":[{"id":"1.111"}]}"#,
        )
        .await;

        // Wait until the client has processed the MCM and stored the clk
        wait_for_watch(mcm_received_server, "market change received", |seen| *seen).await;

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

        reconnected_tx.send_replace(true);
        drop(write_half);
    });

    let cred = test_credential();
    let handler: TcpMessageHandler = Arc::new(move |data: &[u8]| {
        if data.windows(b"clk-xyz".len()).any(|w| w == b"clk-xyz") {
            mcm_received_tx.send_replace(true);
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

    wait_for_watch(reconnected.clone(), "client reconnect", |seen| *seen).await;

    assert!(*reconnected.borrow(), "client should have reconnected");

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

    client.close().await.expect("close stream");
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

    client.close().await.expect("close stream");
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

    client.close().await.expect("close stream");

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

    let (reconnected_tx, reconnected) = watch::channel(false);
    let reconnect_ops: Arc<tokio::sync::Mutex<Vec<String>>> =
        Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let (mcm_received_tx, mcm_received) = watch::channel(false);

    let reconnect_ops2 = Arc::clone(&reconnect_ops);
    let mcm_received_server = mcm_received.clone();

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
        wait_for_watch(mcm_received_server, "market change received", |seen| *seen).await;
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

        reconnected_tx.send_replace(true);
        drop(write_half);
    });

    let cred = test_credential();
    let handler: TcpMessageHandler = Arc::new(move |data: &[u8]| {
        if data.windows(b"ckX".len()).any(|w| w == b"ckX") {
            mcm_received_tx.send_replace(true);
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

    wait_for_watch(reconnected, "both subscriptions replayed", |seen| *seen).await;

    let ops = reconnect_ops.lock().await;
    assert!(ops.contains(&"authentication".to_string()));
    assert!(ops.contains(&"marketSubscription".to_string()));
    assert!(ops.contains(&"orderSubscription".to_string()));

    client.close().await.expect("close stream");
}

/// An explicit reconnect opens a replacement socket and replays current auth before the retained
/// market subscription, including its latest clock pair.
#[rstest]
#[tokio::test]
async fn test_request_reconnect_uses_updated_auth_and_clk() {
    let (port, listener) = bind().await;

    let (mcm_received_tx, mcm_received) = watch::channel(false);

    let mcm_received_server = mcm_received.clone();

    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (read_half, mut write_half) = socket.into_split();
        let mut reader = BufReader::new(read_half);

        let auth_msg = read_line(&mut reader).await;
        let auth_json: serde_json::Value = serde_json::from_str(&auth_msg).unwrap();
        assert_eq!(auth_json["session"], "old-token");

        let initial_sub = read_line(&mut reader).await;
        let initial_sub_json: serde_json::Value = serde_json::from_str(&initial_sub).unwrap();
        assert_eq!(initial_sub_json["op"], "marketSubscription");

        write_line(
            &mut write_half,
            r#"{"op":"mcm","pt":1000,"clk":"clk1","initialClk":"initial-clk1","mc":[{"id":"1.111"}]}"#,
        )
        .await;

        wait_for_watch(mcm_received_server, "market change received", |seen| *seen).await;

        let (socket, _) = listener.accept().await.unwrap();
        let (read_half, _write_half) = socket.into_split();
        let mut reader = BufReader::new(read_half);

        let auth_msg = read_line(&mut reader).await;
        let auth_json: serde_json::Value = serde_json::from_str(&auth_msg).unwrap();
        assert_eq!(auth_json["op"], "authentication");
        assert_eq!(auth_json["session"], "refreshed-token");

        let replayed_sub = read_line(&mut reader).await;
        let replayed_sub_json: serde_json::Value = serde_json::from_str(&replayed_sub).unwrap();
        assert_eq!(replayed_sub_json["op"], "marketSubscription");
        assert_eq!(replayed_sub_json["id"], initial_sub_json["id"]);
        assert_eq!(replayed_sub_json["clk"], "clk1");
        assert_eq!(replayed_sub_json["initialClk"], "initial-clk1");
    });

    let cred = test_credential();
    let handler: TcpMessageHandler = Arc::new(move |data: &[u8]| {
        if data.windows(b"clk1".len()).any(|w| w == b"clk1") {
            mcm_received_tx.send_replace(true);
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

    wait_for_watch(mcm_received, "market change received", |seen| *seen).await;

    client.update_auth("test-app-key", "refreshed-token".to_string());
    assert!(client.request_reconnect());
    assert!(
        !client.request_reconnect(),
        "a duplicate request must be coalesced while reconnecting"
    );

    server.await.unwrap();

    client.close().await.expect("close stream");
}

#[rstest]
#[tokio::test]
async fn test_request_reconnect_after_close_does_not_open_connection() {
    let (port, listener) = bind().await;
    let (accepted_tx, accepted_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (read_half, _write_half) = socket.into_split();
        let mut reader = BufReader::new(read_half);
        let _ = accepted_tx.send(());
        let _ = read_line(&mut reader).await;

        let accepted = tokio::time::timeout(Duration::from_millis(300), listener.accept()).await;
        assert!(
            accepted.is_err(),
            "a reconnect request after close must not open a replacement socket"
        );
    });

    let cred = test_credential();
    let handler: TcpMessageHandler = Arc::new(|_| {});
    let client = BetfairStreamClient::connect(
        &cred,
        "session-token".to_string(),
        handler,
        plain_config(port),
    )
    .await
    .unwrap();

    accepted_rx.await.unwrap();
    client.close().await.expect("close stream");
    assert!(!client.request_reconnect());

    server.await.unwrap();
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

/// Both auxiliary stream variants delegate explicit reconnects to the same socket path and replay
/// current auth before their retained subscription.
#[rstest]
#[case::race(false, "raceSubscription")]
#[case::cricket(true, "cricketSubscription")]
#[tokio::test]
async fn test_aux_stream_request_reconnect_uses_updated_auth(
    #[case] cricket: bool,
    #[case] subscription_op: &'static str,
) {
    use nautilus_betfair::stream::client::BetfairRaceStreamClient;

    let (port, listener) = bind().await;

    let (race_fatal_tx, _race_fatal_rx) = tokio::sync::mpsc::unbounded_channel();
    let (initial_read_tx, initial_read_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (read_half, _write_half) = socket.into_split();
        let mut reader = BufReader::new(read_half);

        let initial_auth = read_line(&mut reader).await;
        let initial_auth_json: serde_json::Value = serde_json::from_str(&initial_auth).unwrap();
        assert_eq!(initial_auth_json["session"], "old-race-token");
        let initial_sub = read_line(&mut reader).await;
        let initial_sub_json: serde_json::Value = serde_json::from_str(&initial_sub).unwrap();
        assert_eq!(initial_sub_json["op"], subscription_op);
        let _ = initial_read_tx.send(());

        let (socket, _) = listener.accept().await.unwrap();
        let (read_half, _write_half) = socket.into_split();
        let mut reader = BufReader::new(read_half);

        let auth = read_line(&mut reader).await;
        let auth_json: serde_json::Value = serde_json::from_str(&auth).unwrap();
        assert_eq!(auth_json["op"], "authentication");
        assert_eq!(auth_json["session"], "new-race-token");

        let replayed_sub = read_line(&mut reader).await;
        let replayed_sub_json: serde_json::Value = serde_json::from_str(&replayed_sub).unwrap();
        assert_eq!(replayed_sub_json, initial_sub_json);
    });

    let cred = test_credential();
    let handler: TcpMessageHandler = Arc::new(|_| {});
    let config = BetfairStreamConfig {
        reconnect_delay_initial_ms: 100,
        reconnect_delay_max_ms: 500,
        ..plain_config(port)
    };

    let client = if cricket {
        BetfairRaceStreamClient::connect_cricket(
            &cred,
            "old-race-token".to_string(),
            handler,
            config,
            race_fatal_tx,
        )
        .await
        .unwrap()
    } else {
        BetfairRaceStreamClient::connect(
            &cred,
            "old-race-token".to_string(),
            handler,
            config,
            race_fatal_tx,
        )
        .await
        .unwrap()
    };

    initial_read_rx.await.unwrap();
    client.update_auth("test-app-key", "new-race-token".to_string());
    assert!(client.request_reconnect());

    server.await.unwrap();

    client.close().await;
}
