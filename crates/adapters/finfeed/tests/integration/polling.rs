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

//! Shared activity polling, deduplication, pacing, and cancellation.

use std::time::Duration;

use nautilus_common::{
    clients::DataClient,
    messages::{DataEvent, DataResponse, data::*},
};
use nautilus_core::{UUID4, UnixNanos, string::secret::SecretString};
use nautilus_finfeed::{
    common::symbol::to_instrument_id, config::FinFeedDataClientConfig, data::FinFeedDataClient,
    http::client::FinFeedHttpConfig, instruments::FinFeedInstrumentSelection,
};
use nautilus_model::{
    data::{Data, QuoteTick, TradeTick},
    enums::AggressorSide,
    identifiers::{ClientId, InstrumentId, TradeId},
    types::{Price, Quantity},
};
use nautilus_network::{
    dst::{task, time},
    retry::RetryConfig,
};
use rstest::rstest;
use serde_json::Value;
use tokio::io::AsyncWriteExt;

use super::dst::{assert_request, bind_peer, read_request, start_peer};

const EXCHANGES: &str = include_str!("../../test_data/exchanges.json");
const MARKET: &str = include_str!("../../test_data/polymarket-market.json");
const ACTIVITY: &str = include_str!("../../test_data/poll-activity.json");
const OLDER: &str = include_str!("../../test_data/poll-older.json");
const ABSENT: &str = include_str!("../../test_data/poll-absent.json");
const MARKET_ID: &str = "TEST-OUTCOME_NO";

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn shared_latest_poll_preserves_every_field_ties_duplicates_and_older_events() {
    let (peer, http) = start_peer(vec![
        (200, "", EXCHANGES),
        (200, "", MARKET),
        (200, "", ACTIVITY),
        (200, "", ACTIVITY),
        (200, "", OLDER),
    ])
    .await;
    let (mut client, mut receiver) = connected(http.clone()).await;
    subscribe(&mut client, true, true);
    let mut events = Vec::new();
    for _ in 0..6 {
        events.push(event(&mut receiver).await);
    }

    unsubscribe(&mut client, true, true);
    client.disconnect().await.unwrap();
    assert_eq!(
        events,
        vec![
            quote("0.3", "0.4", "3.000003", "4.000004", 1, 2),
            quote("0.31", "0.41", "7.000007", "8.000008", 1, 2),
            trade("trade-1", "0.31", "2.000002", 2),
            trade("trade-3", "0.32", "4.000004", 2),
            quote("0.2", "0.3", "5.000005", "6.000006", 0, 1),
            trade("trade-2", "0.21", "3.000003", 1)
        ]
    );
    let requests = time::timeout(Duration::from_secs(5), peer)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(requests.len(), 5);

    for request in &requests[2..] {
        assert_request(
            request,
            &format!("/v1/activity/POLYMARKET/{MARKET_ID}/latest?limit=100"),
            &http,
        );
    }

    assert!(receiver.try_recv().is_err());
    #[cfg(all(feature = "simulation", madsim))]
    println!("wire: {requests:?}\ndomain: {events:?}");
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn unknown_trade_side_preserves_sibling_activity_and_subsequent_polls() {
    let logs = capture_diagnostics();
    let mut unknown: Value = serde_json::from_str(ACTIVITY).unwrap();
    unknown["trades"][0]["side"] = "FutureSide".into();
    let market = MARKET.replace("\"Open\"", "\"FutureStatus\"");
    let (peer, http) = start_peer(vec![
        (200, "", EXCHANGES.to_owned()),
        (200, "", market),
        (200, "", unknown.to_string()),
        (200, "", ACTIVITY.replace("trade-1", "trade-4")),
        (200, "", OLDER.to_owned()),
    ])
    .await;
    let (mut client, mut receiver) = connected(http).await;
    subscribe(&mut client, true, true);
    let mut events = Vec::new();
    for _ in 0..7 {
        events.push(event(&mut receiver).await);
    }

    client.disconnect().await.unwrap();
    assert_eq!(
        events,
        vec![
            quote("0.3", "0.4", "3.000003", "4.000004", 1, 2),
            quote("0.31", "0.41", "7.000007", "8.000008", 1, 2),
            trade("trade-1", "0.31", "2.000002", 2),
            trade("trade-3", "0.32", "4.000004", 2),
            trade("trade-4", "0.31", "2.000002", 2),
            quote("0.2", "0.3", "5.000005", "6.000006", 0, 1),
            trade("trade-2", "0.21", "3.000003", 1)
        ]
    );
    let requests = time::timeout(Duration::from_secs(5), peer)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(requests.len(), 5);

    for request in &requests[2..] {
        assert_latest(request);
    }

    assert!(receiver.try_recv().is_err());
    let messages = logs.lock().unwrap();
    assert!(messages.contains(&(
        log::Level::Warn,
        format!(
            "Unknown FinFeed market status for {}, retaining metadata without settlement inference",
            id()
        )
    )));
    assert!(messages.contains(&(
        log::Level::Warn,
        format!(
            "Unknown FinFeed trade side for {}, trade_id=trade-1, no aggressor-side inference",
            id()
        )
    )));
}

#[rstest]
#[case(true, false)]
#[case(false, true)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn only_requested_types_publish_and_duplicate_commands_are_idempotent(
    #[case] quotes: bool,
    #[case] trades: bool,
) {
    let (peer, http) = start_peer(vec![
        (200, "", EXCHANGES),
        (200, "", MARKET),
        (200, "", ACTIVITY),
    ])
    .await;
    let (mut client, mut receiver) = connected(http).await;
    subscribe(&mut client, quotes, trades);
    subscribe(&mut client, quotes, trades);
    let events = vec![event(&mut receiver).await, event(&mut receiver).await];
    unsubscribe(&mut client, quotes, trades);
    client.disconnect().await.unwrap();
    assert_eq!(
        events,
        if quotes {
            vec![
                quote("0.3", "0.4", "3.000003", "4.000004", 1, 2),
                quote("0.31", "0.41", "7.000007", "8.000008", 1, 2),
            ]
        } else {
            vec![
                trade("trade-1", "0.31", "2.000002", 2),
                trade("trade-3", "0.32", "4.000004", 2),
            ]
        }
    );
    assert_eq!(
        time::timeout(Duration::from_secs(5), peer)
            .await
            .unwrap()
            .unwrap()
            .len(),
        3
    );
    assert!(receiver.try_recv().is_err());
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn absent_and_malformed_records_do_not_consume_valid_identities() {
    let mut bad: Value = serde_json::from_str(ACTIVITY).unwrap();
    bad["quotes"][0]["bid"] = Value::String("1.1".into());
    bad["quotes"][1]["bid_volume"] = Value::String("-1".into());
    bad["quotes"][2]["bid"] = Value::String("0.30001".into());
    bad["trades"][0]["market_id"] = Value::String("other_NO".into());
    bad["trades"][1]["id"] = Value::Null;
    bad["trades"][2]["outcome"] = Value::String("Yes".into());
    let (peer, http) = start_peer(vec![
        (200, "", EXCHANGES.to_owned()),
        (200, "", MARKET.to_owned()),
        (200, "", ABSENT.to_owned()),
        (200, "", bad.to_string()),
        (200, "", ACTIVITY.to_owned()),
    ])
    .await;
    let (mut client, mut receiver) = connected(http).await;
    subscribe(&mut client, true, true);
    let events = vec![
        event(&mut receiver).await,
        event(&mut receiver).await,
        event(&mut receiver).await,
        event(&mut receiver).await,
    ];
    client.disconnect().await.unwrap();
    assert_eq!(
        events,
        vec![
            quote("0.3", "0.4", "3.000003", "4.000004", 1, 2),
            quote("0.31", "0.41", "7.000007", "8.000008", 1, 2),
            trade("trade-1", "0.31", "2.000002", 2),
            trade("trade-3", "0.32", "4.000004", 2)
        ]
    );
    assert_eq!(
        time::timeout(Duration::from_secs(5), peer)
            .await
            .unwrap()
            .unwrap()
            .len(),
        5
    );
    assert!(receiver.try_recv().is_err());
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn retry_after_and_admission_spacing_apply_to_shared_poll() {
    #[cfg(all(feature = "simulation", madsim))]
    madsim::net::NetSim::current().update_config(|config| {
        config.send_latency = Duration::ZERO..Duration::from_nanos(1);
    });

    let (peer, mut http, mut observed) = timed_peer(vec![
        (200, EXCHANGES, Duration::ZERO),
        (200, MARKET, Duration::ZERO),
        (429, ABSENT, Duration::ZERO),
        (503, ABSENT, Duration::ZERO),
        (200, ACTIVITY, Duration::ZERO),
    ])
    .await;
    http.min_request_interval_ms = 150;
    let minimum_spacing = Duration::from_millis(http.min_request_interval_ms);
    let (mut client, mut receiver) = connected(http).await;
    subscribe(&mut client, true, true);

    for _ in 0..4 {
        event(&mut receiver).await;
    }

    client.disconnect().await.unwrap();
    let requests = time::timeout(Duration::from_secs(5), peer)
        .await
        .unwrap()
        .unwrap();
    let mut times = Vec::new();
    while let Ok((_, at)) = observed.try_recv() {
        times.push(at);
    }

    assert_eq!(requests.len(), 5);
    let retry_after = Duration::from_secs(1);
    assert!(times[3].duration_since(times[2]) >= retry_after);
    // Anchor before backoff because peer reads can lag their admission times
    assert!(times[4].duration_since(times[2]) >= retry_after + minimum_spacing);

    for request in &requests[2..] {
        assert_latest(request);
    }

    assert!(receiver.try_recv().is_err());
}

#[rstest]
#[case(401)]
#[case(403)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn permanent_failure_suspends_until_reconnect_and_retains_intent(#[case] status: u16) {
    let (peer, http) = start_peer(vec![
        (200, "", EXCHANGES),
        (200, "", MARKET),
        (status, "", ABSENT),
        (200, "", EXCHANGES),
        (200, "", MARKET),
        (200, "", ACTIVITY),
    ])
    .await;
    let (mut client, mut receiver) = connected(http).await;
    subscribe(&mut client, true, true);
    time::sleep(Duration::from_millis(350)).await;
    assert!(receiver.try_recv().is_err());
    client.disconnect().await.unwrap();
    client.connect().await.unwrap();
    assert!(matches!(
        receiver.try_recv().unwrap(),
        DataEvent::Data(Data::Instrument(_))
    ));
    let events = vec![
        event(&mut receiver).await,
        event(&mut receiver).await,
        event(&mut receiver).await,
        event(&mut receiver).await,
    ];
    client.disconnect().await.unwrap();
    assert_eq!(
        events,
        vec![
            quote("0.3", "0.4", "3.000003", "4.000004", 1, 2),
            quote("0.31", "0.41", "7.000007", "8.000008", 1, 2),
            trade("trade-1", "0.31", "2.000002", 2),
            trade("trade-3", "0.32", "4.000004", 2)
        ]
    );
    let requests = time::timeout(Duration::from_secs(5), peer)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(requests.len(), 6);
    assert!(requests[3].starts_with("GET /v1/exchanges HTTP/1.1"));
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn reconnect_uses_fresh_polls_and_suppresses_retained_duplicates() {
    let (peer, http) = start_peer(vec![
        (200, "", EXCHANGES),
        (200, "", MARKET),
        (200, "", ACTIVITY),
        (200, "", EXCHANGES),
        (200, "", MARKET),
        (200, "", ACTIVITY),
        (200, "", OLDER),
    ])
    .await;
    let (mut client, mut receiver) = connected(http).await;
    subscribe(&mut client, true, true);

    for _ in 0..4 {
        event(&mut receiver).await;
    }

    client.disconnect().await.unwrap();
    client.connect().await.unwrap();
    assert!(matches!(
        receiver.try_recv().unwrap(),
        DataEvent::Data(Data::Instrument(_))
    ));
    let events = vec![event(&mut receiver).await, event(&mut receiver).await];
    client.disconnect().await.unwrap();
    assert_eq!(
        events,
        vec![
            quote("0.2", "0.3", "5.000005", "6.000006", 0, 1),
            trade("trade-2", "0.21", "3.000003", 1)
        ]
    );
    assert_eq!(
        time::timeout(Duration::from_secs(5), peer)
            .await
            .unwrap()
            .unwrap()
            .len(),
        7
    );
    assert!(receiver.try_recv().is_err());
}

#[rstest]
#[case(false)]
#[case(true)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn unsubscribe_or_disconnect_cancels_inflight_without_late_ticks(#[case] disconnect: bool) {
    let (peer, http, mut observed) = timed_peer(vec![
        (200, EXCHANGES, Duration::ZERO),
        (200, MARKET, Duration::ZERO),
        (200, ACTIVITY, Duration::from_millis(300)),
    ])
    .await;
    let (mut client, mut receiver) = connected(http).await;
    subscribe(&mut client, true, true);

    for expected in 0..3 {
        assert_eq!(observed.recv().await.unwrap().0, expected);
    }

    if disconnect {
        client.disconnect().await.unwrap();
    } else {
        unsubscribe(&mut client, true, true);
    }

    time::sleep(Duration::from_millis(400)).await;
    client.disconnect().await.unwrap();
    assert_eq!(
        time::timeout(Duration::from_secs(5), peer)
            .await
            .unwrap()
            .unwrap()
            .len(),
        3
    );
    assert!(receiver.try_recv().is_err());
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn changing_one_type_cancels_old_shared_response_and_keeps_the_other() {
    let (peer, http, mut observed) = timed_peer(vec![
        (200, EXCHANGES, Duration::ZERO),
        (200, MARKET, Duration::ZERO),
        (200, ACTIVITY, Duration::from_secs(2)),
        (200, OLDER, Duration::ZERO),
    ])
    .await;
    let (mut client, mut receiver) = connected(http).await;
    subscribe(&mut client, true, true);

    for _ in 0..3 {
        observed.recv().await.unwrap();
    }

    unsubscribe(&mut client, true, false);
    let (index, _) = time::timeout(Duration::from_secs(1), observed.recv())
        .await
        .expect("fresh request must precede the cancelled response")
        .unwrap();
    let tick = event(&mut receiver).await;
    client.disconnect().await.unwrap();
    assert_eq!(index, 3);
    assert_eq!(tick, trade("trade-2", "0.21", "3.000003", 1));
    assert_eq!(
        time::timeout(Duration::from_secs(5), peer)
            .await
            .unwrap()
            .unwrap()
            .len(),
        4
    );
    assert!(receiver.try_recv().is_err());
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn exhausted_retries_and_invalid_wire_data_recover_on_later_poll() {
    let mut malformed: Value = serde_json::from_str(ACTIVITY).unwrap();
    malformed["quotes"] = Value::String("invalid-list".into());
    let (peer, http) = start_peer(vec![
        (200, "", EXCHANGES.to_owned()),
        (200, "", MARKET.to_owned()),
        (503, "", ABSENT.to_owned()),
        (503, "", ABSENT.to_owned()),
        (503, "", ABSENT.to_owned()),
        (404, "", ABSENT.to_owned()),
        (200, "", malformed.to_string()),
        (200, "", ACTIVITY.to_owned()),
    ])
    .await;
    let (mut client, mut receiver) = connected(http).await;
    subscribe(&mut client, true, true);
    let events = vec![
        event(&mut receiver).await,
        event(&mut receiver).await,
        event(&mut receiver).await,
        event(&mut receiver).await,
    ];
    client.disconnect().await.unwrap();
    assert_eq!(
        events,
        vec![
            quote("0.3", "0.4", "3.000003", "4.000004", 1, 2),
            quote("0.31", "0.41", "7.000007", "8.000008", 1, 2),
            trade("trade-1", "0.31", "2.000002", 2),
            trade("trade-3", "0.32", "4.000004", 2)
        ]
    );
    assert_eq!(
        time::timeout(Duration::from_secs(5), peer)
            .await
            .unwrap()
            .unwrap()
            .len(),
        8
    );
    assert!(receiver.try_recv().is_err());
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn account_attempt_budget_stops_wire_polls_without_erasing_intent() {
    let (peer, mut http, mut observed) = timed_peer(vec![
        (200, EXCHANGES, Duration::ZERO),
        (200, MARKET, Duration::ZERO),
        (200, ACTIVITY, Duration::ZERO),
        (200, EXCHANGES, Duration::ZERO),
        (200, MARKET, Duration::ZERO),
        (200, OLDER, Duration::ZERO),
    ])
    .await;
    http.attempt_budget = 3;
    let (mut client, mut receiver) = connected(http).await;
    subscribe(&mut client, true, true);

    for _ in 0..4 {
        event(&mut receiver).await;
    }

    time::sleep(Duration::from_millis(350)).await;
    assert_eq!(observed.try_recv().unwrap().0, 0);
    assert_eq!(observed.try_recv().unwrap().0, 1);
    assert_eq!(observed.try_recv().unwrap().0, 2);
    assert!(observed.try_recv().is_err());
    assert!(receiver.try_recv().is_err());
    client.disconnect().await.unwrap();
    client.connect().await.unwrap();
    assert!(matches!(
        receiver.try_recv().unwrap(),
        DataEvent::Data(Data::Instrument(_))
    ));
    let events = vec![event(&mut receiver).await, event(&mut receiver).await];
    client.disconnect().await.unwrap();
    assert_eq!(
        events,
        vec![
            quote("0.2", "0.3", "5.000005", "6.000006", 0, 1),
            trade("trade-2", "0.21", "3.000003", 1)
        ]
    );
    assert_eq!(
        time::timeout(Duration::from_secs(5), peer)
            .await
            .unwrap()
            .unwrap()
            .len(),
        6
    );
    assert_eq!(observed.try_recv().unwrap().0, 3);
    assert_eq!(observed.try_recv().unwrap().0, 4);
    assert_eq!(observed.try_recv().unwrap().0, 5);
    assert!(observed.try_recv().is_err());
    assert!(receiver.try_recv().is_err());
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn unsubscribe_and_resubscribe_rejects_old_response() {
    let (peer, http, mut observed) = timed_peer(vec![
        (200, EXCHANGES, Duration::ZERO),
        (200, MARKET, Duration::ZERO),
        (200, ACTIVITY, Duration::from_millis(200)),
        (200, OLDER, Duration::ZERO),
    ])
    .await;
    let (mut client, mut receiver) = connected(http).await;
    subscribe(&mut client, true, true);

    for _ in 0..3 {
        observed.recv().await.unwrap();
    }

    unsubscribe(&mut client, true, true);
    subscribe(&mut client, true, true);
    let events = vec![event(&mut receiver).await, event(&mut receiver).await];
    client.disconnect().await.unwrap();
    assert_eq!(
        events,
        vec![
            quote("0.2", "0.3", "5.000005", "6.000006", 0, 1),
            trade("trade-2", "0.21", "3.000003", 1)
        ]
    );
    assert_eq!(
        time::timeout(Duration::from_secs(5), peer)
            .await
            .unwrap()
            .unwrap()
            .len(),
        4
    );
    assert!(receiver.try_recv().is_err());
}

#[rstest]
#[case(1_000)]
#[case(1_001)]
#[case(10_000)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn large_activity_batches_preserve_records_deduplication_and_cancellation(
    #[case] limit: u32,
) {
    #[cfg(all(feature = "simulation", madsim))]
    madsim::net::NetSim::current().update_config(|config| {
        config.send_latency = Duration::ZERO..Duration::from_nanos(1);
    });

    let template: Value = serde_json::from_str(ACTIVITY).unwrap();
    let mut batch = template.clone();
    let mut quotes = Vec::new();
    let mut trades = Vec::new();
    let mut expected = Vec::new();

    for index in 0..u64::from(limit) {
        let mut record = template["quotes"][0].clone();
        record["entry_time"] = timestamp(index + 1).to_datetime_utc().to_string().into();
        record["recv_time"] = timestamp(index + 2).to_datetime_utc().to_string().into();
        quotes.push(record);
        expected.push(quote(
            "0.3",
            "0.4",
            "3.000003",
            "4.000004",
            index + 1,
            index + 2,
        ));
        let mut record = template["trades"][0].clone();
        record["id"] = format!("capacity-{index}").into();
        record["timestamp"] = timestamp(index + 2).to_datetime_utc().to_string().into();
        trades.push(record);
    }

    for index in 0..u64::from(limit) {
        expected.push(trade(
            &format!("capacity-{index}"),
            "0.31",
            "2.000002",
            index + 2,
        ));
    }

    batch["quotes"] = Value::Array(quotes);
    batch["trades"] = Value::Array(trades);
    let (peer, mut http, mut observed) = timed_peer(vec![
        (200, EXCHANGES.to_owned(), Duration::ZERO),
        (200, MARKET.to_owned(), Duration::ZERO),
        (200, batch.to_string(), Duration::from_millis(150)),
        (200, batch.to_string(), Duration::ZERO),
        (200, OLDER.to_owned(), Duration::ZERO),
        (200, ACTIVITY.to_owned(), Duration::from_millis(500)),
    ])
    .await;
    http.max_concurrent_requests = 4;
    http.attempt_budget = 6;
    http.retry.max_retries = 0;
    http.min_request_interval_ms = 20;
    let minimum_spacing = Duration::from_millis(http.min_request_interval_ms);
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();

    let config = FinFeedDataClientConfig {
        api_key: Some(SecretString::from("test-api-key")),
        http: http.clone(),
        instruments: vec![FinFeedInstrumentSelection {
            exchange_id: "POLYMARKET".into(),
            market_ids: vec![MARKET_ID.into()],
        }],
        poll_limit: limit,
        poll_interval_ms: 100,
        ..Default::default()
    };

    let mut client =
        FinFeedDataClient::new(ClientId::from("FINFEED"), config, sender.into()).unwrap();
    let bootstrap_started = time::Instant::now();
    client.connect().await.unwrap();
    assert!(matches!(
        receiver.try_recv().unwrap(),
        DataEvent::Data(Data::Instrument(_))
    ));
    subscribe(&mut client, true, true);
    let mut actual = Vec::new();
    for _ in &expected {
        actual.push(event(&mut receiver).await);
    }

    actual.push(event(&mut receiver).await);
    actual.push(event(&mut receiver).await);
    expected.push(quote("0.2", "0.3", "5.000005", "6.000006", 0, 1));
    expected.push(trade("trade-2", "0.21", "3.000003", 1));
    let mut starts = Vec::new();

    for index in 0..6 {
        let (actual_index, started) = time::timeout(Duration::from_secs(5), observed.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(actual_index, index);
        starts.push(started);
    }

    client.disconnect().await.unwrap();
    let requests = time::timeout(Duration::from_secs(5), peer)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(actual, expected);
    assert!(receiver.try_recv().is_err());
    assert_eq!(requests.len(), 6);

    for request in &requests[2..] {
        assert_request(
            request,
            &format!("/v1/activity/POLYMARKET/{MARKET_ID}/latest?limit={limit}"),
            &http,
        );
    }

    // Anchor before bootstrap because peer reads can lag admission times.
    assert!(starts[2].duration_since(bootstrap_started) >= 2 * minimum_spacing);
    assert!(starts[3].duration_since(starts[2]) >= Duration::from_millis(250));
}

#[rstest]
#[case(true, true)]
#[case(true, false)]
#[case(false, true)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn saturation_diagnostics_follow_independent_list_limits(
    #[case] quotes_subscribed: bool,
    #[case] trades_subscribed: bool,
) {
    let logs = capture_diagnostics();
    let first_message = logs.lock().unwrap().len();
    let mut quotes: Value = serde_json::from_str(ACTIVITY).unwrap();
    quotes["trades"] = Value::Array(Vec::new());
    let mut trades: Value = serde_json::from_str(ACTIVITY).unwrap();
    trades["quotes"] = Value::Array(Vec::new());
    let mut short: Value = serde_json::from_str(ACTIVITY).unwrap();
    short["quotes"].as_array_mut().unwrap().truncate(2);
    short["trades"].as_array_mut().unwrap().truncate(2);
    let (peer, http, mut observed) = timed_peer(vec![
        (200, EXCHANGES.to_owned(), Duration::ZERO),
        (200, MARKET.to_owned(), Duration::ZERO),
        (200, quotes.to_string(), Duration::ZERO),
        (200, trades.to_string(), Duration::ZERO),
        (200, short.to_string(), Duration::ZERO),
        (200, ABSENT.to_owned(), Duration::from_millis(200)),
    ])
    .await;
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();

    let config = FinFeedDataClientConfig {
        api_key: Some(SecretString::from("test-api-key")),
        http,
        instruments: vec![FinFeedInstrumentSelection {
            exchange_id: "POLYMARKET".into(),
            market_ids: vec![MARKET_ID.into()],
        }],
        poll_limit: 3,
        poll_interval_ms: 100,
        ..Default::default()
    };

    let mut client =
        FinFeedDataClient::new(ClientId::from("FINFEED"), config, sender.into()).unwrap();
    client.connect().await.unwrap();
    receiver.try_recv().unwrap();
    subscribe(&mut client, quotes_subscribed, trades_subscribed);

    for _ in 0..6 {
        time::timeout(Duration::from_secs(5), observed.recv())
            .await
            .unwrap()
            .unwrap();
    }

    client.disconnect().await.unwrap();
    peer.await.unwrap();
    let messages = logs.lock().unwrap();
    let messages = &messages[first_message..];

    for (quotes, trades) in [(3, 0), (0, 3), (2, 2)] {
        let quotes_saturated = quotes_subscribed && quotes == 3;
        let trades_saturated = trades_subscribed && trades == 3;
        assert!(messages.contains(&(log::Level::Debug, format!(
            "FinFeed activity received for {}, quotes={quotes}, trades={trades}, per-list limit=3, quotes_saturated={quotes_saturated}, trades_saturated={trades_saturated}", id()
        ))));
    }

    assert_eq!(
        messages
            .iter()
            .filter(|(_, message)| message.contains("per-list limit=3"))
            .count(),
        3
    );
}

pub(super) fn capture_diagnostics() -> &'static std::sync::Mutex<Vec<(log::Level, String)>> {
    static INSTALL_LOGGER: std::sync::Once = std::sync::Once::new();
    INSTALL_LOGGER.call_once(|| {
        log::set_logger(&SATURATION_LOGS).unwrap();
        log::set_max_level(log::LevelFilter::Debug);
    });

    &SATURATION_LOGS.0
}

struct SaturationLogs(std::sync::Mutex<Vec<(log::Level, String)>>);

static SATURATION_LOGS: SaturationLogs = SaturationLogs(std::sync::Mutex::new(Vec::new()));

impl log::Log for SaturationLogs {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Debug
            && matches!(
                metadata.target(),
                "nautilus_finfeed::data::poll"
                    | "nautilus_finfeed::data::parse"
                    | "nautilus_finfeed::http::parse"
            )
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            self.0
                .lock()
                .unwrap()
                .push((record.level(), record.args().to_string()));
        }
    }

    fn flush(&self) {}
}

#[rstest]
#[case(99, 100, false)]
#[case(100, 1, true)]
#[case(86_400_000, 1_000, true)]
#[case(86_400_001, 100, false)]
#[case(100, 0, false)]
#[case(100, 1_001, true)]
#[case(100, 10_000, true)]
#[case(100, 10_001, false)]
#[case(100, u32::MAX, false)]
fn polling_configuration_is_bounded(
    #[case] interval: u64,
    #[case] limit: u32,
    #[case] accepted: bool,
) {
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();

    let config = FinFeedDataClientConfig {
        api_key: Some(SecretString::from("test-api-key")),
        poll_interval_ms: interval,
        poll_limit: limit,
        instruments: vec![FinFeedInstrumentSelection {
            exchange_id: "POLYMARKET".into(),
            market_ids: vec![MARKET_ID.into()],
        }],
        ..Default::default()
    };

    assert_eq!(
        FinFeedDataClient::new(ClientId::from("FINFEED"), config, sender.into()).is_ok(),
        accepted
    );
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn unrelated_scope_changes_keep_inflight_poll_and_do_not_poll_unsubscribed_market() {
    let second_id = "other_NO";
    let second = second_definition(second_id);
    let (peer, mut http, mut observed) = timed_peer(vec![
        (200, EXCHANGES.to_owned(), Duration::ZERO),
        (200, MARKET.to_owned(), Duration::ZERO),
        (200, second, Duration::ZERO),
        (200, ACTIVITY.to_owned(), Duration::from_millis(200)),
    ])
    .await;
    http.max_concurrent_requests = 4;
    let (mut client, mut receiver) =
        connected_selection(http, vec![MARKET_ID.into(), second_id.into()], 100).await;
    subscribe(&mut client, true, true);

    for _ in 0..4 {
        observed.recv().await.unwrap();
    }

    let instrument_id = to_instrument_id("POLYMARKET", second_id).unwrap();

    for _ in 0..8 {
        client
            .subscribe_quotes(SubscribeQuotes::new(
                instrument_id,
                Some(client.client_id()),
                None,
                UUID4::new(),
                UnixNanos::from(0),
                None,
                None,
            ))
            .unwrap();
        client
            .unsubscribe_quotes(&UnsubscribeQuotes::new(
                instrument_id,
                Some(client.client_id()),
                None,
                UUID4::new(),
                UnixNanos::from(0),
                None,
                None,
            ))
            .unwrap();
        time::sleep(Duration::from_millis(10)).await;
    }

    let events = vec![
        event(&mut receiver).await,
        event(&mut receiver).await,
        event(&mut receiver).await,
        event(&mut receiver).await,
    ];
    client.disconnect().await.unwrap();
    assert_eq!(
        events,
        vec![
            quote("0.3", "0.4", "3.000003", "4.000004", 1, 2),
            quote("0.31", "0.41", "7.000007", "8.000008", 1, 2),
            trade("trade-1", "0.31", "2.000002", 2),
            trade("trade-3", "0.32", "4.000004", 2)
        ]
    );
    let requests = time::timeout(Duration::from_secs(5), peer)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(requests.len(), 4);
    assert_latest(&requests[3]);
    assert!(receiver.try_recv().is_err());
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn reset_clears_intent_before_reconnect() {
    let (peer, http, _) = timed_peer(vec![
        (200, EXCHANGES, Duration::ZERO),
        (200, MARKET, Duration::ZERO),
        (200, ACTIVITY, Duration::ZERO),
        (200, EXCHANGES, Duration::ZERO),
        (200, MARKET, Duration::ZERO),
    ])
    .await;
    let (mut client, mut receiver) = connected(http).await;
    subscribe(&mut client, true, true);

    for _ in 0..4 {
        event(&mut receiver).await;
    }

    client.reset().unwrap();
    client.connect().await.unwrap();
    assert!(matches!(
        receiver.try_recv().unwrap(),
        DataEvent::Data(Data::Instrument(_))
    ));
    time::sleep(Duration::from_millis(350)).await;
    client.disconnect().await.unwrap();
    assert_eq!(
        time::timeout(Duration::from_secs(5), peer)
            .await
            .unwrap()
            .unwrap()
            .len(),
        5
    );
    assert!(receiver.try_recv().is_err());
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn unsubscribe_cancels_retry_backoff_without_sending_retry() {
    let (peer, http, mut observed) = timed_peer(vec![
        (200, EXCHANGES, Duration::ZERO),
        (200, MARKET, Duration::ZERO),
        (429, ABSENT, Duration::ZERO),
    ])
    .await;
    let (mut client, mut receiver) = connected(http).await;
    subscribe(&mut client, true, true);

    for expected in 0..3 {
        assert_eq!(observed.recv().await.unwrap().0, expected);
    }

    wait_for_retry_backoff(&client).await;
    unsubscribe(&mut client, true, true);
    time::sleep(Duration::from_millis(1_050)).await;
    client.disconnect().await.unwrap();
    assert_eq!(
        time::timeout(Duration::from_secs(5), peer)
            .await
            .unwrap()
            .unwrap()
            .len(),
        3
    );
    assert!(receiver.try_recv().is_err());
}

pub(super) async fn wait_for_retry_backoff(client: &FinFeedDataClient) {
    time::timeout(Duration::from_secs(3), async {
        // The observed 429 attempt releases its sole permit before waiting to retry
        while !format!("{client:?}")
            .contains("concurrency: Semaphore { ll_sem: Semaphore { permits: 1 } }")
        {
            time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("FinFeed poll must enter retry backoff before unsubscribe");
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn reconnect_reloads_subscribed_definition_requested_after_startup() {
    let second_id = "other_NO";
    let second = second_definition(second_id);
    let (peer, http) = start_peer(vec![
        (200, "", EXCHANGES.to_owned()),
        (200, "", MARKET.to_owned()),
        (200, "", second.clone()),
        (200, "", ACTIVITY.to_owned()),
        (200, "", EXCHANGES.to_owned()),
        (200, "", MARKET.to_owned()),
        (200, "", second),
        (200, "", ACTIVITY.to_owned()),
        (200, "", OLDER.to_owned()),
    ])
    .await;
    let (mut client, mut receiver) = connected(http).await;
    let instrument_id = to_instrument_id("POLYMARKET", second_id).unwrap();
    client
        .request_instrument(RequestInstrument::new(
            instrument_id,
            None,
            None,
            Some(client.client_id()),
            UUID4::new(),
            UnixNanos::from(0),
            None,
        ))
        .unwrap();

    let DataResponse::Instrument(response) = super::data::response(&mut receiver).await else {
        panic!("expected requested instrument")
    };

    assert_eq!(response.instrument_id, instrument_id);
    client
        .subscribe_quotes(SubscribeQuotes::new(
            instrument_id,
            Some(client.client_id()),
            None,
            UUID4::new(),
            UnixNanos::from(0),
            None,
            None,
        ))
        .unwrap();

    for _ in 0..2 {
        let Data::Quote(tick) = event(&mut receiver).await else {
            panic!("expected quote")
        };

        assert_eq!(tick.instrument_id, instrument_id);
    }

    client.disconnect().await.unwrap();
    client.connect().await.unwrap();

    for _ in 0..2 {
        assert!(matches!(
            receiver.try_recv().unwrap(),
            DataEvent::Data(Data::Instrument(_))
        ));
    }

    let tick = event(&mut receiver).await;
    client.disconnect().await.unwrap();

    let Data::Quote(mut expected) = quote("0.2", "0.3", "5.000005", "6.000006", 0, 1) else {
        unreachable!()
    };

    expected.instrument_id = instrument_id;
    assert_eq!(tick, Data::Quote(expected));
    let requests = time::timeout(Duration::from_secs(5), peer)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(requests.len(), 9);
    assert_eq!(requests[2].lines().next(), requests[6].lines().next());
    assert_eq!(
        requests[3].lines().next().unwrap(),
        "GET /v1/activity/POLYMARKET/other_NO/latest?limit=100 HTTP/1.1"
    );
    assert!(receiver.try_recv().is_err());
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn unavailable_retained_definition_fails_reconnect_and_keeps_intent() {
    let second_id = "other_NO";
    let second = second_definition(second_id);
    let (peer, http) = start_peer(vec![
        (200, "", EXCHANGES.to_owned()),
        (200, "", MARKET.to_owned()),
        (200, "", second.clone()),
        (200, "", ACTIVITY.to_owned()),
        (200, "", EXCHANGES.to_owned()),
        (200, "", MARKET.to_owned()),
        (404, "", "{}".to_owned()),
        (200, "", EXCHANGES.to_owned()),
        (200, "", MARKET.to_owned()),
        (200, "", second),
        (200, "", OLDER.to_owned()),
    ])
    .await;
    let (mut client, mut receiver) = connected(http).await;
    let instrument_id = to_instrument_id("POLYMARKET", second_id).unwrap();
    client
        .request_instrument(RequestInstrument::new(
            instrument_id,
            None,
            None,
            Some(client.client_id()),
            UUID4::new(),
            UnixNanos::from(0),
            None,
        ))
        .unwrap();

    let DataResponse::Instrument(response) = super::data::response(&mut receiver).await else {
        panic!("expected requested instrument")
    };

    assert_eq!(response.instrument_id, instrument_id);
    client
        .subscribe_quotes(SubscribeQuotes::new(
            instrument_id,
            Some(client.client_id()),
            None,
            UUID4::new(),
            UnixNanos::from(0),
            None,
            None,
        ))
        .unwrap();

    for _ in 0..2 {
        let Data::Quote(tick) = event(&mut receiver).await else {
            panic!("expected quote")
        };

        assert_eq!(tick.instrument_id, instrument_id);
    }

    client.disconnect().await.unwrap();
    assert_eq!(
        client.connect().await.unwrap_err().to_string(),
        format!("FinFeed instrument {instrument_id} is unavailable")
    );
    assert!(client.is_disconnected());
    assert!(receiver.try_recv().is_err());
    client.connect().await.unwrap();

    for _ in 0..2 {
        assert!(matches!(
            receiver.try_recv().unwrap(),
            DataEvent::Data(Data::Instrument(_))
        ));
    }

    let tick = event(&mut receiver).await;
    client.disconnect().await.unwrap();

    let Data::Quote(mut expected) = quote("0.2", "0.3", "5.000005", "6.000006", 0, 1) else {
        unreachable!()
    };

    expected.instrument_id = instrument_id;
    assert_eq!(tick, Data::Quote(expected));
    let requests = time::timeout(Duration::from_secs(5), peer)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(requests.len(), 11);
    assert_eq!(requests[2].lines().next(), requests[6].lines().next());
    assert_eq!(requests[2].lines().next(), requests[9].lines().next());
    assert_eq!(
        requests[10].lines().next().unwrap(),
        "GET /v1/activity/POLYMARKET/other_NO/latest?limit=100 HTTP/1.1"
    );
    assert!(receiver.try_recv().is_err());
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn partial_unsubscribe_between_polls_starts_fresh_remaining_scope_immediately() {
    let (peer, http) = start_peer(vec![
        (200, "", EXCHANGES),
        (200, "", MARKET),
        (200, "", ACTIVITY),
        (200, "", OLDER),
    ])
    .await;
    let (mut client, mut receiver) =
        connected_selection(http, vec![MARKET_ID.into()], 86_400_000).await;
    subscribe(&mut client, true, true);

    for _ in 0..4 {
        event(&mut receiver).await;
    }

    unsubscribe(&mut client, true, false);
    let tick = event(&mut receiver).await;
    client.disconnect().await.unwrap();
    assert_eq!(tick, trade("trade-2", "0.21", "3.000003", 1));
    let requests = time::timeout(Duration::from_secs(5), peer)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(requests.len(), 4);
    assert_latest(&requests[3]);
    assert!(receiver.try_recv().is_err());
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn two_instruments_poll_by_due_time_without_starvation() {
    let second_id = "other_NO";
    let (peer, http) = start_peer(vec![
        (200, "", EXCHANGES.to_owned()),
        (200, "", MARKET.to_owned()),
        (200, "", second_definition(second_id)),
        (200, "", OLDER.to_owned()),
        (200, "", OLDER.to_owned()),
        (200, "", ACTIVITY.to_owned()),
        (200, "", ACTIVITY.to_owned()),
    ])
    .await;
    let (mut client, mut receiver) =
        connected_selection(http, vec![MARKET_ID.into(), second_id.into()], 250).await;
    subscribe(&mut client, true, false);
    let second_instrument = to_instrument_id("POLYMARKET", second_id).unwrap();
    client
        .subscribe_quotes(SubscribeQuotes::new(
            second_instrument,
            Some(client.client_id()),
            None,
            UUID4::new(),
            UnixNanos::from(0),
            None,
            None,
        ))
        .unwrap();
    let mut events = Vec::new();
    for _ in 0..6 {
        events.push(event(&mut receiver).await);
    }

    client.disconnect().await.unwrap();

    let expected = [
        (id(), quote("0.2", "0.3", "5.000005", "6.000006", 0, 1)),
        (
            second_instrument,
            quote("0.2", "0.3", "5.000005", "6.000006", 0, 1),
        ),
        (id(), quote("0.3", "0.4", "3.000003", "4.000004", 1, 2)),
        (id(), quote("0.31", "0.41", "7.000007", "8.000008", 1, 2)),
        (
            second_instrument,
            quote("0.3", "0.4", "3.000003", "4.000004", 1, 2),
        ),
        (
            second_instrument,
            quote("0.31", "0.41", "7.000007", "8.000008", 1, 2),
        ),
    ]
    .map(|(instrument_id, data)| {
        let Data::Quote(mut tick) = data else {
            unreachable!()
        };

        tick.instrument_id = instrument_id;
        Data::Quote(tick)
    });

    assert_eq!(events, expected);
    let requests = time::timeout(Duration::from_secs(5), peer)
        .await
        .unwrap()
        .unwrap();
    let paths: Vec<_> = requests[3..]
        .iter()
        .map(|request| request.lines().next().unwrap())
        .collect();
    assert_eq!(requests.len(), 7);
    assert_eq!(
        paths,
        [MARKET_ID, second_id, MARKET_ID, second_id].map(|market| {
            format!("GET /v1/activity/POLYMARKET/{market}/latest?limit=100 HTTP/1.1")
        })
    );
    assert!(receiver.try_recv().is_err());
}

pub(super) fn second_definition(second_id: &str) -> String {
    let mut second: Value = serde_json::from_str(MARKET).unwrap();
    second["market_id"] = Value::String(second_id.into());
    let mut tokens: Value =
        serde_json::from_str(second["source_specific_data"]["Tokens"].as_str().unwrap()).unwrap();
    tokens[1]["token_id"] = Value::String("12345".into());
    second["source_specific_data"]["Tokens"] = Value::String(tokens.to_string());
    second.to_string()
}

fn id() -> InstrumentId {
    to_instrument_id("POLYMARKET", MARKET_ID).unwrap()
}

async fn connected(
    http: FinFeedHttpConfig,
) -> (
    FinFeedDataClient,
    tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
) {
    connected_selection(http, vec![MARKET_ID.into()], 100).await
}

pub(super) async fn connected_selection(
    http: FinFeedHttpConfig,
    market_ids: Vec<String>,
    poll_interval_ms: u64,
) -> (
    FinFeedDataClient,
    tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
) {
    let count = market_ids.len();
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();

    let config = FinFeedDataClientConfig {
        api_key: Some(SecretString::from("test-api-key")),
        http,
        instruments: vec![FinFeedInstrumentSelection {
            exchange_id: "POLYMARKET".into(),
            market_ids: market_ids.iter().map(|id| id.as_str().into()).collect(),
        }],
        poll_interval_ms,
        poll_limit: 100,
        ..Default::default()
    };

    let mut client =
        FinFeedDataClient::new(ClientId::from("FINFEED"), config, sender.into()).unwrap();
    client.connect().await.unwrap();

    for _ in 0..count {
        assert!(matches!(
            receiver.try_recv().unwrap(),
            DataEvent::Data(Data::Instrument(_))
        ));
    }

    (client, receiver)
}

pub(super) fn subscribe(client: &mut FinFeedDataClient, quotes: bool, trades: bool) {
    let client_id = Some(client.client_id());
    if quotes {
        client
            .subscribe_quotes(SubscribeQuotes::new(
                id(),
                client_id,
                None,
                UUID4::new(),
                UnixNanos::from(0),
                None,
                None,
            ))
            .unwrap();
    }

    if trades {
        client
            .subscribe_trades(SubscribeTrades::new(
                id(),
                client_id,
                None,
                UUID4::new(),
                UnixNanos::from(0),
                None,
                None,
            ))
            .unwrap();
    }
}

pub(super) fn unsubscribe(client: &mut FinFeedDataClient, quotes: bool, trades: bool) {
    let client_id = Some(client.client_id());
    if quotes {
        client
            .unsubscribe_quotes(&UnsubscribeQuotes::new(
                id(),
                client_id,
                None,
                UUID4::new(),
                UnixNanos::from(0),
                None,
                None,
            ))
            .unwrap();
    }

    if trades {
        client
            .unsubscribe_trades(&UnsubscribeTrades::new(
                id(),
                client_id,
                None,
                UUID4::new(),
                UnixNanos::from(0),
                None,
                None,
            ))
            .unwrap();
    }
}

pub(super) async fn event(receiver: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>) -> Data {
    let DataEvent::Data(data) = time::timeout(Duration::from_secs(3), receiver.recv())
        .await
        .unwrap()
        .unwrap()
    else {
        panic!("expected polling tick")
    };

    match data {
        Data::Quote(tick) => {
            assert_eq!(tick.bid_price.precision, 4);
            assert_eq!(tick.ask_price.precision, 4);
            assert_eq!(tick.bid_size.precision, 6);
            assert_eq!(tick.ask_size.precision, 6);
        }
        Data::Trade(tick) => {
            assert_eq!(tick.price.precision, 4);
            assert_eq!(tick.size.precision, 6);
        }
        _ => panic!("expected quote or trade tick"),
    }

    data
}

pub(super) fn quote(
    bid: &str,
    ask: &str,
    bid_qty: &str,
    ask_qty: &str,
    entry: u64,
    recv: u64,
) -> Data {
    Data::Quote(
        QuoteTick::new_checked(
            id(),
            Price::from(bid),
            Price::from(ask),
            Quantity::from(bid_qty),
            Quantity::from(ask_qty),
            timestamp(entry),
            timestamp(recv),
        )
        .unwrap(),
    )
}

pub(super) fn trade(trade_id: &str, price: &str, qty: &str, time: u64) -> Data {
    Data::Trade(
        TradeTick::new_checked(
            id(),
            Price::from(price),
            Quantity::from(qty),
            AggressorSide::NoAggressor,
            TradeId::from(trade_id),
            timestamp(time),
            timestamp(time),
        )
        .unwrap(),
    )
}

fn timestamp(seconds: u64) -> UnixNanos {
    UnixNanos::from(1_791_676_800_000_000_000 + seconds * 1_000_000_000)
}

fn assert_latest(request: &str) {
    assert_eq!(
        request.lines().next().unwrap(),
        format!("GET /v1/activity/POLYMARKET/{MARKET_ID}/latest?limit=100 HTTP/1.1")
    );
}

pub(super) async fn timed_peer(
    responses: Vec<(u16, impl Into<String> + Send + 'static, Duration)>,
) -> (
    task::JoinHandle<Vec<String>>,
    FinFeedHttpConfig,
    tokio::sync::mpsc::UnboundedReceiver<(usize, time::Instant)>,
) {
    let (listener, address) = bind_peer().await;
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();

    let config = FinFeedHttpConfig {
        base_url: format!("http://{address}/v1/"),
        min_request_interval_ms: 1,
        retry: RetryConfig {
            initial_delay_ms: 10,
            max_delay_ms: 1000,
            jitter_ms: 0,
            max_retries: 2,
            max_elapsed_ms: Some(5000),
            ..Default::default()
        },
        ..Default::default()
    };

    let peer = task::spawn(async move {
        let mut requests = Vec::new();
        let mut delayed = Vec::new();

        for (i, (status, body, delay)) in responses.into_iter().enumerate() {
            let body = body.into();
            let (mut stream, _) = listener.accept().await.unwrap();
            requests.push(read_request(&mut stream).await);
            let _ = sender.send((i, time::Instant::now()));

            let headers = if status == 429 {
                "Retry-After: 1\r\n"
            } else {
                ""
            };

            let response = format!(
                "HTTP/1.1 {status} Response\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}",
                body.len()
            );

            if delay.is_zero() {
                stream.write_all(response.as_bytes()).await.unwrap();
                stream.flush().await.unwrap();
            } else {
                delayed.push(task::spawn(async move {
                    time::sleep(delay).await;
                    let _ = stream.write_all(response.as_bytes()).await;
                    let _ = stream.flush().await;
                }));
            }
        }

        assert!(
            time::timeout(Duration::from_millis(1_200), listener.accept())
                .await
                .is_err(),
            "unexpected overlapping or queued poll"
        );

        for response in delayed {
            response.await.unwrap();
        }

        requests
    });

    (peer, config, receiver)
}
