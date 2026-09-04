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

//! Live stream validation against Betfair. Ignored: needs `BETFAIR_*` credentials.

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use nautilus_betfair::{
    common::credential::BetfairCredential,
    http::client::BetfairHttpClient,
    stream::{client::BetfairStreamClient, config::BetfairStreamConfig, messages::StreamMessage},
};
use nautilus_common::testing::wait_until_async;
use nautilus_network::socket::TcpMessageHandler;
use parking_lot::Mutex;
use rstest::rstest;

const VALIDATION_SECS: u64 = 60 * 60;
const PROGRESS_SECS: u64 = 5 * 60;

#[derive(Default)]
struct StreamStats {
    inbound: AtomicUsize,
    connections: AtomicUsize,
    connection_closed: AtomicUsize,
    order_heartbeats: AtomicUsize,
    status_errors: AtomicUsize,
    statuses: AtomicUsize,
    connection_ids: Mutex<Vec<String>>,
}

#[rstest]
#[tokio::test]
#[ignore = "requires live Betfair credentials"]
async fn live_stream_stays_active_across_heartbeats() {
    let credential = BetfairCredential::from_env()
        .expect("BETFAIR_USERNAME, BETFAIR_PASSWORD, and BETFAIR_APP_KEY must be set");
    let http = BetfairHttpClient::new(credential.clone(), None, None, None, None, None, None)
        .expect("http client");
    http.connect().await.expect("betfair login");
    let token = http
        .session_token()
        .await
        .expect("session token after login");

    let stats = Arc::new(StreamStats::default());
    let stats_h = Arc::clone(&stats);
    let handler: TcpMessageHandler = Arc::new(move |data| {
        stats_h.inbound.fetch_add(1, Ordering::SeqCst);

        if let Ok(msg) = serde_json::from_slice::<StreamMessage>(data) {
            match msg {
                StreamMessage::Connection(connection) => {
                    stats_h.connections.fetch_add(1, Ordering::SeqCst);
                    stats_h.connection_ids.lock().push(connection.connection_id);
                }
                StreamMessage::Status(status) => {
                    stats_h.statuses.fetch_add(1, Ordering::SeqCst);
                    if status.error_code.is_some() {
                        stats_h.status_errors.fetch_add(1, Ordering::SeqCst);
                    }

                    if status.connection_closed {
                        stats_h.connection_closed.fetch_add(1, Ordering::SeqCst);
                    }
                }
                StreamMessage::OrderChange(ocm) if ocm.is_heartbeat() => {
                    stats_h.order_heartbeats.fetch_add(1, Ordering::SeqCst);
                }
                _ => {}
            }
        }
    });

    let client =
        BetfairStreamClient::connect(&credential, token, handler, BetfairStreamConfig::default())
            .await
            .expect("stream connect");

    wait_until_async(
        || async { stats.connections.load(Ordering::SeqCst) > 0 },
        Duration::from_secs(15),
    )
    .await;
    assert!(
        stats.connections.load(Ordering::SeqCst) > 0,
        "stream produced no connection frame"
    );

    client
        .subscribe_orders(None, None)
        .await
        .expect("order subscription");

    let validation_secs =
        std::env::var("BETFAIR_STREAM_LIVE_SECS")
            .ok()
            .map_or(VALIDATION_SECS, |value| {
                value
                    .parse::<u64>()
                    .expect("valid BETFAIR_STREAM_LIVE_SECS")
            });
    let started = Instant::now();
    let deadline = started + Duration::from_secs(validation_secs);
    let mut next_log = started + Duration::from_secs(PROGRESS_SECS);
    while Instant::now() < deadline {
        tokio::time::sleep(Duration::from_secs(15)).await;
        let now = Instant::now();
        if now >= next_log || !client.is_active() {
            eprintln!(
                "betfair stream validation elapsed={}s inbound={} order_heartbeats={} connections={} closed_status={} status_errors={} statuses={} active={} order_ready={}",
                started.elapsed().as_secs(),
                stats.inbound.load(Ordering::SeqCst),
                stats.order_heartbeats.load(Ordering::SeqCst),
                stats.connections.load(Ordering::SeqCst),
                stats.connection_closed.load(Ordering::SeqCst),
                stats.status_errors.load(Ordering::SeqCst),
                stats.statuses.load(Ordering::SeqCst),
                client.is_active(),
                client.is_order_ready(),
            );
            next_log = now + Duration::from_secs(PROGRESS_SECS);
        }

        if !client.is_active() && now < deadline {
            wait_until_async(|| async { client.is_active() }, Duration::from_secs(30)).await;
        }
    }

    let connections = stats.connections.load(Ordering::SeqCst);
    let closed = stats.connection_closed.load(Ordering::SeqCst);
    eprintln!(
        "betfair stream validation done inbound={} order_heartbeats={} connections={} closed_status={} status_errors={} statuses={} reconnects={}",
        stats.inbound.load(Ordering::SeqCst),
        stats.order_heartbeats.load(Ordering::SeqCst),
        connections,
        closed,
        stats.status_errors.load(Ordering::SeqCst),
        stats.statuses.load(Ordering::SeqCst),
        connections.saturating_sub(1),
    );

    assert!(
        client.is_active(),
        "stream not active after {validation_secs}s"
    );
    assert!(
        client.is_order_ready(),
        "order subscription not current after {validation_secs}s"
    );
    assert!(
        stats.inbound.load(Ordering::SeqCst) > 0,
        "stream produced no inbound frames"
    );
    assert!(
        stats.order_heartbeats.load(Ordering::SeqCst) > 0,
        "stream produced no server order heartbeat"
    );
    assert_eq!(
        stats.status_errors.load(Ordering::SeqCst),
        0,
        "stream produced a status error"
    );
    assert_eq!(closed, 0, "server closed the stream");
    assert_eq!(connections, 1, "stream reconnected during validation");

    client.close().await.expect("close stream");
    http.disconnect().await;
}
